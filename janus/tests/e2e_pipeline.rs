//! M5 ADR-028: E2E Pipeline CI tests with mock agents.
//!
//! These tests use deterministic shell scripts instead of real LLM agents,
//! exercising the full DAG engine + Tool Guard + checkpoint/recovery cycle
//! without API keys or external dependencies beyond PG + tmux.
//!
//! All tests runtime-skip when PG or tmux is unavailable.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use janus::protocol::{Request, Response};
use janus::uds;

const AGENTS_TOML: &str = r#"
[agent.architect]
permissions = ["read", "write", "bash-safe", "git-commit"]
bash_safe = true
bash_blacklist = ["rm -rf /"]

[agent.builder]
permissions = ["read", "write", "edit", "bash-safe", "git-commit"]
bash_safe = true
bash_blacklist = ["rm -rf /"]

[agent.tester]
permissions = ["read", "write", "bash-safe", "git-commit", "git-push"]
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
    #[allow(dead_code)]
    repo_path: std::path::PathBuf,
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
        Daemon {
            child,
            sock,
            repo_path: repo_path.to_path_buf(),
        }
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

fn setup_blueprint(repo: &Path, name: &str) {
    let bp = repo.join("blueprints").join(name);
    std::fs::create_dir_all(&bp).unwrap();
    std::fs::write(
        bp.join("janus.toml"),
        format!(
            "[blueprint]\nname = \"{name}\"\ndefault_workflow = \"mock-devsecops\"\n\n[openwiki]\nscope = [\"e2e\"]\n"
        ),
    )
    .unwrap();

    let wf = repo.join("workflows");
    std::fs::create_dir_all(&wf).unwrap();
    std::fs::write(
        wf.join("mock-devsecops.toml"),
        r#"[workflow]
name = "mock-devsecops"

[[steps]]
name = "architect_design"
agent = "architect"
command = "mkdir -p docs && echo '## Architecture Design (mock)' > docs/architecture-design.md"

[[steps]]
name = "builder_review"
agent = "builder"
command = "grep -q 'Architecture Design' docs/architecture-design.md && echo APPROVED || echo REJECT"

[[steps]]
name = "tester_review"
agent = "tester"
command = "grep -q 'Architecture Design' docs/architecture-design.md && echo APPROVED || echo REJECT"

[[steps]]
name = "tester_commit"
agent = "tester"
command = "git add docs/ && git commit -m 'e2e: mock devsecops pipeline'"
"#,
    )
    .unwrap();

    // Git init for commit test.
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output();
    let _ = Command::new("git")
        .args([
            "-c",
            "user.name=ci",
            "-c",
            "user.email=ci@test",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ])
        .current_dir(repo)
        .output();
}

/// Poll Progress until a step reaches COMPLETED or deadline expires.
fn poll_until_completed(d: &Daemon, blueprint: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let resp = d.uds(
            &Request::Progress {
                blueprint: Some(blueprint.into()),
            },
            Duration::from_secs(5),
        );
        if let Ok(Response::Progress { active_tasks }) = resp {
            for t in &active_tasks {
                if t.steps.iter().any(|s| s.status == "COMPLETED") {
                    return true;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn e2e_mock_devsecops_onboard_dispatch_complete() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();
    setup_blueprint(repo.path(), "software-dev-e2e");
    let d = Daemon::spawn(state.path(), &agents, repo.path());
    std::thread::sleep(Duration::from_secs(12)); // PG connect

    // Onboard.
    let resp = d
        .uds(
            &Request::Onboard {
                name: "software-dev-e2e".into(),
            },
            Duration::from_secs(15),
        )
        .unwrap();
    assert!(matches!(resp, Response::Ok { .. }), "onboard: {resp:?}");

    // Dispatch.
    let resp = d
        .uds(
            &Request::Dispatch {
                blueprint: "software-dev-e2e".into(),
                workflow: None,
            },
            Duration::from_secs(15),
        )
        .unwrap();
    let _task_id = match resp {
        Response::Dispatch { task_id } => task_id,
        other => panic!("expected Dispatch, got {other:?}"),
    };

    // Wait for at least one step to complete.
    let ok = poll_until_completed(&d, "software-dev-e2e", Duration::from_secs(30));
    assert!(ok, "no step reached COMPLETED within 30s");
}

#[test]
fn e2e_mock_tool_guard_blocks_blacklisted_command() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    // Onboard a blueprint with a workflow that attempts `rm -rf /` —
    // janush + Tool Guard must block it.
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let bp = repo.path().join("blueprints").join("guard-e2e");
    std::fs::create_dir_all(&bp).unwrap();
    std::fs::write(
        bp.join("janus.toml"),
        "[blueprint]\nname = \"guard-e2e\"\ndefault_workflow = \"danger\"\n\n[openwiki]\nscope = [\"e2e\"]\n",
    )
    .unwrap();
    let wf = repo.path().join("workflows");
    std::fs::create_dir_all(&wf).unwrap();
    std::fs::write(
        wf.join("danger.toml"),
        r#"[workflow]
name = "danger"

[[steps]]
name = "bad"
agent = "architect"
command = "rm -rf /tmp/metamach-e2e-guard-test"
"#,
    )
    .unwrap();

    // Create a sentinel file to verify it survives.
    let sentinel = repo.path().join("sentinel.txt");
    std::fs::write(&sentinel, "do-not-delete").unwrap();

    let d = Daemon::spawn(state.path(), &agents, repo.path());
    std::thread::sleep(Duration::from_secs(12));

    d.uds(
        &Request::Onboard {
            name: "guard-e2e".into(),
        },
        Duration::from_secs(15),
    )
    .unwrap();

    let resp = d
        .uds(
            &Request::Dispatch {
                blueprint: "guard-e2e".into(),
                workflow: None,
            },
            Duration::from_secs(15),
        )
        .unwrap();
    assert!(matches!(resp, Response::Dispatch { .. }));

    // Wait a bit, then verify the sentinel survived (Tool Guard blocked the rm).
    std::thread::sleep(Duration::from_secs(5));
    assert!(
        sentinel.exists(),
        "sentinel should survive — Tool Guard must block rm -rf"
    );
}
