//! Workflow-execution engine (M4 Task 4.1 Phase 0b; `docs/M4-4.1-design.md` §0.4).
//!
//! The engine consumes the Phase 0a absurd adapter ([`crate::absurd::DurableEngine`])
//! and the [`crate::tmux::DurableBackend`] to drive a blueprint's workflow:
//!
//! 1. `create_queue` + `spawn_task` (absurd mints the `task_id`) + `claim_task`
//!    (pull-mode lease; one absurd run = one workflow dispatch).
//! 2. For each step: pin `target_sha` = git HEAD, upsert `metamach_step_meta`
//!    (`STARTING` -> `RUNNING`), create a tmux session running `janush -c "<cmd>"`
//!    (so each Agent command is Tool-Guard-reconciled), poll the pane for exit,
//!    capture `stdout_tail`, and `set_checkpoint` + finalize the step.
//! 3. `complete_run` once after all steps (one run per dispatch - absurd's
//!    `complete_run` ends the *task*, so it is NOT called per step), or
//!    `fail_run` on the first failing step. A HITL `SUSPENDED` step leaves the
//!    run non-terminal (resume is the follow-on, design §3.3).
//!
//! Long steps survive past absurd's 30s lease: the poll loop calls
//! `extend_claim` every ~10s so absurd doesn't auto-fail the run mid-step.
//!
//! Generic over `E: DurableEngine` + `B: DurableBackend` so unit tests use the
//! in-memory fakes (no PG, no tmux); production uses `AbsurdPgAdapter` +
//! `TmuxBackend`.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde_json::json;
use tracing::warn;
use uuid::Uuid;

use crate::absurd::AbsurdDb;
use crate::absurd::adapter::DurableEngine;
use crate::paths;
use crate::protocol::truncate_16k;
use crate::recipe::ValidatedRecipe;
use crate::tmux::{DurableBackend, SessionId};

/// Worker id the engine presents to absurd's `claim_task` (pull-mode lease).
const WORKER_ID: &str = "janus-daemon";

/// Poll the pane this often while waiting for the step to exit.
const POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Renew the absurd lease this often (well under the 30s lease window).
const LEASE_EXTEND_INTERVAL: Duration = Duration::from_secs(10);
/// How many seconds each `extend_claim` adds to the lease.
const LEASE_EXTEND_BY: i64 = 30;

/// All-zeros sentinel for a non-git blueprint (the `target_sha` column's default;
/// Task 4.4 enforcement is deferred).
const NULL_SHA: &str = "0000000000000000000000000000000000000000";

/// Build the absurd queue name for a blueprint/workflow, sanitized to a valid
/// unquoted PG identifier (`[a-zA-Z0-9_]`). See [`spawn_workflow`] for why
/// sanitization is required (absurd's `%I` table naming vs. the adapter's
/// unquoted reads).
fn queue_name(recipe: &ValidatedRecipe, workflow_name: &str) -> String {
    fn sanitize(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }
    format!("{}_{}", sanitize(&recipe.name), sanitize(workflow_name))
}

/// Enqueue a workflow on the absurd engine (Phase 1 of dispatch). Returns the
/// absurd-minted `task_id`. The daemon calls this synchronously so it can return
/// the id to the caller *before* the (potentially long) step loop runs, then
/// spawns [`run_workflow`] detached to claim + execute the steps.
pub async fn spawn_workflow<E>(
    engine: &E,
    recipe: &ValidatedRecipe,
    workflow_name: &str,
) -> Result<Uuid>
where
    E: DurableEngine,
{
    // MetaMach queue convention: `<blueprint>_<workflow>`, sanitized to a valid
    // unquoted PG ident ([a-zA-Z0-9_]). Blueprint names are already validated to
    // that charset, but workflow names may contain dashes (e.g. `test-flow`);
    // absurd materializes `t_<queue>` via `%I` (quoted, preserves dashes) while
    // the adapter reads `t_<queue>` unquoted - so a dash would make reads miss
    // the table. Sanitizing at construction makes the adapter's defensive
    // `queue_ident_suffix` a no-op and keeps the two in sync.
    let queue = queue_name(recipe, workflow_name);
    let task_name = format!("{}.{}", recipe.name, workflow_name);
    engine.create_queue(&queue).await?;
    let payload = serde_json::to_value(&recipe.workflow.steps)
        .context("serialize workflow steps for spawn_task")?;
    let task_id = engine.spawn_task(&queue, &task_name, &payload).await?;
    Ok(task_id)
}

