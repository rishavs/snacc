//! `cargo-snacc`: process entry and error reporting.
//!
//! Option parsing lives in [`cli`], package selection in [`metadata`],
//! subcommands in [`commands`], the object cache in [`cache`], and bridge
//! assertion rendering in [`bridge`].

use snacc_compiler::Diagnostics;
use std::{env, path::Path};

pub(crate) mod bridge;
pub(crate) mod cache;
pub(crate) mod cli;
pub(crate) mod commands;
pub(crate) mod metadata;

const HOST_MAIN_TEMPLATE: &str = include_str!("templates/host.rs");

const HOST_MAIN_TEMPLATE_PRE_FERRIS: &str = include_str!("templates/host_pre_ferris.rs");

const HOST_MAIN_TEMPLATE_PRE_RFC_007: &str = include_str!("templates/host_pre_rfc_007.rs");

#[derive(Debug)]
pub(crate) struct CliError(pub(crate) String);

fn main() {
    if let Err(error) = cli::run(env::args().skip(1).collect()) {
        eprintln!("cargo-snacc: {}", error.0);
        std::process::exit(1);
    }
}

pub(crate) fn diagnostic_error(path: &Path, source: &str, diagnostics: &Diagnostics) -> CliError {
    let mut rendered = String::new();
    for diagnostic in &diagnostics.items {
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        rendered.push_str(&path.display().to_string());
        if let Some(span) = &diagnostic.span {
            let (line, column) = line_column(source, span.start);
            rendered.push_str(&format!(":{line}:{column}"));
        }
        rendered.push_str(&format!(
            ": {:?} error: {}",
            diagnostic.phase, diagnostic.message
        ));
    }
    if rendered.is_empty() {
        rendered.push_str("Snacc compilation failed without a diagnostic");
    }
    CliError(rendered)
}

pub(crate) fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let mut bounded = offset.min(source.len());
    while bounded > 0 && !source.is_char_boundary(bounded) {
        bounded -= 1;
    }
    let prefix = &source[..bounded];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, tail)| tail)
        .chars()
        .count()
        + 1;
    (line, column)
}

pub(crate) fn io_error(error: std::io::Error) -> CliError {
    CliError(error.to_string())
}

#[cfg(test)]
mod tests;
