//! Route handling and validation.

use std::{
    io,
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    CSP, MAX_SOURCE, MAX_STDIN, SESSION_HEADER, Shared,
    http::{HttpError, read_request, write_error, write_method_error, write_response},
    run::run_program,
};

struct BusyGuard(Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) fn handle_connection(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => return write_error(&mut stream, error),
    };
    if request.headers.get("host") != Some(&format!("127.0.0.1:{}", shared.address.port())) {
        return write_error(
            &mut stream,
            HttpError {
                status: "400 Bad Request",
                message: "invalid Host header".into(),
            },
        );
    }

    match request.path.as_str() {
        "/" => {
            if request.method != "GET" {
                return write_method_error(&mut stream, "GET");
            }
            write_response(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                shared.page.as_bytes(),
                &[
                    ("Cache-Control", "no-store"),
                    ("Content-Security-Policy", CSP),
                ],
            )
        }
        "/api/snippets" => {
            if request.method != "GET" {
                return write_method_error(&mut stream, "GET");
            }
            if !authorized(&request, shared) {
                return write_error(&mut stream, forbidden("missing or invalid session token"));
            }
            write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                shared.snippets.as_bytes(),
                &[],
            )
        }
        "/api/run" => {
            if request.method != "POST" {
                return write_method_error(&mut stream, "POST");
            }
            if !authorized(&request, shared) {
                return write_error(&mut stream, forbidden("missing or invalid session token"));
            }
            let origin = format!("http://127.0.0.1:{}", shared.address.port());
            if request.headers.get("origin") != Some(&origin) {
                return write_error(&mut stream, forbidden("invalid Origin header"));
            }
            let content_type = request
                .headers
                .get("content-type")
                .map(String::as_str)
                .unwrap_or_default();
            if content_type.split(';').next().map(str::trim) != Some("application/json") {
                return write_error(
                    &mut stream,
                    HttpError {
                        status: "415 Unsupported Media Type",
                        message: "Content-Type must be application/json".into(),
                    },
                );
            }
            let run_request: crate::run::RunRequest = match serde_json::from_slice(&request.body) {
                Ok(value) => value,
                Err(error) => {
                    return write_error(
                        &mut stream,
                        HttpError {
                            status: "400 Bad Request",
                            message: format!("invalid JSON request: {error}"),
                        },
                    );
                }
            };
            if run_request.source.trim().is_empty() {
                return write_error(
                    &mut stream,
                    HttpError {
                        status: "400 Bad Request",
                        message: "source must contain a non-whitespace character".into(),
                    },
                );
            }
            if run_request.source.len() > MAX_SOURCE {
                return write_error(
                    &mut stream,
                    HttpError {
                        status: "413 Payload Too Large",
                        message: "source exceeds the 256 KiB limit".into(),
                    },
                );
            }
            if run_request.stdin.len() > MAX_STDIN {
                return write_error(
                    &mut stream,
                    HttpError {
                        status: "413 Payload Too Large",
                        message: "stdin exceeds the 64 KiB limit".into(),
                    },
                );
            }
            if shared
                .active
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return write_error(
                    &mut stream,
                    HttpError {
                        status: "429 Too Many Requests",
                        message: "another run is already active".into(),
                    },
                );
            }
            let _busy = BusyGuard(Arc::clone(&shared.active));
            let response = run_program(&run_request.source, &run_request.stdin);
            let body = serde_json::to_vec(&response).map_err(io::Error::other)?;
            write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                &body,
                &[],
            )
        }
        _ => write_error(
            &mut stream,
            HttpError {
                status: "404 Not Found",
                message: "not found".into(),
            },
        ),
    }
}

fn authorized(request: &crate::http::Request, shared: &Shared) -> bool {
    match request.headers.get(SESSION_HEADER) {
        Some(token) => constant_time_eq(token, &shared.token),
        None => false,
    }
}

/// Compares the session token in constant time: `==` on `String` short-circuits
/// at the first differing byte, which would let a network attacker recover the
/// token one byte at a time from response-timing differences. The length check
/// does not leak secret information since the expected token's length is fixed
/// and public (32 random bytes, hex-encoded).
pub(crate) fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn forbidden(message: &str) -> HttpError {
    HttpError {
        status: "403 Forbidden",
        message: message.into(),
    }
}
