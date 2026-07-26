//! Herdr plugin contract static tests (M5).
//!
//! Validates `herdr-plugin.toml` format + `HERDR_PLUGIN_*` fallback logic
//! without requiring a running Herdr daemon.
//!
//! Real Herdr integration tests (`herdr plugin link` / overlay popup) are
//! `#[ignore = "requires running herdr server"]` — run manually on macOS.

use std::path::PathBuf;

#[derive(Debug, serde::Deserialize)]
struct PluginManifest {
    id: String,
    name: String,
    version: String,
    min_herdr_version: String,
    #[serde(default)]
    panes: Vec<PaneEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct PaneEntry {
    id: String,
    #[allow(dead_code)]
    title: String,
    placement: String,
    command: Vec<String>,
}

// ── Manifest validation ──────────────────────────────────────────────

#[test]
fn herdr_plugin_toml_parses_and_has_required_fields() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("herdr-plugin.toml");
    let text = std::fs::read_to_string(&path).expect("read herdr-plugin.toml");
    let manifest: PluginManifest =
        toml::from_str(&text).expect("parse herdr-plugin.toml as PluginManifest");

    assert_eq!(manifest.id, "metamach.janus");
    assert_eq!(manifest.min_herdr_version, "0.7.3");
    assert!(!manifest.name.is_empty());
    assert!(!manifest.version.is_empty());
    assert!(
        !manifest.panes.is_empty(),
        "must declare at least one [[panes]]"
    );

    let valid = ["overlay", "split", "tab", "zoomed"];
    for pane in &manifest.panes {
        assert!(!pane.id.is_empty());
        assert!(!pane.command.is_empty());
        assert!(
            valid.contains(&pane.placement.as_str()),
            "invalid placement '{}' for pane '{}'",
            pane.placement,
            pane.id
        );
    }
}

#[test]
fn herdr_plugin_toml_command_matches_binary() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("herdr-plugin.toml");
    let text = std::fs::read_to_string(&path).expect("read herdr-plugin.toml");
    let manifest: PluginManifest = toml::from_str(&text).expect("parse");
    for pane in &manifest.panes {
        assert_eq!(
            pane.command,
            vec!["herdr-janus"],
            "pane '{}' command must be [\"herdr-janus\"]",
            pane.id
        );
    }
}

// ── HERDR_PLUGIN_* fallback + override ───────────────────────────────

#[test]
fn herdr_env_fallback_and_override() {
    // All paths.rs env-var tests in one function to avoid interleaving on
    // global state (set_var/remove_var are unsafe in Rust 2024).

    // Save.
    let saved_state = std::env::var("HERDR_PLUGIN_STATE_DIR").ok();
    let saved_config = std::env::var("HERDR_PLUGIN_CONFIG_DIR").ok();
    let saved_root = std::env::var("HERDR_PLUGIN_ROOT").ok();
    unsafe {
        std::env::remove_var("HERDR_PLUGIN_STATE_DIR");
        std::env::remove_var("HERDR_PLUGIN_CONFIG_DIR");
        std::env::remove_var("HERDR_PLUGIN_ROOT");
    }

    // Fallbacks.
    let dir = janus::paths::state_dir();
    assert!(dir.ends_with(".local/state/herdr/plugins/metamach.janus"));
    let cfg = janus::paths::config_dir();
    assert!(cfg.ends_with(".config/herdr/plugins/config/metamach.janus"));
    let repo = janus::paths::repo_root();
    assert!(repo.is_dir());

    // Override.
    let tmp = "/tmp/test-herdr-paths";
    unsafe { std::env::set_var("HERDR_PLUGIN_STATE_DIR", tmp) };
    let sock = janus::paths::sock_path();
    let pid = janus::paths::pid_path();
    let fb = janus::paths::fallback_path();
    assert!(sock.starts_with(tmp));
    assert!(pid.starts_with(tmp));
    assert!(fb.starts_with(tmp));
    assert_eq!(sock.file_name().unwrap(), "janus.sock");
    assert_eq!(pid.file_name().unwrap(), "janus.pid");
    assert_eq!(fb.file_name().unwrap(), "fallback.db");

    // agents.toml override.
    let t = tempfile::tempdir().unwrap();
    let override_path = t.path().join("custom-agents.toml");
    std::fs::write(&override_path, "[agent.test]\nbash_safe = true\n").unwrap();
    unsafe { std::env::set_var("JANUS_AGENTS_TOML", &override_path) };
    assert_eq!(janus::paths::agents_toml_path(), override_path);
    unsafe { std::env::remove_var("JANUS_AGENTS_TOML") };

    // Restore.
    unsafe {
        if let Some(v) = saved_state {
            std::env::set_var("HERDR_PLUGIN_STATE_DIR", v);
        } else {
            std::env::remove_var("HERDR_PLUGIN_STATE_DIR");
        }
        if let Some(v) = saved_config {
            std::env::set_var("HERDR_PLUGIN_CONFIG_DIR", v);
        } else {
            std::env::remove_var("HERDR_PLUGIN_CONFIG_DIR");
        }
        if let Some(v) = saved_root {
            std::env::set_var("HERDR_PLUGIN_ROOT", v);
        } else {
            std::env::remove_var("HERDR_PLUGIN_ROOT");
        }
    }
}

