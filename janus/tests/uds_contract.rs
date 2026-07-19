//! M5 Task 5.2: `janush` <-> `janus-daemon` UDS contract round-trip (Contracts
//! 3.2/3.4). Spawns a real `janus-daemon` with an isolated state dir + a test
//! `agents.toml`, then drives `Ping` + `GuardCheck` for all three verdict types
//! over the live `janus.sock`. PG is intentionally absent - the daemon runs in
//! degraded mode, which still serves Ping/GuardCheck (the HITL suspend + gateway
//! dispatch are fire-and-forget; their PG failures are warned, not fatal).
//!
//! Covers Test-Spec UTC-01-01 (daemon binds socket + pid) and the Contract
//! 3.2/3.4 payload round-trip across the module boundary.

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use janus::protocol::{Request, Response};
use janus::uds;

/// Test agents.toml: a default agent (bash_safe + root-delete blacklist) and a
/// deployer agent (require_approval on `make flash`, financial on `hi5bot`).
const AGENTS_TOML: &str = r#"
[agent.default]
bash_safe = true
bash_blacklist = ["rm -rf /"]

[agent.deployer]
permissions = ["read", "write", "bash-full", "ssh"]
require_approval = ["make flash"]
financial = ["hi5bot --action execute"]
"#;

/// A spawned daemon, cleaned up on drop.
struct Daemon {
    child: std::process::Child,
    sock: std::path::PathBuf,
}

impl Daemon {
    fn spawn(state_dir: &std::path::Path, agents: &std::path::Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_janus-daemon"))
            .env("HERDR_PLUGIN_STATE_DIR", state_dir)
            .env("JANUS_AGENTS_TOML", agents)
            .env("JANUS_GATEWAY_LISTEN_PORT", "0") // ephemeral; avoid 8443 clashes
            .env("RUST_LOG", "warn")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn janus-daemon");
        let sock = state_dir.join("janus.sock");
        // The daemon binds the socket asynchronously; poll until it appears.
        let start = Instant::now();
        while !sock.exists() && start.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(sock.exists(), "daemon did not bind janus.sock within 15s");
        // Give the listener a beat to enter accept().
        std::thread::sleep(Duration::from_millis(100));
        Daemon { child, sock }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Build a `GuardCheck` for `agent` running `cmd` (as `sh -c "<cmd>"`).
fn guard_check(agent: &str, cmd: &str) -> Request {
    let mut env = HashMap::new();
    env.insert("JANUS_AGENT".to_string(), agent.to_string());
    Request::GuardCheck {
        execution_id: uuid::Uuid::new_v4().to_string(),
        blueprint_id: None,
        task_id: None,
        step_name: None,
        cwd: None,
        argv: vec!["-c".to_string(), cmd.to_string()],
        env_snapshot: env,
    }
}

fn assert_verdict(resp: Response, want: &str) {
    match resp {
        Response::GuardVerdict { verdict, .. } => {
            assert_eq!(verdict, want, "expected verdict {want:?}")
        }
        other => panic!("expected GuardVerdict, got {other:?}"),
    }
}

#[test]
fn utc_01_01_daemon_binds_socket_and_pid() {
    // UTC-01-01: daemon startup produces janus.sock + janus.pid under the
    // state dir (singleton lock with stale detection).
    let dir = tempfile::tempdir().unwrap();
    let agents = dir.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();
    let _d = Daemon::spawn(dir.path(), &agents);
    assert!(dir.path().join("janus.sock").exists(), "socket missing");
    assert!(dir.path().join("janus.pid").exists(), "pid file missing");
}

#[test]
fn contract_3_2_and_3_4_uds_round_trip() {
    // Contract 3.2 (GuardCheck request) + 3.4 (verdict response) over the live
    // UDS path: Ping -> Pong, then ALLOW / BLOCK (blacklist) / BLOCK
    // (require_approval) / REWRITE (financial).
    let dir = tempfile::tempdir().unwrap();
    let agents = dir.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();
    let d = Daemon::spawn(dir.path(), &agents);
    let timeout = Duration::from_secs(5);

    // Contract 3.2 liveness: Ping -> Pong.
    let resp = uds::request_to(&d.sock, &Request::Ping, timeout).unwrap();
    assert!(
        matches!(resp, Response::Pong),
        "expected Pong, got {resp:?}"
    );

    // ALLOW: default agent (bash_safe) running a read command.
    let resp = uds::request_to(&d.sock, &guard_check("default", "ls -la"), timeout).unwrap();
    assert_verdict(resp, "ALLOW");

    // BLOCK (blacklist): default agent running `rm -rf /`.
    let resp = uds::request_to(&d.sock, &guard_check("default", "rm -rf /"), timeout).unwrap();
    assert_verdict(resp, "BLOCK");

    // BLOCK (require_approval): deployer running `make flash`.
    let resp = uds::request_to(&d.sock, &guard_check("deployer", "make flash"), timeout).unwrap();
    assert_verdict(resp, "BLOCK");

    // REWRITE (financial): deployer running `hi5bot --action execute` -> dry-run.
    let resp = uds::request_to(
        &d.sock,
        &guard_check("deployer", "hi5bot --action execute"),
        timeout,
    )
    .unwrap();
    match resp {
        Response::GuardVerdict {
            verdict,
            rewritten_argv,
            ..
        } => {
            assert_eq!(verdict, "REWRITE");
            assert_eq!(
                rewritten_argv.as_deref(),
                Some(
                    &vec![
                        "hi5bot".to_string(),
                        "--action".to_string(),
                        "dry-run".to_string()
                    ][..]
                )
            );
        }
        other => panic!("expected GuardVerdict(REWRITE), got {other:?}"),
    }
}

#[test]
fn utc_01_01_second_launch_refuses_duplicate_pid_lock() {
    // UTC-01-01 (full UAT): while the first daemon is alive, a second launch
    // against the same state dir detects the live PID lock, refuses to bind,
    // and exits non-zero WITHOUT breaking the original socket.
    let dir = tempfile::tempdir().unwrap();
    let agents = dir.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();
    let d1 = Daemon::spawn(dir.path(), &agents); // holds the lock + socket

    // Second launch -> must fail fast (acquire_pid_lock sees a live PID).
    let d2 = Command::new(env!("CARGO_BIN_EXE_janus-daemon"))
        .env("HERDR_PLUGIN_STATE_DIR", dir.path())
        .env("JANUS_AGENTS_TOML", &agents)
        .env("JANUS_GATEWAY_LISTEN_PORT", "0")
        .env("RUST_LOG", "warn")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn second janus-daemon");
    let output = d2.wait_with_output().expect("wait second daemon");
    assert!(
        !output.status.success(),
        "second daemon should exit non-zero (PID lock conflict), got {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already running"),
        "expected PID-lock conflict on stderr, got: {stderr}"
    );

    // The first daemon is unaffected - its socket still serves.
    let resp = uds::request_to(&d1.sock, &Request::Ping, Duration::from_secs(5)).unwrap();
    assert!(matches!(resp, Response::Pong));
}
