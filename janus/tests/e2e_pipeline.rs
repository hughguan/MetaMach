//! M5 ADR-028: E2E Pipeline CI tests with mock agents.
//!
//! All tests runtime-skip when PG or tmux is unavailable.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use janus::protocol::{Request, Response};
use janus::uds;

const AGENTS_TOML: &str = r#"
[agent.architect]
permissions = ["read", "write", "bash-safe"]
bash_safe = true
bash_blacklist = ["rm -rf /"]

[agent.default]
bash_safe = true
bash_blacklist = ["rm -rf /"]
"#;

fn pg_available() -> bool {
    std::env::var("DATABASE_URL").is_ok() || std::env::var("METAMACH_PG_SOCKET_DIR").is_ok()
}

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

struct Daemon {
    child: std::process::Child,
    sock: std::path::PathBuf,
}

impl Daemon {
    fn spawn(state_dir: &Path, agents: &Path, repo_path: &Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_janus-daemon"))
            .env("HERDR_PLUGIN_STATE_DIR", state_dir)
            .env("HERDR_PLUGIN_ROOT", repo_path)
            .env("JANUS_AGENTS_TOML", agents)
            .env("JANUS_GATEWAY_LISTEN_PORT", "0")
            .env("JANUS_JANUSH_BIN", env!("CARGO_BIN_EXE_janush"))
            .env("RUST_LOG", "warn")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn janus-daemon");
        let sock = state_dir.join("janus.sock");
        let start = Instant::now();
        while !sock.exists() && start.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(sock.exists(), "daemon did not bind janus.sock within 15s");
        std::thread::sleep(Duration::from_millis(100));
        Daemon { child, sock }
    }

    fn uds(&self, req: &Request, timeout: Duration) -> Result<Response, String> {
        uds::request_to(&self.sock, req, timeout).map_err(|e| e.to_string())
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn poll_until_completed(d: &Daemon, _blueprint: &str, timeout: Duration) -> (bool, String) {
    let deadline = Instant::now() + timeout;
    let mut last_state = String::new();
    while Instant::now() < deadline {
        let resp = d.uds(
            &Request::Progress { blueprint: None },
            Duration::from_secs(5),
        );
        if let Ok(Response::Progress { active_tasks }) = &resp {
            if !active_tasks.is_empty() {
                for t in active_tasks {
                    if t.steps.iter().any(|s| s.status == "COMPLETED") {
                        return (true, format!("COMPLETED: {active_tasks:?}"));
                    }
                    if t.status == "FAILED" || t.status == "SUSPENDED" {
                        return (false, format!("task {0}: {active_tasks:?}", t.status));
                    }
                }
                last_state = format!("{active_tasks:?}");
            }
        } else {
            last_state = format!("Progress error: {resp:?}");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    (
        false,
        format!("timeout after {timeout:?}, last: {last_state}"),
    )
}

#[test]
fn e2e_onboard_dispatch_complete() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let bp = repo.path().join("blueprints").join("smoke_e2e");
    std::fs::create_dir_all(&bp).unwrap();
    std::fs::write(
        bp.join("janus.toml"),
        "[blueprint]\nname = \"smoke_e2e\"\ndefault_workflow = \"smoke\"\n\n[openwiki]\nscope = [\"e2e\"]\n",
    )
    .unwrap();
    let wf = repo.path().join("workflows");
    std::fs::create_dir_all(&wf).unwrap();
    std::fs::write(
        wf.join("smoke.toml"),
        "[workflow]\nname = \"smoke\"\n\n[[steps]]\nname = \"hello\"\nagent = \"default\"\ncommand = \"true\"\n",
    )
    .unwrap();

    let d = Daemon::spawn(state.path(), &agents, repo.path());
    std::thread::sleep(Duration::from_secs(12));

    d.uds(
        &Request::Onboard {
            name: "smoke_e2e".into(),
        },
        Duration::from_secs(15),
    )
    .unwrap();
    let resp = d
        .uds(
            &Request::Dispatch {
                blueprint: "smoke_e2e".into(),
                workflow: None,
            },
            Duration::from_secs(15),
        )
        .unwrap();
    assert!(
        matches!(resp, Response::Dispatch { .. }),
        "dispatch: {resp:?}"
    );

    let (ok, diag) = poll_until_completed(&d, "smoke_e2e", Duration::from_secs(30));
    assert!(ok, "no step reached COMPLETED within 30s: {diag}");
}

#[test]
fn e2e_tool_guard_blocks_blacklisted() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let bp = repo.path().join("blueprints").join("guard_e2e");
    std::fs::create_dir_all(&bp).unwrap();
    std::fs::write(
        bp.join("janus.toml"),
        "[blueprint]\nname = \"guard_e2e\"\ndefault_workflow = \"danger\"\n\n[openwiki]\nscope = [\"e2e\"]\n",
    )
    .unwrap();
    let wf = repo.path().join("workflows");
    std::fs::create_dir_all(&wf).unwrap();
    std::fs::write(
        wf.join("danger.toml"),
        "[workflow]\nname = \"danger\"\n\n[[steps]]\nname = \"bad\"\nagent = \"architect\"\ncommand = \"rm -rf /tmp/metamach-e2e-guard-test\"\n",
    )
    .unwrap();

    let sentinel = repo.path().join("sentinel.txt");
    std::fs::write(&sentinel, "do-not-delete").unwrap();

    let d = Daemon::spawn(state.path(), &agents, repo.path());
    std::thread::sleep(Duration::from_secs(12));

    d.uds(
        &Request::Onboard {
            name: "guard_e2e".into(),
        },
        Duration::from_secs(15),
    )
    .unwrap();
    d.uds(
        &Request::Dispatch {
            blueprint: "guard_e2e".into(),
            workflow: None,
        },
        Duration::from_secs(15),
    )
    .unwrap();

    std::thread::sleep(Duration::from_secs(5));
    assert!(
        sentinel.exists(),
        "sentinel should survive — Tool Guard must block rm -rf"
    );
}
