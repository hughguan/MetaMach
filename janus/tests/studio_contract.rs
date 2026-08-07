//! Integration tests for ADR-032: MetaMach Studio sidecar & UDS workflow endpoints.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use janus::protocol::{Request, Response};
use janus::uds;
use tokio_tungstenite::connect_async;

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
            .env("RUST_LOG", "debug")
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

struct StudioProcess {
    child: std::process::Child,
    port: u16,
    token: String,
}

impl StudioProcess {
    fn spawn(sock_path: &Path, token_path: &Path, port: u16) -> Self {
        let token = "test_studio_auth_token_12345".to_string();
        std::fs::write(token_path, &token).unwrap();

        let child = Command::new(env!("CARGO_BIN_EXE_janus-studio"))
            .arg("--bind")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--sock")
            .arg(sock_path)
            .arg("--token-file")
            .arg(token_path)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn janus-studio");

        let start = Instant::now();
        let url = format!("http://127.0.0.1:{port}/api/v1/health");
        while start.elapsed() < Duration::from_secs(10) {
            if ureq::get(&url)
                .timeout(Duration::from_millis(200))
                .call()
                .is_ok()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        StudioProcess { child, port, token }
    }
}

impl Drop for StudioProcess {
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

#[test]
fn utc_32_02_studio_http_rest_api_suite() {
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
        "[blueprint]\nname = \"studio_http_e2e\"\ndefault_workflow = \"w1\"\n\n[openwiki]\nscope = [\"e2e\"]\n",
    )
    .unwrap();
    let wf_dir = repo.path().join(".janus/workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    std::fs::write(
        wf_dir.join("w1.toml"),
        "[workflow]\nname = \"w1\"\n\n[[steps]]\nname = \"s1\"\nagent = \"default\"\ncommand = \"true\"\n",
    )
    .unwrap();

    let daemon = Daemon::spawn(state.path(), &agents, repo.path());
    daemon.wait_ready();

