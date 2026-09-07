//! Compile-and-execute lifecycle.

use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{MAX_EXECUTION, MAX_STDERR, MAX_STDOUT};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRequest {
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) stdin: String,
}

#[derive(Serialize)]
struct DiagnosticReport {
    phase: String,
    message: String,
    span: Option<SpanReport>,
}

#[derive(Serialize)]
struct SpanReport {
    #[serde(rename = "startByte")]
    start_byte: usize,
    #[serde(rename = "endByte")]
    end_byte: usize,
    start: PositionReport,
    end: PositionReport,
}

#[derive(Serialize)]
pub(crate) struct PositionReport {
    pub(crate) line: usize,
    pub(crate) column: usize,
}

#[derive(Serialize)]
struct CompileReport {
    status: &'static str,
    diagnostics: Vec<DiagnosticReport>,
    #[serde(rename = "durationMs")]
    duration_ms: f64,
}

#[derive(Serialize)]
pub(crate) struct ExecutionReport {
    pub(crate) status: &'static str,
    #[serde(rename = "exitCode")]
    pub(crate) exit_code: Option<i32>,
    #[serde(rename = "stdinBytes")]
    pub(crate) stdin_bytes: usize,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    #[serde(rename = "stdoutTruncated")]
    pub(crate) stdout_truncated: bool,
    #[serde(rename = "stderrTruncated")]
    pub(crate) stderr_truncated: bool,
    #[serde(rename = "durationMs")]
    pub(crate) duration_ms: f64,
}

#[derive(Serialize)]
pub(crate) struct RunResponse {
    compile: CompileReport,
    execution: Option<ExecutionReport>,
    #[serde(rename = "totalMs")]
    total_ms: f64,
}

pub(crate) struct Captured {
    pub(crate) bytes: Vec<u8>,
    pub(crate) truncated: bool,
}

pub(crate) fn run_program(source: &str, stdin: &str) -> RunResponse {
    let total_start = Instant::now();
    let compile_start = Instant::now();
    let built = match snacc_driver::build(source) {
        Ok(built) => built,
        Err(snacc_driver::DriverError::Compile(diagnostics)) => {
            return RunResponse {
                compile: CompileReport {
                    status: "failed",
                    diagnostics: diagnostic_reports(source, &diagnostics),
                    duration_ms: milliseconds(compile_start.elapsed()),
                },
                execution: None,
                total_ms: milliseconds(total_start.elapsed()),
            };
        }
        Err(error) => {
            return RunResponse {
                compile: CompileReport {
                    status: "failed",
                    diagnostics: vec![DiagnosticReport {
                        phase: "build".into(),
                        message: error.to_string(),
                        span: None,
                    }],
                    duration_ms: milliseconds(compile_start.elapsed()),
                },
                execution: None,
                total_ms: milliseconds(total_start.elapsed()),
            };
        }
    };
    let compile_duration = milliseconds(compile_start.elapsed());
    let execution = execute(&built, stdin);
    RunResponse {
        compile: CompileReport {
            status: "succeeded",
            diagnostics: Vec::new(),
            duration_ms: compile_duration,
        },
        execution: Some(execution),
        total_ms: milliseconds(total_start.elapsed()),
    }
}

fn execute(executable: &snacc_driver::BuiltExecutable, stdin: &str) -> ExecutionReport {
    // Split so process/pipe behavior (stdin delivery, threaded draining, timeout
    // kill) is testable against any executable, not only one produced by the
    // Snacc compiler pipeline that `BuiltExecutable` requires.
    run_process(executable.path(), executable.directory(), stdin)
}

