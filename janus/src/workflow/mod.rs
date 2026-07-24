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
use crate::absurd::adapter::{AwaitOutcome, DurableEngine};
use crate::paths;
use crate::protocol::{self, truncate_16k};
use crate::recipe::ValidatedRecipe;
use crate::tmux::{BackendFactory, DurableBackend, SESSION_PREFIX, SessionId};

pub mod filter;

/// Worker id the engine presents to absurd's `claim_task` (pull-mode lease).
const WORKER_ID: &str = "janus-daemon";

/// Poll the pane this often while waiting for the step to exit.
const POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Renew the absurd lease this often (well under the 30s lease window).
const LEASE_EXTEND_INTERVAL: Duration = Duration::from_secs(10);
/// How many seconds each `extend_claim` adds to the lease.
const LEASE_EXTEND_BY: i64 = 30;

/// Poll `claim_task` this often while waiting for a retry run to become claimable
/// (absurd schedules the retry after a backoff; `claim_task` returns `None` until
/// then). The loop also re-checks `non_terminal_tasks` to distinguish "retry
/// pending" from "task terminal" so it can exit.
const RETRY_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// All-zeros sentinel for a non-git blueprint (the `target_sha` column's default;
/// Task 4.4 enforcement is deferred).
const NULL_SHA: &str = "0000000000000000000000000000000000000000";

/// Outcome of one claim's step loop, returned by [`run_steps`] + acted on by the
/// [`run_workflow`] retry loop.
enum StepOutcome {
    /// All steps from `start_idx` reached `COMPLETED` -> `complete_run` + exit.
    Done,
    /// A step exited non-zero (or the poll/lease failed) -> `fail_run` + retry.
    Failed,
    /// A step `SUSPENDED` (HITL BLOCK) -> await the verdict (`await_event`) +
    /// re-run the step on approval. Carries the suspended step's index + name.
    Suspended { step_idx: usize, step_name: String },
    /// A step completed execution but the HEAD advanced mid-step (Task 4.4
    /// `target_sha` enforcement): the pinned SHA no longer matches current HEAD.
    /// The step result is discarded, the step is marked `SUSPENDED` with
    /// `CONCURRENCY_RACE_ALERT`, and the retry loop picks it up against the new
    /// HEAD (the absurd retry run mints a fresh claim).
    StaleHead { step_name: String },
}

/// What the [`run_workflow`] loop should do at a given resume point, derived from
/// the last absurd checkpoint (see [`resume_point`]).
enum ResumeAction {
    /// Run the steps from `start_idx` (fresh dispatch, skip `COMPLETED`, or
    /// re-run an `APPROVED` HITL step).
    Run,
    /// The last checkpoint is a `SUSPENDED` step with no verdict yet (e.g. a
    /// re-claim after the HITL timeout) -> `await_event` to resolve it.
    AwaitHITL,
    /// The last checkpoint is a `REJECTED` verdict -> `fail_run` + exit.
    FailHITL,
}

/// Build the absurd queue name for a blueprint/workflow, sanitized to a valid
/// unquoted PG identifier (`[a-zA-Z0-9_]`). See [`spawn_workflow`] for why
/// sanitization is required (absurd's `%I` table naming vs. the adapter's
/// unquoted reads).
/// Build the absurd queue name for a blueprint/workflow, sanitized to a valid
/// unquoted PG identifier (`[a-zA-Z0-9_]`). See [`spawn_workflow`] for why
/// sanitization is required (absurd's `%I` table naming vs. the adapter's
/// unquoted reads). Public so the HITL verdict sink can build the same queue
/// name for `emit_event`.
pub fn queue_name(blueprint: &str, workflow_name: &str) -> String {
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
    format!("{}_{}", sanitize(blueprint), sanitize(workflow_name))
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
    let queue = queue_name(&recipe.name, workflow_name);
    let task_name = format!("{}.{}", recipe.name, workflow_name);
    engine.create_queue(&queue).await?;
    let payload = serde_json::to_value(&recipe.workflow.steps)
        .context("serialize workflow steps for spawn_task")?;
    let task_id = engine.spawn_task(&queue, &task_name, &payload).await?;
    Ok(task_id)
}

