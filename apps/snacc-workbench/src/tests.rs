use super::*;
use crate::{
    routes::{constant_time_eq, handle_connection},
    run::{capture_stream, position, run_process},
};
use std::{
    fs,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::PathBuf,
    process::Command,
};
use std::{sync::atomic::Ordering, time::Duration};

/// Serializes tests that invoke a real `rustc` compile+link (through
/// `snacc_driver::build()` or a hand-compiled process-test helper).
/// Running several of these concurrently overloads the linker on a
/// modest machine, which surfaces as spurious "failed-to-start" or
/// compile-tool failures unrelated to the behavior under test.
static BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn request(raw: String, token: &str) -> String {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let raw = raw.replace("127.0.0.1:0", &format!("127.0.0.1:{}", address.port()));
    let shared = Shared {
        page: PAGE.replace("__SNACC_SESSION_TOKEN__", token),
        snippets: SNIPPETS.to_string(),
        token: token.to_string(),
        address,
        active: Arc::new(AtomicBool::new(false)),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        handle_connection(stream, &shared).unwrap();
    });
    let mut client = TcpStream::connect(address).unwrap();
    client.write_all(raw.as_bytes()).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    server.join().unwrap();
    response
}

#[test]
fn positions_are_one_based_and_use_exact_source_bytes() {
    let source = "α\nprint(1)";
    assert_eq!(position(source, 0).line, 1);
    assert_eq!(position(source, 2).line, 1);
    assert_eq!(position(source, 3).line, 2);
    assert_eq!(position(source, source.len()).column, 9);
}

#[test]
fn capture_stream_bounds_output_and_continues_draining() {
    let limited = Arc::new(AtomicBool::new(false));
    let captured = capture_stream(std::io::Cursor::new(b"abcdef"), 3, Arc::clone(&limited));
    assert_eq!(captured.bytes, b"abc");
    assert!(captured.truncated);
    assert!(limited.load(Ordering::Acquire));
}

#[test]
fn html_and_snippet_routes_serve_embedded_content() {
    let token = "a".repeat(64);
    let html = request(
        format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:0\r\n\r\n"),
        &token,
    );
    assert!(html.contains("200 OK"));
    assert!(html.contains("Snacc Workbench"));

    let response = request(
        format!(
            "GET /api/snippets HTTP/1.1\r\nHost: 127.0.0.1:0\r\nX-Snacc-Session: {}\r\n\r\n",
            token
        ),
        &token,
    );
    assert!(response.contains("200 OK"));
    assert!(response.contains("arithmetic"));
}

#[test]
fn constant_time_eq_matches_naive_equality() {
    assert!(constant_time_eq("same-token", "same-token"));
    assert!(!constant_time_eq("token-a", "token-b"));
    assert!(!constant_time_eq("short", "longer-value"));
    assert!(constant_time_eq("", ""));
}

#[test]
fn missing_session_token_is_rejected() {
    let token = "c".repeat(64);
    let response = request(
        "GET /api/snippets HTTP/1.1\r\nHost: 127.0.0.1:0\r\n\r\n".to_string(),
        &token,
    );
    assert!(response.contains("403 Forbidden"));
    assert!(response.contains("missing or invalid session token"));
}

#[test]
fn wrong_session_token_is_rejected() {
    let token = "d".repeat(64);
    let wrong = "e".repeat(64);
    let response = request(
        format!(
            "GET /api/snippets HTTP/1.1\r\nHost: 127.0.0.1:0\r\nX-Snacc-Session: {wrong}\r\n\r\n"
        ),
        &token,
    );
    assert!(response.contains("403 Forbidden"));
    assert!(response.contains("missing or invalid session token"));
}

#[test]
fn foreign_origin_is_rejected() {
    let token = "f".repeat(64);
    let body = r#"{"source":"print(1)","stdin":""}"#;
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://evil.example\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n{}",
            body.len(),
            token,
            body
        ),
        &token,
    );
    assert!(response.contains("403 Forbidden"));
    assert!(response.contains("invalid Origin header"));
}