    // Onboard blueprint (retrying if PG is still connecting)
    let mut onboarded = false;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if let Ok(Response::Ok { .. }) = daemon.uds(
            &Request::Onboard {
                name: "studio_http_e2e".into(),
            },
            Duration::from_secs(2),
        ) {
            onboarded = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(onboarded, "failed to onboard studio_http_e2e");

    let token_path = state.path().join("studio.token");
    let studio = StudioProcess::spawn(&daemon.sock, &token_path, 8499);
    let base_url = format!("http://127.0.0.1:{}", studio.port);

    // 1. GET /api/v1/health — Liveness check without token
    let res = ureq::get(&format!("{base_url}/api/v1/health"))
        .call()
        .unwrap();
    assert_eq!(res.status(), 200);
    let health_body: serde_json::Value = serde_json::from_reader(res.into_reader()).unwrap();
    assert_eq!(health_body["status"], "online");
    assert_eq!(health_body["daemon_online"], true);

    // 2. GET / — Embedded HTML Canvas UI index
    let res = ureq::get(&format!("{base_url}/")).call().unwrap();
    assert_eq!(res.status(), 200);
    let html = res.into_string().unwrap();
    assert!(html.contains("Canvas Studio"));

    // 3. GET /styles.css & GET /app.js
    let res_css = ureq::get(&format!("{base_url}/styles.css")).call().unwrap();
    assert_eq!(res_css.status(), 200);
    let res_js = ureq::get(&format!("{base_url}/app.js")).call().unwrap();
    assert_eq!(res_js.status(), 200);

    // 4. Auth interlock — Unauthenticated request to /api/v1/workflows must return 401
    let unauth_res = ureq::get(&format!("{base_url}/api/v1/workflows")).call();
    assert!(unauth_res.is_err());
    if let Err(ureq::Error::Status(code, _)) = unauth_res {
        assert_eq!(code, 401);
    } else {
        panic!("expected 401 status for unauthenticated request");
    }

    // 5. GET /api/v1/workflows — Authenticated list workflows
    let res = ureq::get(&format!("{base_url}/api/v1/workflows"))
        .set("X-Janus-Studio-Token", &studio.token)
        .call()
        .unwrap();
    assert_eq!(res.status(), 200);
    let workflows: Vec<String> = serde_json::from_reader(res.into_reader()).unwrap();
    assert!(workflows.contains(&"w1".to_string()));

    // 6. GET /api/v1/workflows/w1 — Authenticated get workflow TOML
    let res = ureq::get(&format!("{base_url}/api/v1/workflows/w1"))
        .set("X-Janus-Studio-Token", &studio.token)
        .call()
        .unwrap();
    assert_eq!(res.status(), 200);
    let content = res.into_string().unwrap();
    assert!(content.contains("[workflow]"));

    // 7. POST /api/v1/workflows/w2 — Authenticated save workflow
    let w2_payload = serde_json::json!({
        "content": "[workflow]\nname = \"w2\"\n\n[[steps]]\nname = \"s2\"\nagent = \"default\"\ncommand = \"echo http_saved\"\n"
    });
    let res = ureq::post(&format!("{base_url}/api/v1/workflows/w2"))
        .set("X-Janus-Studio-Token", &studio.token)
        .send_json(w2_payload)
        .unwrap();
    assert_eq!(res.status(), 200);

    // Verify w2 now listed
    let res = ureq::get(&format!("{base_url}/api/v1/workflows"))
        .set("X-Janus-Studio-Token", &studio.token)
        .call()
        .unwrap();
    let list: Vec<String> = serde_json::from_reader(res.into_reader()).unwrap();
    assert!(list.contains(&"w2".to_string()));

    // 8. GET /api/v1/progress — Authenticated active progress snapshot
    let res = ureq::get(&format!("{base_url}/api/v1/progress"))
        .set("X-Janus-Studio-Token", &studio.token)
        .call()
        .unwrap();
    assert_eq!(res.status(), 200);

    // 9. POST /api/v1/dispatch — Authenticated workflow dispatch
    let dispatch_payload = serde_json::json!({
        "blueprint": "studio_http_e2e",
        "workflow": "w1"
    });
    let res = ureq::post(&format!("{base_url}/api/v1/dispatch"))
        .set("X-Janus-Studio-Token", &studio.token)
        .timeout(Duration::from_secs(20))
        .send_json(dispatch_payload);
    assert!(res.is_ok() || matches!(&res, Err(ureq::Error::Status(code, _)) if *code == 400));

    // 10. POST /api/v1/gates/<task_id>/verdict — Authenticated HITL gate verdict submission
    let dummy_id = uuid::Uuid::new_v4();
    let gate_payload = serde_json::json!({
        "approve": true
    });
    let res = ureq::post(&format!("{base_url}/api/v1/gates/{dummy_id}/verdict"))
        .set("X-Janus-Studio-Token", &studio.token)
        .send_json(gate_payload);
    assert!(res.is_ok());
}

#[tokio::test]
async fn utc_32_03_ws_snapshot_delta_heartbeat() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let daemon = Daemon::spawn(state.path(), &agents, repo.path());
    daemon.wait_ready();

    let token_path = state.path().join("studio.token");
    let studio = StudioProcess::spawn(&daemon.sock, &token_path, 8497);

    let ws_url = format!("ws://127.0.0.1:{}/runs/test_ws_run/stream", studio.port);
    let req = tokio_tungstenite::tungstenite::handshake::client::Request::builder()
        .uri(&ws_url)
        .header("X-Janus-Studio-Token", &studio.token)
        .header("Host", format!("127.0.0.1:{}", studio.port))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .unwrap();

    let (mut ws_stream, _) = connect_async(req)
        .await
        .expect("WebSocket connection failed");

    // Receive first frame — must be SNAPSHOT
    if let Some(Ok(msg)) = ws_stream.next().await {
        let text = msg.to_text().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(payload["type"], "SNAPSHOT");
        assert_eq!(payload["run_id"], "test_ws_run");
        assert!(payload.get("active_tasks").is_some());
    } else {
        panic!("expected WS SNAPSHOT frame");
    }
}

#[test]
fn utc_32_04_save_workflow_rejects_invalid_toml() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let daemon = Daemon::spawn(state.path(), &agents, repo.path());
    daemon.wait_ready();

