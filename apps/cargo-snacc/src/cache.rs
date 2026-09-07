//! Build identity, object cache, manifest, and publication.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::{
    CliError,
    cli::Options,
    diagnostic_error, io_error,
    metadata::{Selected, package_relative_entry},
};

#[derive(Deserialize, Serialize)]
struct CacheManifest {
    schema: u32,
    identity: String,
    compiler_version: String,
    backend_build_id: String,
    abi_version: u32,
    llvm_version: String,
    target_triple: String,
    optimization: String,
    profile: String,
    object_sha256: String,
}

pub(crate) fn emit_cached(
    selected: &Selected,
    source: &str,
    options: &Options,
) -> Result<PathBuf, CliError> {
    let release = options.release || options.profile.as_deref() == Some("release");
    let compile_options = snacc_compiler::CompileOptions {
        optimization: if release {
            snacc_compiler::Optimization::Aggressive
        } else {
            snacc_compiler::Optimization::None
        },
    };
    let target_triple = snacc_compiler::target_triple();
    let llvm_version = snacc_compiler::llvm_version();
    let llvm_version = format!("{}.{}.{}", llvm_version.0, llvm_version.1, llvm_version.2);
    let backend_build_id = backend_build_id();
    let profile =
        options
            .profile
            .as_deref()
            .unwrap_or(if options.release { "release" } else { "dev" });
    let mut hash = Sha256::new();
    hash.update(env!("CARGO_PKG_VERSION"));
    hash.update(b"snacc-llvm-object-v1");
    hash.update(&backend_build_id);
    hash.update(snacc_compiler::ABI_VERSION.to_le_bytes());
    hash.update(&llvm_version);
    hash.update(&target_triple);
    hash.update(compile_options.optimization.as_str());
    hash.update(profile);
    hash.update(
        package_relative_entry(selected)
            .to_string_lossy()
            .as_bytes(),
    );
    hash.update(source.as_bytes());
    let identity = format!("{:x}", hash.finalize());
    let directory = selected
        .target_directory
        .join("snacc")
        .join(&target_triple)
        .join(profile)
        .join(&selected.package.name)
        .join(&identity);
    let object = directory.join(if cfg!(windows) { "app.obj" } else { "app.o" });
    let manifest = directory.join("manifest.json");
    if cache_matches(
        &object,
        &manifest,
        &identity,
        &backend_build_id,
        &llvm_version,
        &target_triple,
        compile_options.optimization,
        profile,
    )? {
        if options.verbose {
            println!("Snacc object reused: {}", object.display());
        }
        return Ok(object);
    }
    let emitted = snacc_compiler::emit_object_with_options(source, compile_options)
        .map_err(|diagnostics| diagnostic_error(&selected.entry, source, &diagnostics))?;
    if emitted.target_triple != target_triple {
        return Err(CliError(format!(
            "compiler emitted target '{}' after selecting '{}'",
            emitted.target_triple, target_triple
        )));
    }
    fs::create_dir_all(&directory).map_err(io_error)?;
    let mut temp = tempfile::Builder::new()
        .prefix("app-")
        .suffix(".tmp")
        .tempfile_in(&directory)
        .map_err(io_error)?;
    temp.write_all(&emitted.bytes).map_err(io_error)?;
    if let Err(error) = temp.persist(&object)
        && !object.is_file()
    {
        return Err(CliError(format!(
            "failed to publish cached object: {error}"
        )));
    }
    let cache = CacheManifest {
        schema: 1,
        identity: identity.clone(),
        compiler_version: env!("CARGO_PKG_VERSION").into(),
        backend_build_id,
        abi_version: emitted.abi_version,
        llvm_version,
        target_triple,
        optimization: emitted.optimization.as_str().into(),
        profile: profile.into(),
        object_sha256: sha256(&emitted.bytes),
    };
    let encoded = serde_json::to_vec_pretty(&cache)
        .map_err(|error| CliError(format!("encode cache manifest: {error}")))?;
    let mut manifest_temp = tempfile::Builder::new()
        .prefix("manifest-")
        .suffix(".json.tmp")
        .tempfile_in(&directory)
        .map_err(io_error)?;
    manifest_temp.write_all(&encoded).map_err(io_error)?;
    if let Err(error) = manifest_temp.persist(&manifest)
        && !manifest.is_file()
    {
        return Err(CliError(format!(
            "failed to publish cache manifest: {error}"
        )));
    }
    if options.verbose {
        println!("Snacc object rebuilt: {}", selected.entry.display());
    }
    Ok(object)
}

fn backend_build_id() -> String {
    let mut hash = Sha256::new();
    hash.update(include_bytes!("../../../crates/snacc-compiler/src/lib.rs"));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/diagnostics.rs"
    ));
    hash.update(include_bytes!("../../../crates/snacc-compiler/src/ast.rs"));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/lexer/mod.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/parser/mod.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/parser/types.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/parser/expr.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/parser/stmt.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/types.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/checker/mod.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/checker/calls.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/checker/convert.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/checker/expr.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/checker/places.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/checker/program.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/checker/stmt.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/llvm/mod.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/llvm/types.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/llvm/symbols.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/llvm/imports.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/llvm/lower/mod.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/llvm/lower/expr.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/llvm/lower/stmt.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/llvm/lower/equality.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/llvm/lower/cleanup.rs"
    ));
    hash.update(include_bytes!(
        "../../../crates/snacc-compiler/src/llvm/lower/collections.rs"
    ));
    hash.update(include_bytes!("../../../crates/snacc-runtime/src/lib.rs"));
    hash.update(include_bytes!("../../../crates/snacc-runtime/src/print.rs"));
    hash.update(include_bytes!(
        "../../../crates/snacc-runtime/src/string.rs"
    ));
    hash.update(include_bytes!("../../../crates/snacc-runtime/src/view.rs"));
    hash.update(include_bytes!("../../../crates/snacc-runtime/src/list.rs"));
    hash.update(include_bytes!("../../../crates/snacc-runtime/src/map.rs"));
    hash.update(include_bytes!("../../../crates/snacc-runtime/src/set.rs"));
    hash.update(include_bytes!("../../../crates/snacc-runtime/src/fail.rs"));
    format!("{:x}", hash.finalize())
}

#[allow(clippy::too_many_arguments)]
fn cache_matches(
    object: &Path,
    manifest: &Path,
    identity: &str,
    backend_build_id: &str,
    llvm_version: &str,
    target_triple: &str,
    optimization: snacc_compiler::Optimization,
    profile: &str,
) -> Result<bool, CliError> {
    if !object.is_file() || !manifest.is_file() {
        return Ok(false);
    }
    let bytes = fs::read(object).map_err(io_error)?;
    if bytes.is_empty() {
        return Ok(false);
    }
    let encoded = fs::read(manifest).map_err(io_error)?;
    let Ok(cache) = serde_json::from_slice::<CacheManifest>(&encoded) else {
        return Ok(false);
    };
    Ok(cache.schema == 1
        && cache.identity == identity
        && cache.compiler_version == env!("CARGO_PKG_VERSION")
        && cache.backend_build_id == backend_build_id
        && cache.abi_version == snacc_compiler::ABI_VERSION
        && cache.llvm_version == llvm_version
        && cache.target_triple == target_triple
        && cache.optimization == optimization.as_str()
        && cache.profile == profile
        && cache.object_sha256 == sha256(&bytes))
}

fn sha256(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    format!("{:x}", hash.finalize())
}