/// Claim + execute the steps of an already-spawned workflow (Phase 2 of
/// dispatch), resuming from the last `COMPLETED` checkpoint if one exists
/// (cold-start re-exec). Returns the same `task_id`. Run detached by the daemon.
///
/// This is the retry-claim loop: claim a run -> resume from the checkpoint ->
/// [`run_steps`] -> `complete_run`/`fail_run`. On `fail_run`, absurd schedules a
/// retry run (max_attempts: 3); the loop re-claims + resumes until the task is
/// terminal (`completed`/`failed`) or a step `SUSPENDED`s (HITL - run stays
/// non-terminal, resume is the follow-on). `janush` is the resolved proxy-shell
/// binary (the daemon resolves it; the engine stays pure / unit-testable).
#[allow(clippy::too_many_arguments)] // mirrors WebhookPayload::build; all args are genuinely needed.
pub async fn run_workflow<E, F>(
    db: &AbsurdDb,
    engine: &E,
    factory: &F,
    recipe: &ValidatedRecipe,
    workflow_name: &str,
    repo_root: &Path,
    task_id: Uuid,
    janush: &Path,
) -> Result<Uuid>
where
    E: DurableEngine,
    F: BackendFactory,
{
    let queue = queue_name(&recipe.name, workflow_name);

    loop {
        // Kill any tmux sessions left from a previous attempt/crash for this task
        // (tmux survives the daemon dying) before re-running - prevents
        // double-execution of the resumed step. Iterates the workflow's hosts
        // (local + each remote) so remote stale sessions are cleaned too.
        kill_stale_sessions(factory, recipe, task_id)?;

        // Claim a run (absurd owns the lease). `None` means no claimable run: the
        // retry run isn't available yet (backoff), or the task is terminal.
        let Some(claimed) = engine.claim_task(&queue, WORKER_ID).await? else {
            if !task_non_terminal(engine, &queue, task_id).await? {
                // Terminal (completed/failed) - nothing left to claim.
                return Ok(task_id);
            }
            // Sleeping (retry pending) - wait for the backoff to elapse.
            tokio::time::sleep(RETRY_POLL_INTERVAL).await;
            continue;
        };
        // Single-daemon invariant: the claimed task is the one we're running.
        ensure!(
            claimed.task_id == task_id,
            "claimed task_id {} != expected task_id {}",
            claimed.task_id,
            task_id
        );
        let run_id = claimed.run_id;

        // Resume point: derived from the last absurd checkpoint. `Run` skips
        // COMPLETED steps (or re-runs an APPROVED HITL step); `AwaitHITL` re-awaits
        // a still-suspended step (e.g. after the HITL timeout); `FailHITL` ends a
        // REJECTED step.
        let (start_idx, action) = resume_point(engine, &queue, task_id, recipe).await?;
        let target_sha = git_head(repo_root);

        match action {
            ResumeAction::FailHITL => {
                engine
                    .fail_run(
                        &queue,
                        run_id,
                        &json!({"task_id": task_id, "reason": "hitl_rejected"}),
                    )
                    .await?;
                return Ok(task_id);
            }
            ResumeAction::AwaitHITL => {
                // Re-claimed with a SUSPENDED checkpoint + no verdict yet.
                let step_name = recipe.workflow.steps[start_idx].name.clone();
                if let Some(tid) = hitl_await_and_rerun(
                    db,
                    engine,
                    factory,
                    recipe,
                    workflow_name,
                    &queue,
                    task_id,
                    run_id,
                    &target_sha,
                    janush,
                    repo_root,
                    start_idx,
                    step_name,
                )
                .await?
                {
                    return Ok(tid);
                }
                // else: Suspended (run sleeping, re-claim on wake) or Failed -> loop.
            }
            ResumeAction::Run => {
                match run_steps(
                    db,
                    engine,
                    factory,
                    recipe,
                    workflow_name,
                    &queue,
                    task_id,
                    run_id,
                    &target_sha,
                    janush,
                    repo_root,
                    start_idx,
                )
                .await?
                {
                    StepOutcome::Done => {
                        engine
                            .complete_run(&queue, run_id, &json!({"task_id": task_id}))
                            .await?;
                        return Ok(task_id);
                    }
                    StepOutcome::Failed => {
                        engine
                            .fail_run(
                                &queue,
                                run_id,
                                &json!({"task_id": task_id, "reason": "step_failed"}),
                            )
                            .await?;
                        // Loop: absurd mints a retry run if attempts remain, else
                        // the task goes `failed` -> next claim is None -> exit.
                    }
                    StepOutcome::StaleHead { step_name, .. } => {
                        // Task 4.4: HEAD advanced mid-step. fail_run so absurd
                        // schedules a retry; the next claim picks up the new HEAD.
                        tracing::warn!(
                            task_id = %task_id,
                            step = %step_name,
                            "stale-head reschedule: step will re-run against new HEAD"
                        );
                        engine
                            .fail_run(
                                &queue,
                                run_id,
                                &json!({"task_id": task_id, "reason": "stale_head"}),
                            )
                            .await?;
                    }
                    StepOutcome::Suspended {
                        step_idx,
                        step_name,
                    } => {
                        // Run still claimed. Await the HITL verdict in place; on
                        // approval re-run the step (the GuardCheck ALLOWs it now).
                        if let Some(tid) = hitl_await_and_rerun(
                            db,
                            engine,
                            factory,
                            recipe,
                            workflow_name,
                            &queue,
                            task_id,
                            run_id,
                            &target_sha,
                            janush,
                            repo_root,
                            step_idx,
                            step_name,
                        )
                        .await?
                        {
                            return Ok(tid);
                        }
                        // else: Suspended (run sleeping) or Failed -> loop.
                    }
                }
            }
        }
    }
}

