//! Loopback-only development server: setup and shared state.
//!
//! Request parsing lives in [`http`], route handling in [`routes`], and the
//! compile-and-execute lifecycle in [`run`].

use std::{
    net::{SocketAddr, TcpListener},
    sync::{Arc, atomic::AtomicBool},
    thread,
};

pub(crate) mod http;
pub(crate) mod routes;
pub(crate) mod run;

pub(crate) const PAGE: &str = include_str!("../page.html");
pub(crate) const SNIPPETS: &str = include_str!(concat!(env!("OUT_DIR"), "/snippets.json"));
pub(crate) const SESSION_HEADER: &str = "x-snacc-session";
pub(crate) const MAX_REQUEST_BODY: usize = 320 * 1024;
pub(crate) const MAX_SOURCE: usize = 256 * 1024;
pub(crate) const MAX_STDIN: usize = 64 * 1024;
pub(crate) const MAX_STDOUT: usize = 1024 * 1024;
pub(crate) const MAX_STDERR: usize = 1024 * 1024;
pub(crate) const MAX_EXECUTION: std::time::Duration = std::time::Duration::from_secs(3);
pub(crate) const CSP: &str =
    "default-src 'none'; connect-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'";

#[derive(Clone)]
pub(crate) struct Shared {
    pub(crate) page: String,
    pub(crate) snippets: String,
    pub(crate) token: String,
    pub(crate) address: SocketAddr,
    pub(crate) active: Arc<AtomicBool>,
}

pub fn run() -> std::io::Result<()> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let address = listener.local_addr()?;
    let token = session_token()?;
    let page = PAGE.replace("__SNACC_SESSION_TOKEN__", &token);
    let shared = Arc::new(Shared {
        page,
        snippets: SNIPPETS.to_string(),
        token,
        address,
        active: Arc::new(AtomicBool::new(false)),
    });

    println!("snacc-workbench executes native programs; use trusted local snippets only.");
    println!("snacc-workbench listening at http://{address}/");

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let shared = Arc::clone(&shared);
                thread::spawn(move || {
                    if let Err(error) = crate::routes::handle_connection(stream, &shared) {
                        eprintln!("snacc-workbench request failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("snacc-workbench accept failed: {error}"),
        }
    }
    Ok(())
}

fn session_token() -> std::io::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(std::io::Error::other)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        token.push_str(&format!("{byte:02x}"));
    }
    Ok(token)
}

#[cfg(test)]
mod tests;
