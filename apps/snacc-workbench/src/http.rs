//! HTTP/1.1 request parsing and response writing.

use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::TcpStream,
};

#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) body: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct HttpError {
    pub(crate) status: &'static str,
    pub(crate) message: String,
}

pub(crate) fn read_request(stream: &mut TcpStream) -> Result<Request, HttpError> {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).map_err(io_http_error)?;
        if count == 0 {
            return Err(HttpError {
                status: "400 Bad Request",
                message: "request ended before headers".into(),
            });
        }
        raw.extend_from_slice(&chunk[..count]);
        if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
        if raw.len() > 64 * 1024 {
            return Err(HttpError {
                status: "413 Payload Too Large",
                message: "request headers are too large".into(),
            });
        }
    };
    let header_text = std::str::from_utf8(&raw[..header_end]).map_err(|_| HttpError {
        status: "400 Bad Request",
        message: "request headers are not valid UTF-8".into(),
    })?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().ok_or_else(|| HttpError {
        status: "400 Bad Request",
        message: "request line is missing".into(),
    })?;
    let parts = request_line.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" {
        return Err(HttpError {
            status: "400 Bad Request",
            message: "only HTTP/1.1 requests are supported".into(),
        });
    }
    let mut headers = HashMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(HttpError {
                status: "400 Bad Request",
                message: "malformed request header".into(),
            });
        };
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    let content_length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| HttpError {
            status: "400 Bad Request",
            message: "invalid Content-Length".into(),
        })?
        .unwrap_or(0);
    if content_length > crate::MAX_REQUEST_BODY {
        return Err(HttpError {
            status: "413 Payload Too Large",
            message: "request body exceeds the 320 KiB limit".into(),
        });
    }
    let body_start = header_end + 4;
    let mut body = raw[body_start..].to_vec();
    if body.len() > content_length {
        body.truncate(content_length);
    }
    while body.len() < content_length {
        let remaining = content_length - body.len();
        let mut rest = vec![0_u8; remaining];
        stream.read_exact(&mut rest).map_err(io_http_error)?;
        body.extend_from_slice(&rest);
    }
    body.truncate(content_length);
    Ok(Request {
        method: parts[0].to_string(),
        path: parts[1].to_string(),
        headers,
        body,
    })
}

fn io_http_error(error: io::Error) -> HttpError {
    HttpError {
        status: "400 Bad Request",
        message: format!("failed to read request: {error}"),
    }
}

pub(crate) fn write_method_error(stream: &mut TcpStream, allowed: &str) -> io::Result<()> {
    write_response(
        stream,
        "405 Method Not Allowed",
        "application/json; charset=utf-8",
        br#"{"error":"method not allowed"}"#,
        &[("Allow", allowed)],
    )
}

pub(crate) fn write_error(stream: &mut TcpStream, error: HttpError) -> io::Result<()> {
    let body = serde_json::json!({"error": error.message});
    let body = serde_json::to_vec(&body).map_err(io::Error::other)?;
    write_response(
        stream,
        error.status,
        "application/json; charset=utf-8",
        &body,
        &[],
    )
}

pub(crate) fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    extra_headers: &[(&str, &str)],
) -> io::Result<()> {
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n",
        body.len()
    );
    for (name, value) in extra_headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes())?;
    stream.write_all(body)
}
