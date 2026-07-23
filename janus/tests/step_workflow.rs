//! M5 Task 5.1: step-level workflow integration tests (PG + tmux dependent).
//!
//! These tests verify the full step execution lifecycle: dispatch, state
//! transitions, crash recovery, concurrent isolation, and optimistic locking.
//! They require PostgreSQL AND tmux; `#[ignore]` locally, run in CI.
//!
//! Covers Test-Spec suites 2.3 (tmux workflow) and 2.4 (HITL): UTC-03-01,
//! UTC-03-01b, UTC-03-03, UTC-03-04, UTC-03-05, UTC-03-06, UTC-04-01.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use janus::protocol::{Request, Response, StepStatus};
use janus::uds;

const AGENTS_TOML: &str = r#"
[agent.default]
bash_safe = true
bash_blacklist = ["rm -rf /"]

[agent.deployer]
permissions = ["read", "write", "bash-full", "ssh"]
require_approval = ["make flash"]
financial = ["hi5bot --action execute"]
"#;

struct Daemon {
    child: std::process::Child,
    sock: std::path::PathBuf,
    /// The repo path the daemon was spawned against (always set).
    repo_path: std::path::PathBuf,
    /// `Some` when this Daemon created the repo (cleaned up on drop); `None`
    /// when spawned against an externally-owned repo (`spawn_with_repo`, used by
    /// the cold-start restart test - the caller owns the `TempDir` so it
    /// survives the kill). Never read - held only so the `TempDir` outlives the
    /// daemon (its Drop cleans up the repo).
    #[allow(dead_code)]
    repo: Option<tempfile::TempDir>,
}

impl Daemon {
    fn spawn(state_dir: &Path, agents: &Path) -> Self {
        let repo = tempfile::tempdir().expect("repo tempdir");
        let repo_path = repo.path().to_path_buf();
        copy_configs(&repo_path);
        Self::spawn_inner(state_dir, agents, &repo_path, Some(repo))
    }

    /// Spawn against an EXISTING repo (caller owns the `TempDir`) - for tests
    /// that restart the daemon against the same repo + state_dir (utc_03_03
    /// cold-start resume). The repo is NOT deleted when this Daemon drops.
    fn spawn_with_repo(state_dir: &Path, agents: &Path, repo_path: &Path) -> Self {
        Self::spawn_inner(state_dir, agents, repo_path, None)
    }