/// Claim + execute the steps of an already-spawned workflow (Phase 2 of
/// dispatch). Returns the same `task_id`. Run detached by the daemon; the
/// step-loop duration is bounded only by the steps' real runtimes (lease
/// extended in the background). `janush` is the resolved proxy-shell binary
/// (the daemon resolves it; the engine stays pure / unit-testable).
#[allow(clippy::too_many_arguments)] // mirrors WebhookPayload::build; all args are genuinely needed.
pub async fn run_workflow<E, B>(
    db: &AbsurdDb,
    engine: &E,
    backend: &B,
    recipe: &ValidatedRecipe,
    workflow_name: &str,
    repo_root: &Path,
    task_id: Uuid,
    janush: &Path,
) -> Result<Uuid>
where
    E: DurableEngine,
    B: DurableBackend,
{
    let queue = queue_name(recipe, workflow_name);

    // Pull-claim (absurd owns the lease). The claimed task_id must match the one
    // absurd just minted - if not, another worker raced us (single-daemon, so
    // this is a invariant guard, not a recovery path).
    let claimed = engine
        .claim_task(&queue, WORKER_ID)
        .await?
        .with_context(|| format!("claim_task returned None for queue {queue}"))?;
    ensure!(
        claimed.task_id == task_id,
        "claimed task_id {} != spawned task_id {}",
        claimed.task_id,
        task_id
    );
    let run_id = claimed.run_id;

    let target_sha = git_head(repo_root);

    let mut failed = false;
    let mut suspended = false;

    for (idx, step) in recipe.workflow.steps.iter().enumerate() {
        // One tmux session per step, named tmux-janus-task-<task_id>-<idx> (the
        // idx suffix disambiguates steps; cold-start resume uses a fresh uuid
        // suffix - same prefix shape, ARCH §6.1).
        let session = SessionId::new_for_task(&format!("{}-{}", task_id.simple(), idx));
        let session_name = session.as_str().to_string();

        db.upsert_step_start(
            &recipe.name,
            task_id,
            &step.name,
            workflow_name,
            &target_sha,
            &session_name,
        )
        .await?;
        db.set_step_running(&recipe.name, task_id, &step.name)
            .await?;

        match step.command.as_deref() {
            Some(_) => {
                let cmd = step_command(step, recipe, task_id, workflow_name, janush);
                backend.create_session(&session, &cmd, Some(repo_root))?;

                let exit = poll_exit_with_lease(engine, backend, &queue, run_id, &session).await;
                let stdout_tail = backend
                    .capture_pane(&session)
                    .ok()
                    .map(|s| truncate_16k(&s));
                let _ = backend.kill_session(&session);

                let exit_code = match exit {
                    Ok(c) => c,
                    Err(e) => {
                        // Lease-lost (absurd auto-failed the run) or poll error.
                        warn!(step = %step.name, %task_id, error = %e, "step poll failed");
                        db.finalize_step(
                            &recipe.name,
                            task_id,
                            &step.name,
                            "FAILED",
                            None,
                            stdout_tail.as_deref(),
                        )
                        .await?;
                        failed = true;
                        break;
                    }
                };

                // Re-read the step status: the daemon's GuardCheck handler may
                // have flipped it to SUSPENDED (HITL require_approval/blacklist)
                // while the pane was running - janush exits 126 on BLOCK, but the
                // authoritative signal is the overlay status (126 alone is
                // ambiguous: it's also daemon-unreachable fail-closed).
                let status = db.step_status(&recipe.name, task_id, &step.name).await?;
                match status.as_deref() {
                    Some("SUSPENDED") => {
                        db.finalize_step(
                            &recipe.name,
                            task_id,
                            &step.name,
                            "SUSPENDED",
                            None,
                            stdout_tail.as_deref(),
                        )
                        .await?;
                        engine
                            .set_checkpoint(
                                &queue,
                                task_id,
                                &step.name,
                                &json!({"step": step.name, "status": "SUSPENDED"}),
                                run_id,
                            )
                            .await?;
                        suspended = true;
                        break;
                    }
                    _ => {
                        if exit_code == 0 {
                            engine
                                .set_checkpoint(
                                    &queue,
                                    task_id,
                                    &step.name,
                                    &json!({"step": step.name, "status": "COMPLETED", "exit": 0}),
                                    run_id,
                                )
                                .await?;
                            db.finalize_step(
                                &recipe.name,
                                task_id,
                                &step.name,
                                "COMPLETED",
                                Some(0),
                                stdout_tail.as_deref(),
                            )
                            .await?;
                        } else {
                            db.finalize_step(
                                &recipe.name,
                                task_id,
                                &step.name,
                                "FAILED",
                                Some(exit_code),
                                stdout_tail.as_deref(),
                            )
                            .await?;
                            failed = true;
                            break;
                        }
                    }
                }
            }
            None => {
                // No command -> a manual/placeholder step: no tmux session,
                // immediate COMPLETED. (Real Agent steps always carry a command.)
                engine
                    .set_checkpoint(
                        &queue,
                        task_id,
                        &step.name,
                        &json!({"step": step.name, "status": "COMPLETED", "noop": true}),
                        run_id,
                    )
                    .await?;
                db.finalize_step(
                    &recipe.name,
                    task_id,
                    &step.name,
                    "COMPLETED",
                    Some(0),
                    None,
                )
                .await?;
            }
        }
    }

    // Finalize the absurd run exactly once. complete_run ends the *task* (errors
    // if called twice / not running), so it is called once per dispatch - not
    // per step. A suspended task stays non-terminal (resume is the follow-on).
    if suspended {
        // intentionally no complete_run/fail_run - run stays "running" in absurd.
    } else if failed {
        engine
            .fail_run(
                &queue,
                run_id,
                &json!({"task_id": task_id, "reason": "step_failed"}),
            )
            .await?;
    } else {
        engine
            .complete_run(&queue, run_id, &json!({"task_id": task_id}))
            .await?;
    }

    Ok(task_id)
}

