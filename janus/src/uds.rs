//! Synchronous UDS client used by `herdr-janus` and the `janus` CLI.
//!
//! The Daemon (tokio server) speaks the same newline-delimited JSON protocol;
//! clients stay synchronous so the TUI event loop never needs an async runtime.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::paths;
use crate::protocol::{Request, Response};

/// Default per-request I/O timeout. Local UDS round-trips are sub-millisecond;
/// this only bounds a wedged Daemon so the client never hangs indefinitely.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// Send a request to the Daemon over `janus.sock`.
pub fn request(req: &Request) -> Result<Response> {
    request_to(&paths::sock_path(), req, DEFAULT_TIMEOUT)
}

/// Send a request to a specific socket path with a custom timeout (for tests).
pub fn request_to(sock: &Path, req: &Request, timeout: Duration) -> Result<Response> {
    let mut stream = UnixStream::connect(sock).context("connect janus.sock")?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let payload = serde_json::to_string(req).context("encode request")?;
    stream.write_all(payload.as_bytes())?;
    stream.write_all(b"\n")?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).context("read response")?;
    let resp: Response = serde_json::from_str(line.trim()).context("decode response")?;
    Ok(resp)
}

/// True iff the socket path is connectable right now (cheap liveness check).
pub fn is_daemon_listening() -> bool {
    UnixStream::connect(paths::sock_path()).is_ok()
}
