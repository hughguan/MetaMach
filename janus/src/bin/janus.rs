//! `janus` - unified CLI (ARCH §3). A UDS client of `janus-daemon`.
//!
//! Subcommands:
//!   `janus status [--blueprint <name>] [--json]` - Contract 3.3 progress snapshot.
//!   `janus daemon` - launch the resident `janus-daemon` in the foreground.
//!   `janus onboard --blueprint <name>` - register/reactivate a blueprint (Task 4.3).
//!   `janus offboard --blueprint <name>` - smelt + prune a blueprint (Task 4.2).
//!   `janus tmux open|attach|list` - manage tmux physical sessions (Task 2.4).
//!
//! `status`/`onboard`/`offboard` require the Daemon reachable (lazy-started if
//! absent); `tmux` talks to the isolated tmux server directly, no Daemon needed.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use janus::protocol::{ActiveTask, ProgressPayload, Request, Response};
use janus::tmux::DurableBackend;
use janus::{spawn, tmux, uds};

#[derive(Parser)]
#[command(
    name = "janus",
    version,
    about = "MetaMach unified CLI (UDS client of janus-daemon)"
)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Print a live workflow progress snapshot (Feature-Spec Contract 3.3).
    Status {
        /// Filter to a single blueprint name.
        #[arg(long)]
        blueprint: Option<String>,
        /// Emit the raw Contract 3.3 JSON payload.
        #[arg(long)]
        json: bool,
    },
    /// Launch the resident janus-daemon in the foreground.
    Daemon,
    /// Register / reactivate a blueprint (Feature-Spec §2.5, Task 4.3).
    Onboard {
        /// Blueprint name (the `blueprints/<name>/` directory).
        #[arg(long)]
        blueprint: String,
    },
    /// Smelt execution traces + prune DB cache (Feature-Spec §2.5, Task 4.2).
    Offboard {
        /// Blueprint name to offboard.
        #[arg(long)]
        blueprint: String,
    },
    /// Manage tmux physical sessions (Task 2.4, `janus::tmux`).
    Tmux {
        #[command(subcommand)]
        cmd: TmuxCmd,
    },
}

/// `janus tmux` subcommands.
#[derive(Subcommand)]
enum TmuxCmd {
    /// Create a detached session running a command (persists via remain-on-exit).
    Open {
        /// Shell command to run in the session.
        #[arg(long)]
        command: String,
        /// Session name (default: tmux-janus-task-<uuid>).
        #[arg(long)]
        name: Option<String>,
        /// Working directory.
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Attach the terminal to a live session (foreground; blocks until detach).
    Attach {
        /// Session name to attach.
        name: String,
    },
    /// List live Tether sessions on the isolated tmux server.
    List,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        CliCommand::Status { blueprint, json } => status(blueprint, json),
        CliCommand::Daemon => daemon(),
        CliCommand::Onboard { blueprint } => lifecycle_cmd(Request::Onboard { name: blueprint }),
        CliCommand::Offboard { blueprint } => lifecycle_cmd(Request::Offboard { name: blueprint }),
        CliCommand::Tmux { cmd } => tmux(cmd),
    }
}

fn status(blueprint: Option<String>, json: bool) -> Result<()> {
    if let Err(e) = spawn::ensure_daemon(Duration::from_secs(5)) {
        bail!("janus-daemon not reachable: {e}\n  start it with `janus daemon`");
    }
    let resp = uds::request(&Request::Progress { blueprint })?;
    match resp {
        Response::Progress { active_tasks } => {
            if json {
                let payload = ProgressPayload { active_tasks };
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                print_status_text(&active_tasks);
            }
            Ok(())
        }
        Response::Error { message } => bail!(message),
        other => bail!("unexpected daemon response: {other:?}"),
    }
}

fn print_status_text(tasks: &[ActiveTask]) {
    if tasks.is_empty() {
        println!("No in-flight tasks.");
        return;
    }
    println!("In-flight tasks: {}", tasks.len());
    for t in tasks {
        let step = t.current_step.as_deref().unwrap_or("-");
        let elapsed = t
            .elapsed_seconds
            .map(|s| format!("{s}s"))
            .unwrap_or_else(|| "?".to_string());
        println!(
            "  [{}] {} · step {} · {} · {}",
            t.blueprint_id, t.workflow_name, step, t.status, elapsed
        );
    }
}

/// `janus onboard` / `janus offboard`: send the request, print the Daemon's ack.
fn lifecycle_cmd(req: Request) -> Result<()> {
    if let Err(e) = spawn::ensure_daemon(Duration::from_secs(5)) {
        bail!("janus-daemon not reachable: {e}\n  start it with `janus daemon`");
    }
    let resp = uds::request(&req)?;
    match resp {
        Response::Ok { message } => {
            println!("{message}");
            Ok(())
        }
        Response::Error { message } => bail!(message),
        other => bail!("unexpected daemon response: {other:?}"),
    }
}

fn daemon() -> Result<()> {
    let exe = spawn::resolve_daemon_exe()?;
    let status = Command::new(&exe).status()?;
    if !status.success() {
        bail!("janus-daemon exited with {status}");
    }
    Ok(())
}

/// `janus tmux open|attach|list`: drive the isolated `tmux -L metamach-tmux`
/// server directly (no Daemon round-trip - Task 2.4).
fn tmux(cmd: TmuxCmd) -> Result<()> {
    let backend = tmux::TmuxBackend::new();
    match cmd {
        TmuxCmd::Open { command, name, cwd } => {
            let id = match name {
                Some(n) => tmux::SessionId::from_name(n),
                None => tmux::SessionId::new_for_task(&uuid::Uuid::new_v4().to_string()),
            };
            backend.create_session(&id, &command, cwd.as_deref())?;
            println!(
                "created session {} (attach: janus tmux attach {})",
                id.as_str(),
                id.as_str()
            );
            Ok(())
        }
        TmuxCmd::Attach { name } => {
            let id = tmux::SessionId::from_name(name);
            backend.attach(&id)
        }
        TmuxCmd::List => {
            let sessions = backend.list_sessions()?;
            if sessions.is_empty() {
                println!("(no tmux sessions on -L {})", tmux::TMUX_SOCKET);
            } else {
                for s in sessions {
                    println!("{s}");
                }
            }
            Ok(())
        }
    }
}
