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
    /// Isolated repo root kept alive for the daemon's lifetime. Only the real
    /// `configs/` is copied in (so Offboard can load `configs/offboard.toml`);
    /// each test writes its OWN uniquely-named blueprint and workflow here via
    /// `make_blueprint`. Unique names mean each test owns an isolated catalog
    /// row and `metamach_blueprint_<name>` DB, so the PG-gated tests can run in
    /// parallel without racing on `CREATE DATABASE` or interfering via the
    /// shared catalog. Offboard writes (`production_report.md`, git commit)
    /// land here in the temp dir, never the real repo.
    repo: tempfile::TempDir,
}

impl Daemon {
    fn spawn(state_dir: &Path, agents: &Path) -> Self {
        let repo = tempfile::tempdir().expect("repo tempdir");
        // Copy real configs/ so Offboard can load configs/offboard.toml. We do
        // NOT copy blueprints/workflows - the test writes its own unique
        // blueprint + test-flow workflow via make_blueprint.
        let ws = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let src = ws.join("configs");
        if src.is_dir() {
            let _ = std::process::Command::new("cp")
                .arg("-R")
                .arg(&src)
                .arg(repo.path().join("configs"))
                .status();
        }
        let child = Command::new(env!("CARGO_BIN_EXE_janus-daemon"))
            .env("HERDR_PLUGIN_STATE_DIR", state_dir)
            .env("HERDR_PLUGIN_ROOT", repo.path())
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
        Daemon { child, sock, repo }
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

/// Build a valid minimal blueprint recipe under `base/blueprints/<name>/` plus
/// a `test-flow` workflow. The recipe matches Contract 3.6 (`[blueprint]` name
/// and `default_workflow`, `[openwiki]` scope) and the workflow matches
/// Contract 3.7 (steps keyed by `name`). Returns the blueprint directory path.
fn make_blueprint(base: &Path, name: &str) -> std::path::PathBuf {
    let bp = base.join("blueprints").join(name);
    std::fs::create_dir_all(&bp).unwrap();
    let recipe = format!(
        r#"[blueprint]
name = "{name}"
default_workflow = "test-flow"

[openwiki]
scope = ["test"]
"#
    );
    std::fs::write(bp.join("janus.toml"), recipe).unwrap();

    // Minimal workflow file (Contract 3.7: step `name`, not `id`).
    let wf_dir = base.join("workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    std::fs::write(
        wf_dir.join("test-flow.toml"),
        r#"[workflow]
name = "test-flow"

[[steps]]
name = "scout"
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
    // Unique blueprint name => isolated catalog row + blueprint DB, so this test
    // never collides with the other PG-gated onboards running in parallel.
    const NAME: &str = "joy_05_04";
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let d = Daemon::spawn(state.path(), &agents);
    make_blueprint(d.repo.path(), NAME);

    // Wait for PG to come online (daemon retries 5× @2s = 10s max).
    std::thread::sleep(Duration::from_secs(12));

    // Onboard the blueprint.
    let resp = d
        .uds(
            &Request::Onboard { name: NAME.into() },
            Duration::from_secs(10),
        )
        .expect("onboard request");

    match resp {
        Response::Ok { message } => assert!(
            message.contains(NAME)
                && (message.contains("registered") || message.contains("reactivated")),
            "expected {NAME} onboarded, got: {message}"
        ),
        other => panic!("expected Ok from Onboard, got {other:?}"),
    }

    // Second Onboard is idempotent.
    let resp2 = d
        .uds(
            &Request::Onboard { name: NAME.into() },
            Duration::from_secs(10),
        )
        .expect("second onboard");
    assert!(
        matches!(resp2, Response::Ok { .. }),
        "second Onboard must be idempotent, got {resp2:?}"
    );

    // Blueprints list includes the onboarded blueprint.
    let resp = d.uds(&Request::Blueprints, Duration::from_secs(5)).unwrap();
    match resp {
        Response::Blueprints { blueprints } => {
            assert!(
                blueprints.iter().any(|b| b.name == NAME),
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
    // Two uniquely-named blueprints => two isolated catalog rows + blueprint DBs.
    const JOY: &str = "joy_05_04b";
    const GATE: &str = "gate_05_04b";
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let d = Daemon::spawn(state.path(), &agents);
    make_blueprint(d.repo.path(), JOY);
    make_blueprint(d.repo.path(), GATE);
    std::thread::sleep(Duration::from_secs(12));

    // Onboard both blueprints.
    for name in [JOY, GATE] {
        let resp = d
            .uds(
                &Request::Onboard { name: name.into() },
                Duration::from_secs(10),
            )
            .unwrap();
        assert!(
            matches!(resp, Response::Ok { .. }),
            "{name} onboard failed: {resp:?}"
        );
    }

    // Both appear in the blueprint list. The catalog is shared across the
    // parallel PG-gated tests, so assert presence of our two (not an exact
    // count, which would flake on other tests' blueprints).
    let resp = d.uds(&Request::Blueprints, Duration::from_secs(5)).unwrap();
    match resp {
        Response::Blueprints { blueprints } => {
            assert!(
                blueprints.iter().any(|b| b.name == JOY),
                "{JOY} missing from list: {blueprints:?}"
            );
            assert!(
                blueprints.iter().any(|b| b.name == GATE),
                "{GATE} missing from list: {blueprints:?}"
            );
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
    // Unique name => this test's offboard (which marks the catalog row
    // OFFBOARDED + purges the blueprint DB) never touches another test's
    // blueprint, so parallel onboards are unaffected.
    const NAME: &str = "gate_05_02";
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let d = Daemon::spawn(state.path(), &agents);
    make_blueprint(d.repo.path(), NAME);
    std::thread::sleep(Duration::from_secs(12));

    // Onboard.
    d.uds(
        &Request::Onboard { name: NAME.into() },
        Duration::from_secs(10),
    )
    .unwrap();

    // Offboard.
    let resp = d
        .uds(
            &Request::Offboard { name: NAME.into() },
            Duration::from_secs(10),
        )
        .unwrap();
    match resp {
        Response::Ok { message } => assert!(
            message.contains(NAME) && message.contains("offboarded"),
            "expected {NAME} offboarded, got: {message}"
        ),
        other => panic!("expected Ok from Offboard, got {other:?}"),
    }

    // Blueprint no longer in list.
    let resp = d.uds(&Request::Blueprints, Duration::from_secs(5)).unwrap();
    match resp {
        Response::Blueprints { blueprints } => {
            assert!(
                !blueprints.iter().any(|b| b.name == NAME),
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

// ── UTC-05-03: Git Experience Inheritance (offboard commits the report) ────

#[test]
#[ignore = "requires PostgreSQL"]
fn utc_05_03_offboard_commits_production_report_to_git() {
    // Offboard best-effort `git commit`s production_report.md into the blueprint
    // repo so the next Onboard inherits it (the UTC-05-05 loop). Verify the
    // commit lands when the repo is a git repo with an identity configured.
    const NAME: &str = "gate_05_03";
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let d = Daemon::spawn(state.path(), &agents);
    make_blueprint(d.repo.path(), NAME);

    // git_commit_report runs `git` in repo_root; init a repo + identity there so
    // the commit succeeds (a non-git repo would no-op and return None).
    let repo = d.repo.path();
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("git")
    };
    assert!(git(&["init", "-q"]).status.success(), "git init");
    assert!(git(&["config", "user.name", "Test"]).status.success());
    assert!(
        git(&["config", "user.email", "test@example.com"])
            .status
            .success()
    );
    // Seed an initial commit so the offboard commit has a base HEAD.
    std::fs::write(repo.join("README.md"), "test\n").unwrap();
    assert!(git(&["add", "README.md"]).status.success(), "git add seed");
    assert!(
        git(&["commit", "-q", "-m", "seed"]).status.success(),
        "git commit seed: {}",
        String::from_utf8_lossy(&git(&["commit", "-q", "-m", "seed"]).stderr)
    );

    std::thread::sleep(Duration::from_secs(12));

    // Onboard + Offboard. Offboard writes production_report.md and commits it.
    d.uds(
        &Request::Onboard { name: NAME.into() },
        Duration::from_secs(10),
    )
    .unwrap();
    let resp = d
        .uds(
            &Request::Offboard { name: NAME.into() },
            Duration::from_secs(10),
        )
        .unwrap();
    assert!(
        matches!(resp, Response::Ok { .. }),
        "offboard failed: {resp:?}"
    );

    // The offboard commit landed in the repo's history.
    let log = git(&["log", "--oneline"]);
    assert!(
        log.status.success(),
        "git log: {}",
        String::from_utf8_lossy(&log.stderr)
    );
    let log_text = String::from_utf8_lossy(&log.stdout);
    assert!(
        log_text.contains("offboard production_report.md"),
        "expected offboard commit in git log: {log_text}"
    );
}

// ── UTC-05-05: Re-Onboard & Experience Inheritance ─────────────────────────

#[test]
#[ignore = "requires PostgreSQL"]
fn utc_05_05_re_onboard_inherits_previous_incidents() {
    // A prior Offboard's production_report.md is recycled as `## Previous
    // Incidents` few-shots on the next Onboard (experience inheritance). We
    // simulate a prior report (bullet lines are the parsed incidents) and verify
    // Onboard inherits them via the `inherited N previous-incident(s)` message.
    const NAME: &str = "gate_05_05";
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let d = Daemon::spawn(state.path(), &agents);
    make_blueprint(d.repo.path(), NAME);

    // Simulate a prior Offboard's report: `- ` bullet lines are the incidents
    // parse_incidents extracts.
    let openwiki = d.repo.path().join("blueprints").join(NAME).join("openwiki");
    std::fs::create_dir_all(&openwiki).unwrap();
    std::fs::write(
        openwiki.join("production_report.md"),
        "# Production Report\n\n## Previous Incidents\n\n\
         - PIN_CONFLICT_MARKER_21 on gpio5 (clash with mpu6050)\n\
         - i2c address 0x68 double-bound by scout + code steps\n",
    )
    .unwrap();

    std::thread::sleep(Duration::from_secs(12));

    let resp = d
        .uds(
            &Request::Onboard { name: NAME.into() },
            Duration::from_secs(10),
        )
        .expect("onboard request");
    match resp {
        Response::Ok { message } => assert!(
            message.contains("inherited 2 previous-incident"),
            "expected 2 inherited incidents from prior report, got: {message}"
        ),
        other => panic!("expected Ok from Onboard, got {other:?}"),
    }
}

// ── UTC-0a: Absurd schema loads on onboard (Phase 0a) ──────────────────────

#[test]
#[ignore = "requires PostgreSQL"]
fn utc_0a_absurd_schema_loads_on_onboard() {
    // Phase 0a: `ensure_blueprint_db` loads the vendored absurd.sql before the
    // 002/003 MetaMach overlays. Verify: absurd.get_schema_version() == "main",
    // absurd.create_queue + spawn_task are callable, and metamach_step_meta has
    // the new session_name column.
    const NAME: &str = "absurd0a";
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();
    let d = Daemon::spawn(state.path(), &agents);
    make_blueprint(d.repo.path(), NAME);
    std::thread::sleep(Duration::from_secs(12));

    // Onboard triggers ensure_blueprint_db -> init_absurd_schema + 002 + 003.
    // If absurd.sql fails to load, onboard errors here.
    let resp = d
        .uds(
            &Request::Onboard { name: NAME.into() },
            Duration::from_secs(15),
        )
        .unwrap();
    assert!(
        matches!(resp, Response::Ok { .. }),
        "onboard failed (absurd load?): {resp:?}"
    );

    // Connect to the blueprint DB and verify absurd is loaded + usable.
    let catalog_url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let bp_url = catalog_url.replace("metamach_db", &format!("metamach_blueprint_{NAME}"));
    let psql = |sql: &str| {
        std::process::Command::new("psql")
            .args(["-t", "-A"])
            .arg(&bp_url)
            .arg("-c")
            .arg(sql)
            .output()
            .expect("psql")
    };

    let out = psql("SELECT absurd.get_schema_version()");
    assert!(
        out.status.success(),
        "get_schema_version: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "main",
        "absurd schema version"
    );

    // absurd stored procs are callable (queue + task round-trip).
    let out = psql("SELECT absurd.create_queue('utc0a_q', 'unpartitioned')");
    assert!(
        out.status.success(),
        "create_queue: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = psql("SELECT task_id FROM absurd.spawn_task('utc0a_q', 't', '{}'::jsonb)");
    assert!(
        out.status.success(),
        "spawn_task: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "spawn_task must return a task_id"
    );

    // session_name column on the MetaMach overlay.
    let out = psql(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name='metamach_step_meta' AND column_name='session_name'",
    );
    assert!(
        out.status.success(),
        "columns query: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "session_name",
        "metamach_step_meta.session_name column"
    );
}
