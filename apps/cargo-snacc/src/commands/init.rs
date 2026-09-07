//! The `init` command: scaffold a Snacc application package.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    CliError, HOST_MAIN_TEMPLATE, HOST_MAIN_TEMPLATE_PRE_FERRIS, HOST_MAIN_TEMPLATE_PRE_RFC_007,
    cli::cargo, io_error,
};

pub(crate) fn init(args: &[String]) -> Result<(), CliError> {
    let mut path = PathBuf::from(".");
    let mut name = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--name" => {
                index += 1;
                name = args.get(index).cloned();
                if name.is_none() {
                    return Err(CliError("--name requires a value".into()));
                }
            }
            value if value.starts_with('-') => {
                return Err(CliError(format!("unknown init option '{value}'")));
            }
            value if path != Path::new(".") => {
                return Err(CliError(format!("unexpected init argument '{value}'")));
            }
            value => path = PathBuf::from(value),
        }
        index += 1;
    }
    let mut command = Command::new(cargo());
    command.arg("init").arg(&path).arg("--vcs").arg("none");
    if let Some(name) = &name {
        command.arg("--name").arg(name);
    }
    let status = command
        .status()
        .map_err(|error| CliError(format!("failed to run cargo init: {error}")))?;
    if !status.success() {
        return Err(CliError(format!("cargo init failed with {status}")));
    }
    let root = fs::canonicalize(&path).map_err(io_error)?;
    let manifest = root.join("Cargo.toml");
    let mut cargo_toml = fs::read_to_string(&manifest).map_err(io_error)?;
    if cargo_toml.contains("[package.metadata.snacc]") {
        return Err(CliError(format!(
            "refusing to reinitialize existing Snacc package '{}'",
            root.display()
        )));
    }
    let src = root.join("src");
    let main_nrs = src.join("main.nrs");
    let main_rs = src.join("main.rs");
    let interop_rs = src.join("interop.rs");
    ensure_new_path(&main_nrs)?;
    ensure_new_path(&interop_rs)?;
    ensure_cargo_main_template(&main_rs)?;

    if cargo_toml.contains("[dependencies]") {
        cargo_toml = cargo_toml.replacen(
            "[dependencies]",
            "[dependencies]\nsnacc-runtime = \"0.1\"\nferris-says = \"=0.3.2\"",
            1,
        );
    } else {
        cargo_toml
            .push_str("\n[dependencies]\nsnacc-runtime = \"0.1\"\nferris-says = \"=0.3.2\"\n");
    }
    cargo_toml.push_str(
        "\n[package.metadata.snacc]\nschema-version = 1\nentry = \"src/main.nrs\"\nhost-bin = \"",
    );
    let package_name = name
        .or_else(|| {
            root.file_name()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .ok_or_else(|| CliError("cannot determine package name".into()))?;
    cargo_toml.push_str(&package_name);
    cargo_toml.push_str("\"\n");
    cargo_toml.push_str(
        "\n[lints.rust]\nunexpected_cfgs = { level = \"warn\", check-cfg = ['cfg(snacc_bridge_assertions)'] }\n",
    );
    fs::write(&manifest, cargo_toml).map_err(io_error)?;
    fs::write(&main_nrs, "print(0)\n").map_err(io_error)?;
    fs::write(&main_rs, HOST_MAIN_TEMPLATE).map_err(io_error)?;
    fs::write(
        &interop_rs,
        "// This module exports explicitly selected Rust APIs through Snacc's C-compatible ABI.\n",
    )
    .map_err(io_error)?;
    println!("initialized Snacc package at {}", root.display());
    Ok(())
}

pub(crate) fn ensure_new_path(path: &Path) -> Result<(), CliError> {
    if path.exists() {
        return Err(CliError(format!(
            "refusing to overwrite existing file '{}'",
            path.display()
        )));
    }
    Ok(())
}

pub(crate) fn ensure_cargo_main_template(path: &Path) -> Result<(), CliError> {
    if path.is_file() {
        let existing = fs::read_to_string(path).map_err(io_error)?;
        let trimmed = existing.trim();
        let known = [
            "fn main() {\n    println!(\"Hello, world!\");\n}",
            HOST_MAIN_TEMPLATE_PRE_RFC_007.trim(),
            HOST_MAIN_TEMPLATE_PRE_FERRIS.trim(),
            HOST_MAIN_TEMPLATE.trim(),
        ];
        if !known.contains(&trimmed) {
            return Err(CliError(format!(
                "refusing to overwrite non-template Rust host '{}'",
                path.display()
            )));
        }
    }
    Ok(())
}