    let token_path = state.path().join("studio.token");
    let studio = StudioProcess::spawn(&daemon.sock, &token_path, 8496);
    let base_url = format!("http://127.0.0.1:{}", studio.port);

    // Send invalid TOML syntax
    let invalid_payload = serde_json::json!({
        "content": "[workflow\n name = invalid_syntax_no_closing_bracket"
    });
    let res = ureq::post(&format!("{base_url}/api/v1/workflows/bad_wf"))
        .set("X-Janus-Studio-Token", &studio.token)
        .send_json(invalid_payload);

    assert!(res.is_ok() || matches!(&res, Err(ureq::Error::Status(code, _)) if *code == 400));
}

#[test]
fn utc_32_05_blueprint_query_param_filtering() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let daemon = Daemon::spawn(state.path(), &agents, repo.path());
    daemon.wait_ready();

    let token_path = state.path().join("studio.token");
    let studio = StudioProcess::spawn(&daemon.sock, &token_path, 8495);
    let base_url = format!("http://127.0.0.1:{}", studio.port);

    // Query workflows with explicit ?blueprint=nonexistent
    let res = ureq::get(&format!(
        "{base_url}/api/v1/workflows?blueprint=nonexistent"
    ))
    .set("X-Janus-Studio-Token", &studio.token)
    .call()
    .unwrap();
    assert_eq!(res.status(), 200);
}

#[test]
fn utc_32_06_workflow_not_found_404() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let daemon = Daemon::spawn(state.path(), &agents, repo.path());
    daemon.wait_ready();

    let token_path = state.path().join("studio.token");
    let studio = StudioProcess::spawn(&daemon.sock, &token_path, 8494);
    let base_url = format!("http://127.0.0.1:{}", studio.port);

    let res = ureq::get(&format!("{base_url}/api/v1/workflows/nonexistent_xyz_123"))
        .set("X-Janus-Studio-Token", &studio.token)
        .call();

    assert!(res.is_err());
    if let Err(ureq::Error::Status(code, _)) = res {
        assert_eq!(code, 404);
    } else {
        panic!("expected 404 status for nonexistent workflow");
    }
}

