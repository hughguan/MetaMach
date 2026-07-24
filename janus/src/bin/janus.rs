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

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use janus::pipeline::PipelineConfig;
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
    /// Pipeline DAG operations (ADR-021, 0.4.9).
    Pipeline {
        #[command(subcommand)]
        cmd: PipelineCmd,
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
    /// List live tmux sessions on the isolated tmux server.
    List,
}

/// `janus pipeline` subcommands (ADR-021/ADR-022).
#[derive(Subcommand)]
enum PipelineCmd {
    /// Generate a Pipeline TOML from a natural-language description (ADR-022).
    Plan {
        /// Blueprint name (for context — the pipeline will be named after it).
        #[arg(long)]
        blueprint: String,
        /// Natural-language description of the desired pipeline.
        #[arg(long)]
        description: String,
    },
    /// Validate a pipeline TOML file without executing it.
    Validate {
        /// Path to the pipeline TOML file.
        path: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        CliCommand::Status { blueprint, json } => status(blueprint, json),
        CliCommand::Daemon => daemon(),
        CliCommand::Onboard { blueprint } => lifecycle_cmd(Request::Onboard { name: blueprint }),
        CliCommand::Offboard { blueprint } => lifecycle_cmd(Request::Offboard { name: blueprint }),
        CliCommand::Tmux { cmd } => tmux(cmd),
        CliCommand::Pipeline { cmd } => pipeline(cmd),
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

// ── Pipeline commands (ADR-021/ADR-022) ─────────────────────────────────

fn pipeline(cmd: PipelineCmd) -> Result<()> {
    let repo_root = janus::paths::repo_root();
    match cmd {
        PipelineCmd::Plan {
            blueprint,
            description,
        } => plan_pipeline(&blueprint, &description, &repo_root),
        PipelineCmd::Validate { path } => validate_pipeline(&path),
    }
}

fn plan_pipeline(name: &str, description: &str, repo_root: &Path) -> Result<()> {
    // Discover available workflows.
    let catalog = discover_workflows(repo_root)?;
    if catalog.is_empty() {
        bail!("no workflows found in {}/workflows/", repo_root.display());
    }

    let catalog_text: String = catalog
        .iter()
        .map(|w| format!("- {} ({})", w.0, w.1))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You are a MetaMach Pipeline architect. Given the following Workflow \
         library and a natural-language description, generate a valid Pipeline \
         TOML file.\n\n## Available Workflows\n{catalog_text}\n\n## Request\n\
         {description}\n\nOutput ONLY the TOML, no explanation."
    );

    let endpoint = std::env::var("JANUS_PLANNER_ENDPOINT")
        .unwrap_or_else(|_| "https://ark.cn-beijing.volces.com/api/coding/v3/responses".into());
    let model =
        std::env::var("JANUS_PLANNER_MODEL").unwrap_or_else(|_| "deepseek-v4-pro-260425".into());
    let api_key = std::env::var("JANUS_PLANNER_API_KEY")
        .or_else(|_| std::env::var("ZAI_CODING_CN_API_KEY"))
        .context("JANUS_PLANNER_API_KEY or ZAI_CODING_CN_API_KEY not set")?;

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "messages": [
            {"role": "system", "content": "You are a Pipeline architect. Output valid TOML only."},
            {"role": "user", "content": prompt},
        ]
    });

    eprintln!("Generating pipeline for '{name}'...");
    let resp = ureq::post(&endpoint)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(60))
        .send_json(body)
        .map_err(|e| anyhow::anyhow!("LLM request failed: {e}"))?;
    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| anyhow::anyhow!("parse LLM response: {e}"))?;
    let toml_text = v["choices"][0]["message"]["content"]
        .as_str()
        .context("LLM response missing content")?;

    // Validate the generated TOML.
    let config: PipelineConfig = toml::from_str(toml_text).context("LLM generated invalid TOML")?;
    config
        .validate()
        .context("LLM generated invalid pipeline")?;

    let pipelines_dir = repo_root.join("pipelines");
    let _ = std::fs::create_dir_all(&pipelines_dir);
    let path = pipelines_dir.join(format!("{name}.toml"));
    std::fs::write(&path, toml_text)?;
    println!("Pipeline written to {}", path.display());
    Ok(())
}

fn validate_pipeline(path: &Path) -> Result<()> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let _config: PipelineConfig =
        toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    println!("Pipeline {} is valid.", path.display());
    Ok(())
}

/// Discover available workflows in `workflows/`. Returns (name, description) pairs.
fn discover_workflows(repo_root: &Path) -> Result<Vec<(String, String)>> {
    let wf_dir = repo_root.join("workflows");
    let mut workflows = Vec::new();
    let entries = match std::fs::read_dir(&wf_dir) {
        Ok(e) => e,
        Err(_) => return Ok(workflows),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "toml").unwrap_or(false)
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(val) = toml::from_str::<toml::Table>(&text)
        {
            let name = val
                .get("workflow")
                .and_then(|w| w.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let desc = val
                .get("workflow")
                .and_then(|w| w.get("description"))
                .and_then(|v| v.as_str())
                .unwrap_or("(no description)");
            workflows.push((name.to_string(), desc.to_string()));
        }
    }
    Ok(workflows)
}
