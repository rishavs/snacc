//! Bridge assertion rendering and host validation.

use sha2::{Digest, Sha256};
use snacc_compiler::{CollectionDef, ParamMode, Program, TParam, Ty};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    CliError, io_error,
    metadata::{Selected, package_relative_entry},
};

pub(crate) struct BridgeAssertions {
    path: PathBuf,
}

pub(crate) fn declares_bridge_assertion_include(host_source: &str) -> bool {
    let lines: Vec<&str> = host_source.lines().map(str::trim).collect();
    lines.windows(2).any(|pair| {
        pair[0] == "#[cfg(snacc_bridge_assertions)]"
            && pair[1].starts_with("include!")
            && pair[1].contains("SNACC_BRIDGE_ASSERTIONS")
    })
}

fn validate_host_assertion_include(selected: &Selected) -> Result<(), CliError> {
    let host_source = fs::read_to_string(&selected.host_src_path).map_err(io_error)?;
    if declares_bridge_assertion_include(&host_source) {
        Ok(())
    } else {
        Err(CliError(format!(
            "host '{}' is missing the bridge assertion include; add these two lines after 'mod interop;':\n#[cfg(snacc_bridge_assertions)]\ninclude!(env!(\"SNACC_BRIDGE_ASSERTIONS\"));",
            selected.host_src_path.display()
        )))
    }
}

pub(crate) fn prepare_bridge_assertions(
    selected: &Selected,
    checked: &Program,
    source: &str,
) -> Result<BridgeAssertions, CliError> {
    validate_host_assertion_include(selected)?;
    let path = write_bridge_assertions(selected, checked, source)?;
    Ok(BridgeAssertions { path })
}

pub(crate) fn apply_bridge_assertions(command: &mut Command, assertions: &BridgeAssertions) {
    command.env("SNACC_BRIDGE_ASSERTIONS", &assertions.path);
    command.arg("--cfg").arg("snacc_bridge_assertions");
}

fn rust_abi_type(ty: Ty) -> &'static str {
    match ty {
        Ty::Int64 => "i64",
        Ty::Float64 => "f64",
        Ty::Bool => "u8",
        Ty::Byte => "u8",
        Ty::UInt16 => "u16",
        Ty::UInt32 => "u32",
        Ty::UInt64 => "u64",
        Ty::Float32 => "f32",
        Ty::String
        | Ty::Unicode
        | Ty::ViewByte
        | Ty::ViewUnicode
        | Ty::Array(_)
        | Ty::List(_)
        | Ty::View(_)
        | Ty::Map(_)
        | Ty::Set(_) => unreachable!(
            "internal error: strings, Unicode values, and views are not supported by the Rust bridge"
        ),
        // Specification 010 section 16 rejects every user-defined type at every
        // `extern rust` parameter and result, and Specification 012 section 12
        // removes standalone `Nil` from the bridge entirely. Declaration
        // collection rejects both, so neither reaches assertion rendering.
        Ty::User(_) | Ty::Nil => unreachable!(
            "internal error: a user-defined type or standalone 'Nil' reached bridge \
             rendering; declaration collection rejects both"
        ),
        // Specification 018 section 10: declaration collection rejects every
        // inline sum at a Rust bridge parameter or result, so none reaches
        // assertion rendering either.
        Ty::Sum(_) => unreachable!(
            "internal error: an inline sum type reached bridge rendering; declaration \
             collection rejects it"
        ),
        // Specification 016 section 10: declaration collection rejects
        // 'Box<T>' and every type transitively containing one at every
        // 'extern rust' parameter and result, so none reaches assertion
        // rendering either.
        Ty::Box(_) => unreachable!(
            "internal error: a box type reached bridge rendering; declaration collection \
             rejects it"
        ),
    }
}

fn rust_view_element_type(checked: &Program, ty: Ty) -> Option<&'static str> {
    let element = match ty {
        Ty::ViewByte => Ty::Byte,
        Ty::ViewUnicode => Ty::Unicode,
        Ty::View(id) => match checked.collections.get(id.0 as usize)? {
            CollectionDef::View { elem } => *elem,
            _ => return None,
        },
        _ => return None,
    };
    match element {
        Ty::Byte => Some("u8"),
        Ty::UInt16 => Some("u16"),
        Ty::UInt32 => Some("u32"),
        Ty::UInt64 => Some("u64"),
        Ty::Int64 => Some("i64"),
        Ty::Bool => Some("u8"),
        Ty::Unicode => Some("u32"),
        Ty::Float32 => Some("f32"),
        Ty::Float64 => Some("f64"),
        _ => None,
    }
}

/// The ordinary Rust signature exposed to a host author. Views are borrowed
/// slices; the generated adapter below is the only code that sees their raw
/// pointer and length representation.
fn rust_source_type(checked: &Program, ty: Ty) -> String {
    if let Some(element) = rust_view_element_type(checked, ty) {
        return format!("&[{element}]");
    }
    rust_abi_type(ty).to_string()
}

/// Specification 011 section 12.2: `Ref<T>` maps to `&mut R` in ordinary Rust
/// source. The generated C ABI adapter receives a mutable pointer for it.
fn rust_source_param_type(checked: &Program, param: &TParam) -> String {
    match param.mode {
        ParamMode::Value => rust_source_type(checked, param.ty),
        ParamMode::Reference => format!("&mut {}", rust_source_type(checked, param.ty)),
    }
}