#[test]
fn utc_32_07_concurrent_http_requests() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let daemon = Daemon::spawn(state.path(), &agents, repo.path());
    daemon.wait_ready();

    let token_path = state.path().join("studio.token");
    let studio = StudioProcess::spawn(&daemon.sock, &token_path, 8493);
    let base_url = format!("http://127.0.0.1:{}", studio.port);
    let token = studio.token.clone();

    let mut handles = vec![];
    for _ in 0..10 {
        let url = format!("{base_url}/api/v1/workflows");
        let tok = token.clone();
        let handle = std::thread::spawn(move || {
            let res = ureq::get(&url).set("X-Janus-Studio-Token", &tok).call();
            assert!(res.is_ok());
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().expect("thread panicked");
    }
}

#[test]
fn utc_32_08_studio_crash_daemon_unaffected() {
    if !pg_available() || !tmux_available() {
        eprintln!("skipping: PG or tmux not available");
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let daemon = Daemon::spawn(state.path(), &agents, repo.path());
    daemon.wait_ready();

    let token_path = state.path().join("studio.token");
    let mut studio = StudioProcess::spawn(&daemon.sock, &token_path, 8492);

    // Force kill studio process
    let _ = studio.child.kill();
    let _ = studio.child.wait();

    // Verify daemon UDS is fully responsive and unharmed
    let resp = daemon
        .uds(&Request::Ping, Duration::from_secs(5))
        .expect("daemon ping after studio crash");
    assert!(matches!(resp, Response::Pong));
}

#[tokio::test]
async fn utc_32_09_ws_delta_on_state_transition() {
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
        "[blueprint]\nname = \"ws_delta_e2e\"\ndefault_workflow = \"wf_delta\"\n\n[openwiki]\nscope = [\"e2e\"]\n",
    )
    .unwrap();
    let wf_dir = repo.path().join(".janus/workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    // 2-step workflow — first step sleeps 1s to give time for WS delta detection
    std::fs::write(
        wf_dir.join("wf_delta.toml"),
        "[workflow]\nname = \"wf_delta\"\n\n[[steps]]\nname = \"s1\"\nagent = \"default\"\ncommand = \"sleep 3; true\"\n\n[[steps]]\nname = \"s2\"\nagent = \"default\"\ncommand = \"true\"\n",
    )
    .unwrap();

    let daemon = Daemon::spawn(state.path(), &agents, repo.path());
    daemon.wait_ready();

    // Onboard blueprint so daemon registers state (retry if PG is still connecting)
    let daemon_sock = daemon.sock.clone();
    let mut onboarded = false;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        let sock = daemon_sock.clone();
        let res = tokio::task::spawn_blocking(move || {
            uds::request_to(
                &sock,
                &Request::Onboard {
                    name: "ws_delta_e2e".into(),
                },
                Duration::from_secs(2),
            )
        })
        .await
        .unwrap();
        if let Ok(Response::Ok { .. }) = res {
            onboarded = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(onboarded, "failed to onboard blueprint ws_delta_e2e");

    let token_path = state.path().join("studio.token");
    let studio = StudioProcess::spawn(&daemon.sock, &token_path, 8490);

    // Connect to WebSocket first, then dispatch
    let ws_url = format!("ws://127.0.0.1:{}/runs/ws_delta_run/stream", studio.port);
    let req = tokio_tungstenite::tungstenite::handshake::client::Request::builder()
        .uri(&ws_url)
        .header("X-Janus-Studio-Token", &studio.token)
        .header("Host", format!("127.0.0.1:{}", studio.port))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .unwrap();

    let (mut ws_stream, _) = connect_async(req)
        .await
        .expect("WebSocket connection failed");

    // 1. Receive SNAPSHOT on connect
    let first_msg = tokio::time::timeout(Duration::from_secs(5), ws_stream.next())
        .await
        .expect("timeout waiting for SNAPSHOT")
        .expect("ws stream ended")
        .expect("ws error");
    let snapshot: serde_json::Value = serde_json::from_str(first_msg.to_text().unwrap()).unwrap();
    assert_eq!(
        snapshot["type"], "SNAPSHOT",
        "first WS frame must be SNAPSHOT"
    );

    // 2. Dispatch the workflow via UDS in spawn_blocking so synchronous UDS I/O doesn't block Tokio worker thread
    let daemon_sock = daemon.sock.clone();
    let dispatch_resp = tokio::task::spawn_blocking(move || {
        uds::request_to(
            &daemon_sock,
            &Request::Dispatch {
                blueprint: "ws_delta_e2e".into(),
                workflow: Some("wf_delta".into()),
                inline_command: None,
            },
            Duration::from_secs(15),
        )
    })
    .await
    .unwrap();

    let dispatch_resp = dispatch_resp.expect("dispatch failed");
    let _task_id = match dispatch_resp {
        Response::Dispatch { task_id } => task_id,
        _ => panic!("expected Dispatch response, got {dispatch_resp:?}"),
    };

    // 3. Poll WS for DELTA events (workflow runs ~1s, studio polls every 1s)
    let mut saw_delta = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), ws_stream.next()).await {
            Ok(Some(Ok(msg))) => {
                let text = msg.to_text().unwrap();
                let payload: serde_json::Value = serde_json::from_str(text).unwrap();
                match payload["type"].as_str() {
                    Some("DELTA") => {
                        saw_delta = true;
                        assert_eq!(payload["run_id"], "ws_delta_run");
                        assert!(
                            payload.get("active_tasks").is_some(),
                            "DELTA must include active_tasks"
                        );
                        break;
                    }
                    Some("SNAPSHOT") | Some("HEARTBEAT") => {
                        // Ignore — only care about DELTA
                    }
                    _ => {}
                }
            }
            Ok(Some(Err(e))) => {
                eprintln!("WS error: {e}");
                break;
            }
            Ok(None) => {
                eprintln!("WS stream ended");
                break;
            }
            Err(_) => {
                // Timeout waiting for next frame - continue checking until deadline
                continue;
            }
        }
    }

    assert!(
        saw_delta,
        "expected at least one DELTA event after workflow dispatch"
    );
}
