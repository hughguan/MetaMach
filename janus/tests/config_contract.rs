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
