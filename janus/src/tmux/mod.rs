//! `janus::tmux` - physical execution engine (0.3.0 §2.4).
//!
//! Internalized from the external herdr-tether plugin: the core tmux session
//! lifecycle (create / attach / kill / inspect) against an isolated tmux server
//! (`tmux -L metamach-tmux`), with per-session `remain-on-exit on` so physical
//! sessions survive process exit, SSH drop, or frontend destruction (ARCH §6.1).
//! In-process signal linkage to Tool Guard (<1ms) replaces the prior external
//! UDS IPC path. Cross-host SSH transport and checkpoint-driven restart land with
//! M4 workflow execution; this module delivers the local session core + the
//! `janus tmux open|attach|list` CLI (Project-Plan Task 2.4).

pub mod lifecycle;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, bail};
use thiserror::Error;

/// Isolated tmux server socket name (never pollutes the director's personal tmux).
pub const TMUX_SOCKET: &str = "metamach-tmux";

/// Session name prefix per ARCH §4 sequence (`tmux-janus-task-<uuid>`).
pub const SESSION_PREFIX: &str = "tmux-janus-task-";

/// A tmux session identity (the `-t` target). Newtyped so it is never confused
/// with the absurd `task_id` UUID, which seeds the name but is not the tmux target.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Construct from an already-validated session name.
    pub fn from_name(name: String) -> Self {
        Self(name)
    }

    /// Mint a fresh session id for a new task dispatch: `tmux-janus-task-<uuid>`.
    pub fn new_for_task(task_uuid: &str) -> Self {
        Self(format!("{SESSION_PREFIX}{task_uuid}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Abstracts the durable tmux session backend so the Daemon talks to a real tmux
/// server in production and a fake in tests (0.3.0 §2.4 `DurableBackend`).
pub trait DurableBackend: Send + Sync {
    /// Create a detached session running `command` (cwd optional). The session
    /// MUST have `remain-on-exit on` set so it survives process exit (ARCH §6.1).
    fn create_session(&self, id: &SessionId, command: &str, cwd: Option<&Path>) -> Result<()>;

    /// Attach the calling terminal to the session (foreground; blocks until detach).
    fn attach(&self, id: &SessionId) -> Result<()>;

    /// Kill a session (GC / abort). No-op if already gone.
    fn kill_session(&self, id: &SessionId) -> Result<()>;

    /// Whether the session is currently alive.
    fn has_session(&self, id: &SessionId) -> Result<bool>;

    /// List all live sessions on the server.
    fn list_sessions(&self) -> Result<Vec<String>>;

    /// Capture the latest pane text (for HITL stdout_tail / progress dashboard).
    fn capture_pane(&self, id: &SessionId) -> Result<String>;
}

/// Errors specific to the tmux engine.
#[derive(Debug, Error)]
pub enum TmuxError {
    #[error("tmux binary not found on PATH (install tmux 3.3+)")]
    TmuxNotFound,
    #[error("session {0} not found")]
    NotFound(String),
    #[error("tmux command failed: {0}")]
    Command(String),
}

/// Production backend: drives a real `tmux -L metamach-tmux` server.
#[derive(Clone, Debug)]
pub struct TmuxBackend {
    tmux: PathBuf,
}

impl Default for TmuxBackend {
    /// Delegates to `new()` so callers never get a non-functional backend with an
    /// empty path (which would immediately fail with `TmuxError::TmuxNotFound`).
    fn default() -> Self {
        Self::new()
    }
}

impl TmuxBackend {
    /// Resolve the tmux binary (PATH lookup with fallback to standard dirs).
    pub fn new() -> Self {
        Self {
            tmux: resolve_tmux(),
        }
    }

    /// Build a `tmux -L metamach-tmux ...` command.
    fn tmux_cmd(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.tmux);
        cmd.args(["-L", TMUX_SOCKET]);
        cmd.args(args.iter().copied());
        cmd
    }

    /// Run a tmux control command and capture its output.
    fn run(&self, args: &[&str]) -> Result<Output> {
        if self.tmux.as_os_str().is_empty() {
            bail!(TmuxError::TmuxNotFound);
        }
        self.tmux_cmd(args)
            .output()
            .with_context(|| format!("spawn tmux ({})", self.tmux.display()))
    }
}

impl DurableBackend for TmuxBackend {
    fn create_session(&self, id: &SessionId, command: &str, cwd: Option<&Path>) -> Result<()> {
        // Create the session with a placeholder shell FIRST, then set
        // remain-on-exit, then respawn the pane with the real workload. This
        // ordering is race-free: a short-lived command (e.g. `true`) would exit
        // and destroy the session before a post-create set-option could land.
        // Per-session remain-on-exit (NOT -g) - Review-Spec Metric 2.1.
        let mut args: Vec<&str> = vec!["new-session", "-d", "-s", id.as_str()];
        let cwd_str;
        if let Some(cwd) = cwd {
            cwd_str = cwd.to_string_lossy().into_owned();
            args.extend(["-c", cwd_str.as_str()]);
        }
        let out = self.run(&args)?;
        if !out.status.success() {
            bail!(TmuxError::Command(lossy_stderr(&out)));
        }
        let out = self.run(&["set-option", "-t", id.as_str(), "remain-on-exit", "on"])?;
        if !out.status.success() {
            bail!(TmuxError::Command(lossy_stderr(&out)));
        }
        // Replace the placeholder shell with the workload. With remain-on-exit
        // now on, the pane survives the workload's exit (ARCH §6.1 invariant).
        let out = self.run(&["respawn-pane", "-t", id.as_str(), "-k", command])?;
        if !out.status.success() {
            bail!(TmuxError::Command(lossy_stderr(&out)));
        }
        Ok(())
    }

    fn attach(&self, id: &SessionId) -> Result<()> {
        if !self.has_session(id)? {
            bail!(TmuxError::NotFound(id.as_str().to_string()));
        }
        // attach-session inherits the caller's TTY (foreground, blocks until detach).
        let mut cmd = self.tmux_cmd(&["attach-session", "-t", id.as_str()]);
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let status = cmd.status().context("spawn tmux attach")?;
        if !status.success() {
            bail!(TmuxError::Command(format!("attach exited {status}")));
        }
        Ok(())
    }

    fn kill_session(&self, id: &SessionId) -> Result<()> {
        // Propagate spawn errors (tmux missing, permission denied, etc.) via the `?`.
        // The tmux exit status is deliberately ignored: kill-session returns non-zero
        // when the session is already gone, which is a no-op, not a failure.
        let _ = self.run(&["kill-session", "-t", id.as_str()])?;
        Ok(())
    }

    fn has_session(&self, id: &SessionId) -> Result<bool> {
        let out = self.run(&["has-session", "-t", id.as_str()])?;
        Ok(out.status.success())
    }

    fn list_sessions(&self) -> Result<Vec<String>> {
        let out = self.run(&["list-sessions", "-F", "#{session_name}"])?;
        if !out.status.success() {
            // No server / no sessions -> empty (not an error).
            return Ok(vec![]);
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    }

    fn capture_pane(&self, id: &SessionId) -> Result<String> {
        let out = self.run(&["capture-pane", "-p", "-t", id.as_str()])?;
        // Check exit status like every other method. If the session does not exist,
        // `tmux capture-pane` exits with code 1 and writes an error to stderr;
        // without this check, callers would silently get an empty string and
        // misinterpret it as "no output yet" rather than "session missing".
        if !out.status.success() {
            bail!(TmuxError::Command(lossy_stderr(&out)));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// Resolve the tmux binary: PATH lookup, then standard dirs. Falls back to the
/// bare name `tmux` so spawn fails with a clear error if truly absent.
fn resolve_tmux() -> PathBuf {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("tmux");
            if is_executable(&candidate) {
                return candidate;
            }
        }
    }
    for dir in ["/usr/bin", "/bin", "/opt/homebrew/bin", "/usr/local/bin"] {
        let candidate = PathBuf::from(dir).join("tmux");
        if is_executable(&candidate) {
            return candidate;
        }
    }
    PathBuf::from("tmux")
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn lossy_stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A no-op backend for unit tests that must not touch a real tmux server.
    /// Records the operations so call-site behavior can be asserted.
    #[derive(Default)]
    struct FakeBackend {
        created: std::sync::Mutex<Vec<String>>,
        alive: std::sync::Mutex<std::collections::HashSet<String>>,
    }

    impl DurableBackend for FakeBackend {
        fn create_session(
            &self,
            id: &SessionId,
            _command: &str,
            _cwd: Option<&Path>,
        ) -> Result<()> {
            self.created.lock().unwrap().push(id.as_str().to_string());
            self.alive.lock().unwrap().insert(id.as_str().to_string());
            Ok(())
        }
        fn attach(&self, id: &SessionId) -> Result<()> {
            if !self.has_session(id)? {
                bail!(TmuxError::NotFound(id.as_str().to_string()));
            }
            Ok(())
        }
        fn kill_session(&self, id: &SessionId) -> Result<()> {
            self.alive.lock().unwrap().remove(id.as_str());
            Ok(())
        }
        fn has_session(&self, id: &SessionId) -> Result<bool> {
            Ok(self.alive.lock().unwrap().contains(id.as_str()))
        }
        fn list_sessions(&self) -> Result<Vec<String>> {
            Ok(self.alive.lock().unwrap().iter().cloned().collect())
        }
        fn capture_pane(&self, _id: &SessionId) -> Result<String> {
            Ok(String::new())
        }
    }

    #[test]
    fn session_id_names_task_with_uuid() {
        let id = SessionId::new_for_task("0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a");
        assert_eq!(
            id.as_str(),
            "tmux-janus-task-0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a"
        );
    }

    #[test]
    fn fake_backend_create_kill_round_trip() {
        let backend = FakeBackend::default();
        let id = SessionId::from_name("test-rt".into());
        backend.create_session(&id, "sleep 1", None).unwrap();
        assert!(backend.has_session(&id).unwrap());
        backend.kill_session(&id).unwrap();
        assert!(!backend.has_session(&id).unwrap());
    }

    #[test]
    fn attach_missing_session_errors() {
        let backend = FakeBackend::default();
        let id = SessionId::from_name("nope".into());
        let err = backend.attach(&id).unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn lifecycle_restart_creates_session() {
        let backend = FakeBackend::default();
        let id = lifecycle::LifecycleService::restart_session(
            &backend,
            "abc-123",
            "make cross-compile",
            None,
        )
        .unwrap();
        assert!(id.as_str().starts_with(SESSION_PREFIX));
        assert!(backend.has_session(&id).unwrap());
    }
}
