//! Lazy-start self-healing: spawn `janus-daemon` detached from the controlling
//! terminal (Feature-Spec §2.1).
//!
//! Uses `std::process::Command::spawn()` + `pre_exec(setsid)` (not raw
//! `fork()`+`exec()`), with stdio redirected to /dev/null. On spawn failure the
//! caller reports an error rather than silently crashing.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

/// Resolve the `janus-daemon` binary path. Shared by [`spawn_daemon_detached`]
/// (lazy-start) and the `janus daemon` CLI subcommand so both launch the same
/// binary - divergent resolution risked a protocol/behavior mismatch.
///
/// Precedence: `JANUS_DAEMON_BIN` env > sibling of the current executable >
/// `${HERDR_PLUGIN_ROOT}/bin` > error. Never falls back to a bare `$PATH`
/// lookup: an unrelated `janus-daemon` on PATH could otherwise be exec'd.
pub fn resolve_daemon_exe() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("JANUS_DAEMON_BIN") {
        return Ok(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let p = dir.join("janus-daemon");
        if p.exists() {
            return Ok(p);
        }
    }
    if let Ok(root) = std::env::var("HERDR_PLUGIN_ROOT") {
        let p = PathBuf::from(root).join("bin").join("janus-daemon");
        if p.exists() {
            return Ok(p);
        }
    }
    bail!("janus-daemon binary not found; run `make compile` or set JANUS_DAEMON_BIN");
}

/// Spawn the Daemon in the background, fully detached. Returns `Ok(())` on a
/// successful spawn (does not wait).
pub fn spawn_daemon_detached() -> Result<()> {
    let exe = resolve_daemon_exe()?;
    let mut cmd = Command::new(&exe);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Detach from the controlling terminal so the Daemon outlives the
        // shadow client that launched it (Feature-Spec §2.1).
        unsafe {
            cmd.pre_exec(|| {
                // Best-effort setsid; failure here is non-fatal.
                let _ = libc::setsid();
                Ok(())
            });
        }
    }

    cmd.spawn()
        .with_context(|| format!("spawn {}", exe.display()))?;
    Ok(())
}

/// Probe the socket; if absent, lazy-start the Daemon and retry until it answers
/// or `timeout` elapses. Returns the resolved Daemon liveness.
pub fn ensure_daemon(timeout: std::time::Duration) -> Result<()> {
    use std::time::Instant;

    if crate::uds::is_daemon_listening() {
        return Ok(());
    }
    spawn_daemon_detached()?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if crate::uds::is_daemon_listening() {
            return Ok(());
        }
    }
    Err(anyhow!(
        "janus-daemon did not become reachable within {:?} (check {})",
        timeout,
        crate::paths::log_path().display()
    ))
}
