//! Absurd durable-engine adapter (M4 Task 4.1 Phase 0a; `docs/Absurd-Integration.md` §7.3).
//!
//! [`DurableEngine`] is MetaMach's trait over absurd's pull-mode stored-proc API.
//! [`AbsurdPgAdapter`] is the production impl (sqlx -> `SELECT absurd.<fn>(...)`);
//! [`FakeEngine`] is the in-memory test impl. The trait uses manual boxed
//! futures (`Pin<Box<dyn Future + Send>>`) rather than the `async-trait` crate -
//! the codebase has no `async-trait` dependency, and this keeps the trait
//! object-safe + Send for `tokio::spawn` (the Phase 0b workflow engine is
//! generic over `E: DurableEngine`).
//!
//! Method -> stored-proc mapping is documented on each trait method. Signatures
//! are pinned to `janus/sql/absurd.sql` v0.4.0 (upstream `9b77b35`).

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use serde_json::Value;
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

/// Boxed, Send future returned by every [`DurableEngine`] method.
type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A task a worker has pull-claimed (`absurd.claim_task`).
#[derive(Debug, Clone)]
pub struct ClaimedTask {
    pub run_id: Uuid,
    pub task_id: Uuid,
    pub task_name: String,
    pub params: Value,
}

/// A non-terminal task for cold-start reconciliation (`pending`/`running`/`sleeping`).
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub task_id: Uuid,
    pub task_name: String,
    pub state: String,
}

/// MetaMach's contract over absurd's pull-mode durable-execution API.
///
/// Each method maps 1:1 to an `absurd.<fn>(...)` stored procedure. absurd owns
/// task/run/checkpoint state; MetaMach's `metamach_step_meta` (002) is a thin
/// overlay for fields absurd has no concept of (`target_sha`, `stdout_tail`,
/// `hitl_verdict`, `session_name`).
pub trait DurableEngine: Send + Sync {
    /// `absurd.create_queue(queue)` (unpartitioned). Idempotent.
    fn create_queue<'a>(&'a self, queue: &'a str) -> BoxFut<'a, Result<()>>;
    /// `absurd.spawn_task(queue, task_name, params, '{}')` - absurd mints +
    /// returns the task_id (UUIDv7).
    fn spawn_task<'a>(
        &'a self,
        queue: &'a str,
        task_name: &'a str,
        params: &'a Value,
    ) -> BoxFut<'a, Result<Uuid>>;
    /// `absurd.claim_task(queue, worker_id, 30, 1)` - pull-lease one task.
    fn claim_task<'a>(
        &'a self,
        queue: &'a str,
        worker_id: &'a str,
    ) -> BoxFut<'a, Result<Option<ClaimedTask>>>;
    /// `absurd.complete_run(queue, run_id, state)` - mark the run done.
    fn complete_run<'a>(
        &'a self,
        queue: &'a str,
        run_id: Uuid,
        state: &'a Value,
    ) -> BoxFut<'a, Result<()>>;
    /// `absurd.fail_run(queue, run_id, reason, NULL)` - fail without auto-retry
    /// (MetaMach controls reschedule via cold-start / Task 4.4).
    fn fail_run<'a>(
        &'a self,
        queue: &'a str,
        run_id: Uuid,
        reason: &'a Value,
    ) -> BoxFut<'a, Result<()>>;
    /// `absurd.set_task_checkpoint_state(queue, task_id, step, state, owner_run, NULL)`.
    fn set_checkpoint<'a>(
        &'a self,
        queue: &'a str,
        task_id: Uuid,
        step: &'a str,
        state: &'a Value,
        owner_run: Uuid,
    ) -> BoxFut<'a, Result<()>>;
    /// Most-recent checkpoint for a task (cold-start resume point). Returns
    /// `(checkpoint_name, state)`. Queries absurd's per-queue `c_<queue>` table.
    fn get_last_checkpoint<'a>(
        &'a self,
        queue: &'a str,
        task_id: Uuid,
    ) -> BoxFut<'a, Result<Option<(String, Value)>>>;
    /// Non-terminal tasks (`pending`/`running`/`sleeping`) for cold-start.
    /// Queries absurd's per-queue `t_<queue>` table.
    fn non_terminal_tasks<'a>(&'a self, queue: &'a str) -> BoxFut<'a, Result<Vec<TaskInfo>>>;
    /// `absurd.emit_event(queue, event_name, payload)` - HITL resume signal.
    fn emit_event<'a>(
        &'a self,
        queue: &'a str,
        event_name: &'a str,
        payload: &'a Value,
    ) -> BoxFut<'a, Result<()>>;
    /// `absurd.await_event(queue, task_id, run_id, step, event_name, timeout)` -
    /// blocks (PG-side poll) until the event fires or timeout; HITL SUSPEND.
    /// Returns the event payload. Defined in Phase 0a; driven by the 0b engine.
    fn await_event<'a>(
        &'a self,
        queue: &'a str,
        task_id: Uuid,
        run_id: Uuid,
        step: &'a str,
        event_name: &'a str,
        timeout_secs: Option<i64>,
    ) -> BoxFut<'a, Result<Value>>;
}

