//! Tether integration tests against a real `tmux -L metamach-tether` server.
//!
//! Skipped automatically when tmux is not on PATH (CI installs tmux 3.3+;
//! dev machines without tmux still get the lib unit tests in `tether/mod.rs`).

use std::process::Command;
use std::time::Duration;

use janus::tether::{DurableBackend, SessionId, TMUX_SOCKET, TetherBackend};

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn cleanup(id: &SessionId) {
    let _ = Command::new("tmux")
        .args(["-L", TMUX_SOCKET, "kill-session", "-t", id.as_str()])
        .output();
}

#[test]
fn create_persists_and_lists() {
    if !tmux_available() {
        eprintln!("skip: tmux not installed");
        return;
    }
    let backend = TetherBackend::new();
    let id = SessionId::from_name("tether-janus-it-create".into());
    cleanup(&id);
    backend
        .create_session(&id, "sleep 100", None)
        .expect("create");
    assert!(
        backend.has_session(&id).unwrap(),
        "session not alive after create"
    );
    let list = backend.list_sessions().unwrap();
    assert!(
        list.iter().any(|s| s == id.as_str()),
        "session missing from list: {list:?}"
    );
    cleanup(&id);
}

#[test]
fn kill_removes_session() {
    if !tmux_available() {
        eprintln!("skip: tmux not installed");
        return;
    }
    let backend = TetherBackend::new();
    let id = SessionId::from_name("tether-janus-it-kill".into());
    cleanup(&id);
    backend.create_session(&id, "sleep 100", None).unwrap();
    backend.kill_session(&id).unwrap();
    assert!(
        !backend.has_session(&id).unwrap(),
        "session still alive after kill"
    );
}

#[test]
fn remain_on_exit_survives_process_exit() {
    // UAT (Project-Plan Task 2.4): a session whose command exits must stay alive
    // (remain-on-exit on, set per-session by TetherBackend).
    if !tmux_available() {
        eprintln!("skip: tmux not installed");
        return;
    }
    let backend = TetherBackend::new();
    let id = SessionId::from_name("tether-janus-it-roe".into());
    cleanup(&id);
    backend.create_session(&id, "true", None).unwrap();
    // Give the short-lived command time to exit, then verify the pane survived.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        backend.has_session(&id).unwrap(),
        "session did not survive process exit (remain-on-exit not set?)"
    );
    cleanup(&id);
}

#[test]
fn capture_pane_returns_text() {
    if !tmux_available() {
        eprintln!("skip: tmux not installed");
        return;
    }
    let backend = TetherBackend::new();
    let id = SessionId::from_name("tether-janus-it-capture".into());
    cleanup(&id);
    backend
        .create_session(&id, "echo tether-marker; sleep 100", None)
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let pane = backend.capture_pane(&id).unwrap();
    assert!(
        pane.contains("tether-marker"),
        "marker not in pane: {pane:?}"
    );
    cleanup(&id);
}
