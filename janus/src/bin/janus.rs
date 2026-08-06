//! `janus` - unified CLI (ARCH §3). A UDS client of `janus-daemon`.
//!
//! Subcommands:
//!   `janus init [path]` - scaffold .janus/, validate, and register blueprint with daemon.
//!   `janus start [--workflow <name>]` - execute workflow (Linear or DAG mode).
//!   `janus status [--blueprint <name>] [--json]` - Contract 3.3 progress snapshot.
//!   `janus stop / continue` - halt or resume tasks.
//!   `janus offboard --blueprint <name>` - smelt + prune a blueprint (Task 4.2).
//!   `janus daemon` - launch the resident `janus-daemon` in the foreground.
//!   `janus tmux open|attach|list` - manage tmux physical sessions (Task 2.4).
//!
//! `init`/`start`/`status`/`offboard` require the Daemon reachable (lazy-started if
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
use uuid::Uuid;

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
    /// Smelt execution traces + prune DB cache (Feature-Spec §2.5, Task 4.2).
    Offboard {
        /// Blueprint name to offboard (defaults to current directory name).
        #[arg(short, long)]
        blueprint: Option<String>,
    },
    /// Start blueprint execution (blueprint default or workflow override).
    Start {
        /// Blueprint name to start (defaults to current directory name).
        #[arg(short, long)]
        blueprint: Option<String>,
        /// Optional workflow name override.
        #[arg(short, long, group = "target")]
        workflow: Option<String>,
        /// Dry-run mode: validate and print workflow execution plan without dispatching.
        #[arg(long)]
        dry_run: bool,
        /// Inline command mode: execute a single shell command as a transient workflow step.
        #[arg(long, group = "target")]
        inline: Option<String>,
    },
    /// Stop active step session(s) for a blueprint or task.
    Stop {
        /// Blueprint name.
        #[arg(short, long)]
        blueprint: Option<String>,
        /// Task ID.
        #[arg(long)]
        task_id: Option<Uuid>,
    },
    /// Resume stopped/non-terminal tasks from last COMPLETED step checkpoint.
    Continue {
        /// Blueprint name.
        #[arg(short, long)]
        blueprint: Option<String>,
        /// Task ID.
        #[arg(long)]
        task_id: Option<Uuid>,
    },
    /// Manage tmux physical sessions (Task 2.4, `janus::tmux`).
    Tmux {
        #[command(subcommand)]
        cmd: TmuxCmd,
    },
    /// Initialize a project: scaffold .janus/, validate, and register with the daemon.
    Init {
        /// Project directory (defaults to current directory).
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Dry-run: scaffold and validate, but skip daemon registration.
        #[arg(long)]
        dry_run: bool,
    },
    /// Generate a Pipeline TOML from a natural-language description (ADR-022).
    Plan {
        /// Blueprint name (defaults to current directory name).
        #[arg(short, long)]
        blueprint: Option<String>,
        /// Natural-language description of the desired pipeline.
        #[arg(long)]
        description: String,
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

fn resolve_blueprint_name(bp_opt: Option<String>) -> Result<String> {
    if let Some(name) = bp_opt
        && !name.trim().is_empty()
    {
        return Ok(name);
    }
    let cwd = std::env::current_dir().context("get current working directory")?;
    let folder_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("cannot derive blueprint name from current directory"))?;
    Ok(folder_name.to_string())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        CliCommand::Status { blueprint, json } => status(blueprint, json),
        CliCommand::Daemon => daemon(),
        CliCommand::Offboard { blueprint } => {
            let bp = resolve_blueprint_name(blueprint)?;
            lifecycle_cmd(Request::Offboard { name: bp })
        }
        CliCommand::Start {
            blueprint,
            workflow,
            dry_run,
            inline,
        } => {
            let bp = resolve_blueprint_name(blueprint)?;
            start(bp, workflow, dry_run, inline)
        }
        CliCommand::Stop { blueprint, task_id } => {
            let bp = if blueprint.is_some() || task_id.is_none() {
                Some(resolve_blueprint_name(blueprint)?)
            } else {
                None
            };
            stop(bp, task_id)
        }
        CliCommand::Continue { blueprint, task_id } => {
            let bp = if blueprint.is_some() || task_id.is_none() {
                Some(resolve_blueprint_name(blueprint)?)
            } else {
                None
            };
            continue_cmd(bp, task_id)
        }
        CliCommand::Tmux { cmd } => tmux(cmd),
        CliCommand::Init { path, dry_run } => init_project(&path, dry_run),
        CliCommand::Plan {
            blueprint,
            description,
        } => {
            let bp = resolve_blueprint_name(blueprint)?;
            let repo_root = janus::paths::repo_root();
            plan_pipeline(&bp, &description, &repo_root)
        }
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

/// Lifecycle UDS helper (offboard): send the request, print the Daemon's ack.
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

fn start(
    blueprint: String,
    workflow: Option<String>,
    dry_run: bool,
    inline: Option<String>,
) -> Result<()> {
    let repo_root = janus::paths::repo_root();

    if dry_run {
        let name = if let Some(w) = workflow {
            w
        } else if let Some(ref cmd) = inline {
            println!("Linear Workflow Dry-Run (Inline Command):");
            println!("  Blueprint: {blueprint}");
            println!("  Step 1 (run) [builder] -> {cmd}");
            return Ok(());
        } else {
            janus::recipe::read_blueprint_name(&repo_root)
                .ok()
                .and_then(|name| janus::recipe::validate(&name, &repo_root).ok())
                .map(|r| r.default_workflow)
                .unwrap_or_else(|| "dev-flow".to_string())
        };

        match janus::recipe::load_unified_workflow(&name, &repo_root)? {
            janus::recipe::UnifiedWorkflow::Linear(wf) => {
                println!("Linear Workflow Dry-Run: {}", wf.workflow.name);
                println!("  Blueprint: {blueprint}");
                if let Some(ref desc) = wf.workflow.description {
                    println!("  Description: {desc}");
                }
                println!("  Steps ({}):", wf.steps.len());
                for (idx, step) in wf.steps.iter().enumerate() {
                    let cmd = step.command.as_deref().unwrap_or("-");
                    println!("    {}. {} [{}] -> {}", idx + 1, step.name, step.agent, cmd);
                }
            }
            janus::recipe::UnifiedWorkflow::Dag {
                config,
                inline_register,
            } => {
                println!(
                    "DAG Workflow Execution Plan Dry-Run: {}",
                    config.pipeline.name
                );
                println!("  Blueprint: {blueprint}");
                if let Some(ref desc) = config.pipeline.description {
                    println!("  Description: {desc}");
                }
                let plan = config.plan()?;
                println!("  Levels ({} total):", plan.levels.len());
                for (lvl_idx, level) in plan.levels.iter().enumerate() {
                    println!("    Level {}:", lvl_idx + 1);
                    for node in level {
                        if let Some(inline_wf) = inline_register.get(&node.workflow) {
                            println!("      - Node '{}' (inline steps):", node.id);
                            for s in &inline_wf.steps {
                                let cmd = s.command.as_deref().unwrap_or("-");
                                println!("          * {} [{}] -> {}", s.name, s.agent, cmd);
                            }
                        } else {
                            let deps = if node.needs.is_empty() {
                                "".to_string()
                            } else {
                                format!(" (needs: {})", node.needs.join(", "))
                            };
                            println!(
                                "      - Node '{}' -> workflow '{}'{}",
                                node.id, node.workflow, deps
                            );
                        }
                    }
                }
            }
        }
        return Ok(());
    }

    if let Err(e) = spawn::ensure_daemon(Duration::from_secs(5)) {
        bail!("janus-daemon not reachable: {e}\n  start it with `janus daemon`");
    }
    let resp = uds::request(&Request::Dispatch {
        blueprint: blueprint.clone(),
        workflow,
        inline_command: inline,
    })?;
    match resp {
        Response::Dispatch { task_id } => {
            println!("🚀 Started blueprint '{blueprint}' (task_id: {task_id})");
            Ok(())
        }
        Response::Error { message } => bail!(message),
        other => bail!("unexpected daemon response: {other:?}"),
    }
}

fn stop(blueprint: Option<String>, task_id: Option<uuid::Uuid>) -> Result<()> {
    if let Err(e) = spawn::ensure_daemon(Duration::from_secs(5)) {
        bail!("janus-daemon not reachable: {e}\n  start it with `janus daemon`");
    }
    let resp = uds::request(&Request::Stop { blueprint, task_id })?;
    match resp {
        Response::Ok { message } => {
            println!("🛑 {message}");
            Ok(())
        }
        Response::Error { message } => bail!(message),
        other => bail!("unexpected daemon response: {other:?}"),
    }
}

fn continue_cmd(blueprint: Option<String>, task_id: Option<uuid::Uuid>) -> Result<()> {
    if let Err(e) = spawn::ensure_daemon(Duration::from_secs(5)) {
        bail!("janus-daemon not reachable: {e}\n  start it with `janus daemon`");
    }
    let resp = uds::request(&Request::Continue { blueprint, task_id })?;
    match resp {
        Response::Ok { message } => {
            println!("🔄 {message}");
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

// ── Project init (janus init) ──────────────────────────────────────────

fn init_project(path: &Path, dry_run: bool) -> Result<()> {
    let repo_root = janus::paths::repo_root();
    let root = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let janus_dir = root.join(".janus");
    let already_exists = janus_dir.exists();

    if already_exists {
        eprintln!(
            ".janus/ already exists at {} — validating existing blueprint.",
            janus_dir.display()
        );
    } else {
        std::fs::create_dir_all(&janus_dir)?;
        let templates_root = repo_root.join("templates");
        if !templates_root.exists() {
            anyhow::bail!(
                "templates/ not found at {}. Is MetaMach installed?",
                templates_root.display()
            );
        }

        // Copy blueprint template.
        let bp_src = templates_root.join("blueprint.toml");
        if bp_src.exists() {
            std::fs::copy(&bp_src, janus_dir.join("blueprint.toml"))?;
            println!("   blueprint.toml → .janus/blueprint.toml");
        } else {
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "my_project".to_string())
                .replace('-', "_");
            std::fs::write(
                janus_dir.join("blueprint.toml"),
                format!(
                    "[blueprint]\nname = \"{name}\"\ndefault_workflow = \"req2spec\"\n\n[openwiki]\nscope = [\"{name}\"]\n"
                ),
            )?;
            println!("   blueprint.toml → .janus/blueprint.toml (auto-generated)");
        }

        // Copy agent templates.
        let agents_src = templates_root.join("agents");
        if agents_src.is_dir() {
            let agents_dst = janus_dir.join("agents");
            copy_dir(&agents_src, &agents_dst)?;
            println!("   agents/ → .janus/agents/");
        }

        // Copy workflow templates.
        let wf_src = templates_root.join("workflows");
        if wf_src.is_dir() {
            let wf_dst = janus_dir.join("workflows");
            copy_dir(&wf_src, &wf_dst)?;
            println!("   workflows/ → .janus/workflows/");
        }

        // Create openwiki directory for production whitepapers.
        let openwiki_dir = janus_dir.join("openwiki");
        std::fs::create_dir_all(&openwiki_dir)?;
    }

    // Read blueprint name for validation + registration.
    let name = janus::recipe::read_blueprint_name(&root)?;

    // Validate blueprint and default workflow.
    let recipe = janus::recipe::validate(&name, &root)
        .with_context(|| format!("blueprint validation failed for '{name}'"))?;

    println!();
    println!("✅ Blueprint validated: {name}");
    println!("   Default workflow: {}", recipe.default_workflow);
    println!("   OpenWiki scope: {}", recipe.openwiki_scope.join(", "));

    if dry_run {
        println!("\n   (dry-run: skipping daemon registration)");
        println!("   Run 'janus init' without --dry-run to register with the daemon.");
        return Ok(());
    }

    // Register with daemon.
    if let Err(e) = spawn::ensure_daemon(Duration::from_secs(5)) {
        bail!("janus-daemon not reachable: {e}\n  start it with `janus daemon`");
    }
    let resp = uds::request(&Request::Onboard { name })?;
    match resp {
        Response::Ok { message } => {
            println!("   {message}");
            println!();
            println!("Next steps:");
            println!("  1. Edit .janus/workflows/ to define your workflow");
            println!("  2. Dry-run:  janus start --dry-run");
            println!("  3. Execute:  janus start");
            Ok(())
        }
        Response::Error { message } => bail!(message),
        other => bail!("unexpected daemon response: {other:?}"),
    }
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// ── Pipeline commands (ADR-021/ADR-022) ─────────────────────────────────

fn plan_pipeline(name: &str, description: &str, repo_root: &Path) -> Result<()> {
    // Discover available workflows.
    let catalog = discover_workflows(repo_root)?;
    if catalog.is_empty() {
        bail!(
            "no workflows found in {}/templates/workflows/",
            repo_root.display()
        );
    }

    let catalog_text: String = catalog
        .iter()
        .map(|w| format!("- {} ({})", w.0, w.1))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You are a MetaMach Workflow architect. Given the following Workflow \
         library and a natural-language description, generate a valid Unified Workflow DAG \
         TOML file using [workflow] header (name, description) and [[nodes]] array (id, workflow or steps = [...], needs).\n\n## Available Workflows\n{catalog_text}\n\n## Request\n\
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
            {"role": "system", "content": "You are a Workflow architect. Output valid Unified Workflow TOML only."},
            {"role": "user", "content": prompt},
        ]
    });

    eprintln!("Generating workflow for '{name}'...");
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

    // Validate the generated TOML as a Unified Workflow.
    let config: PipelineConfig =
        toml::from_str(toml_text).context("LLM generated invalid workflow TOML")?;
    config
        .validate()
        .context("LLM generated invalid workflow DAG")?;

    let workflows_dir = repo_root.join(".janus/workflows");
    std::fs::create_dir_all(&workflows_dir)?;
    let path = workflows_dir.join(format!("{name}.toml"));
    std::fs::write(&path, toml_text)?;
    println!("Workflow written to {}", path.display());
    Ok(())
}

/// Discover available workflows in `templates/workflows/` and legacy `workflows/`.
fn discover_workflows(repo_root: &Path) -> Result<Vec<(String, String)>> {
    let mut workflows = Vec::new();
    for dir in &["templates/workflows", "workflows"] {
        let wf_dir = repo_root.join(dir);
        if let Ok(entries) = std::fs::read_dir(&wf_dir) {
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
        }
    }
    Ok(workflows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_workflows(dir: &Path, files: &[(&str, &str, Option<&str>)]) {
        let wf_dir = dir.join("workflows");
        std::fs::create_dir_all(&wf_dir).unwrap();
        for (name, _body, desc) in files {
            let desc_line = desc
                .map(|d| format!("description = \"{d}\"\n"))
                .unwrap_or_default();
            std::fs::write(
                wf_dir.join(format!("{name}.toml")),
                format!("[workflow]\nname = \"{name}\"\n{desc_line}"),
            )
            .unwrap();
        }
    }

    #[test]
    fn discover_workflows_finds_toml_files() {
        let dir = tempfile::tempdir().unwrap();
        setup_workflows(
            dir.path(),
            &[
                ("wf_build", "", Some("Build firmware")),
                ("wf_flash", "", Some("Flash to device")),
            ],
        );
        let wfs = discover_workflows(dir.path()).unwrap();
        assert_eq!(wfs.len(), 2);
        assert!(
            wfs.iter()
                .any(|(n, d)| n == "wf_build" && d == "Build firmware")
        );
        assert!(
            wfs.iter()
                .any(|(n, d)| n == "wf_flash" && d == "Flash to device")
        );
    }

    #[test]
    fn discover_workflows_returns_empty_for_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let wfs = discover_workflows(dir.path()).unwrap();
        assert!(wfs.is_empty());
    }

    #[test]
    fn discover_workflows_skips_non_toml() {
        let dir = tempfile::tempdir().unwrap();
        let wf_dir = dir.path().join("workflows");
        std::fs::create_dir_all(&wf_dir).unwrap();
        std::fs::write(wf_dir.join("README.md"), "docs").unwrap();
        setup_workflows(dir.path(), &[("wf_build", "", None)]);
        let wfs = discover_workflows(dir.path()).unwrap();
        assert_eq!(wfs.len(), 1);
    }

    #[test]
    fn validate_pipeline_rejects_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let wf_dir = dir.path().join(".janus/workflows");
        std::fs::create_dir_all(&wf_dir).unwrap();

        let toml = "[workflow]\nname = \"cycle\"\n\n[[nodes]]\nid = \"a\"\nworkflow = \"wf_a\"\nneeds = [\"b\"]\n\n[[nodes]]\nid = \"b\"\nworkflow = \"wf_b\"\nneeds = [\"a\"]\n";
        std::fs::write(wf_dir.join("cycle.toml"), toml).unwrap();

        let err = janus::recipe::load_unified_workflow("cycle", dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("cycle"),
            "expected cycle error, got: {err}"
        );
    }

    #[test]
    fn validate_pipeline_accepts_valid() {
        let dir = tempfile::tempdir().unwrap();
        let wf_dir = dir.path().join(".janus/workflows");
        std::fs::create_dir_all(&wf_dir).unwrap();

        let toml = "[workflow]\nname = \"ok\"\n\n[[nodes]]\nid = \"a\"\nworkflow = \"wf_a\"\n";
        std::fs::write(wf_dir.join("ok.toml"), toml).unwrap();

        let unified =
            janus::recipe::load_unified_workflow("ok", dir.path()).expect("valid workflow");
        assert!(matches!(
            unified,
            janus::recipe::UnifiedWorkflow::Dag { .. }
        ));
    }

    #[test]
    fn test_janus_init_fresh_project_creates_dir_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let fresh_root = dir.path().join("fresh_app");
        std::fs::create_dir_all(&fresh_root).unwrap();

        // Run init_project with dry_run = true (skip daemon UDS call)
        init_project(&fresh_root, true).expect("init_project on fresh dir should succeed");

        assert!(fresh_root.join(".janus").is_dir());
        assert!(fresh_root.join(".janus/blueprint.toml").is_file());
        assert!(fresh_root.join(".janus/openwiki").is_dir());
        assert!(fresh_root.join(".janus/agents").is_dir());
        assert!(fresh_root.join(".janus/workflows").is_dir());
    }

    #[test]
    fn test_resolve_blueprint_name() {
        assert_eq!(
            resolve_blueprint_name(Some("explicit_bp".to_string())).unwrap(),
            "explicit_bp"
        );
        let cwd_bp = resolve_blueprint_name(None).unwrap();
        let expected_cwd = std::env::current_dir()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(cwd_bp, expected_cwd);
    }
}