/// Run the workflow's steps from `start_idx`, writing a checkpoint per
/// `COMPLETED` step. Returns the outcome; the caller ([`run_workflow`]) does the
/// `complete_run`/`fail_run`. `start_idx > 0` skips steps already `COMPLETED` in
/// a prior attempt (resume).
#[allow(clippy::too_many_arguments)] // mirrors run_workflow; all args are genuinely needed.
async fn run_steps<E, F>(
    db: &AbsurdDb,
    engine: &E,
    factory: &F,
    recipe: &ValidatedRecipe,
    workflow_name: &str,
    queue: &str,
    task_id: Uuid,
    run_id: Uuid,
    target_sha: &str,
    janush: &Path,
    repo_root: &Path,
    start_idx: usize,
) -> Result<StepOutcome>
where
    E: DurableEngine,
    F: BackendFactory,
{
    for (idx, step) in recipe.workflow.steps.iter().enumerate().skip(start_idx) {
        // One tmux session per step, named tmux-janus-task-<task_id>-<idx> (the
        // idx suffix disambiguates steps; cold-start resume uses a fresh uuid
        // suffix - same prefix shape, ARCH §6.1).
        let session = SessionId::new_for_task(&format!("{}-{}", task_id.simple(), idx));
        let session_name = session.as_str().to_string();
        // Per-step backend selection (ADR-017): the step's `host` -> blueprint
        // `[remote]` fallback. `None` -> local `TmuxBackend`; `Some` -> remote
        // `TmuxBackend::with_ssh` (cached per-host by the factory).
        let host = step.host.as_deref().or(recipe.remote_host.as_deref());
        let backend = factory.get(host);

        db.upsert_step_start(
            &recipe.name,
            task_id,
            &step.name,
            workflow_name,
            target_sha,
            &session_name,
        )
        .await?;
        db.set_step_running(&recipe.name, task_id, &step.name)
            .await?;

        match step.command.as_deref() {
            Some(_) => {
                let cmd = step_command(step, recipe, task_id, workflow_name, janush, host);
                backend.create_session(&session, &cmd, Some(repo_root))?;

                let exit = poll_exit_with_lease(engine, &*backend, queue, run_id, &session).await;
                let stdout_tail = backend
                    .capture_pane(&session)
                    .ok()
                    .map(|s| truncate_16k(&filter::clean_pty_output(&s)));
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
                        return Ok(StepOutcome::Failed);
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
                        // NOTE: no absurd `set_checkpoint` here - a SUSPENDED checkpoint
                        // would make `await_event` return Resolved (its first check sees
                        // any checkpoint) instead of Suspended, so the run would never
                        // sleep + the engine would misread it as a verdict. The overlay
                        // `finalize_step` above (status=SUSPENDED) is enough for the
                        // dashboard; the verdict checkpoint is written by `emit_event`.
                        return Ok(StepOutcome::Suspended {
                            step_idx: idx,
                            step_name: step.name.clone(),
                        });
                    }
                    _ => {
                        if exit_code == 0 {
                            // Task 4.4 target_sha enforcement: if HEAD advanced
                            // mid-step (code was pushed while the step ran),
                            // the step result is stale - discard it, mark
                            // SUSPENDED with CONCURRENCY_RACE_ALERT, and let the
                            // retry loop re-run against the new HEAD.
                            if target_sha != NULL_SHA {
                                let current = git_head(repo_root);
                                if current != *target_sha {
                                    tracing::warn!(
                                        task_id = %task_id,
                                        step = %step.name,
                                        pinned = %target_sha,
                                        current = %current,
                                        "CONCURRENCY_RACE_ALERT: HEAD advanced mid-step"
                                    );
                                    db.finalize_step(
                                        &recipe.name,
                                        task_id,
                                        &step.name,
                                        "SUSPENDED",
                                        Some(exit_code),
                                        stdout_tail.as_deref(),
                                    )
                                    .await?;
                                    return Ok(StepOutcome::StaleHead {
                                        step_name: step.name.clone(),
                                    });
                                }
                            }
                            engine
                                .set_checkpoint(
                                    queue,
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
                            return Ok(StepOutcome::Failed);
                        }
                    }
                }
            }
            None => {
                // No command -> a manual/placeholder step: no tmux session,
                // immediate COMPLETED. (Real Agent steps always carry a command.)
                engine
                    .set_checkpoint(
                        queue,
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

    Ok(StepOutcome::Done)
}

/// Kill any tmux sessions for `task_id` left from a previous attempt/crash (tmux
/// survives the daemon dying). Called at the top of each claim iteration before
/// re-running, so a resumed step doesn't double-execute against a stale pane.
/// Iterates the workflow's distinct hosts (local `None` + each remote) so remote
/// stale sessions are cleaned on retry/resume too (ADR-017).
fn kill_stale_sessions<F: BackendFactory>(
    factory: &F,
    recipe: &ValidatedRecipe,
    task_id: Uuid,
) -> Result<()> {
    let prefix = format!("{}{}-", SESSION_PREFIX, task_id.simple());
    // Distinct hosts the workflow touches: local always + each step's host
    // (-> recipe.remote_host fallback).
    let mut hosts: Vec<Option<&str>> = vec![None];
    for step in &recipe.workflow.steps {
        let h = step.host.as_deref().or(recipe.remote_host.as_deref());
        if !hosts.contains(&h) {
            hosts.push(h);
        }
    }
    for host in hosts {
        let backend = factory.get(host);
        for name in backend.list_sessions()? {
            if name.starts_with(&prefix) {
                let _ = backend.kill_session(&SessionId::from_name(name));
            }
        }
    }
    Ok(())
}

/// Whether `task_id` is still non-terminal (`pending`/`sleeping`/`running`) in
/// absurd - i.e. a retry is pending. Used by the loop to distinguish "retry
/// pending, keep polling" from "terminal, exit" when `claim_task` returns `None`.
async fn task_non_terminal<E: DurableEngine>(
    engine: &E,
    queue: &str,
    task_id: Uuid,
) -> Result<bool> {
    Ok(engine
        .non_terminal_tasks(queue)
        .await?
        .into_iter()
        .any(|t| t.task_id == task_id))
}

/// The resume point + action, derived from the last absurd checkpoint:
/// - no checkpoint -> `(0, Run)` (fresh dispatch)
/// - `status == "COMPLETED"` -> `(idx+1, Run)` (skip the done step)
/// - `hitl_verdict == "APPROVED"|"OVERRIDDEN"` -> `(idx, Run)` (re-run the approved step)
/// - `hitl_verdict == "REJECTED"` -> `(idx, FailHITL)`
/// - else (SUSPENDED / no verdict yet) -> `(idx, AwaitHITL)`
async fn resume_point<E: DurableEngine>(
    engine: &E,
    queue: &str,
    task_id: Uuid,
    recipe: &ValidatedRecipe,
) -> Result<(usize, ResumeAction)> {
    let Some((step_name, state)) = engine.get_last_checkpoint(queue, task_id).await? else {
        return Ok((0, ResumeAction::Run));
    };
    let idx = recipe
        .workflow
        .steps
        .iter()
        .position(|s| s.name == step_name)
        .unwrap_or(0);
    let status = state.get("status").and_then(|v| v.as_str());
    let verdict = state.get("hitl_verdict").and_then(|v| v.as_str());
    Ok(if status == Some("COMPLETED") {
        (idx + 1, ResumeAction::Run)
    } else if matches!(verdict, Some("APPROVED") | Some("OVERRIDDEN")) {
        (idx, ResumeAction::Run)
    } else if verdict == Some("REJECTED") {
        (idx, ResumeAction::FailHITL)
    } else {
        (idx, ResumeAction::AwaitHITL)
    })
}

/// Result of awaiting a HITL verdict for a suspended step.
enum HitlAwaitResult {
    /// `await_event` returned `Suspended` - the run is now `sleeping`; re-claim
    /// on wake (the loop continues).
    Suspended,
    /// The verdict is `APPROVED`/`OVERRIDDEN` - re-run the step.
    Approved,
    /// The verdict is `REJECTED` or the await timed out (null payload) - fail.
    RejectedOrTimeout,
}

/// `await_event` on the HITL verdict event `hitl.verdict:<task_id>`, mapped to a
/// [`HitlAwaitResult`]. Timeout = `protocol::hitl_timeout_secs()` (30 min default).
async fn hitl_await<E: DurableEngine>(
    engine: &E,
    queue: &str,
    task_id: Uuid,
    run_id: Uuid,
    step_name: &str,
) -> Result<HitlAwaitResult> {
    let event_name = format!("hitl.verdict:{task_id}");
    match engine
        .await_event(
            queue,
            task_id,
            run_id,
            step_name,
            &event_name,
            Some(protocol::hitl_timeout_secs()),
        )
        .await?
    {
        AwaitOutcome::Suspended => Ok(HitlAwaitResult::Suspended),
        AwaitOutcome::Resolved(payload) => {
            match payload.get("hitl_verdict").and_then(|v| v.as_str()) {
                Some("APPROVED") | Some("OVERRIDDEN") => Ok(HitlAwaitResult::Approved),
                _ => Ok(HitlAwaitResult::RejectedOrTimeout),
            }
        }
    }
}

/// Await the HITL verdict for `step_idx`/`step_name` + act on it. Returns
/// `Some(task_id)` when the task is terminal (caller returns); `None` when the
/// loop should continue (`Suspended` -> run sleeping; `Failed` -> retry).
#[allow(clippy::too_many_arguments)] // mirrors run_steps; all args are genuinely needed.
async fn hitl_await_and_rerun<E, F>(
    db: &AbsurdDb,
    engine: &E,
    factory: &F,
    recipe: &ValidatedRecipe,
    workflow_name: &str,
    queue: &str,
    task_id: Uuid,
    run_id: Uuid,
    target_sha: &str,
    janush: &Path,
    repo_root: &Path,
    step_idx: usize,
    step_name: String,
) -> Result<Option<Uuid>>
where
    E: DurableEngine,
    F: BackendFactory,
{
    match hitl_await(engine, queue, task_id, run_id, &step_name).await? {
        HitlAwaitResult::Suspended => Ok(None), // run sleeping; loop re-claims on wake
        HitlAwaitResult::Approved => {
            // Re-run the step (the GuardCheck handler ALLOWs it now that
            // hitl_verdict=APPROVED is recorded on the overlay).
            match run_steps(
                db,
                engine,
                factory,
                recipe,
                workflow_name,
                queue,
                task_id,
                run_id,
                target_sha,
                janush,
                repo_root,
                step_idx,
            )
            .await?
            {
                StepOutcome::Done => {
                    engine
                        .complete_run(queue, run_id, &json!({"task_id": task_id}))
                        .await?;
                    Ok(Some(task_id))
                }
                StepOutcome::Failed | StepOutcome::StaleHead { .. } => {
                    engine
                        .fail_run(
                            queue,
                            run_id,
                            &json!({"task_id": task_id, "reason": "step_failed"}),
                        )
                        .await?;
                    Ok(None)
                }
                StepOutcome::Suspended { .. } => Ok(None), // re-await on the next loop iteration
            }
        }
        HitlAwaitResult::RejectedOrTimeout => {
            engine
                .fail_run(
                    queue,
                    run_id,
                    &json!({"task_id": task_id, "reason": "hitl_rejected"}),
                )
                .await?;
            Ok(Some(task_id))
        }
    }
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
    B: DurableBackend + ?Sized,
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

/// Build the tmux workload string for a step: `env <SOCK_ENV>=... JANUS_AGENT=...
/// ... janush -c '<command>'`. `janush` reads the `JANUS_*` env vars to populate
/// its `GuardCheck` (verified in `bin/janush.rs`), so the daemon's `suspend_step`
/// path fires with the correct task/step context on a HITL BLOCK.
///
/// `host` selects the socket env: `None` (local) -> `HERDR_PLUGIN_STATE_DIR` (set
/// explicitly so janush resolves the daemon socket even when the long-lived
/// `metamach-tmux` server's frozen env points at a stale state dir); `Some(host)`
/// (remote) -> `JANUS_SOCK_PATH=/tmp/mm-<host>.sock` (the SSH `-R` reverse tunnel
/// back to the local daemon - ADR-017).
fn step_command(
    step: &crate::recipe::WorkflowStep,
    recipe: &ValidatedRecipe,
    task_id: Uuid,
    workflow_name: &str,
    janush: &Path,
    host: Option<&str>,
) -> String {
    let command = step.command.as_deref().unwrap_or("true");
    // Local: HERDR_PLUGIN_STATE_DIR (janush -> state_dir/janus.sock). Remote:
    // JANUS_SOCK_PATH=/tmp/mm-<host>.sock (the reverse tunnel).
    let sock_env = match host {
        None => format!(
            "HERDR_PLUGIN_STATE_DIR={}",
            shell_quote(&paths::state_dir().to_string_lossy())
        ),
        Some(h) => format!("JANUS_SOCK_PATH=/tmp/mm-{h}.sock"),
    };
    format!(
        "env {sock_env} JANUS_AGENT={agent} \
         JANUS_BLUEPRINT={blueprint} JANUS_TASK_ID={task_id} JANUS_STEP={step_name} \
         JANUS_WORKFLOW={workflow} {janush} -c {command}",
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
    use std::collections::HashSet;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// In-memory `DurableBackend` for engine unit tests: completes each session
    /// instantly with a configurable per-step exit code (popped in
    /// `create_session` order via `poll_exit`). Tracks live sessions so
    /// [`kill_stale_sessions`] + `has_session`/`list_sessions` are exercisable.
    struct FakeBackend {
        exit_codes: Mutex<VecDeque<i32>>,
        created: Mutex<Vec<String>>,
        alive: Mutex<HashSet<String>>,
        pane: String,
    }

    impl FakeBackend {
        fn new(codes: &[i32]) -> Self {
            Self {
                exit_codes: Mutex::new(codes.iter().copied().collect()),
                created: Mutex::new(Vec::new()),
                alive: Mutex::new(HashSet::new()),
                pane: "pane-output\n".to_string(),
            }
        }
        fn created_count(&self) -> usize {
            self.created.lock().unwrap().len()
        }
        /// Inject a live session (for `kill_stale_sessions` tests).
        fn seed_alive(&self, name: &str) {
            self.alive.lock().unwrap().insert(name.to_string());
        }
        fn is_alive(&self, name: &str) -> bool {
            self.alive.lock().unwrap().contains(name)
        }
    }

    impl DurableBackend for FakeBackend {
        fn create_session(
            &self,
            id: &SessionId,
            _command: &str,
            _cwd: Option<&Path>,
        ) -> Result<()> {
            let name = id.as_str().to_string();
            self.created.lock().unwrap().push(name.clone());
            self.alive.lock().unwrap().insert(name);
            Ok(())
        }
        fn attach(&self, _id: &SessionId) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, id: &SessionId) -> Result<()> {
            self.alive.lock().unwrap().remove(id.as_str());
            Ok(())
        }
        fn has_session(&self, id: &SessionId) -> Result<bool> {
            Ok(self.alive.lock().unwrap().contains(id.as_str()))
        }
        fn list_sessions(&self) -> Result<Vec<String>> {
            Ok(self.alive.lock().unwrap().iter().cloned().collect())
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
            remote_user: None,
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

    /// In-memory [`BackendFactory`] for engine unit tests. Always returns the
    /// same shared [`FakeBackend`] regardless of host (the tests don't use
    /// remote steps). Wraps the [`FakeBackend`] so the engine's per-step
    /// `factory.get(host)` calls resolve to it.
    struct FakeFactory(Arc<FakeBackend>);

    impl FakeFactory {
        fn new(codes: &[i32]) -> Self {
            Self(Arc::new(FakeBackend::new(codes)))
        }
    }

    impl BackendFactory for FakeFactory {
        fn get(&self, _host: Option<&str>) -> Arc<dyn DurableBackend> {
            self.0.clone()
        }
    }

    impl BackendFactory for FakeBackend {
        fn get(&self, _host: Option<&str>) -> Arc<dyn DurableBackend> {
            Arc::new(FakeBackend::new(&[]))
        }
    }

    #[tokio::test]
    async fn run_workflow_happy_path_completes_all_steps() {
        let tmp = tempfile::tempdir().unwrap();
        let db = degraded_db(tmp.path());
        let engine = FakeEngine::new();
        let factory = FakeFactory::new(&[0, 0]);
        let recipe = two_step_recipe();

        let task_id = spawn_workflow(&engine, &recipe, "test-flow")
            .await
            .expect("spawn");
        run_workflow(
            &db,
            &engine,
            &factory,
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
        assert_eq!(factory.0.created_count(), 2);
        let cp = engine.last_checkpoint(task_id).expect("checkpoint");
        assert_eq!(cp.0, "build"); // last checkpoint is the final step
        assert!(cp.1.to_string().contains("COMPLETED"));
    }

    #[tokio::test]
    async fn run_workflow_retries_then_succeeds() {
        // step 2 fails on attempt 1 (exit 1), succeeds on attempt 2 (exit 0).
        // Codes pop per create_session: a1 scout(0)+build(1); a2 resumes at build
        // (scout's checkpoint) -> build(0).
        let tmp = tempfile::tempdir().unwrap();
        let db = degraded_db(tmp.path());
        let engine = FakeEngine::new(); // max_attempts: 3
        let factory = FakeFactory::new(&[0, 1, 0]);
        let recipe = two_step_recipe();

        let task_id = spawn_workflow(&engine, &recipe, "test-flow")
            .await
            .expect("spawn");
        run_workflow(
            &db,
            &engine,
            &factory,
            &recipe,
            "test-flow",
            tmp.path(),
            task_id,
            Path::new("/bin/janush"),
        )
        .await
        .expect("workflow ok");

        // Retried once then completed.
        assert_eq!(engine.task_state(task_id).as_deref(), Some("completed"));
        // a1: scout+build sessions; a2: build session (scout skipped via checkpoint).
        assert_eq!(factory.0.created_count(), 3);
        let cp = engine.last_checkpoint(task_id).expect("checkpoint");
        assert_eq!(cp.0, "build");
    }

    #[tokio::test]
    async fn run_workflow_retries_exhausted() {
        // step 2 always fails; max_attempts: 2 -> failed after 2 attempts.
        let tmp = tempfile::tempdir().unwrap();
        let db = degraded_db(tmp.path());
        let engine = FakeEngine::with_max_attempts(2);
        let factory = FakeFactory::new(&[0, 1, 1]);
        let recipe = two_step_recipe();

        let task_id = spawn_workflow(&engine, &recipe, "test-flow")
            .await
            .expect("spawn");
        run_workflow(
            &db,
            &engine,
            &factory,
            &recipe,
            "test-flow",
            tmp.path(),
            task_id,
            Path::new("/bin/janush"),
        )
        .await
        .expect("workflow ok");

        assert_eq!(engine.task_state(task_id).as_deref(), Some("failed"));
        // a1: scout+build; a2: build (resumed). 3 sessions.
        assert_eq!(factory.0.created_count(), 3);
        // step 2 never completed -> last checkpoint stays at scout.
        let cp = engine.last_checkpoint(task_id).expect("checkpoint");
        assert_eq!(cp.0, "scout");
    }

    #[tokio::test]
    async fn run_workflow_resumes_from_checkpoint() {
        // Cold-start resume: scout already COMPLETED (seeded checkpoint) ->
        // run_workflow skips it, runs only build.
        let tmp = tempfile::tempdir().unwrap();
        let db = degraded_db(tmp.path());
        let engine = FakeEngine::new();
        let factory = FakeFactory::new(&[0]); // build succeeds
        let recipe = two_step_recipe();

        let task_id = spawn_workflow(&engine, &recipe, "test-flow")
            .await
            .expect("spawn");
        engine.seed_checkpoint(task_id, "scout"); // scout already done
        run_workflow(
            &db,
            &engine,
            &factory,
            &recipe,
            "test-flow",
            tmp.path(),
            task_id,
            Path::new("/bin/janush"),
        )
        .await
        .expect("workflow ok");

        assert_eq!(engine.task_state(task_id).as_deref(), Some("completed"));
        // Only build's session was created (scout skipped).
        assert_eq!(factory.0.created_count(), 1);
        let cp = engine.last_checkpoint(task_id).expect("checkpoint");
        assert_eq!(cp.0, "build");
    }

    #[test]
    fn kill_stale_sessions_kills_only_the_task_sessions() {
        let factory = FakeFactory::new(&[]);
        let backend = &factory.0;
        let tid = Uuid::new_v4();
        let stale = format!("{}{}-0", SESSION_PREFIX, tid.simple());
        let other_task = format!("{}{}-0", SESSION_PREFIX, Uuid::new_v4().simple());
        let unrelated = "some-other-session".to_string();
        for s in [&stale, &other_task, &unrelated] {
            backend.seed_alive(s);
        }
        let recipe = two_step_recipe();
        kill_stale_sessions(&factory, &recipe, tid).expect("kill stale");
        assert!(
            !backend.is_alive(&stale),
            "stale session for tid should be killed"
        );
        assert!(
            backend.is_alive(&other_task),
            "other task's session should remain"
        );
        assert!(
            backend.is_alive(&unrelated),
            "unrelated session should remain"
        );
    }

    #[tokio::test]
    async fn resume_point_branches_on_checkpoint_state() {
        // two_step_recipe: scout (idx 0), build (idx 1).
        let engine = FakeEngine::new();
        let recipe = two_step_recipe();
        let task_id = Uuid::new_v4();
        let queue = "testbp_test_flow";

        // No checkpoint -> fresh dispatch (0, Run).
        let (idx, act) = resume_point(&engine, queue, task_id, &recipe)
            .await
            .unwrap();
        assert_eq!((idx, matches!(act, ResumeAction::Run)), (0, true));

        // COMPLETED scout -> skip to build (1, Run).
        engine.seed_checkpoint_state(task_id, "scout", json!({"status": "COMPLETED"}));
        let (idx, act) = resume_point(&engine, queue, task_id, &recipe)
            .await
            .unwrap();
        assert_eq!((idx, matches!(act, ResumeAction::Run)), (1, true));

        // APPROVED build -> re-run build (1, Run).
        engine.seed_checkpoint_state(task_id, "build", json!({"hitl_verdict": "APPROVED"}));
        let (idx, act) = resume_point(&engine, queue, task_id, &recipe)
            .await
            .unwrap();
        assert_eq!((idx, matches!(act, ResumeAction::Run)), (1, true));

        // REJECTED build -> fail (1, FailHITL).
        engine.seed_checkpoint_state(task_id, "build", json!({"hitl_verdict": "REJECTED"}));
        let (idx, act) = resume_point(&engine, queue, task_id, &recipe)
            .await
            .unwrap();
        assert_eq!((idx, matches!(act, ResumeAction::FailHITL)), (1, true));

        // SUSPENDED build (no verdict yet) -> await (1, AwaitHITL).
        engine.seed_checkpoint_state(task_id, "build", json!({"status": "SUSPENDED"}));
        let (idx, act) = resume_point(&engine, queue, task_id, &recipe)
            .await
            .unwrap();
        assert_eq!((idx, matches!(act, ResumeAction::AwaitHITL)), (1, true));
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
            remote_user: None,
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
        assert_eq!(queue_name(&recipe.name, "test-flow"), "gate_test_flow");
        assert_eq!(queue_name(&recipe.name, "a.b c"), "gate_a_b_c");
        assert_eq!(queue_name(&recipe.name, "clean"), "gate_clean");
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
            remote_user: None,
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
        let cmd = step_command(&step, &recipe, task_id, "fw", &janush, None);
        assert!(cmd.starts_with("env HERDR_PLUGIN_STATE_DIR="));
        assert!(cmd.contains("JANUS_AGENT='deployer'"));
        assert!(cmd.contains("JANUS_BLUEPRINT='gatemetric'"));
        assert!(cmd.contains("JANUS_TASK_ID=00000000-0000-0000-0000-000000000000"));
        assert!(cmd.contains("JANUS_STEP='flash'"));
        assert!(cmd.contains("JANUS_WORKFLOW='fw'"));
        assert!(cmd.contains("'/bin/janush' -c 'make flash'"));
    }
}
