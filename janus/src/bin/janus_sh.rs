//! `janus-sh` - proxy shell (Project-Plan M3 Task 3.1; Feature-Spec §2.2).
//!
//! Injected by Tether as `SHELL` (absolute path `${HERDR_PLUGIN_ROOT}/bin/janus-sh`).
//! It never executes a command directly: it forwards the argv to `janus-daemon`
//! over `janus.sock`, blocks for a verdict (default 30s), then:
//!   ALLOW   -> exec `/bin/sh` with the original argv
//!   REWRITE -> exec `/bin/sh -c "<rewritten command>"`
//!   BLOCK   -> exit non-zero WITHOUT executing (fail-closed)
//! If the Daemon is unreachable or the 30s window elapses, `janus-sh`
//! fail-closes as BLOCK (never lets the command through - Feature-Spec §2.2).

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Command, exit};
use std::time::Duration;

use janus::paths;
use janus::protocol::{Request, Response};
use janus::uds;

/// Synchronous blocking window before fail-closed BLOCK (Feature-Spec §2.2).
const VERDICT_TIMEOUT: Duration = Duration::from_secs(30);
/// Exit code for a blocked / fail-closed command (126 = "permission denied").
const EXIT_BLOCKED: i32 = 126;

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let args = &argv[1..];

    // An interactive invocation (no args) isn't intercepted - just run /bin/sh.
    // (Tether-launched agent panes always use `-c "<cmd>"`.)
    if args.is_empty() {
        exec_sh(args);
    }

    let req = build_guard_check(args);
    let resp = match uds::request_to(&paths::sock_path(), &req, VERDICT_TIMEOUT) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("janus-sh: daemon unreachable ({e}); fail-closed BLOCK");
            exit(EXIT_BLOCKED);
        }
    };

    match resp {
        Response::GuardVerdict {
            verdict,
            reason,
            rewritten_argv,
            ..
        } => match verdict.as_str() {
            "ALLOW" => exec_sh(args),
            "REWRITE" => {
                let joined = rewritten_argv.unwrap_or_default().join(" ");
                exec_sh(&["-c".to_string(), joined]);
            }
            "BLOCK" => {
                eprintln!(
                    "janus-sh: BLOCKED by Tool Guard: {}",
                    reason.unwrap_or_else(|| "unspecified".to_string())
                );
                exit(EXIT_BLOCKED);
            }
            other => {
                eprintln!("janus-sh: unknown verdict '{other}'; fail-closed BLOCK");
                exit(EXIT_BLOCKED);
            }
        },
        _ => {
            eprintln!("janus-sh: unexpected daemon response; fail-closed BLOCK");
            exit(EXIT_BLOCKED);
        }
    }
}

/// Build the Contract 3.2 GuardCheck request from the invocation + environment.
fn build_guard_check(args: &[String]) -> Request {
    Request::GuardCheck {
        execution_id: uuid::Uuid::new_v4().to_string(),
        blueprint_id: std::env::var("JANUS_BLUEPRINT").ok(),
        task_id: std::env::var("JANUS_TASK_ID")
            .ok()
            .and_then(|s| s.parse().ok()),
        step_name: std::env::var("JANUS_STEP").ok(),
        cwd: std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().into_owned()),
        argv: args.to_vec(),
        env_snapshot: snapshot_env(),
    }
}

/// Capture the env subset the guard needs (role + task context + identity).
fn snapshot_env() -> HashMap<String, String> {
    const KEYS: &[&str] = &[
        "USER",
        "SHELL",
        "JANUS_AGENT",
        "JANUS_BLUEPRINT",
        "JANUS_TASK_ID",
        "JANUS_STEP",
        "JANUS_WORKFLOW",
    ];
    let mut m = HashMap::new();
    for k in KEYS {
        if let Ok(v) = std::env::var(k)
            && !v.is_empty()
        {
            m.insert((*k).to_string(), v);
        }
    }
    m
}

/// Replace this process with `/bin/sh <args>`. Only returns on exec failure.
fn exec_sh(args: &[String]) -> ! {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(args);
    let err = cmd.exec();
    // Reached only if exec failed.
    eprintln!("janus-sh: exec /bin/sh failed: {err}");
    exit(EXIT_BLOCKED);
}
