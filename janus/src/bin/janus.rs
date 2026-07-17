//! `janus` - unified CLI (ARCH §3). A UDS client of `janus-daemon`.
//!
//! Subcommands:
//!   `janus status [--blueprint <name>] [--json]` - Contract 3.3 progress snapshot.
//!   `janus daemon` - launch the resident `janus-daemon` in the foreground.
//!   `janus onboard --blueprint <name>` - register/reactivate a blueprint (Task 4.3).
//!   `janus offboard --blueprint <name>` - smelt + prune a blueprint (Task 4.2).
//!
//! All subcommands require the Daemon reachable; `status`/`onboard`/`offboard`
//! lazy-start it if absent (Feature-Spec §2.1 self-heal).

use std::process::Command;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use janus::protocol::{ActiveTask, ProgressPayload, Request, Response};
use janus::{spawn, uds};

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
}

fn main() -> Result<()> {
    match Cli::parse().command {
        CliCommand::Status { blueprint, json } => status(blueprint, json),
        CliCommand::Daemon => daemon(),
        CliCommand::Onboard { blueprint } => lifecycle_cmd(Request::Onboard { name: blueprint }),
        CliCommand::Offboard { blueprint } => lifecycle_cmd(Request::Offboard { name: blueprint }),
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