#[test]
fn mismatched_host_is_rejected() {
    let token = "g".repeat(64);
    let response = request(
        "GET / HTTP/1.1\r\nHost: example.com\r\n\r\n".to_string(),
        &token,
    );
    assert!(response.contains("400 Bad Request"));
    assert!(response.contains("invalid Host header"));
}

#[test]
fn unsupported_method_returns_405_with_allow_header() {
    let token = "h".repeat(64);

    let response = request(
        "POST / HTTP/1.1\r\nHost: 127.0.0.1:0\r\n\r\n".to_string(),
        &token,
    );
    assert!(response.contains("405 Method Not Allowed"));
    assert!(response.contains("Allow: GET"));

    let response = request(
        "POST /api/snippets HTTP/1.1\r\nHost: 127.0.0.1:0\r\n\r\n".to_string(),
        &token,
    );
    assert!(response.contains("405 Method Not Allowed"));
    assert!(response.contains("Allow: GET"));

    let response = request(
        "GET /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\n\r\n".to_string(),
        &token,
    );
    assert!(response.contains("405 Method Not Allowed"));
    assert!(response.contains("Allow: POST"));
}

#[test]
fn non_json_content_type_is_rejected() {
    let token = "i".repeat(64);
    let body = "not json";
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://127.0.0.1:0\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n{}",
            body.len(),
            token,
            body
        ),
        &token,
    );
    assert!(response.contains("415 Unsupported Media Type"));
    assert!(response.contains("Content-Type must be application/json"));
}

#[test]
fn malformed_json_body_is_rejected() {
    let token = "j".repeat(64);
    let body = "{not valid json";
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://127.0.0.1:0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n{}",
            body.len(),
            token,
            body
        ),
        &token,
    );
    assert!(response.contains("400 Bad Request"));
    assert!(response.contains("invalid JSON request"));
}

#[test]
fn unknown_json_fields_are_rejected() {
    let token = "k".repeat(64);
    let body = r#"{"source":"print(1)","stdin":"","extra":true}"#;
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://127.0.0.1:0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n{}",
            body.len(),
            token,
            body
        ),
        &token,
    );
    assert!(response.contains("400 Bad Request"));
    assert!(response.contains("invalid JSON request"));
}

#[test]
fn oversized_request_headers_are_rejected() {
    let token = "l".repeat(64);
    let prefix = "GET / HTTP/1.1\r\nHost: 127.0.0.1:0\r\nX-Big: ";
    let total_len = 64 * 1024 + 1;
    let raw = format!("{prefix}{}", "a".repeat(total_len - prefix.len()));
    let response = request(raw, &token);
    assert!(response.contains("413 Payload Too Large"));
    assert!(response.contains("request headers are too large"));
}

#[test]
fn oversized_request_body_is_rejected_from_content_length_alone() {
    let token = "m".repeat(64);
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://127.0.0.1:0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n",
            MAX_REQUEST_BODY + 1,
            token
        ),
        &token,
    );
    assert!(response.contains("413 Payload Too Large"));
    assert!(response.contains("request body exceeds the 320 KiB limit"));
}

#[test]
fn oversized_source_is_rejected() {
    let token = "n".repeat(64);
    let big_source = "a".repeat(MAX_SOURCE + 1);
    let body = serde_json::json!({"source": big_source, "stdin": ""}).to_string();
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://127.0.0.1:0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n{}",
            body.len(),
            token,
            body
        ),
        &token,
    );
    assert!(response.contains("413 Payload Too Large"));
    assert!(response.contains("source exceeds the 256 KiB limit"));
}

#[test]
fn oversized_stdin_is_rejected() {
    let token = "o".repeat(64);
    let big_stdin = "a".repeat(MAX_STDIN + 1);
    let body = serde_json::json!({"source": "print(1)", "stdin": big_stdin}).to_string();
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://127.0.0.1:0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n{}",
            body.len(),
            token,
            body
        ),
        &token,
    );
    assert!(response.contains("413 Payload Too Large"));
    assert!(response.contains("stdin exceeds the 64 KiB limit"));
}