fn rust_physical_params(checked: &Program, params: &[TParam]) -> Vec<String> {
    let mut physical = Vec::new();
    for param in params {
        if param.mode == ParamMode::Value
            && let Some(element) = rust_view_element_type(checked, param.ty)
        {
            physical.push(format!("*const {element}"));
            physical.push("usize".to_string());
            continue;
        }
        physical.push(match param.mode {
            ParamMode::Value => rust_abi_type(param.ty).to_string(),
            ParamMode::Reference => format!("*mut {}", rust_abi_type(param.ty)),
        });
    }
    physical
}

fn rust_bridge_result_type(ty: Option<Ty>) -> String {
    match ty {
        Some(ty) => rust_abi_type(ty).to_string(),
        None => "()".to_string(),
    }
}

pub(crate) fn render_bridge_assertions(checked: &Program, entry: &Path, source: &str) -> String {
    let mut names: Vec<&String> = checked.externs.keys().collect();
    names.sort();
    let mut rendered = String::from(
        "// Generated by cargo-snacc from checked `extern rust` declarations (RFC 007).\n\
         // Do not edit; this file is regenerated on every build.\n",
    );
    rendered.push_str(&format!(
        "const _: () = assert!(snacc_runtime::ABI_VERSION == {}, \"snacc compiler/runtime ABI version mismatch\");\n",
        snacc_compiler::ABI_VERSION
    ));
    for name in names {
        let extern_decl = &checked.externs[name];
        let source_params = extern_decl
            .params
            .iter()
            .map(|param| rust_source_param_type(checked, param))
            .collect::<Vec<_>>()
            .join(", ");
        let result = rust_bridge_result_type(extern_decl.result);
        let physical_params = rust_physical_params(checked, &extern_decl.params);
        let mut physical_names = Vec::new();
        let mut call_args = Vec::new();
        let mut conversions = Vec::new();
        let mut physical_index = 0usize;
        for (source_index, param) in extern_decl.params.iter().enumerate() {
            let source_name = format!("arg{source_index}");
            if param.mode == ParamMode::Value {
                if let Some(element) = rust_view_element_type(checked, param.ty) {
                    let pointer_name = format!("{source_name}_ptr");
                    let length_name = format!("{source_name}_len");
                    physical_names.push(pointer_name.clone());
                    physical_names.push(length_name.clone());
                    conversions.push(format!(
                        "    let {source_name} = if {length_name} == 0 {{\n        unsafe {{ core::slice::from_raw_parts(core::ptr::NonNull::<{element}>::dangling().as_ptr(), 0) }}\n    }} else {{\n        unsafe {{ core::slice::from_raw_parts({pointer_name}, {length_name}) }}\n    }};\n"
                    ));
                    call_args.push(source_name);
                    physical_index += 2;
                    let _ = element;
                } else {
                    physical_names.push(source_name.clone());
                    call_args.push(source_name);
                    physical_index += 1;
                }
            } else {
                physical_names.push(source_name.clone());
                conversions.push(format!(
                    "    let {source_name} = unsafe {{ &mut *{source_name} }};\n"
                ));
                call_args.push(source_name);
                physical_index += 1;
            }
        }
        debug_assert_eq!(physical_index, physical_params.len());
        let physical_signature = physical_names
            .into_iter()
            .zip(physical_params)
            .map(|(name, ty)| format!("{name}: {ty}"))
            .collect::<Vec<_>>()
            .join(", ");
        let call = format!(
            "crate::interop::{}({})",
            extern_decl.symbol,
            call_args.join(", ")
        );
        let (line, column) = crate::line_column(source, extern_decl.span.start);
        rendered.push_str(&format!(
            "const _: fn({source_params}) -> {result} = crate::interop::{symbol}; // snacc: {name} ({entry}:{line}:{column})\n\
             #[unsafe(export_name = \"{symbol}\")]\n\
             pub extern \"C\" fn __snacc_bridge_{symbol}({physical_signature}) -> {result} {{\n\
             {conversions}    {call}\n\
             }}\n",
            result = result,
            symbol = extern_decl.symbol,
            entry = entry.display(),
            conversions = conversions.concat(),
            call = call,
        ));
    }
    rendered
}

fn write_bridge_assertions(
    selected: &Selected,
    checked: &Program,
    source: &str,
) -> Result<PathBuf, CliError> {
    let content = render_bridge_assertions(checked, package_relative_entry(selected), source);
    let mut hash = Sha256::new();
    hash.update(selected.package_id.as_bytes());
    hash.update(content.as_bytes());
    let digest = format!("{:x}", hash.finalize());
    let directory = selected.target_directory.join("snacc").join("bridges");
    fs::create_dir_all(&directory).map_err(io_error)?;
    let path = directory.join(format!("{}-{}.rs", selected.package.name, digest));
    if !path.is_file() {
        let mut temp = tempfile::Builder::new()
            .prefix(&format!("{}-{}-", selected.package.name, digest))
            .suffix(".rs.tmp")
            .tempfile_in(&directory)
            .map_err(io_error)?;
        temp.write_all(content.as_bytes()).map_err(io_error)?;
        if let Err(error) = temp.persist(&path)
            && !path.is_file()
        {
            return Err(CliError(format!(
                "failed to publish bridge assertions: {error}"
            )));
        }
    }
    Ok(path)
}
