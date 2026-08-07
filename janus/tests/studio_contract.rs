//! Integration tests for ADR-032: MetaMach Studio sidecar & UDS workflow endpoints.

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
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
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
fn utc_32_01_uds_workflow_list_get_save() {
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
        "[blueprint]\nname = \"studio_e2e\"\ndefault_workflow = \"w1\"\n\n[openwiki]\nscope = [\"e2e\"]\n",
    )
    .unwrap();
    let wf_dir = repo.path().join(".janus/workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    std::fs::write(
        wf_dir.join("w1.toml"),
        "[workflow]\nname = \"w1\"\n\n[[steps]]\nname = \"s1\"\nagent = \"default\"\ncommand = \"true\"\n",
    )
    .unwrap();

    let d = Daemon::spawn(state.path(), &agents, repo.path());
    d.wait_ready();

    // 1. ListWorkflows UDS request
    let resp = d
        .uds(
            &Request::ListWorkflows { blueprint: None },
            Duration::from_secs(5),
        )
        .unwrap();
    let Response::Workflows { names } = resp else {
        panic!("expected Workflows, got {resp:?}");
    };
    assert!(names.contains(&"w1".to_string()));

    // 2. GetWorkflow UDS request
    let resp = d
        .uds(
            &Request::GetWorkflow {
                blueprint: None,
                name: "w1".into(),
            },
            Duration::from_secs(5),
        )
        .unwrap();
    let Response::WorkflowContent { name, content } = resp else {
        panic!("expected WorkflowContent, got {resp:?}");
    };
    assert_eq!(name, "w1");
    assert!(content.contains("[workflow]"));

    // 3. SaveWorkflow UDS request
    let new_toml = "[workflow]\nname = \"w2\"\n\n[[steps]]\nname = \"s2\"\nagent = \"default\"\ncommand = \"echo saved\"\n";
    let resp = d
        .uds(
            &Request::SaveWorkflow {
                blueprint: None,
                name: "w2".into(),
                content: new_toml.into(),
            },
            Duration::from_secs(5),
        )
        .unwrap();
    assert!(matches!(resp, Response::Ok { .. }));

    // Verify w2 was saved and can be listed
    let resp = d
        .uds(
            &Request::ListWorkflows { blueprint: None },
            Duration::from_secs(5),
        )
        .unwrap();
    let Response::Workflows { names } = resp else {
        panic!("expected Workflows, got {resp:?}");
    };
    assert!(names.contains(&"w2".to_string()));
}