#[test]
fn compile_failure_yields_null_execution() {
    let token = "p".repeat(64);
    let body = serde_json::json!({"source": "while nil do false end", "stdin": ""}).to_string();
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://127.0.0.1:0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n{}",
            body.len(),
            token,
            body
        ),
        &token,
    );
    assert!(response.contains("200 OK"));
    assert!(response.contains(r#""status":"failed""#));
    assert!(response.contains(r#""execution":null"#));
}

fn spawn_accepting_server(
    mut shared: Shared,
    connections: usize,
) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    shared.address = address;
    let shared = Arc::new(shared);
    let handle = thread::spawn(move || {
        for _ in 0..connections {
            let (stream, _) = listener.accept().unwrap();
            let shared = Arc::clone(&shared);
            thread::spawn(move || handle_connection(stream, &shared).unwrap());
        }
    });
    (address, handle)
}

fn send_raw(address: SocketAddr, raw: &str) -> String {
    let mut client = TcpStream::connect(address).unwrap();
    client.write_all(raw.as_bytes()).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn second_run_while_one_is_active_receives_429() {
    let _guard = BUILD_LOCK.lock().unwrap();
    let token = "q".repeat(64);
    let shared = Shared {
        page: PAGE.replace("__SNACC_SESSION_TOKEN__", &token),
        snippets: SNIPPETS.to_string(),
        token: token.clone(),
        address: "127.0.0.1:0".parse().unwrap(),
        active: Arc::new(AtomicBool::new(false)),
    };
    let (address, server) = spawn_accepting_server(shared, 2);

    let slow_body = serde_json::json!({"source": "while true do 1 end", "stdin": ""}).to_string();
    let slow_raw = format!(
        "POST /api/run HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {token}\r\n\r\n{slow_body}",
        slow_body.len()
    );
    let first = thread::spawn(move || send_raw(address, &slow_raw));

    thread::sleep(Duration::from_millis(300));

    let quick_body = serde_json::json!({"source": "print(1)", "stdin": ""}).to_string();
    let quick_raw = format!(
        "POST /api/run HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {token}\r\n\r\n{quick_body}",
        quick_body.len()
    );
    let second_response = send_raw(address, &quick_raw);
    assert!(second_response.contains("429 Too Many Requests"));
    assert!(second_response.contains("another run is already active"));

    let first_response = first.join().unwrap();
    assert!(first_response.contains(r#""status":"timed-out""#));

    server.join().unwrap();
}

#[test]
fn timeout_kills_child_and_reports_timed_out() {
    let _guard = BUILD_LOCK.lock().unwrap();
    let token = "r".repeat(64);
    let body = serde_json::json!({"source": "while true do 1 end", "stdin": ""}).to_string();
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://127.0.0.1:0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n{}",
            body.len(),
            token,
            body
        ),
        &token,
    );
    assert!(response.contains("200 OK"));
    assert!(response.contains(r#""status":"timed-out""#));
}

#[test]
fn build_directory_with_a_space_still_compiles_and_runs() {
    let _guard = BUILD_LOCK.lock().unwrap();

    let spaced_dir =
        std::env::temp_dir().join(format!("snacc workbench build {}", std::process::id()));
    fs::create_dir_all(&spaced_dir).expect("failed to create spaced scratch directory");

    let original = (
        std::env::var_os("TMP"),
        std::env::var_os("TEMP"),
        std::env::var_os("TMPDIR"),
    );
    // SAFETY: ENV_LOCK (held above) prevents this from racing the restore
    // at the end of this same test, the only other env mutation here.
    unsafe {
        std::env::set_var("TMP", &spaced_dir);
        std::env::set_var("TEMP", &spaced_dir);
        std::env::set_var("TMPDIR", &spaced_dir);
    }

    let token = "s".repeat(64);
    let body = serde_json::json!({"source": "print(1 + 2)", "stdin": ""}).to_string();
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://127.0.0.1:0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n{}",
            body.len(),
            token,
            body
        ),
        &token,
    );

    // SAFETY: same justification as above; restores the pre-test values.
    unsafe {
        match original.0 {
            Some(value) => std::env::set_var("TMP", value),
            None => std::env::remove_var("TMP"),
        }
        match original.1 {
            Some(value) => std::env::set_var("TEMP", value),
            None => std::env::remove_var("TEMP"),
        }
        match original.2 {
            Some(value) => std::env::set_var("TMPDIR", value),
            None => std::env::remove_var("TMPDIR"),
        }
    }
    let _ = fs::remove_dir_all(&spaced_dir);

    assert!(response.contains("200 OK"), "response: {response}");
    assert!(
        response.contains(r#""status":"succeeded""#),
        "response: {response}"
    );
    assert!(
        response.contains(r#""stdout":"3\n""#),
        "response: {response}"
    );
}

fn scratch_dir(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "snacc-workbench-test-{}-{name}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("failed to create scratch directory for workbench test");
    dir
}

/// Compiles a standalone Rust helper program (not a Snacc program) so
/// process-piping behavior (stdin delivery, threaded stdout/stderr
/// draining) can be exercised directly through `run_process`: the Snacc
/// language itself has no builtin for reading stdin or writing stderr, so
/// only a hand-written child can exhibit that behavior on purpose.
fn compile_rust_helper(name: &str, source: &str) -> (PathBuf, PathBuf) {
    let dir = scratch_dir(name);
    let src_path = dir.join("main.rs");
    fs::write(&src_path, source).expect("failed to write process-test helper source");
    let exe_path = dir.join(format!("helper{}", std::env::consts::EXE_SUFFIX));
    let status = Command::new("rustc")
        .arg("--edition=2021")
        .arg(&src_path)
        .arg("-o")
        .arg(&exe_path)
        .status()
        .expect("rustc must be on PATH to build workbench process-test helpers");
    assert!(
        status.success(),
        "failed to compile process-test helper '{name}'"
    );
    (dir, exe_path)
}

#[test]
fn stdin_reaches_child_and_child_observes_eof() {
    let _guard = BUILD_LOCK.lock().unwrap();
    let (dir, exe) = compile_rust_helper(
        "stdin_echo",
        r#"
fn main() {
    use std::io::Read;
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).expect("read stdin to EOF");
    println!("{}", buf.len());
}
"#,
    );
    let payload = "hello snacc workbench ".repeat(500);
    let report = run_process(&exe, &dir, &payload);
    assert_eq!(report.status, "exited");
    assert_eq!(report.exit_code, Some(0));
    assert_eq!(report.stdin_bytes, payload.len());
    assert_eq!(report.stdout.trim(), payload.len().to_string());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn large_simultaneous_stdout_and_stderr_do_not_deadlock() {
    let _guard = BUILD_LOCK.lock().unwrap();
    let (dir, exe) = compile_rust_helper(
        "dual_stream_flood",
        r#"
fn main() {
    use std::io::Write;
    let out_chunk = vec![b'o'; 8192];
    let err_chunk = vec![b'e'; 8192];
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    for _ in 0..40 {
        stdout.write_all(&out_chunk).unwrap();
        stderr.write_all(&err_chunk).unwrap();
    }
}
"#,
    );
    let report = run_process(&exe, &dir, "");
    assert_eq!(report.status, "exited");
    assert_eq!(report.exit_code, Some(0));
    assert_eq!(report.stdout.len(), 40 * 8192);
    assert_eq!(report.stderr.len(), 40 * 8192);
    assert!(!report.stdout_truncated);
    assert!(!report.stderr_truncated);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_route_compiles_and_executes_in_process() {
    let _guard = BUILD_LOCK.lock().unwrap();
    let token = "b".repeat(64);
    let body = r#"{"source":"print(1 + 2)","stdin":""}"#;
    let response = request(
        format!(
            "POST /api/run HTTP/1.1\r\nHost: 127.0.0.1:0\r\nOrigin: http://127.0.0.1:0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Snacc-Session: {}\r\n\r\n{}",
            body.len(),
            token,
            body
        ),
        &token,
    );
    assert!(response.contains("200 OK"));
    assert!(response.contains(r#""status":"exited""#));
    assert!(response.contains(r#""stdout":"3\n""#));
}
