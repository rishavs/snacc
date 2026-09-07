//! The `doctor` command: verify the host toolchain.

use std::{
    env, fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use crate::{
    CliError,
    cli::{cargo, command_output, command_version},
    io_error,
    metadata::{find_manifest, select_package_optional},
};

pub(crate) fn doctor() -> Result<(), CliError> {
    let mut failures = Vec::new();
    if let Err(error) = command_version(&cargo()) {
        failures.push(error.0);
    }
    let rustc = command_output("rustc", &["-vV"]);
    let rustc_host = match rustc {
        Ok(output) => {
            println!(
                "{}",
                output.lines().next().unwrap_or("rustc version unavailable")
            );
            output
                .lines()
                .find_map(|line| line.strip_prefix("host: "))
                .map(str::to_owned)
        }
        Err(error) => {
            failures.push(error.0);
            None
        }
    };
    let llvm_target = snacc_compiler::target_triple();
    if let Some(rustc_host) = rustc_host
        && rustc_host != llvm_target
    {
        failures.push(format!(
            "rustc host '{rustc_host}' does not match LLVM target '{llvm_target}'"
        ));
    }
    let llvm_version = snacc_compiler::llvm_version();
    if llvm_version != (22, 1, 8) {
        failures.push(format!(
            "loaded LLVM version is {}.{}.{}, expected 22.1.8",
            llvm_version.0, llvm_version.1, llvm_version.2
        ));
    }
    let dll = if let Some(prefix) = env::var_os("LLVM_SYS_221_PREFIX") {
        PathBuf::from(prefix).join("bin/LLVM-C.dll")
    } else {
        env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join("LLVM-C.dll")))
            .unwrap_or_else(|| PathBuf::from("LLVM-C.dll"))
    };
    if !dll.is_file() {
        failures.push(format!("LLVM runtime DLL is missing: {}", dll.display()));
    } else {
        println!("LLVM runtime: {}", dll.display());
        #[cfg(windows)]
        match loaded_llvm_path() {
            Ok(loaded) => match (fs::canonicalize(&dll), fs::canonicalize(&loaded)) {
                (Ok(expected), Ok(loaded)) => {
                    if !expected
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&loaded.to_string_lossy())
                    {
                        failures.push(format!(
                            "loaded LLVM runtime '{}' does not match selected '{}'",
                            loaded.display(),
                            expected.display()
                        ));
                    }
                }
                (Err(error), _) => failures.push(format!(
                    "cannot resolve selected LLVM runtime '{}': {error}",
                    dll.display()
                )),
                (_, Err(error)) => failures.push(format!(
                    "cannot resolve loaded LLVM runtime '{}': {error}",
                    loaded.display()
                )),
            },
            Err(error) => failures.push(error.0),
        }
    }
    if let Err(error) = probe_linker() {
        failures.push(error.0);
    }
    if let Some(manifest) = find_manifest(&env::current_dir().map_err(io_error)?) {
        let manifest_source = fs::read_to_string(&manifest).map_err(io_error)?;
        if manifest_source.contains("[package.metadata.snacc]") {
            match select_package_optional(None, None) {
                Ok(Some(_)) => {}
                Ok(None) => failures.push("package.metadata.snacc is missing".into()),
                Err(error) => failures.push(error.0),
            }
        } else {
            println!(
                "doctor: current Cargo package is not a Snacc application; package checks skipped"
            );
        }
    }
    if failures.is_empty() {
        println!("doctor: all checks passed");
        Ok(())
    } else {
        Err(CliError(format!(
            "doctor found {} problem(s):\n- {}",
            failures.len(),
            failures.join("\n- ")
        )))
    }
}

fn probe_linker() -> Result<(), CliError> {
    let directory = tempfile::Builder::new()
        .prefix("snacc-doctor-")
        .tempdir()
        .map_err(io_error)?;
    let output = directory.path().join(if cfg!(windows) {
        "link-probe.exe"
    } else {
        "link-probe"
    });
    let mut child = Command::new("rustc")
        .arg("-")
        .arg("--crate-name")
        .arg("snacc_link_probe")
        .arg("-o")
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CliError(format!("failed to start native linker probe: {error}")))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| CliError("native linker probe has no stdin".into()))?
        .write_all(b"fn main() {}\n")
        .map_err(io_error)?;
    let output = child
        .wait_with_output()
        .map_err(|error| CliError(format!("failed to wait for native linker probe: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CliError(format!(
            "native linker probe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(windows)]
fn loaded_llvm_path() -> Result<PathBuf, CliError> {
    use std::ffi::{OsStr, c_void};
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetModuleFileNameW(module: *mut c_void, path: *mut u16, size: u32) -> u32;
    }

    let name = OsStr::new("LLVM-C.dll")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut path = vec![0u16; 32768];
    // `doctor` queries LLVM before this probe, so the module handle is live;
    // these calls only borrow it and initialize at most `path.len()` units.
    let length = unsafe {
        let module = GetModuleHandleW(name.as_ptr());
        if module.is_null() {
            return Err(CliError("LLVM-C.dll is not loaded in this process".into()));
        }
        GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32)
    };
    if length == 0 || length as usize == path.len() {
        return Err(CliError(
            "Windows could not resolve the loaded LLVM-C.dll path".into(),
        ));
    }
    path.truncate(length as usize);
    Ok(PathBuf::from(String::from_utf16_lossy(&path)))
}