// --- Production impl: sqlx -> absurd stored procs -------------------------

/// Production [`DurableEngine`] over a per-blueprint PG pool.
pub struct AbsurdPgAdapter {
    pool: PgPool,
}

impl AbsurdPgAdapter {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl DurableEngine for AbsurdPgAdapter {
    fn create_queue<'a>(&'a self, queue: &'a str) -> BoxFut<'a, Result<()>> {
        Box::pin(async move {
            sqlx::query("SELECT absurd.create_queue($1)")
                .bind(queue)
                .execute(&self.pool)
                .await?;
            Ok(())
        })
    }

    fn spawn_task<'a>(
        &'a self,
        queue: &'a str,
        task_name: &'a str,
        params: &'a Value,
    ) -> BoxFut<'a, Result<Uuid>> {
        Box::pin(async move {
            let task_id: Uuid = sqlx::query_scalar(
                "SELECT task_id FROM absurd.spawn_task($1, $2, $3, '{}'::jsonb)",
            )
            .bind(queue)
            .bind(task_name)
            .bind(params)
            .fetch_one(&self.pool)
            .await?;
            Ok(task_id)
        })
    }

    fn claim_task<'a>(
        &'a self,
        queue: &'a str,
        worker_id: &'a str,
    ) -> BoxFut<'a, Result<Option<ClaimedTask>>> {
        Box::pin(async move {
            let row: Option<(Uuid, Uuid, String, Value)> = sqlx::query_as(
                "SELECT run_id, task_id, task_name, params FROM absurd.claim_task($1, $2, 30, 1)",
            )
            .bind(queue)
            .bind(worker_id)
            .fetch_optional(&self.pool)
            .await?;
            Ok(row.map(|(run_id, task_id, task_name, params)| ClaimedTask {
                run_id,
                task_id,
                task_name,
                params,
            }))
        })
    }

    fn complete_run<'a>(
        &'a self,
        queue: &'a str,
        run_id: Uuid,
        state: &'a Value,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move {
            sqlx::query("SELECT absurd.complete_run($1, $2, $3)")
                .bind(queue)
                .bind(run_id)
                .bind(state)
                .execute(&self.pool)
                .await?;
            Ok(())
        })
    }

    fn fail_run<'a>(
        &'a self,
        queue: &'a str,
        run_id: Uuid,
        reason: &'a Value,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move {
            sqlx::query("SELECT absurd.fail_run($1, $2, $3, NULL)")
                .bind(queue)
                .bind(run_id)
                .bind(reason)
                .execute(&self.pool)
                .await?;
            Ok(())
        })
    }

    fn set_checkpoint<'a>(
        &'a self,
        queue: &'a str,
        task_id: Uuid,
        step: &'a str,
        state: &'a Value,
        owner_run: Uuid,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move {
            sqlx::query("SELECT absurd.set_task_checkpoint_state($1, $2, $3, $4, $5, NULL)")
                .bind(queue)
                .bind(task_id)
                .bind(step)
                .bind(state)
                .bind(owner_run)
                .execute(&self.pool)
                .await?;
            Ok(())
        })
    }

    fn get_last_checkpoint<'a>(
        &'a self,
        queue: &'a str,
        task_id: Uuid,
    ) -> BoxFut<'a, Result<Option<(String, Value)>>> {
        Box::pin(async move {
            let suffix = queue_ident_suffix(queue, "c_");
            let sql = format!(
                "SELECT checkpoint_name, state FROM absurd.{suffix} \
                 WHERE task_id = $1 ORDER BY updated_at DESC LIMIT 1"
            );
            let row: Option<(String, Value)> = sqlx::query_as(&sql)
                .bind(task_id)
                .fetch_optional(&self.pool)
                .await?;
            Ok(row)
        })
    }

    fn non_terminal_tasks<'a>(&'a self, queue: &'a str) -> BoxFut<'a, Result<Vec<TaskInfo>>> {
        Box::pin(async move {
            let suffix = queue_ident_suffix(queue, "t_");
            let sql = format!(
                "SELECT task_id, task_name, state FROM absurd.{suffix} \
                 WHERE state IN ('pending', 'sleeping', 'running')"
            );
            let rows: Vec<(Uuid, String, String)> =
                sqlx::query_as(&sql).fetch_all(&self.pool).await?;
            Ok(rows
                .into_iter()
                .map(|(task_id, task_name, state)| TaskInfo {
                    task_id,
                    task_name,
                    state,
                })
                .collect())
        })
    }

    fn emit_event<'a>(
        &'a self,
        queue: &'a str,
        event_name: &'a str,
        payload: &'a Value,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move {
            sqlx::query("SELECT absurd.emit_event($1, $2, $3)")
                .bind(queue)
                .bind(event_name)
                .bind(payload)
                .execute(&self.pool)
                .await?;
            Ok(())
        })
    }

    fn await_event<'a>(
        &'a self,
        queue: &'a str,
        task_id: Uuid,
        run_id: Uuid,
        step: &'a str,
        event_name: &'a str,
        timeout_secs: Option<i64>,
    ) -> BoxFut<'a, Result<Value>> {
        Box::pin(async move {
            // absurd.await_event blocks in PG until the event fires or timeout.
            let payload: Value = sqlx::query_scalar(
                "SELECT payload FROM absurd.await_event($1, $2, $3, $4, $5, $6)",
            )
            .bind(queue)
            .bind(task_id)
            .bind(run_id)
            .bind(step)
            .bind(event_name)
            .bind(timeout_secs)
            .fetch_one(&self.pool)
            .await?;
            Ok(payload)
        })
    }
}

