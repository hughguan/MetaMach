//! Physical path resolution for the Mutable State zone.
//!
//! All runtime state (socket, PID lock, fallback.db, PG socket dir) lives under
//! `HERDR_PLUGIN_STATE_DIR` (injected by Herdr 0.7.3 = `~/.local/state/herdr/
//! plugins/metamach.janus`; see `docs/contracts/herdr.md`). When run standalone
//! (no Herdr), we default to that same path so the Daemon and clients agree.

use std::path::PathBuf;

const STATE_SUBPATH: &str = ".local/state/herdr/plugins/metamach.janus";

/// Resolve the Mutable State directory, creating it if missing.
pub fn state_dir() -> PathBuf {
    let dir = match std::env::var("HERDR_PLUGIN_STATE_DIR") {
        Ok(s) if !s.is_empty() => {
            let trimmed = s.trim_matches('\'').trim_matches('"');
            PathBuf::from(trimmed)
        }
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(STATE_SUBPATH)
        }
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn sock_path() -> PathBuf {
    // M4 Phase 2 (ADR-017): on a remote host, janush reaches the local daemon via
    // an SSH `-R` reverse tunnel that maps `/tmp/mm-<host>.sock` on the remote to
    // the local `janus.sock`. The engine sets `JANUS_SOCK_PATH=/tmp/mm-<host>.sock`
    // in the remote step's env; janush honors it here. Locally, unset -> state_dir.
    if let Ok(p) = std::env::var("JANUS_SOCK_PATH")
        && !p.is_empty()
    {
        let trimmed = p.trim_matches('\'').trim_matches('"');
        return PathBuf::from(trimmed);
    }
    state_dir().join("janus.sock")
}

pub fn pid_path() -> PathBuf {
    state_dir().join("janus.pid")
}

pub fn log_path() -> PathBuf {
    state_dir().join("janus.log")
}

pub fn fallback_path() -> PathBuf {
    state_dir().join("fallback.db")
}

pub fn pg_socket_dir() -> PathBuf {
    state_dir().join("pg_socket")
}

/// Mutable Config directory (`HERDR_PLUGIN_CONFIG_DIR`, injected by Herdr 0.7.3
/// = `~/.config/herdr/plugins/config/metamach.janus`; the extra `/config/`
/// segment is per `docs/contracts/herdr.md`). Hosts `agents.toml`.
pub fn config_dir() -> PathBuf {
    match std::env::var("HERDR_PLUGIN_CONFIG_DIR") {
        Ok(s) if !s.is_empty() => PathBuf::from(s),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".config/herdr/plugins/config/metamach.janus")
        }
    }
}

/// Resolve `agents.toml`: `JANUS_AGENTS_TOML` override (tests/dev) wins, else
/// the Mutable Config dir.
pub fn agents_toml_path() -> PathBuf {
    if let Ok(p) = std::env::var("JANUS_AGENTS_TOML")
        && !p.is_empty()
    {
        return PathBuf::from(p);
    }
    config_dir().join("agents.toml")
}

/// Resolve the per-project agent config if it exists (`.janus/agents/agents.toml`
/// under the repo root). Returns `None` if the project hasn't run `janus init`.
pub fn project_agents_toml() -> Option<PathBuf> {
    let p = repo_root().join(".janus/agents/agents.toml");
    if p.exists() { Some(p) } else { None }
}

/// All agents.toml paths in priority order (project override → global fallback).
pub fn agents_toml_paths() -> Vec<PathBuf> {
    if let Ok(p) = std::env::var("JANUS_AGENTS_TOML")
        && !p.is_empty()
    {
        return vec![PathBuf::from(p)];
    }
    let mut paths = Vec::new();
    if let Some(p) = project_agents_toml() {
        paths.push(p);
    }
    paths.push(config_dir().join("agents.toml"));
    paths
}

/// Immutable ROOT: the plugin source checkout (`HERDR_PLUGIN_ROOT`, injected by
/// Herdr 0.7.3; the repo dir when standalone). Hosts `.janus/`, `workflows/`,
/// `configs/`, and `target/release/`. Onboard/Offboard resolve recipes + the
/// offboard LLM config relative to this.
pub fn repo_root() -> PathBuf {
    if let Ok(s) = std::env::var("HERDR_PLUGIN_ROOT")
        && !s.is_empty()
    {
        return PathBuf::from(s);
    }
    let cur = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if !cur.join("templates").exists()
        && let Some(parent) = cur.parent()
        && parent.join("templates").exists()
    {
        return parent.to_path_buf();
    }
    cur
}
