//! Physical path resolution for the Mutable State zone.
//!
//! All runtime state (socket, PID lock, fallback.db, PG socket dir) lives under
//! `HERDR_PLUGIN_STATE_DIR` (injected by Herdr 0.7.3 = `~/.local/state/herdr/
//! plugins/metamach.janus`; see herdr-v1-contract §5/§6). When run standalone
//! (no Herdr), we default to that same path so the Daemon and clients agree.

use std::path::PathBuf;

const STATE_SUBPATH: &str = ".local/state/herdr/plugins/metamach.janus";

/// Resolve the Mutable State directory, creating it if missing.
pub fn state_dir() -> PathBuf {
    let dir = match std::env::var("HERDR_PLUGIN_STATE_DIR") {
        Ok(s) if !s.is_empty() => PathBuf::from(s),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(STATE_SUBPATH)
        }
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn sock_path() -> PathBuf {
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
/// segment is per herdr-v1-contract §6). Hosts `agents.toml`.
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
