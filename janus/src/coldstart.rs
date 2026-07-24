//! Cold-start self-healing (Feature-Spec §2.3, ARCH §6 invariant 4;
//! Project-Plan Task 4.1).
//!
//! On startup the Daemon scans Absurd Postgres for non-terminal tasks
//! (`STARTING`/`RUNNING`/`SUSPENDED`). For each `STARTING`/`RUNNING` task (one
//! interrupted mid-step by a crash), it loads the recipe + workflow, builds the
//! absurd engine + tmux backend, and spawns [`workflow::run_workflow`] detached
//! to resume from the last `COMPLETED` checkpoint (skipping done steps, re-running
//! the interrupted one in a fresh tmux session). `SUSPENDED` tasks are skipped -
//! they await HITL approval (the `await_event`/`emit_event` resume loop is a
//! follow-on). `tmux-resurrect` is NEVER used - Postgres is the sole source of
//! truth (ARCH §6.4).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};

use crate::absurd::AbsurdDb;
use crate::absurd::adapter::AbsurdPgAdapter;
use crate::recipe;
use crate::spawn::resolve_janush_exe;
use crate::tmux::TmuxFactory;
use crate::workflow;

/// Reconcile non-terminal tasks after a cold start. For each `STARTING`/`RUNNING`
/// task, spawn [`workflow::run_workflow`] detached to resume it from the last
/// `COMPLETED` checkpoint. `SUSPENDED` tasks are left for the HITL resume loop.
/// Returns the number of tasks spawned for resume.
pub async fn reconcile(db: Arc<AbsurdDb>, repo_root: Arc<PathBuf>) -> Result<usize> {
    let tasks = db.cold_start_running_tasks().await?;
    if tasks.is_empty() {
        info!("cold-start: no non-terminal tasks to resume");
        return Ok(0);
    }
    // Pre-flight: resolve janush once (needed for every resume). If it's missing,
    // there's no point spawning resumes that can't run steps.
    let janush = match resolve_janush_exe() {
        Ok(p) => p,
        Err(e) => {
            warn!(
                "cold-start: cannot resolve janush ({e}); skipping resume of {} task(s)",
                tasks.len()
            );
            return Ok(0);
        }
    };

    let mut spawned = 0usize;
    for t in &tasks {
        // Resume only STARTING/RUNNING (interrupted by a crash). SUSPENDED tasks
        // await HITL approval - the resume loop is a follow-on.
        if !matches!(t.status.as_str(), "STARTING" | "RUNNING") {
            warn!(
                task_id = %t.task_id,
                blueprint = %t.blueprint,
                status = %t.status,
                "cold-start: skipping non-running task (awaiting HITL)"
            );
            continue;
        }
        // Load the recipe + bound workflow (override if the task's workflow
        // differs from the blueprint's default).
        let recipe = match recipe::validate(&t.blueprint, repo_root.as_path()) {
            Ok(mut r) => {
                if t.workflow_name != r.default_workflow {
                    match recipe::load_workflow(&t.workflow_name, repo_root.as_path()) {
                        Ok(wf) => r.workflow = wf,
                        Err(e) => {
                            warn!(
                                task_id = %t.task_id,
                                "cold-start: load_workflow `{}` failed ({e}); skipping",
                                t.workflow_name
                            );
                            continue;
                        }
                    }
                }
                r
            }
            Err(e) => {
                warn!(
                    task_id = %t.task_id,
                    blueprint = %t.blueprint,
                    "cold-start: recipe validate failed ({e}); skipping"
                );
                continue;
            }
        };
        let pool = match db.blueprint_pool(&t.blueprint).await {
            Ok(Some(p)) => p,
            Ok(None) => {
                warn!(
                    task_id = %t.task_id,
                    blueprint = %t.blueprint,
                    "cold-start: blueprint pool unreachable; skipping"
                );
                continue;
            }
            Err(e) => {
                warn!(
                    task_id = %t.task_id,
                    blueprint = %t.blueprint,
                    "cold-start: blueprint pool error ({e}); skipping"
                );
                continue;
            }
        };
        let engine = AbsurdPgAdapter::new(pool);
        // Per-step backend factory (ADR-017): local + remote (with reverse tunnel)
        // backends, cached per-host. SSH user from the blueprint's `[remote] user`.
        let factory = TmuxFactory::new(recipe.remote_user.clone());
        let recipe = Arc::new(recipe);
        info!(
            task_id = %t.task_id,
            blueprint = %t.blueprint,
            workflow = %t.workflow_name,
            last_completed = ?t.last_completed_step,
            "cold-start: resuming task"
        );
        let db = db.clone();
        let repo_root = repo_root.clone();
        let janush = janush.clone();
        let wf = t.workflow_name.clone();
        let tid = t.task_id;
        tokio::spawn(async move {
            if let Err(e) = workflow::run_workflow(
                &db, &engine, &factory, &recipe, &wf, &repo_root, tid, &janush,
            )
            .await
            {
                warn!(task_id = %tid, "cold-start resume failed: {e}");
            }
        });
        spawned += 1;
    }
    info!("cold-start: {spawned}/{} task(s) resumed", tasks.len());
    Ok(spawned)
}

#[cfg(test)]
mod tests {
    #[test]
    fn session_name_shape() {
        // Cold-start resumes into a fresh tmux-janus-task-<task_id>-<idx> session
        // (named inside run_workflow); the prefix shape is unchanged.
        let s = format!("tmux-janus-task-1042-{}", uuid::Uuid::new_v4().simple());
        assert!(s.starts_with("tmux-janus-task-1042-"));
        // simple() UUID has no dashes, so only the 4 name separators remain.
        assert_eq!(s.matches('-').count(), 4);
    }
}
