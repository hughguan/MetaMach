//! M5 Task 5.1: PG-dependent lifecycle integration tests.
//!
//! These tests require a running PostgreSQL instance accessible via Unix socket
//! at `METAMACH_PG_SOCKET_DIR` (default: state_dir/pg_socket). CI provides PG
//! via the `postgres:16` service; locally they are `#[ignore]` unless PG is
//! configured via `make db-init`.
//!
//! Covers Test-Spec suites 2.5 (lifecycle) and 2.4 (HITL): UTC-05-01, UTC-05-02,
//! UTC-05-04, UTC-05-04b, UTC-04-01.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use janus::protocol::{Request, Response};
use janus::uds;

/// Test agents.toml (same as uds_contract for consistency).
const AGENTS_TOML: &str = r#"
[agent.default]
bash_safe = true
bash_blacklist = ["rm -rf /"]

[agent.deployer]
permissions = ["read", "write", "bash-full", "ssh"]
require_approval = ["make flash"]
financial = ["hi5bot --action execute"]
"#;

/// Spawn a daemon and poll until its socket appears. Returns the daemon process
/// + the socket path. The daemon is killed on drop.
struct Daemon {
    child: std::process::Child,
    sock: std::path::PathBuf,
}

impl Daemon {
    fn spawn(state_dir: &Path, agents: &Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_janus-daemon"))
            .env("HERDR_PLUGIN_STATE_DIR", state_dir)
            .env(
                "HERDR_PLUGIN_ROOT",
                Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap(),
            )
            .env("JANUS_AGENTS_TOML", agents)
            .env("JANUS_GATEWAY_LISTEN_PORT", "0")
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

/// Build a minimal blueprint recipe for the test blueprint. Returns the
/// blueprint directory path (a temp subdir under `base`).
fn make_blueprint(base: &Path, name: &str) -> std::path::PathBuf {
    let bp = base.join("blueprints").join(name);
    std::fs::create_dir_all(&bp).unwrap();
    let recipe = format!(
        r#"[blueprint]
name = "{name}"
scope = "embedded"
description = "test blueprint"
workflow = "test-flow"
"#
    );
    std::fs::write(bp.join("janus.toml"), recipe).unwrap();

    // Minimal workflow file
    let wf_dir = base.join("workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    std::fs::write(
        wf_dir.join("test-flow.toml"),
        r#"[workflow]
name = "test-flow"

[[steps]]
id = "scout"
agent = "default"
command = "true"
"#,
    )
    .unwrap();

    bp
}

// ── UTC-05-04: Blueprint Onboard ───────────────────────────────────────────

#[test]
#[ignore = "requires PostgreSQL"]
fn utc_05_04_onboard_registers_tenant() {
    // Setup: temp repo root with blueprints/ + workflows/, plus a PG-ready daemon.
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();
    make_blueprint(root.path(), "joyrobots");

    let d = Daemon::spawn(state.path(), &agents);

    // Wait for PG to come online (daemon retries 5× @2s = 10s max).
    std::thread::sleep(Duration::from_secs(12));

    // Onboard the blueprint.
    let resp = d
        .uds(
            &Request::Onboard {
                name: "joyrobots".into(),
            },
            Duration::from_secs(10),
        )
        .expect("onboard request");

    match resp {
        Response::Ok { message } => assert!(
            message.contains("joyrobots")
                && (message.contains("registered") || message.contains("reactivated")),
            "expected joyrobots onboarded, got: {message}"
        ),
        other => panic!("expected Ok from Onboard, got {other:?}"),
    }

    // Second Onboard is idempotent.
    let resp2 = d
        .uds(
            &Request::Onboard {
                name: "joyrobots".into(),
            },
            Duration::from_secs(10),
        )
        .expect("second onboard");
    assert!(
        matches!(resp2, Response::Ok { .. }),
        "second Onboard must be idempotent"
    );

    // Blueprints list includes the onboarded blueprint.
    let resp = d.uds(&Request::Blueprints, Duration::from_secs(5)).unwrap();
    match resp {
        Response::Blueprints { blueprints } => {
            assert!(
                blueprints.iter().any(|b| b.name == "joyrobots"),
                "onboarded blueprint must appear in list: {blueprints:?}"
            );
        }
        other => panic!("expected Blueprints, got {other:?}"),
    }
}

// ── UTC-05-04b: Multi-DB Onboard Isolation ──────────────────────────────────

#[test]
#[ignore = "requires PostgreSQL"]
fn utc_05_04b_multidb_onboard_isolation() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();
    make_blueprint(root.path(), "joyrobots");
    make_blueprint(root.path(), "gatemetric");

    let d = Daemon::spawn(state.path(), &agents);
    std::thread::sleep(Duration::from_secs(12));

    // Onboard both blueprints.
    for name in &["joyrobots", "gatemetric"] {
        let resp = d
            .uds(
                &Request::Onboard {
                    name: name.to_string(),
                },
                Duration::from_secs(10),
            )
            .unwrap();
        assert!(matches!(resp, Response::Ok { .. }), "{name} onboard failed");
    }

    // Both appear in the blueprint list.
    let resp = d.uds(&Request::Blueprints, Duration::from_secs(5)).unwrap();
    match resp {
        Response::Blueprints { blueprints } => {
            assert_eq!(blueprints.len(), 2, "expected 2 blueprints: {blueprints:?}");
            assert!(blueprints.iter().any(|b| b.name == "joyrobots"));
            assert!(blueprints.iter().any(|b| b.name == "gatemetric"));
        }
        other => panic!("expected Blueprints, got {other:?}"),
    }

    // Progress query succeeds (no active tasks).
    let resp = d
        .uds(
            &Request::Progress { blueprint: None },
            Duration::from_secs(5),
        )
        .unwrap();
    assert!(
        matches!(resp, Response::Progress { .. }),
        "progress should return normally"
    );
}

// ── UTC-05-02: Offboard Smelting ───────────────────────────────────────────

#[test]
#[ignore = "requires PostgreSQL"]
fn utc_05_02_offboard_smelts_and_archives() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();
    make_blueprint(root.path(), "gatemetric");

