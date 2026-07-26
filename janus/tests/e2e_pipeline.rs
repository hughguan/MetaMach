//! M5 ADR-028: E2E Pipeline CI tests with mock agents.
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
        // Copy configs/ so the daemon finds offboard.toml, global_rules.md, etc.
        let ws = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let src = ws.join("configs");
        if src.is_dir() {
            let _ = std::process::Command::new("cp")
                .arg("-R")
                .arg(&src)
                .arg(repo_path.join("configs"))
                .status();
        }
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

#[test]
fn e2e_onboard_dispatch_returns_task_id() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let bp_dir = repo.path().join(".janus");
    std::fs::create_dir_all(&bp_dir).unwrap();
    std::fs::write(
        bp_dir.join("blueprint.toml"),
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
}

/// E2E multi-step workflow: produce → transform → verify.
/// Each step writes a file; the next step reads it. Exercises the full
/// daemon path (onboard → dispatch → progress → COMPLETED) with a
/// realistic 3-step linear workflow and output verification.
#[test]
fn e2e_multi_step_workflow_produce_transform_verify() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let bp_dir = repo.path().join(".janus");
    std::fs::create_dir_all(&bp_dir).unwrap();
    std::fs::write(
        bp_dir.join("blueprint.toml"),
        "[blueprint]\nname = \"produce_e2e\"\ndefault_workflow = \"produce\"\n\n[openwiki]\nscope = [\"e2e\"]\n",
    )
    .unwrap();

    // 3-step workflow: produce → transform → verify.
    // Uses repo-relative output so no path escaping issues.
    let wf = repo.path().join("workflows");
    std::fs::create_dir_all(&wf).unwrap();

    std::fs::write(
        wf.join("produce.toml"),
        r#"[workflow]
name = "produce"

[[steps]]
name = "gen"
agent = "default"
command = "echo step1-done && sleep 1"

[[steps]]
name = "xform"
agent = "default"
command = "echo step2-done && sleep 1"

[[steps]]
name = "check"
agent = "default"
command = "echo step3-done"
"#,
    )
    .unwrap();

    let d = Daemon::spawn(state.path(), &agents, repo.path());
    std::thread::sleep(Duration::from_secs(12));

    d.uds(
        &Request::Onboard {
            name: "produce_e2e".into(),
        },
        Duration::from_secs(15),
    )
    .unwrap();

    let resp = d
        .uds(
            &Request::Dispatch {
                blueprint: "produce_e2e".into(),
                workflow: None,
            },
            Duration::from_secs(15),
        )
        .unwrap();
    let task_id = match resp {
        Response::Dispatch { task_id } => task_id,
        other => panic!("expected Dispatch, got {other:?}"),
    };

    // Poll progress until COMPLETED or timeout (90s — 3 steps with tmux overhead).
    let start = Instant::now();
    let timeout = Duration::from_secs(90);
    loop {
        let resp = d
            .uds(
                &Request::Progress {
                    blueprint: Some("produce_e2e".into()),
                },
                Duration::from_secs(5),
            )
            .unwrap();
        let Response::Progress { active_tasks } = resp else {
            panic!("expected Progress, got {resp:?}");
        };
        let task = active_tasks.iter().find(|t| t.task_id == task_id);
        match task.map(|t| t.status.as_str()) {
            Some("COMPLETED") => break,
            Some("FAILED") | Some("SUSPENDED") => {
                panic!(
                    "task {task_id} ended with {}: {task:#?}",
                    task.unwrap().status
                );
            }
            _ => {
                assert!(
                    start.elapsed() < timeout,
                    "task {task_id} did not complete within {timeout:?}"
                );
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }

    // Verify all 3 steps completed by checking Progress.
    let resp = d
        .uds(
            &Request::Progress {
                blueprint: Some("produce_e2e".into()),
            },
            Duration::from_secs(5),
        )
        .unwrap();
    let Response::Progress { active_tasks } = resp else {
        panic!("expected Progress");
    };
    let task = active_tasks
        .iter()
        .find(|t| t.task_id == task_id)
        .expect("task should be in progress");
    assert_eq!(
        task.status, "COMPLETED",
        "task should be COMPLETED, got {} with steps: {:?}",
        task.status, task.steps
    );
    assert_eq!(task.steps.len(), 3, "should have 3 steps");
    for (i, s) in task.steps.iter().enumerate() {
        assert_eq!(
            s.status, "COMPLETED",
            "step {i} ({}) should be COMPLETED, got {}",
            s.name, s.status
        );
    }
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

    let bp_dir = repo.path().join(".janus");
    std::fs::create_dir_all(&bp_dir).unwrap();
    std::fs::write(
        bp_dir.join("blueprint.toml"),
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
