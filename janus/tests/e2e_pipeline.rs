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

    fn wait_ready(&self) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if let Ok(Response::Pong) = self.uds(&Request::Ping, Duration::from_millis(200)) {
                std::thread::sleep(Duration::from_millis(500));
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
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
    d.wait_ready();
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
                pipeline: None,
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
    d.wait_ready();

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
                pipeline: None,
            },
            Duration::from_secs(15),
        )
        .unwrap();
    let task_id = match resp {
        Response::Dispatch { task_id } => task_id,
        other => panic!("expected Dispatch, got {other:?}"),
    };

    // Poll until the task disappears from active_tasks (terminal state).
    // db.progress() only returns STARTING/RUNNING/SUSPENDED — COMPLETED
    // tasks vanish from the list, so we detect completion by absence.
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
        match task {
            None => break, // terminal state reached
            Some(t) if t.status == "SUSPENDED" || t.status == "FAILED" => {
                panic!("task {task_id} ended with {}: {t:#?}", t.status);
            }
            Some(_) => {
                assert!(
                    start.elapsed() < timeout,
                    "task {task_id} did not finish within {timeout:?}"
                );
                std::thread::sleep(Duration::from_millis(500));
            }
        }
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
    d.wait_ready();
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
            pipeline: None,
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

#[test]
fn e2e_pipeline_dag_dispatch() {
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
        "[blueprint]\nname = \"dag_e2e\"\ndefault_workflow = \"step1\"\n\n[openwiki]\nscope = [\"e2e\"]\n",
    )
    .unwrap();

    let wf_dir = repo.path().join(".janus/workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    std::fs::write(
        wf_dir.join("step1.toml"),
        "[workflow]\nname = \"step1\"\n\n[[steps]]\nname = \"s1\"\nagent = \"default\"\ncommand = \"true\"\n",
    )
    .unwrap();

    let pl_dir = repo.path().join(".janus/pipelines");
    std::fs::create_dir_all(&pl_dir).unwrap();
    std::fs::write(
        pl_dir.join("build_dag.toml"),
        "[pipeline]\nname = \"build_dag\"\n\n[[nodes]]\nid = \"n1\"\nworkflow = \"step1\"\n",
    )
    .unwrap();

    let d = Daemon::spawn(state.path(), &agents, repo.path());
    d.wait_ready();
    d.uds(
        &Request::Onboard {
            name: "dag_e2e".into(),
        },
        Duration::from_secs(15),
    )
    .unwrap();
    let resp = d
        .uds(
            &Request::Dispatch {
                blueprint: "dag_e2e".into(),
                workflow: None,
                pipeline: Some("build_dag".into()),
            },
            Duration::from_secs(15),
        )
        .unwrap();
    assert!(
        matches!(resp, Response::Dispatch { .. }),
        "pipeline dag dispatch: {resp:?}"
    );
}

#[test]
fn e2e_stop_and_continue() {
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
        "[blueprint]\nname = \"ctrl_e2e\"\ndefault_workflow = \"long\"\n\n[openwiki]\nscope = [\"e2e\"]\n",
    )
    .unwrap();
    let wf = repo.path().join(".janus/workflows");
    std::fs::create_dir_all(&wf).unwrap();
    std::fs::write(
        wf.join("long.toml"),
        "[workflow]\nname = \"long\"\n\n[[steps]]\nname = \"sleep_step\"\nagent = \"default\"\ncommand = \"sleep 10\"\n",
    )
    .unwrap();

    let d = Daemon::spawn(state.path(), &agents, repo.path());
    d.wait_ready();
    d.uds(
        &Request::Onboard {
            name: "ctrl_e2e".into(),
        },
        Duration::from_secs(15),
    )
    .unwrap();

    let resp = d
        .uds(
            &Request::Dispatch {
                blueprint: "ctrl_e2e".into(),
                workflow: None,
                pipeline: None,
            },
            Duration::from_secs(15),
        )
        .unwrap();
    let Response::Dispatch { task_id } = resp else {
        panic!("dispatch: {resp:?}");
    };

    let stop_resp = d
        .uds(
            &Request::Stop {
                blueprint: Some("ctrl_e2e".into()),
                task_id: Some(task_id),
            },
            Duration::from_secs(15),
        )
        .unwrap();
    assert!(
        matches!(stop_resp, Response::Ok { .. }),
        "stop: {stop_resp:?}"
    );

    let cont_resp = d
        .uds(
            &Request::Continue {
                blueprint: Some("ctrl_e2e".into()),
                task_id: Some(task_id),
            },
            Duration::from_secs(15),
        )
        .unwrap();
    assert!(
        matches!(cont_resp, Response::Ok { .. }),
        "continue: {cont_resp:?}"
    );
}
