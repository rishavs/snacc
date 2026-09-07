//! The `check`, `build`, `run`, `test`, and `clean` commands.

use std::{fs, process::Command};

use crate::{
    CliError,
    bridge::{apply_bridge_assertions, prepare_bridge_assertions},
    cache::emit_cached,
    cli::{
        append_cargo_options, append_metadata_options, cargo, cargo_executable, parse_options,
        reject_extra, render_cargo_messages, run_forwarded,
    },
    diagnostic_error, io_error,
    metadata::select_package,
};

pub(crate) fn check_command(args: &[String]) -> Result<(), CliError> {
    let parsed = parse_options(args)?;
    reject_extra(&parsed, "check")?;
    let options = parsed.options;
    if options.release || options.profile.is_some() {
        return Err(CliError(
            "check does not support --release or --profile; use build --release to check in release mode".into(),
        ));
    }
    let selected = select_package(options.package.as_deref(), Some(&options))?;
    let source = fs::read_to_string(&selected.entry).map_err(io_error)?;
    let checked = snacc_compiler::check(&source)
        .map_err(|diagnostics| diagnostic_error(&selected.entry, &source, &diagnostics))?;
    let assertions = prepare_bridge_assertions(&selected, &checked, &source)?;
    let mut command = Command::new(cargo());
    command
        .arg("rustc")
        .arg("--profile")
        .arg("check")
        .arg("--manifest-path")
        .arg(&selected.package.manifest_path)
        .arg("--package")
        .arg(&selected.package.name)
        .arg("--bin")
        .arg(&selected.host_bin);
    append_metadata_options(&mut command, &options);
    command.arg("--");
    apply_bridge_assertions(&mut command, &assertions);
    run_forwarded(command, "cargo check")
}

pub(crate) fn build_command(args: &[String], run_program: bool) -> Result<(), CliError> {
    let parsed = parse_options(args)?;
    if !parsed.positionals.is_empty() {
        return Err(CliError(format!(
            "unexpected argument '{}'",
            parsed.positionals[0]
        )));
    }
    if !run_program && !parsed.trailing.is_empty() {
        return Err(CliError(
            "build does not accept arguments after '--'".into(),
        ));
    }
    let options = parsed.options;
    let program_args = parsed.trailing;
    let selected = select_package(options.package.as_deref(), Some(&options))?;
    let source = fs::read_to_string(&selected.entry).map_err(io_error)?;
    let checked = snacc_compiler::check(&source)
        .map_err(|diagnostics| diagnostic_error(&selected.entry, &source, &diagnostics))?;
    let assertions = prepare_bridge_assertions(&selected, &checked, &source)?;
    let object = emit_cached(&selected, &source, &options)?;

    let mut command = Command::new(cargo());
    command
        .arg("rustc")
        .arg("--message-format=json-render-diagnostics")
        .arg("--manifest-path")
        .arg(&selected.package.manifest_path)
        .arg("--package")
        .arg(&selected.package.name)
        .arg("--bin")
        .arg(&selected.host_bin);
    append_cargo_options(&mut command, &options);
    command.arg("--");
    apply_bridge_assertions(&mut command, &assertions);
    command
        .arg("-C")
        .arg(format!("link-arg={}", object.display()));
    let output = command
        .output()
        .map_err(|error| CliError(format!("failed to run cargo rustc: {error}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{stderr}");
    render_cargo_messages(&stdout);
    if !output.status.success() {
        return Err(CliError(format!(
            "cargo rustc failed with {}",
            output.status
        )));
    }
    let executable = cargo_executable(&stdout, &selected, &selected.host_bin, "bin")
        .ok_or_else(|| CliError("cargo produced no executable artifact".into()))?;
    println!("Snacc executable: {}", executable.display());
    if run_program {
        let status = Command::new(&executable)
            .args(program_args)
            .status()
            .map_err(io_error)?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
    }
    Ok(())
}

pub(crate) fn clean_command(args: &[String]) -> Result<(), CliError> {
    let parsed = parse_options(args)?;
    reject_extra(&parsed, "clean")?;
    let options = parsed.options;
    if options.all {
        let mut command = Command::new(cargo());
        command.arg("clean");
        if let Some(package) = options.package {
            command.arg("--package").arg(package);
        }
        return run_forwarded(command, "cargo clean");
    }
    let selected = select_package(options.package.as_deref(), Some(&options))?;
    let state = selected.target_directory.join("snacc");
    if state.exists() {
        fs::remove_dir_all(&state).map_err(io_error)?;
    }
    println!("removed Snacc artifacts from {}", state.display());
    Ok(())
}

pub(crate) fn test_command(args: &[String]) -> Result<(), CliError> {
    let parsed = parse_options(args)?;
    if parsed.positionals.len() > 1 {
        return Err(CliError("test accepts at most one filter".into()));
    }
    let filter = parsed.positionals.first().cloned();
    let test_args = parsed.trailing;
    let options = parsed.options;
    let selected = select_package(options.package.as_deref(), Some(&options))?;
    let source = fs::read_to_string(&selected.entry).map_err(io_error)?;
    let checked = snacc_compiler::check(&source)
        .map_err(|diagnostics| diagnostic_error(&selected.entry, &source, &diagnostics))?;
    let assertions = prepare_bridge_assertions(&selected, &checked, &source)?;
    let object = emit_cached(&selected, &source, &options)?;

    let mut command = Command::new(cargo());
    command
        .arg("rustc")
        .arg("--message-format=json-render-diagnostics")
        .arg("--manifest-path")
        .arg(&selected.package.manifest_path)
        .arg("--package")
        .arg(&selected.package.name)
        .arg("--bin")
        .arg(&selected.host_bin);
    append_cargo_options(&mut command, &options);
    command.arg("--");
    apply_bridge_assertions(&mut command, &assertions);
    command
        .arg("--test")
        .arg("-C")
        .arg(format!("link-arg={}", object.display()));
    let output = command
        .output()
        .map_err(|error| CliError(format!("failed to build Snacc integration test: {error}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    print!("{}", String::from_utf8_lossy(&output.stderr));
    render_cargo_messages(&stdout);
    if !output.status.success() {
        return Err(CliError(format!(
            "Snacc integration test build failed with {}",
            output.status
        )));
    }
    let executable = cargo_executable(&stdout, &selected, &selected.host_bin, "bin")
        .ok_or_else(|| CliError("Cargo produced no Snacc host test executable".into()))?;
    let mut test = Command::new(executable);
    if let Some(filter) = filter {
        test.arg(filter);
    }
    test.args(test_args);
    let status = test.status().map_err(io_error)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError(format!(
            "Snacc integration tests failed with {status}"
        )))
    }
}