/// Build a sanitized `absurd.<prefix><queue>` table identifier for the
/// per-queue materialized tables (`t_<queue>` tasks, `c_<queue>` checkpoints).
///
/// The queue name flows from recipe data, so this is a SQL-injection guard on
/// the dynamic table name. absurd's `validate_queue_name` already constrains
/// it; we additionally require `[a-zA-Z0-9_]` and replace anything else with
/// `_` (with a warning) rather than panic in production. MetaMach convention is
/// `<blueprint>_<workflow>` (underscore, never `.`).
fn queue_ident_suffix(queue: &str, prefix: &str) -> String {
    let sanitized: String = queue
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized != queue {
        warn!(queue = %queue, sanitized = %sanitized, "queue name had non-ident chars; sanitized for table suffix");
    }
    format!("{prefix}{sanitized}")
}

// --- Test impl -----------------------------------------------------------

/// In-memory [`DurableEngine`] for unit-testing the workflow engine (Phase 0b)
/// without PG. Phase 0a ships a no-op stub to prove the trait is implementable
/// and object-safe; Phase 0b expands it into a functional in-memory impl that
/// tracks tasks/checkpoints/events in a `Mutex<HashMap>`.
#[cfg(test)]
#[derive(Default)]
pub struct FakeEngine;

#[cfg(test)]
impl DurableEngine for FakeEngine {
    fn create_queue<'a>(&'a self, _queue: &'a str) -> BoxFut<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }
    fn spawn_task<'a>(
        &'a self,
        _queue: &'a str,
        _task_name: &'a str,
        _params: &'a Value,
    ) -> BoxFut<'a, Result<Uuid>> {
        Box::pin(async move { Ok(Uuid::new_v4()) })
    }
    fn claim_task<'a>(
        &'a self,
        _queue: &'a str,
        _worker_id: &'a str,
    ) -> BoxFut<'a, Result<Option<ClaimedTask>>> {
        Box::pin(async move { Ok(None) })
    }
    fn complete_run<'a>(
        &'a self,
        _queue: &'a str,
        _run_id: Uuid,
        _state: &'a Value,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }
    fn fail_run<'a>(
        &'a self,
        _queue: &'a str,
        _run_id: Uuid,
        _reason: &'a Value,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }
    fn set_checkpoint<'a>(
        &'a self,
        _queue: &'a str,
        _task_id: Uuid,
        _step: &'a str,
        _state: &'a Value,
        _owner_run: Uuid,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }
    fn get_last_checkpoint<'a>(
        &'a self,
        _queue: &'a str,
        _task_id: Uuid,
    ) -> BoxFut<'a, Result<Option<(String, Value)>>> {
        Box::pin(async move { Ok(None) })
    }
    fn non_terminal_tasks<'a>(&'a self, _queue: &'a str) -> BoxFut<'a, Result<Vec<TaskInfo>>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn emit_event<'a>(
        &'a self,
        _queue: &'a str,
        _event_name: &'a str,
        _payload: &'a Value,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }
    fn await_event<'a>(
        &'a self,
        _queue: &'a str,
        _task_id: Uuid,
        _run_id: Uuid,
        _step: &'a str,
        _event_name: &'a str,
        _timeout_secs: Option<i64>,
    ) -> BoxFut<'a, Result<Value>> {
        Box::pin(async move { Ok(Value::Null) })
    }
}
