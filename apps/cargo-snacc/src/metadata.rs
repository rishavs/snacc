//! Package selection and Snacc metadata.
//!
//! `cargo metadata` output is parsed with the canonical `cargo_metadata`
//! types and converted once into the local selection shapes, so the rest of
//! the application (and its tests) keeps working with plain paths and
//! strings. `Selected` and the canonical-path rules are unchanged.

use cargo_metadata::Metadata;
use serde::Deserialize;
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    CliError,
    cli::{Options, append_metadata_options, cargo},
    io_error,
};

/// A selected package in plain standard-library types. Converted once from
/// `cargo_metadata` output by [`select_package_optional`]; everything
/// downstream keeps using paths and strings exactly as before.
// Retained whole even where fields are only read by tests and the selection
// boundary: the shape mirrors `cargo_metadata` output one-to-one.
#[allow(dead_code)]
pub(crate) struct Package {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) manifest_path: PathBuf,
    pub(crate) targets: Vec<Target>,
    pub(crate) metadata: Option<Value>,
}

/// A cargo target in plain standard-library types. Converted alongside
/// [`Package`]; `kind` keeps the raw kind strings (`"bin"`, ...).
// Retained whole for the same reason as [`Package`].
#[allow(dead_code)]
pub(crate) struct Target {
    pub(crate) name: String,
    pub(crate) kind: Vec<String>,
    pub(crate) src_path: PathBuf,
}

pub(crate) struct Selected {
    pub(crate) package: Package,
    pub(crate) package_id: String,
    /// Canonicalized package root. `entry` and `host_src_path` are canonical
    /// too, so this is the only prefix that can be stripped from them; the
    /// manifest path from Cargo metadata is not canonical and does not match.
    pub(crate) package_root: PathBuf,
    pub(crate) entry: PathBuf,
    pub(crate) host_bin: String,
    pub(crate) host_src_path: PathBuf,
    pub(crate) target_directory: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct SnaccMetadata {
    schema_version: u32,
    entry: String,
    host_bin: String,
}

pub(crate) fn select_package(
    requested: Option<&str>,
    options: Option<&Options>,
) -> Result<Selected, CliError> {
    select_package_optional(requested, options)?
        .ok_or_else(|| CliError("package.metadata.snacc is missing".into()))
}

pub(crate) fn select_package_optional(
    requested: Option<&str>,
    options: Option<&Options>,
) -> Result<Option<Selected>, CliError> {
    let root = env::current_dir().map_err(io_error)?;
    let mut command = Command::new(cargo());
    command
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--no-deps")
        .current_dir(&root);
    if let Some(options) = options {
        append_metadata_options(&mut command, options);
    }
    let output = command
        .output()
        .map_err(|error| CliError(format!("failed to run cargo metadata: {error}")))?;
    if !output.status.success() {
        return Err(CliError(
            String::from_utf8_lossy(&output.stderr).trim().into(),
        ));
    }
    let metadata: Metadata = serde_json::from_slice(&output.stdout)
        .map_err(|error| CliError(format!("invalid cargo metadata JSON: {error}")))?;
    let target_directory = metadata.target_directory.clone().into_std_path_buf();
    let package = if let Some(name) = requested {
        metadata
            .packages
            .into_iter()
            .find(|package| package.name == name)
            .ok_or_else(|| CliError(format!("package '{name}' was not found")))?
    } else {
        let canonical = fs::canonicalize(&root).map_err(io_error)?;
        let mut packages = metadata.packages;
        let exact = packages.iter().position(|package| {
            package.manifest_path.parent().is_some_and(|parent| {
                fs::canonicalize(parent)
                    .ok()
                    .is_some_and(|parent| parent == canonical)
            })
        });
        let mut matches = if let Some(index) = exact {
            vec![packages.swap_remove(index)]
        } else {
            packages
                .into_iter()
                .filter(|package| {
                    package
                        .manifest_path
                        .parent()
                        .and_then(|parent| fs::canonicalize(parent).ok())
                        .is_some_and(|parent| canonical.starts_with(parent))
                })
                .collect::<Vec<_>>()
        };
        if matches.len() != 1 {
            return Err(CliError(
                "package selection is ambiguous; use --package".into(),
            ));
        }
        matches.remove(0)
    };
    let Some(metadata_value) = package.metadata.get("snacc") else {
        return Ok(None);
    };
    let snacc_metadata: SnaccMetadata = serde_json::from_value(metadata_value.clone())
        .map_err(|error| CliError(format!("invalid package.metadata.snacc: {error}")))?;
    if snacc_metadata.schema_version != 1 {
        return Err(CliError(format!(
            "unsupported package.metadata.snacc schema-version {}",
            snacc_metadata.schema_version
        )));
    }
    let entry = snacc_metadata.entry;
    let host_bin = snacc_metadata.host_bin;
    let package_root = package
        .manifest_path
        .parent()
        .ok_or_else(|| CliError("package manifest has no parent".into()))?;
    let package_root = fs::canonicalize(package_root).map_err(io_error)?;
    let entry = fs::canonicalize(package_root.join(&entry)).map_err(io_error)?;
    if !entry.starts_with(&package_root) || !entry.is_file() {
        return Err(CliError("Snacc entry must be a package-owned file".into()));
    }
    let host_target = package
        .targets
        .iter()
        .filter(|target| target.name == host_bin && target.kind.iter().any(|kind| kind == "bin"))
        .collect::<Vec<_>>();
    if host_target.len() != 1 {
        return Err(CliError(format!(
            "host binary '{host_bin}' does not resolve to exactly one binary target"
        )));
    }
    let host_src_path = host_target[0].src_path.clone().into_std_path_buf();
    let package_id = package.id.to_string();
    let package = Package {
        id: package_id.clone(),
        name: package.name,
        manifest_path: package.manifest_path.into_std_path_buf(),
        targets: package
            .targets
            .into_iter()
            .map(|target| Target {
                name: target.name,
                kind: target
                    .kind
                    .into_iter()
                    .map(|kind| kind.to_string())
                    .collect(),
                src_path: target.src_path.into_std_path_buf(),
            })
            .collect(),
        metadata: match package.metadata {
            Value::Null => None,
            value => Some(value),
        },
    };
    Ok(Some(Selected {
        package,
        package_id,
        package_root,
        entry,
        host_bin,
        host_src_path,
        target_directory,
    }))
}

pub(crate) fn find_manifest(start: &Path) -> Option<PathBuf> {
    let mut directory = Some(start);
    while let Some(path) = directory {
        let manifest = path.join("Cargo.toml");
        if manifest.is_file() {
            return Some(manifest);
        }
        directory = path.parent();
    }
    None
}

pub(crate) fn package_relative_entry(selected: &Selected) -> &Path {
    selected
        .entry
        .strip_prefix(&selected.package_root)
        .unwrap_or(&selected.entry)
}
