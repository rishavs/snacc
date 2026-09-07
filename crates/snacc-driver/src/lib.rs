use snacc_compiler::Diagnostics;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::{Builder, TempDir};

const RUNTIME_SOURCE: &str = include_str!("../../snacc-runtime/src/lib.rs");

/// One entry per runtime module beside `lib.rs` (Specification 029 Phase 5).
/// The generated host compiles beside these files so its `mod` declarations
/// resolve exactly as they do in the Cargo build.
const RUNTIME_MODULES: &[(&str, &str)] = &[
    ("print.rs", include_str!("../../snacc-runtime/src/print.rs")),
    (
        "string.rs",
        include_str!("../../snacc-runtime/src/string.rs"),
    ),
    ("view.rs", include_str!("../../snacc-runtime/src/view.rs")),
    ("list.rs", include_str!("../../snacc-runtime/src/list.rs")),
    ("map.rs", include_str!("../../snacc-runtime/src/map.rs")),
    ("set.rs", include_str!("../../snacc-runtime/src/set.rs")),
    ("fail.rs", include_str!("../../snacc-runtime/src/fail.rs")),
];
const HOST_SUFFIX: &str = r#"

unsafe extern "C" {
    fn snacc_main() -> i32;
}

fn main() {
    force_link();
    // SAFETY: the linked Snacc object defines snacc_main with this ABI.
    let status = unsafe { snacc_main() };
    std::process::exit(status);
}
"#;

#[derive(Debug)]
pub enum DriverError {
    Compile(Diagnostics),
    Filesystem(String),
    Tool(String),
}

impl fmt::Display for DriverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile(diagnostics) => {
                write!(formatter, "compiler diagnostics: {diagnostics:?}")
            }
            Self::Filesystem(message) | Self::Tool(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for DriverError {}

pub struct BuiltExecutable {
    directory: TempDir,
    path: PathBuf,
}

impl BuiltExecutable {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn directory(&self) -> &Path {
        self.directory.path()
    }
}

/// Locates the `paste` proc-macro artifact the workspace build produced, so
/// the temporary host crate expands the runtime's macros with the same
/// toolchain that built this driver. Searches `deps` directories beside the
/// running executable upward through the Cargo target layout.
fn paste_library() -> Option<PathBuf> {
    const EXTENSIONS: &[&str] = if cfg!(windows) {
        &["dll"]
    } else if cfg!(target_os = "macos") {
        &["dylib"]
    } else {
        &["so"]
    };
    const PREFIX: &str = if cfg!(windows) { "paste-" } else { "libpaste-" };
    let mut directory = std::env::current_exe().ok()?;
    directory.pop();
    for _ in 0..5 {
        for candidate in [directory.join("deps"), directory.clone()] {
            if let Ok(entries) = fs::read_dir(&candidate) {
                let mut hits: Vec<PathBuf> = entries
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .filter(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| {
                                name.starts_with(PREFIX)
                                    && EXTENSIONS.iter().any(|extension| name.ends_with(extension))
                            })
                    })
                    .collect();
                hits.sort();
                if let Some(hit) = hits.into_iter().next() {
                    return Some(hit);
                }
            }
        }
        if !directory.pop() {
            return None;
        }
    }
    None
}

pub fn build(source: &str) -> Result<BuiltExecutable, DriverError> {
    let directory = Builder::new()
        .prefix("snacc-build-")
        .tempdir()
        .map_err(|error| {
            DriverError::Filesystem(format!("failed to create native build directory: {error}"))
        })?;
    let object_path = directory.path().join("program.o");
    let host_path = directory.path().join("host.rs");
    let executable_path = directory
        .path()
        .join(format!("program{}", std::env::consts::EXE_SUFFIX));

    let emitted = snacc_compiler::emit_object(source).map_err(DriverError::Compile)?;
    fs::write(&object_path, emitted).map_err(|error| {
        DriverError::Filesystem(format!("failed to write native object: {error}"))
    })?;
    let abi_assertion = format!(
        "\nconst _: () = assert!(ABI_VERSION == {}, \"snacc compiler/runtime ABI version mismatch\");\n",
        snacc_compiler::ABI_VERSION
    );
    fs::write(
        &host_path,
        format!("{RUNTIME_SOURCE}{abi_assertion}{HOST_SUFFIX}"),
    )
    .map_err(|error| DriverError::Filesystem(format!("failed to write generated host: {error}")))?;
    for &(name, source) in RUNTIME_MODULES {
        fs::write(directory.path().join(name), source).map_err(|error| {
            DriverError::Filesystem(format!("failed to write runtime module '{name}': {error}"))
        })?;
    }
    let paste = paste_library().ok_or_else(|| {
        DriverError::Tool(
            "the Snacc runtime macros need the workspace's paste artifact beside this driver; build through Cargo so it exists".into(),
        )
    })?;

    let status = Command::new("rustc")
        .arg("--edition=2024")
        .arg(&host_path)
        .arg("--extern")
        .arg(format!("paste={}", paste.display()))
        .arg("-C")
        .arg(format!("link-arg={}", object_path.display()))
        .arg("-o")
        .arg(&executable_path)
        .status()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DriverError::Tool(
                    "rustc was not found on PATH; install a Rust toolchain to link Snacc programs"
                        .into(),
                )
            } else {
                DriverError::Tool(format!("failed to run rustc: {error}"))
            }
        })?;
    if !status.success() {
        return Err(DriverError::Tool(format!(
            "linking with 'rustc' failed (exit {status})"
        )));
    }

    Ok(BuiltExecutable {
        directory,
        path: executable_path,
    })
}

pub fn build_to(source: &str, output: &Path) -> Result<(), DriverError> {
    let executable = build(source)?;
    fs::copy(executable.path(), output).map_err(|error| {
        DriverError::Filesystem(format!(
            "failed to place executable at '{}': {error}",
            output.display()
        ))
    })?;
    Ok(())
}