// ── Manual integration tests (requires Herdr binary) ───────────────────

/// Check whether `herdr` is on PATH.
fn herdr_available() -> bool {
    std::process::Command::new("herdr")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
#[ignore = "requires running herdr server"]
fn herdr_plugin_link_parses_manifest() {
    if !herdr_available() {
        eprintln!("skipping: herdr not on PATH");
        return;
    }
    // Unlink first (idempotent on first run; ignores errors).
    let _ = std::process::Command::new("herdr")
        .args(["plugin", "unlink", "metamach.janus"])
        .output();

    // Link from the CARGO_MANIFEST_DIR (contains herdr-plugin.toml).
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let out = std::process::Command::new("herdr")
        .args(["plugin", "link", manifest_dir])
        .output()
        .expect("run herdr plugin link");
    assert!(
        out.status.success(),
        "herdr plugin link failed: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // Verify the plugin appears in the list.
    let out = std::process::Command::new("herdr")
        .args(["plugin", "list", "--json"])
        .output()
        .expect("run herdr plugin list");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("metamach.janus"),
        "plugin list should contain metamach.janus: {stdout}"
    );

    // Clean up.
    let _ = std::process::Command::new("herdr")
        .args(["plugin", "unlink", "metamach.janus"])
        .output();
}

#[test]
#[ignore = "requires running herdr server"]
fn herdr_min_version_is_satisfied() {
    if !herdr_available() {
        eprintln!("skipping: herdr not on PATH");
        return;
    }
    // Read the declared min_herdr_version from the manifest.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let manifest_path = std::path::Path::new(manifest_dir).join("herdr-plugin.toml");
    let text = std::fs::read_to_string(manifest_path).expect("read herdr-plugin.toml");
    let val: toml::Table = toml::from_str(&text).expect("parse");
    let min_ver = val["min_herdr_version"]
        .as_str()
        .expect("min_herdr_version");

    // Get the installed Herdr version (e.g. "0.7.3").
    let out = std::process::Command::new("herdr")
        .arg("--version")
        .output()
        .expect("herdr --version");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Herdr output: "herdr 0.7.3" — extract the version.
    let installed_ver = stdout.split_whitespace().nth(1).unwrap_or("0.0.0");

    // Simple comparison: major.minor.patch must be >=.
    let min_parts: Vec<u32> = min_ver.split('.').filter_map(|s| s.parse().ok()).collect();
    let inst_parts: Vec<u32> = installed_ver
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    assert!(
        inst_parts >= min_parts,
        "installed Herdr {installed_ver} is older than min_herdr_version {min_ver}"
    );
}

// ── End-to-end smoke test (PG + tmux + Herdr + janus-daemon) ───────────

#[test]
#[ignore = "requires PG + tmux + herdr"]
#[allow(clippy::collapsible_if)]
fn e2e_smoke_onboard_dispatch_progress() {
    let pg_ok =
        std::env::var("DATABASE_URL").is_ok() || std::env::var("METAMACH_PG_SOCKET_DIR").is_ok();
    let tmux_ok = std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let herdr_ok = std::process::Command::new("herdr")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !pg_ok || !tmux_ok || !herdr_ok {
        eprintln!("skipping e2e: PG={pg_ok} tmux={tmux_ok} herdr={herdr_ok}");
        return;
    }

    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let agents = state.path().join("agents.toml");
    std::fs::write(
        &agents,
        "[agent.default]\nbash_safe = true\nbash_blacklist = [\"rm -rf /\"]\n",
    )
    .unwrap();

    let bp_name = format!("e2e_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let bp_dir = repo.path().join(".janus");
    std::fs::create_dir_all(&bp_dir).unwrap();
    std::fs::write(
        bp_dir.join("blueprint.toml"),
        format!("[blueprint]\nname = \"{bp_name}\"\ndefault_workflow = \"smoke\"\n\n[openwiki]\nscope = [\"e2e\"]\n"),
    )
    .unwrap();
    let wf_dir = repo.path().join("workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    std::fs::write(
        wf_dir.join("smoke.toml"),
        "[workflow]\nname = \"smoke\"\n\n[[steps]]\nname = \"hello\"\nagent = \"default\"\ncommand = \"echo e2e-ok\"\n",
    )
    .unwrap();

    let mut daemon = std::process::Command::new(env!("CARGO_BIN_EXE_janus-daemon"))
        .env("HERDR_PLUGIN_STATE_DIR", state.path())
        .env("HERDR_PLUGIN_ROOT", repo.path())
        .env("JANUS_AGENTS_TOML", &agents)
        .env("JANUS_JANUSH_BIN", env!("CARGO_BIN_EXE_janush"))
        .env("JANUS_GATEWAY_LISTEN_PORT", "0")
        .env("RUST_LOG", "warn")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn janus-daemon");

    let sock = state.path().join("janus.sock");
    let start = std::time::Instant::now();
    while !sock.exists() && start.elapsed() < std::time::Duration::from_secs(15) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(sock.exists(), "daemon did not bind janus.sock");
    std::thread::sleep(std::time::Duration::from_secs(12));

    let req = janus::protocol::Request::Onboard {
        name: bp_name.clone(),
    };
    let resp =
        janus::uds::request_to(&sock, &req, std::time::Duration::from_secs(15)).expect("onboard");
    assert!(
        matches!(resp, janus::protocol::Response::Ok { .. }),
        "onboard: {resp:?}"
    );

    let req = janus::protocol::Request::Dispatch {
        blueprint: bp_name.clone(),
        workflow: None,
    };
    let resp =
        janus::uds::request_to(&sock, &req, std::time::Duration::from_secs(15)).expect("dispatch");
    let _task_id = match resp {
        janus::protocol::Response::Dispatch { task_id } => task_id,
        other => panic!("expected Dispatch, got {other:?}"),
    };

    // Poll until the task disappears from active_tasks (terminal state).
    // db.progress() only returns STARTING/RUNNING/SUSPENDED — COMPLETED
    // tasks vanish, so we detect completion by task absence.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut completed = false;
    while std::time::Instant::now() < deadline {
        let req = janus::protocol::Request::Progress {
            blueprint: Some(bp_name.clone()),
        };
        if let Ok(janus::protocol::Response::Progress { active_tasks }) =
            janus::uds::request_to(&sock, &req, std::time::Duration::from_secs(5))
        {
            // Task disappeared → terminal state reached.
            if active_tasks.is_empty() {
                completed = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    assert!(completed, "workflow did not reach COMPLETED within 30s");

    let _ = daemon.kill();
    let _ = daemon.wait();
}