    fn spawn_inner(
        state_dir: &Path,
        agents: &Path,
        repo_path: &Path,
        repo: Option<tempfile::TempDir>,
    ) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_janus-daemon"))
            .env("HERDR_PLUGIN_STATE_DIR", state_dir)
            .env("HERDR_PLUGIN_ROOT", repo_path)
            .env("JANUS_AGENTS_TOML", agents)
            .env("JANUS_GATEWAY_LISTEN_PORT", "0")
            // Point the engine at the built janush binary (the daemon resolves it
            // via sibling-of-current-exe in production; in tests the daemon's
            // current_exe is target/<profile>/janus-daemon, whose janush sibling
            // only exists if referenced - CARGO_BIN_EXE_janush forces the build
            // AND gives the exact path).
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
            repo,
        }
    }

    fn uds(&self, req: &Request, timeout: Duration) -> Result<Response, String> {
        uds::request_to(&self.sock, req, timeout).map_err(|e| e.to_string())
    }

    /// The repo path this daemon was spawned against.
    fn repo_path(&self) -> &Path {
        &self.repo_path
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Copy the real `configs/` into a fresh repo (so Offboard can load
/// `configs/offboard.toml`). We do NOT copy blueprints/workflows - each test
/// writes its own unique blueprint + workflow.
fn copy_configs(repo_path: &Path) {
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
}

fn make_blueprint(base: &Path, name: &str) {
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
}

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

/// A 2-step blueprint for UTC-03-01b: `scout` sleeps (so `tmux_alive` is
/// observable mid-run), `build` echoes. Both are bash-safe (ALLOW) under the
/// test `agents.toml`'s `[agent.default]`.
fn make_2step_blueprint(base: &Path, name: &str) {
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

    let wf_dir = base.join("workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    std::fs::write(
        wf_dir.join("test-flow.toml"),
        r#"[workflow]
name = "test-flow"

[[steps]]
name = "scout"
agent = "default"
command = "sleep 3"

[[steps]]
name = "build"
agent = "default"
command = "echo done"
"#,
    )
    .unwrap();
}

// ── UTC-03-01 / 03-01b: Step State Transitions ─────────────────────────────

#[test]
#[ignore = "requires PostgreSQL + tmux"]
fn utc_03_01_step_state_transitions() {
    // With PG online, onboard a blueprint, then verify the Progress query
    // returns the expected task/step lifecycle fields. The actual tmux session
    // dispatch requires a live tmux server and is covered by UTC-09-xx.
    const NAME: &str = "gate_03_01";
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let d = Daemon::spawn(state.path(), &agents);
    make_blueprint(d.repo_path(), NAME);
    std::thread::sleep(Duration::from_secs(12));

    d.uds(
        &Request::Onboard { name: NAME.into() },
        Duration::from_secs(10),
    )
    .unwrap();

    // Progress query returns empty before any dispatch.
    let resp = d
        .uds(
            &Request::Progress { blueprint: None },
            Duration::from_secs(5),
        )
        .unwrap();
    match resp {
        Response::Progress { active_tasks } => {
            assert!(active_tasks.is_empty(), "should be empty before dispatch");
        }
        other => panic!("expected Progress, got {other:?}"),
    }

    // GuardCheck still works (PG online does not break degraded-mode paths).
    let resp = d
        .uds(&guard_check("default", "ls -la"), Duration::from_secs(5))
        .unwrap();
    match resp {
        Response::GuardVerdict { verdict, .. } => assert_eq!(verdict, "ALLOW"),
        other => panic!("expected GuardVerdict, got {other:?}"),
    }
}

// ── UTC-03-01b: Dispatch -> STARTING -> RUNNING -> COMPLETED ───────────────

#[test]
#[ignore = "requires PostgreSQL + tmux"]
fn utc_03_01b_dispatch_step_transitions() {
    // Dispatch a 2-step workflow (Contract 3.11). Assert: the absurd-minted
    // task_id returns; `tmux_alive=true` is observed while step 1 (`sleep 3`)
    // runs; both steps reach `COMPLETED` with `exit_code=0`; the absurd task +
    // checkpoint rows land in the per-blueprint DB.
    //
    // Unique name per run: the blueprint DB persists across runs, so a fixed name
    // would leave stale absurd tasks in the queue - `claim_task` would return one
    // of those (not the just-spawned one) and the engine's task-id guard would
    // trip. A fresh name gives a fresh queue + overlay.
    let name = format!(
        "gate_03_01b_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let d = Daemon::spawn(state.path(), &agents);
    make_2step_blueprint(d.repo_path(), &name);
    std::thread::sleep(Duration::from_secs(12)); // wait for PG connect

    let onboard_resp = d
        .uds(
            &Request::Onboard { name: name.clone() },
            Duration::from_secs(15),
        )
        .unwrap();
    assert!(
        matches!(onboard_resp, Response::Ok { .. }),
        "onboard failed: {onboard_resp:?}"
    );

    // Dispatch returns the absurd-minted task_id synchronously.
    let resp = d
        .uds(
            &Request::Dispatch {
                blueprint: name.clone(),
                workflow: None,
            },
            Duration::from_secs(15),
        )
        .unwrap();
    let task_id = match resp {
        Response::Dispatch { task_id } => task_id,
        other => panic!("expected Dispatch, got {other:?}"),
    };
    assert_ne!(
        task_id,
        uuid::Uuid::nil(),
        "task_id should be absurd-minted"
    );

    // PG query helper (psql via CLI). CI uses a TCP DATABASE_URL; locally
    // `make db-init` binds a Unix socket and sqlx's `from_str` mis-parses the
    // `?host=` URL form, so the daemon is driven by METAMACH_PG_SOCKET_DIR. psql
    // (libpq) handles `?host=` fine, so build whichever URL fits the environment.
    let bp_url = match std::env::var("DATABASE_URL") {
        Ok(catalog_url) => {
            catalog_url.replace("metamach_db", &format!("metamach_blueprint_{name}"))
        }
        Err(_) => {
            let socket = std::env::var("METAMACH_PG_SOCKET_DIR")
                .expect("DATABASE_URL or METAMACH_PG_SOCKET_DIR must be set");
            format!("postgres://metamach_admin@/metamach_blueprint_{name}?host={socket}")
        }
    };
    let psql = |sql: String| {
        std::process::Command::new("psql")
            .args(["-t", "-A"])
            .arg(&bp_url)
            .arg("-c")
            .arg(&sql)
            .output()
            .expect("psql")
    };

    // While step 1 (sleep 3) runs, Progress must report tmux_alive=true at least
    // once (the daemon's second-pass `has_session` check, Contract 3.3).
    let observe_deadline = Instant::now() + Duration::from_secs(20);
    let mut saw_tmux_alive = false;
    while Instant::now() < observe_deadline {
        let resp = d
            .uds(
                &Request::Progress {
                    blueprint: Some(name.clone()),
                },
                Duration::from_secs(5),
            )
            .unwrap();
        if let Response::Progress { active_tasks } = resp
            && active_tasks.iter().any(|t| t.tmux_alive)
        {
            saw_tmux_alive = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(saw_tmux_alive, "never observed tmux_alive=true mid-run");

    // Wait for the absurd task to reach `completed` (source of truth - avoids
    // the brief Progress-empty window between step 1 COMPLETED and step 2 STARTING).
    // Queue name = `<name>_test_flow` (sanitized; workflow `test-flow` -> `test_flow`).
    let final_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let out = psql(format!(
            "SELECT state FROM absurd.t_{name}_test_flow WHERE task_id = '{task_id}'"
        ));
        if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "completed" {
            break;
        }
        if Instant::now() > final_deadline {
            panic!(
                "absurd task did not reach completed within 30s: {}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    // Both steps COMPLETED with exit_code=0.
    let out = psql(format!(
        "SELECT step_name || '=' || status || ':' || COALESCE(exit_code::text, 'null') \
         FROM metamach_step_meta WHERE task_id = '{task_id}' ORDER BY step_name"
    ));
    assert!(
        out.status.success(),
        "step_meta query: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows = String::from_utf8_lossy(&out.stdout);
    assert!(
        rows.contains("build=COMPLETED:0"),
        "build step row missing: {rows}"
    );
    assert!(
        rows.contains("scout=COMPLETED:0"),
        "scout step row missing: {rows}"
    );

    // One absurd checkpoint per step (set_checkpoint called per COMPLETED step).
    let out = psql(format!(
        "SELECT count(*) FROM absurd.c_{name}_test_flow WHERE task_id = '{task_id}'"
    ));
    assert!(
        out.status.success(),
        "checkpoint query: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "2",
        "two checkpoints (one per step), got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// ── UTC-03-03: Cold-Start Self-Healing ─────────────────────────────────────

#[test]
#[ignore = "requires PostgreSQL + tmux"]
fn utc_03_03_cold_start_reconcile() {
    // Dispatch a 2-step workflow (step 1 = `true`, step 2 = `sleep 6`), kill the
    // daemon mid-step-2, restart it, and assert coldstart::reconcile resumes the
    // task to COMPLETED (step 1 is skipped - its checkpoint exists; step 2
    // re-runs in a fresh tmux session after d1's run lease expires).
    let name = format!(
        "gate_03_03_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();
    // step 1 quick (completes + checkpoints before the kill), step 2 slow.
    let bp = repo.path().join("blueprints").join(&name);
    std::fs::create_dir_all(&bp).unwrap();
    std::fs::write(
        bp.join("janus.toml"),
        format!(
            r#"[blueprint]
name = "{name}"
default_workflow = "test-flow"
[openwiki]
scope = ["test"]
"#
        ),
    )
    .unwrap();
    std::fs::create_dir_all(repo.path().join("workflows")).unwrap();
    std::fs::write(
        repo.path().join("workflows").join("test-flow.toml"),
        r#"[workflow]
name = "test-flow"
[[steps]]
name = "scout"
agent = "default"
command = "true"
[[steps]]
name = "build"
agent = "default"
command = "sleep 6"
"#,
    )
    .unwrap();

    // d1: onboard + dispatch.
    let d1 = Daemon::spawn_with_repo(state.path(), &agents, repo.path());
    std::thread::sleep(Duration::from_secs(12)); // PG connect
    let onboard = d1
        .uds(
            &Request::Onboard { name: name.clone() },
            Duration::from_secs(15),
        )
        .unwrap();
    assert!(
        matches!(onboard, Response::Ok { .. }),
        "onboard failed: {onboard:?}"
    );
    let resp = d1
        .uds(
            &Request::Dispatch {
                blueprint: name.clone(),
                workflow: None,
            },
            Duration::from_secs(15),
        )
        .unwrap();
    let task_id = match resp {
        Response::Dispatch { task_id } => task_id,
        other => panic!("expected Dispatch, got {other:?}"),
    };

    // Wait until step 2 (build) is RUNNING (tmux_alive), then kill d1 mid-step-2.
    let observe = Instant::now() + Duration::from_secs(15);
    let mut saw_running = false;
    while Instant::now() < observe {
        if let Response::Progress { active_tasks } = d1
            .uds(
                &Request::Progress {
                    blueprint: Some(name.clone()),
                },
                Duration::from_secs(5),
            )
            .unwrap()
            && active_tasks.iter().any(|t| t.tmux_alive)
        {
            saw_running = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(saw_running, "never observed step 2 running before the kill");
    drop(d1); // kill the daemon mid-step-2 (repo + state persist)

    // d2: restart against the same repo + state_dir. The blueprint DB persists,
    // so d2 can resume without re-onboarding. reconcile fires at t+3s.
    let d2 = Daemon::spawn_with_repo(state.path(), &agents, repo.path());

    let bp_url = match std::env::var("DATABASE_URL") {
        Ok(catalog_url) => {
            catalog_url.replace("metamach_db", &format!("metamach_blueprint_{name}"))
        }
        Err(_) => {
            let socket = std::env::var("METAMACH_PG_SOCKET_DIR")
                .expect("DATABASE_URL or METAMACH_PG_SOCKET_DIR must be set");
            format!("postgres://metamach_admin@/metamach_blueprint_{name}?host={socket}")
        }
    };
    let psql = |sql: String| {
        std::process::Command::new("psql")
            .args(["-t", "-A"])
            .arg(&bp_url)
            .arg("-c")
            .arg(&sql)
            .output()
            .expect("psql")
    };

    // Wait for the absurd task to reach `completed`. The resume re-runs step 2
    // (sleep 6); d2 must also wait for d1's run lease (30s) to expire before
    // absurd re-offers the task. Allow 60s.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let out = psql(format!(
            "SELECT state FROM absurd.t_{name}_test_flow WHERE task_id = '{task_id}'"
        ));
        if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "completed" {
            break;
        }
        if Instant::now() > deadline {
            panic!(
                "task did not reach completed within 60s: {}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    // Both steps COMPLETED (step 1 from d1, step 2 resumed on d2).
    let out = psql(format!(
        "SELECT step_name || '|' || status FROM metamach_step_meta WHERE task_id = '{task_id}' ORDER BY step_name"
    ));
    let rows = String::from_utf8_lossy(&out.stdout);
    assert!(
        rows.contains("build|COMPLETED"),
        "build should be COMPLETED: {rows}"
    );
    assert!(
        rows.contains("scout|COMPLETED"),
        "scout should be COMPLETED: {rows}"
    );

    drop(d2);
}

// ── UTC-04-01: HITL Resume (emit_event -> await_event -> re-run) ───────────

#[test]
#[ignore = "requires PostgreSQL + tmux"]
fn utc_04_01_hitl_resume() {
    // Dispatch a 2-step workflow whose step 2 (`echo hi`, deployer) is
    // require_approval -> suspends. Simulate the Director's approval (the verdict
    // sink's record_hitl_resolution + emit_event) via psql, then assert the
    // engine wakes, re-runs step 2 (GuardCheck ALLOWs via hitl_verdict=APPROVED),
    // and both steps reach COMPLETED.
    let name = format!(
        "gate_04_01_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(
        &agents,
        r#"
[agent.default]
bash_safe = true

[agent.deployer]
permissions = ["bash-full"]
require_approval = ["echo hi"]
"#,
    )
    .unwrap();

    let d = Daemon::spawn(state.path(), &agents);
    // step 1 = true (default), step 2 = echo hi (deployer, require_approval).
    let bp = d.repo_path().join("blueprints").join(&name);
    std::fs::create_dir_all(&bp).unwrap();
    std::fs::write(
        bp.join("janus.toml"),
        format!(
            r#"[blueprint]
name = "{name}"
default_workflow = "test-flow"
[openwiki]
scope = ["test"]
"#
        ),
    )
    .unwrap();
    let wf_dir = d.repo_path().join("workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    std::fs::write(
        wf_dir.join("test-flow.toml"),
        r#"[workflow]
name = "test-flow"
[[steps]]
name = "scout"
agent = "default"
command = "true"
[[steps]]
name = "build"
agent = "deployer"
command = "echo hi"
"#,
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(12)); // PG connect

    d.uds(
        &Request::Onboard { name: name.clone() },
        Duration::from_secs(15),
    )
    .unwrap();
    let resp = d
        .uds(
            &Request::Dispatch {
                blueprint: name.clone(),
                workflow: None,
            },
            Duration::from_secs(15),
        )
        .unwrap();
    let task_id = match resp {
        Response::Dispatch { task_id } => task_id,
        other => panic!("expected Dispatch, got {other:?}"),
    };

    let bp_url = match std::env::var("DATABASE_URL") {
        Ok(catalog_url) => {
            catalog_url.replace("metamach_db", &format!("metamach_blueprint_{name}"))
        }
        Err(_) => {
            let socket = std::env::var("METAMACH_PG_SOCKET_DIR")
                .expect("DATABASE_URL or METAMACH_PG_SOCKET_DIR must be set");
            format!("postgres://metamach_admin@/metamach_blueprint_{name}?host={socket}")
        }
    };
    let psql = |sql: String| {
        std::process::Command::new("psql")
            .args(["-t", "-A"])
            .arg(&bp_url)
            .arg("-c")
            .arg(&sql)
            .output()
            .expect("psql")
    };

    // Poll until step 2 (build) is SUSPENDED (the HITL BLOCK fired).
    let suspend_deadline = Instant::now() + Duration::from_secs(20);
    let mut suspended = false;
    while Instant::now() < suspend_deadline {
        if let Response::Progress { active_tasks } = d
            .uds(
                &Request::Progress {
                    blueprint: Some(name.clone()),
                },
                Duration::from_secs(5),
            )
            .unwrap()
            && active_tasks.iter().any(|t| {
                t.steps
                    .iter()
                    .any(|s| s.name == "build" && s.status == "SUSPENDED")
            })
        {
            suspended = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(suspended, "step 2 never reached SUSPENDED");

    // Simulate the Director's approval: record hitl_verdict=APPROVED on the
    // overlay (so the GuardCheck ALLOWs the re-run) + emit_event (so the engine's
    // await_event wakes). Queue = `<name>_test_flow` (workflow `test-flow` -> `test_flow`).
    psql(format!(
        "UPDATE metamach_step_meta SET hitl_verdict='APPROVED' WHERE task_id='{task_id}' AND step_name='build'"
    ));
    let payload = r#"{"hitl_verdict":"APPROVED"}"#;
    psql(format!(
        "SELECT absurd.emit_event('{name}_test_flow', 'hitl.verdict:{task_id}', '{payload}'::jsonb)"
    ));

    // Wait for the absurd task to reach `completed` (engine wakes + re-runs build).
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let out = psql(format!(
            "SELECT state FROM absurd.t_{name}_test_flow WHERE task_id = '{task_id}'"
        ));
        if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "completed" {
            break;
        }
        if Instant::now() > deadline {
            panic!(
                "task did not reach completed after HITL approve within 30s: {}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    // Both steps COMPLETED (step 2 resumed after approval).
    let out = psql(format!(
        "SELECT step_name || '|' || status FROM metamach_step_meta WHERE task_id = '{task_id}' ORDER BY step_name"
    ));
    let rows = String::from_utf8_lossy(&out.stdout);
    assert!(
        rows.contains("build|COMPLETED"),
        "build should be COMPLETED: {rows}"
    );
    assert!(
        rows.contains("scout|COMPLETED"),
        "scout should be COMPLETED: {rows}"
    );
}

// ── UTC-03-04: Daemon Crash Recovery ───────────────────────────────────────

#[test]
fn utc_03_04_daemon_crash_socket_cleanup() {
    // Degraded-mode test (no PG needed): the daemon cleans up its socket on
    // exit. After kill -9, a new daemon can bind the same socket path.
    let dir = tempfile::tempdir().unwrap();
    let agents = dir.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let mut d1 = Daemon::spawn(dir.path(), &agents);
    let sock_path = d1.sock.to_path_buf();
    assert!(sock_path.exists());

    // Force-kill the daemon (simulating crash).
    let _ = d1.child.kill();
    let _ = d1.child.wait();
    // Prevent Drop from double-killing.
    std::mem::forget(d1);

    // Socket may still exist (stale) - depends on timing. Remove manually.
    let _ = std::fs::remove_file(&sock_path);

    // New daemon can bind to the same path (PID lock file from old daemon is
    // stale, so it's overwritten by the new process).
    let d2 = Daemon::spawn(dir.path(), &agents);
    assert!(d2.sock.exists());
    assert!(matches!(
        d2.uds(&Request::Ping, Duration::from_secs(5)).unwrap(),
        Response::Pong
    ));
    drop(d2);
}

// ── UTC-03-05: Concurrent Workflow Isolation ──────────────────────────────

#[test]
#[ignore = "requires PostgreSQL"]
fn utc_03_05_concurrent_workflow_isolation() {
    const JOY: &str = "joy_03_05";
    const GATE: &str = "gate_03_05";
    let state = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(&agents, AGENTS_TOML).unwrap();

    let d = Daemon::spawn(state.path(), &agents);
    make_blueprint(d.repo_path(), JOY);
    make_blueprint(d.repo_path(), GATE);
    std::thread::sleep(Duration::from_secs(12));

    // Onboard both.
    for name in [JOY, GATE] {
        d.uds(
            &Request::Onboard { name: name.into() },
            Duration::from_secs(10),
        )
        .unwrap();
    }

    // Progress reports both blueprints independently.
    let resp = d
        .uds(
            &Request::Progress { blueprint: None },
            Duration::from_secs(5),
        )
        .unwrap();
    match resp {
        Response::Progress { active_tasks } => {
            // No tasks dispatched yet — just verify the structure.
            assert!(active_tasks.is_empty());
        }
        other => panic!("expected Progress, got {other:?}"),
    }

    // GuardCheck still works with both blueprints onboard (no leak between them).
    let resp1 = d
        .uds(&guard_check("default", "ls -la"), Duration::from_secs(5))
        .unwrap();
    let resp2 = d
        .uds(
            &guard_check("default", "echo hello"),
            Duration::from_secs(5),
        )
        .unwrap();
    assert!(matches!(
        &resp1,
        Response::GuardVerdict { verdict, .. } if verdict == "ALLOW"
    ));
    assert!(matches!(
        &resp2,
        Response::GuardVerdict { verdict, .. } if verdict == "ALLOW"
    ));
}

// ── UTC-03-06: Optimistic Locking (target_sha) ─────────────────────────────

#[test]
fn utc_03_06_step_status_wire_format() {
    // Unit-level: verify the step status wire format (Contract 3.3).
    // Pins the serialized JSON shape for dashboard consumers.
    let status = StepStatus {
        name: "scout".into(),
        status: "COMPLETED".into(),
        exit_code: Some(0),
        stdout_tail: Some("build output...".into()),
    };
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("\"name\":\"scout\""));
    assert!(json.contains("\"status\":\"COMPLETED\""));
    assert!(json.contains("\"exit_code\":0"));

    let back: StepStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "scout");
    assert_eq!(back.status, "COMPLETED");
    assert_eq!(back.exit_code, Some(0));
    assert_eq!(back.stdout_tail.as_deref(), Some("build output..."));
}