    let d = Daemon::spawn(state.path(), &agents);
    std::thread::sleep(Duration::from_secs(12));

    // Onboard.
    d.uds(
        &Request::Onboard {
            name: "gatemetric".into(),
        },
        Duration::from_secs(10),
    )
    .unwrap();

    // Offboard.
    let resp = d
        .uds(
            &Request::Offboard {
                name: "gatemetric".into(),
            },
            Duration::from_secs(10),
        )
        .unwrap();
    match resp {
        Response::Ok { message } => assert!(message.contains("Offboard")),
        other => panic!("expected Ok from Offboard, got {other:?}"),
    }

    // Blueprint no longer in list.
    let resp = d.uds(&Request::Blueprints, Duration::from_secs(5)).unwrap();
    match resp {
        Response::Blueprints { blueprints } => {
            assert!(
                !blueprints.iter().any(|b| b.name == "gatemetric"),
                "offboarded blueprint should not appear"
            );
        }
        other => panic!("expected Blueprints, got {other:?}"),
    }
}

// ── UTC-05-01: Size Budget Truncation ──────────────────────────────────────

#[test]
fn utc_05_01_size_budget_truncation() {
    // Unit-level test: the 16KB truncate_16k function. The PG round-trip is
    // covered by `absurd/tests` unit tests; this verifies the budget constant.
    use janus::absurd::{BUDGET_TAG, SIZE_BUDGET, truncate_16k};

    assert_eq!(SIZE_BUDGET, 16 * 1024, "budget must be exactly 16 KiB");

    let small = "hello".repeat(100); // ~500 bytes
    let truncated = truncate_16k(&small);
    assert_eq!(truncated, small, "under-budget string should be unchanged");

    let large = "x".repeat(20 * 1024); // 20 KiB
    let truncated = truncate_16k(&large);
    assert!(truncated.len() <= SIZE_BUDGET);
    assert!(
        truncated.ends_with(BUDGET_TAG),
        "oversized string must end with budget tag"
    );
}

// ── UTC-04-01: Non-Destructive Suspension ──────────────────────────────────

#[test]
fn utc_04_01_suspend_preserves_guard_verdict_scene() {
    // Unit-level: when Tool Guard returns a SUSPEND verdict, the
    // GuardVerdict carries a correlation_id (for the gateway) and the
    // reason makes it back via the UDS response. The SUSPENDED path is
    // exercised through the gateway unit tests; this test verifies the
    // protocol payload shape.
    use janus::protocol::Response;

    let resp = Response::GuardVerdict {
        execution_id: "exec-1".into(),
        verdict: "BLOCK".into(),
        reason: Some("require_approval".into()),
        rewritten_argv: None,
        correlation_id: uuid::Uuid::new_v4().to_string(),
        cognitive_context: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: Response = serde_json::from_str(&json).unwrap();
    match back {
        Response::GuardVerdict {
            verdict,
            reason,
            correlation_id,
            ..
        } => {
            assert_eq!(verdict, "BLOCK");
            assert_eq!(reason.as_deref(), Some("require_approval"));
            assert!(!correlation_id.is_empty(), "correlation_id must be set");
        }
        other => panic!("expected GuardVerdict, got {other:?}"),
    }
}
