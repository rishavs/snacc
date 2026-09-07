//! Option parsing and command dispatch.

use serde_json::Value;
use std::{env, path::PathBuf, process::Command};

use crate::{CliError, metadata::Selected};

#[derive(Default)]
pub(crate) struct Options {
    pub(crate) package: Option<String>,
    pub(crate) release: bool,
    pub(crate) profile: Option<String>,
    pub(crate) features: Vec<String>,
    pub(crate) all_features: bool,
    pub(crate) no_default_features: bool,
    pub(crate) locked: bool,
    pub(crate) frozen: bool,
    pub(crate) offline: bool,
    pub(crate) verbose: bool,
    pub(crate) all: bool,
}

pub(crate) struct ParsedOptions {
    pub(crate) options: Options,
    pub(crate) positionals: Vec<String>,
    pub(crate) trailing: Vec<String>,
}

pub(crate) fn run(args: Vec<String>) -> Result<(), CliError> {
    let args = args.strip_prefix(&[String::from("snacc")]).unwrap_or(&args);
    let Some(command) = args.first().map(String::as_str) else {
        return Err(CliError(
            "expected one of init, check, build, run, test, clean, doctor".into(),
        ));
    };
    match command {
        "init" => crate::commands::init::init(&args[1..]),
        "check" => crate::commands::build::check_command(&args[1..]),
        "build" => crate::commands::build::build_command(&args[1..], false),
        "run" => crate::commands::build::build_command(&args[1..], true),
        "test" => crate::commands::build::test_command(&args[1..]),
        "clean" => crate::commands::build::clean_command(&args[1..]),
        "doctor" => crate::commands::doctor::doctor(),
        _ => Err(CliError(format!("unknown command '{command}'"))),
    }
}

pub(crate) fn cargo() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

pub(crate) fn parse_options(args: &[String]) -> Result<ParsedOptions, CliError> {
    let mut options = Options::default();
    let mut positionals = Vec::new();
    let mut trailing = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            trailing.extend_from_slice(&args[index + 1..]);
            break;
        }
        match arg.as_str() {
            "--release" => options.release = true,
            "--all-features" => options.all_features = true,
            "--no-default-features" => options.no_default_features = true,
            "--locked" => options.locked = true,
            "--frozen" => options.frozen = true,
            "--offline" => options.offline = true,
            "--all" => options.all = true,
            "--verbose" | "-v" => options.verbose = true,
            "--package" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(CliError("--package requires a value".into()));
                };
                if value.starts_with('-') {
                    return Err(CliError("--package requires a package name".into()));
                }
                options.package = Some(value.clone());
            }
            "--profile" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(CliError("--profile requires a value".into()));
                };
                if value.starts_with('-') {
                    return Err(CliError("--profile requires a profile name".into()));
                }
                options.profile = Some(value.clone());
            }
            "--features" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(CliError("--features requires a value".into()));
                };
                if value.starts_with('-') {
                    return Err(CliError("--features requires a feature list".into()));
                }
                options.features = value.split(',').map(str::to_owned).collect();
            }
            value if value.starts_with('-') => {
                return Err(CliError(format!("unknown option '{value}'")));
            }
            value => positionals.push(value.to_owned()),
        }
        index += 1;
    }
    if options.release && options.profile.is_some() {
        return Err(CliError(
            "--release and --profile are mutually exclusive".into(),
        ));
    }
    if let Some(profile) = &options.profile
        && profile != "dev"
        && profile != "release"
    {
        return Err(CliError(format!("unsupported Cargo profile '{profile}'")));
    }
    Ok(ParsedOptions {
        options,
        positionals,
        trailing,
    })
}

pub(crate) fn reject_extra(parsed: &ParsedOptions, command: &str) -> Result<(), CliError> {
    if let Some(value) = parsed.positionals.first() {
        return Err(CliError(format!("unexpected {command} argument '{value}'")));
    }
    if !parsed.trailing.is_empty() {
        return Err(CliError(format!(
            "{command} does not accept arguments after '--'"
        )));
    }
    Ok(())
}

pub(crate) fn append_cargo_options(command: &mut Command, options: &Options) {
    if options.release {
        command.arg("--release");
    }
    if let Some(profile) = &options.profile {
        command.arg("--profile").arg(profile);
    }
    if options.all_features {
        command.arg("--all-features");
    }
    if options.no_default_features {
        command.arg("--no-default-features");
    }
    if !options.features.is_empty() {
        command.arg("--features").arg(options.features.join(","));
    }
    if options.locked {
        command.arg("--locked");
    }
    if options.frozen {
        command.arg("--frozen");
    }
    if options.offline {
        command.arg("--offline");
    }
}

pub(crate) fn append_metadata_options(command: &mut Command, options: &Options) {
    if options.all_features {
        command.arg("--all-features");
    }
    if options.no_default_features {
        command.arg("--no-default-features");
    }
    if !options.features.is_empty() {
        command.arg("--features").arg(options.features.join(","));
    }
    if options.locked {
        command.arg("--locked");
    }
    if options.frozen {
        command.arg("--frozen");
    }
    if options.offline {
        command.arg("--offline");
    }
}

pub(crate) fn cargo_executable(
    stdout: &str,
    selected: &Selected,
    target_name: &str,
    target_kind: &str,
) -> Option<PathBuf> {
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let package_matches = value
            .get("package_id")
            .and_then(Value::as_str)
            .is_some_and(|package_id| package_id == selected.package_id);
        let target = value.get("target");
        let name_matches = target
            .and_then(|target| target.get("name"))
            .and_then(Value::as_str)
            .is_some_and(|name| name == target_name);
        let kind_matches = target
            .and_then(|target| target.get("kind"))
            .and_then(Value::as_array)
            .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some(target_kind)));
        if package_matches && name_matches && kind_matches {
            return value
                .get("executable")
                .and_then(Value::as_str)
                .map(PathBuf::from);
        }
    }
    None
}

pub(crate) fn render_cargo_messages(stdout: &str) {
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            if !line.trim().is_empty() {
                eprintln!("{line}");
            }
            continue;
        };
        if value.get("reason").and_then(Value::as_str) != Some("compiler-message") {
            continue;
        }
        if let Some(rendered) = value
            .get("message")
            .and_then(|message| message.get("rendered"))
            .and_then(Value::as_str)
        {
            eprint!("{rendered}");
        }
    }
}

pub(crate) fn run_forwarded(mut command: Command, label: &str) -> Result<(), CliError> {
    let status = command
        .status()
        .map_err(|error| CliError(format!("failed to run {label}: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError(format!("{label} failed with {status}")))
    }
}

pub(crate) fn command_version(name: &str) -> Result<(), CliError> {
    let status = Command::new(name)
        .arg("--version")
        .status()
        .map_err(|error| CliError(format!("{name} is unavailable: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError(format!("{name} --version failed with {status}")))
    }
}

pub(crate) fn command_output(name: &str, args: &[&str]) -> Result<String, CliError> {
    let output = Command::new(name)
        .args(args)
        .output()
        .map_err(|error| CliError(format!("{name} is unavailable: {error}")))?;
    if !output.status.success() {
        return Err(CliError(format!(
            "{name} {} failed with {}: {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