pub(crate) fn run_process(path: &Path, working_dir: &Path, stdin: &str) -> ExecutionReport {
    let start = Instant::now();
    let mut command = Command::new(path);
    command
        .current_dir(working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ExecutionReport {
                status: "failed-to-start",
                exit_code: None,
                stdin_bytes: stdin.len(),
                stdout: String::new(),
                stderr: error.to_string(),
                stdout_truncated: false,
                stderr_truncated: false,
                duration_ms: milliseconds(start.elapsed()),
            };
        }
    };

    let stdout = child.stdout.take().expect("spawned child has stdout pipe");
    let stderr = child.stderr.take().expect("spawned child has stdin pipe");
    let child_stdin = child.stdin.take().expect("spawned child has stdin pipe");
    let stdout_limited = Arc::new(AtomicBool::new(false));
    let stderr_limited = Arc::new(AtomicBool::new(false));
    let stdout_limited_for_thread = Arc::clone(&stdout_limited);
    let stderr_limited_for_thread = Arc::clone(&stderr_limited);
    let stdout_thread =
        thread::spawn(move || capture_stream(stdout, MAX_STDOUT, stdout_limited_for_thread));
    let stderr_thread =
        thread::spawn(move || capture_stream(stderr, MAX_STDERR, stderr_limited_for_thread));
    let stdin_bytes = stdin.len();
    let stdin_data = stdin.as_bytes().to_vec();
    let stdin_thread = thread::spawn(move || {
        let mut writer = child_stdin;
        let _ = writer.write_all(&stdin_data);
    });

    let process_status;
    let mut final_status = "exited";
    loop {
        if stdout_limited.load(Ordering::Acquire) || stderr_limited.load(Ordering::Acquire) {
            final_status = "output-limit";
            let _ = child.kill();
            process_status = child.wait().ok();
            break;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                process_status = Some(status);
                break;
            }
            Ok(None) if start.elapsed() >= MAX_EXECUTION => {
                final_status = "timed-out";
                let _ = child.kill();
                process_status = child.wait().ok();
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                final_status = "failed-to-start";
                let _ = child.kill();
                process_status = child.wait().ok();
                break;
            }
        }
    }
    let _ = stdin_thread.join();
    let captured_stdout = stdout_thread.join().unwrap_or(Captured {
        bytes: Vec::new(),
        truncated: false,
    });
    let captured_stderr = stderr_thread.join().unwrap_or(Captured {
        bytes: Vec::new(),
        truncated: false,
    });
    if final_status == "exited" && (captured_stdout.truncated || captured_stderr.truncated) {
        final_status = "output-limit";
    }
    ExecutionReport {
        status: final_status,
        exit_code: process_status.and_then(|status| status.code()),
        stdin_bytes,
        stdout: String::from_utf8_lossy(&captured_stdout.bytes).into_owned(),
        stderr: String::from_utf8_lossy(&captured_stderr.bytes).into_owned(),
        stdout_truncated: captured_stdout.truncated,
        stderr_truncated: captured_stderr.truncated,
        duration_ms: milliseconds(start.elapsed()),
    }
}

pub(crate) fn capture_stream<R: Read>(
    mut reader: R,
    limit: usize,
    limited: Arc<AtomicBool>,
) -> Captured {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    let mut truncated = false;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if bytes.len() < limit {
            let remaining = limit - bytes.len();
            let keep = remaining.min(count);
            bytes.extend_from_slice(&buffer[..keep]);
            if keep < count {
                truncated = true;
                limited.store(true, Ordering::Release);
            }
        } else {
            truncated = true;
            limited.store(true, Ordering::Release);
        }
    }
    Captured { bytes, truncated }
}

fn diagnostic_reports(
    source: &str,
    diagnostics: &snacc_compiler::Diagnostics,
) -> Vec<DiagnosticReport> {
    diagnostics
        .items
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.as_ref().map(|span| SpanReport {
                start_byte: span.start,
                end_byte: span.end,
                start: position(source, span.start),
                end: position(source, span.end),
            });
            DiagnosticReport {
                phase: format!("{:?}", diagnostic.phase).to_ascii_lowercase(),
                message: diagnostic.message.clone(),
                span,
            }
        })
        .collect()
}

pub(crate) fn position(source: &str, offset: usize) -> PositionReport {
    let offset = offset.min(source.len());
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit('\n')
        .next()
        .map(|line| line.chars().count() + 1)
        .unwrap_or(1);
    PositionReport { line, column }
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