/// Poll a tmux session for pane death, renewing the absurd lease every
/// ~[`LEASE_EXTEND_INTERVAL`] so absurd doesn't auto-fail the run mid-step.
/// Returns the pane exit code. A lease-lost (run already failed by absurd)
/// surfaces as an `Err` from `extend_claim` -> the caller marks the step FAILED.
async fn poll_exit_with_lease<E, B>(
    engine: &E,
    backend: &B,
    queue: &str,
    run_id: Uuid,
    session: &SessionId,
) -> Result<i32>
where
    E: DurableEngine,
    B: DurableBackend,
{
    let mut since_extend = Duration::ZERO;
    loop {
        if let Some(code) = backend.poll_exit(session)? {
            return Ok(code);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
        since_extend += POLL_INTERVAL;
        if since_extend >= LEASE_EXTEND_INTERVAL {
            engine.extend_claim(queue, run_id, LEASE_EXTEND_BY).await?;
            since_extend = Duration::ZERO;
        }
    }
}

/// Build the tmux workload string for a step: `env HERDR_PLUGIN_STATE_DIR=...
/// JANUS_AGENT=... ... janush -c '<command>'`. `janush` reads the `JANUS_*` env
/// vars to populate its `GuardCheck` (verified in `bin/janush.rs`), so the
/// daemon's `suspend_step` path fires with the correct task/step context on a
/// HITL BLOCK. `HERDR_PLUGIN_STATE_DIR` is set explicitly so janush resolves the
/// daemon socket even when the long-lived `metamach-tmux` server's frozen env
/// points at a stale state dir.
fn step_command(
    step: &crate::recipe::WorkflowStep,
    recipe: &ValidatedRecipe,
    task_id: Uuid,
    workflow_name: &str,
    janush: &Path,
) -> String {
    let command = step.command.as_deref().unwrap_or("true");
    format!(
        "env HERDR_PLUGIN_STATE_DIR={state_dir} JANUS_AGENT={agent} \
         JANUS_BLUEPRINT={blueprint} JANUS_TASK_ID={task_id} JANUS_STEP={step_name} \
         JANUS_WORKFLOW={workflow} {janush} -c {command}",
        state_dir = shell_quote(&paths::state_dir().to_string_lossy()),
        agent = shell_quote(&step.agent),
        blueprint = shell_quote(&recipe.name),
        task_id = task_id,
        step_name = shell_quote(&step.name),
        workflow = shell_quote(workflow_name),
        janush = shell_quote(&janush.to_string_lossy()),
        command = shell_quote(command),
    )
}

/// Single-quote a string for shell consumption, escaping internal single-quotes
/// as `'\''` (the POSIX idiom). Keeps arbitrary step commands safe to pass as
/// one `janush -c` argument.
fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Resolve the current git HEAD short hash at `repo_root` (40 hex chars), or the
/// all-zeros sentinel for a non-git blueprint (Task 4.4 enforcement is deferred;
/// Phase 0b only *populates* the column). Best-effort: any git failure -> the
/// sentinel, so a missing git never blocks dispatch.
pub fn git_head(repo_root: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let h = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit()) {
                h
            } else {
                NULL_SHA.to_string()
            }
        }
        _ => NULL_SHA.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::absurd::adapter::FakeEngine;
    use crate::recipe::{Workflow, WorkflowSection, WorkflowStep};
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// In-memory `DurableBackend` for engine unit tests: completes each session
    /// instantly with a configurable per-step exit code (popped in
    /// `create_session` order via `poll_exit`).
    struct FakeBackend {
        exit_codes: Mutex<VecDeque<i32>>,
        created: Mutex<Vec<String>>,
        pane: String,
    }

    impl FakeBackend {
        fn new(codes: &[i32]) -> Self {
            Self {
                exit_codes: Mutex::new(codes.iter().copied().collect()),
                created: Mutex::new(Vec::new()),
                pane: "pane-output\n".to_string(),
            }
        }
        fn created_count(&self) -> usize {
            self.created.lock().unwrap().len()
        }
    }

    impl DurableBackend for FakeBackend {
        fn create_session(
            &self,
            id: &SessionId,
            _command: &str,
            _cwd: Option<&Path>,
        ) -> Result<()> {
            self.created.lock().unwrap().push(id.as_str().to_string());
            Ok(())
        }
        fn attach(&self, _id: &SessionId) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _id: &SessionId) -> Result<()> {
            Ok(())
        }
        fn has_session(&self, _id: &SessionId) -> Result<bool> {
            Ok(false)
        }
        fn list_sessions(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }
        fn capture_pane(&self, _id: &SessionId) -> Result<String> {
            Ok(self.pane.clone())
        }
        fn poll_exit(&self, _id: &SessionId) -> Result<Option<i32>> {
            // Instant completion with the next configured exit code.
            Ok(Some(
                self.exit_codes.lock().unwrap().pop_front().unwrap_or(0),
            ))
        }
    }

    /// A 2-step recipe for the unit tests (no on-disk files needed - the engine
    /// reads `recipe.workflow.steps` directly).
    fn two_step_recipe() -> ValidatedRecipe {
        ValidatedRecipe {
            name: "testbp".to_string(),
            default_workflow: "test-flow".to_string(),
            remote_host: None,
            openwiki_scope: vec!["test".to_string()],
            config_text: String::new(),
            workflow: Workflow {
                workflow: WorkflowSection {
                    name: "test-flow".to_string(),
                    description: None,
                },
                steps: vec![
                    WorkflowStep {
                        name: "scout".to_string(),
                        agent: "default".to_string(),
                        command: Some("true".to_string()),
                        host: None,
                        toolset: None,
                    },
                    WorkflowStep {
                        name: "build".to_string(),
                        agent: "default".to_string(),
                        command: Some("echo ok".to_string()),
                        host: None,
                        toolset: None,
                    },
                ],
            },
        }
    }

    /// A degraded AbsurdDb (no PG) - step_meta writes degrade to the fallback
    /// ring (no error) and `step_status` returns `None`, so the engine's control
    /// flow is exercised without PG. The SUSPENDED branch (which needs a real
    /// `step_status`) is covered by the PG-gated integration test.
    fn degraded_db(dir: &Path) -> AbsurdDb {
        AbsurdDb::open_degraded(&dir.join("fallback.db")).expect("open degraded")
    }

    #[tokio::test]
    async fn run_workflow_happy_path_completes_all_steps() {
        let tmp = tempfile::tempdir().unwrap();
        let db = degraded_db(tmp.path());
        let engine = FakeEngine::new();
        let backend = FakeBackend::new(&[0, 0]);
        let recipe = two_step_recipe();

        let task_id = spawn_workflow(&engine, &recipe, "test-flow")
            .await
            .expect("spawn");
        run_workflow(
            &db,
            &engine,
            &backend,
            &recipe,
            "test-flow",
            tmp.path(),
            task_id,
            Path::new("/bin/janush"),
        )
        .await
        .expect("workflow ok");

        // absurd run completed (complete_run called once at the end).
        assert_eq!(engine.task_state(task_id).as_deref(), Some("completed"));
        // Two steps -> two tmux sessions created + two checkpoints written.
        assert_eq!(backend.created_count(), 2);
        let cp = engine.last_checkpoint(task_id).expect("checkpoint");
        assert_eq!(cp.0, "build"); // last checkpoint is the final step
        assert!(cp.1.to_string().contains("COMPLETED"));
    }

    #[tokio::test]
    async fn run_workflow_stops_on_first_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let db = degraded_db(tmp.path());
        let engine = FakeEngine::new();
        // step 1 ok, step 2 exits 1.
        let backend = FakeBackend::new(&[0, 1]);
        let recipe = two_step_recipe();

        let task_id = spawn_workflow(&engine, &recipe, "test-flow")
            .await
            .expect("spawn");
        run_workflow(
            &db,
            &engine,
            &backend,
            &recipe,
            "test-flow",
            tmp.path(),
            task_id,
            Path::new("/bin/janush"),
        )
        .await
        .expect("workflow ok");

        // fail_run called once -> task failed.
        assert_eq!(engine.task_state(task_id).as_deref(), Some("failed"));
        // Step 2's session WAS created (the engine creates the session before
        // polling), but no third step ran.
        assert_eq!(backend.created_count(), 2);
        // Last checkpoint is step 1 (step 2 failed before its checkpoint write).
        let cp = engine.last_checkpoint(task_id).expect("checkpoint");
        assert_eq!(cp.0, "scout");
    }

    #[test]
    fn git_head_returns_full_hash_in_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let sh = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        if !sh(&["init"])
            || !sh(&["config", "user.email", "ci@example.com"])
            || !sh(&["config", "user.name", "ci"])
        {
            eprintln!("git unavailable; skipping git_head_returns_full_hash_in_git_repo");
            return;
        }
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        sh(&["add", "a.txt"]);
        sh(&["commit", "-m", "x"]);
        let h = git_head(repo);
        assert_eq!(h.len(), 40, "expected 40-hex SHA, got {h}");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()), "non-hex: {h}");
        assert_ne!(h, NULL_SHA);
    }

    #[test]
    fn git_head_all_zeros_for_non_git() {
        let dir = tempfile::tempdir().unwrap();
        // A fresh tempdir is not a git repo -> sentinel.
        let h = git_head(dir.path());
        assert_eq!(h, NULL_SHA);
    }

    #[test]
    fn queue_name_sanitizes_non_ident_chars() {
        // Blueprint names are already [a-zA-Z0-9_]; workflow names may carry
        // dashes/dots/spaces. All must collapse to `_` so absurd's `t_<queue>`
        // table name matches the adapter's unquoted read.
        let recipe = ValidatedRecipe {
            name: "gate".to_string(),
            default_workflow: "test-flow".to_string(),
            remote_host: None,
            openwiki_scope: vec![],
            config_text: String::new(),
            workflow: Workflow {
                workflow: WorkflowSection {
                    name: "test-flow".to_string(),
                    description: None,
                },
                steps: vec![],
            },
        };
        assert_eq!(queue_name(&recipe, "test-flow"), "gate_test_flow");
        assert_eq!(queue_name(&recipe, "a.b c"), "gate_a_b_c");
        assert_eq!(queue_name(&recipe, "clean"), "gate_clean");
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        // POSIX idiom: a literal ' becomes '\''.
        assert_eq!(shell_quote("simple"), "'simple'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
        // Round-trips through /bin/sh -c echo (best-effort; skip if sh absent).
        let dangerous = "foo'; rm -rf /; echo 'bar";
        let quoted = shell_quote(dangerous);
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", &format!("printf %s {quoted}")])
            .output();
        if let Ok(o) = out {
            assert_eq!(String::from_utf8_lossy(&o.stdout), dangerous);
        }
    }

    #[test]
    fn step_command_includes_janush_and_env_context() {
        let step = WorkflowStep {
            name: "flash".to_string(),
            agent: "deployer".to_string(),
            command: Some("make flash".to_string()),
            host: None,
            toolset: None,
        };
        let recipe = ValidatedRecipe {
            name: "gatemetric".to_string(),
            default_workflow: "fw".to_string(),
            remote_host: None,
            openwiki_scope: vec![],
            config_text: String::new(),
            workflow: Workflow {
                workflow: WorkflowSection {
                    name: "fw".to_string(),
                    description: None,
                },
                steps: vec![step.clone()],
            },
        };
        let janush = PathBuf::from("/bin/janush");
        let task_id = Uuid::nil();
        let cmd = step_command(&step, &recipe, task_id, "fw", &janush);
        assert!(cmd.starts_with("env HERDR_PLUGIN_STATE_DIR="));
        assert!(cmd.contains("JANUS_AGENT='deployer'"));
        assert!(cmd.contains("JANUS_BLUEPRINT='gatemetric'"));
        assert!(cmd.contains("JANUS_TASK_ID=00000000-0000-0000-0000-000000000000"));
        assert!(cmd.contains("JANUS_STEP='flash'"));
        assert!(cmd.contains("JANUS_WORKFLOW='fw'"));
        assert!(cmd.contains("'/bin/janush' -c 'make flash'"));
    }
}
