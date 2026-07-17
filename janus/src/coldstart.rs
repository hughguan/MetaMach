//! Cold-start self-healing (Feature-Spec §2.3, ARCH §6 invariant 4;
//! Project-Plan Task 4.1).
//!
//! On startup the Daemon scans Absurd Postgres for non-terminal tasks
//! (`STARTING`/`RUNNING`/`SUSPENDED`). For each, it reads the last `COMPLETED`
//! Step checkpoint and assigns a fresh Tether session UUID, picking up at the
//! breakpoint. `tmux-resurrect` is NEVER used - Postgres is the sole source of
//! truth (ARCH §6.4).
//!
//! M4 scope: the scan + checkpoint read + UUID assignment + resume-plan logging
//! are implemented here. The actual step re-execution (driving `herdr-tether`)
//! is deferred to Task 2.4 (herdr-tether integration); until then a present
//! non-terminal task is logged and left in its non-terminal state for the
//! workflow engine to resume.

use anyhow::Result;
use tracing::{info, warn};
use uuid::Uuid;

use crate::absurd::AbsurdDb;

/// Reconcile non-terminal tasks after a cold start. Returns the number of tasks
/// that have a resumable (last-`COMPLETED`) checkpoint.
pub async fn reconcile(db: &AbsurdDb) -> Result<usize> {
    let tasks = db.cold_start_running_tasks().await?;
    if tasks.is_empty() {
        info!("cold-start: no non-terminal tasks to resume");
        return Ok(0);
    }
    let mut resumable = 0usize;
    for t in &tasks {
        let session = format!(
            "tether-janus-task-{}-{}",
            t.task_id,
            Uuid::new_v4().simple()
        );
        match &t.last_completed_step {
            Some(step) => {
                info!(
                    task_id = t.task_id,
                    blueprint = %t.blueprint,
                    workflow = %t.workflow_name,
                    status = %t.status,
                    last_completed = %step,
                    session = %session,
                    "cold-start: resumable task - pick up after `{step}`"
                );
                resumable += 1;
            }
            None => warn!(
                task_id = t.task_id,
                blueprint = %t.blueprint,
                status = %t.status,
                "cold-start: non-terminal task has no COMPLETED checkpoint - leaving for HITL"
            ),
        }
    }
    info!(
        "cold-start: {resumable}/{} task(s) resumable (re-exec deferred to Task 2.4)",
        tasks.len()
    );
    Ok(resumable)
}

#[cfg(test)]
mod tests {
    #[test]
    fn session_name_shape() {
        // Cold-start assigns a fresh session UUID per resumable task.
        let s = format!("tether-janus-task-1042-{}", uuid::Uuid::new_v4().simple());
        assert!(s.starts_with("tether-janus-task-1042-"));
        // simple() UUID has no dashes, so only the 4 name separators remain.
        assert_eq!(s.matches('-').count(), 4);
    }
}
