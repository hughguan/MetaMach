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
    /// `absurd.extend_claim(queue, run_id, extend_by_secs)` - renew the lease so
    /// a long-running step doesn't expire mid-execution (`claim_task`'s lease is
    /// 30s; absurd auto-fails expired runs). The engine calls this every ~10s
    /// while polling for pane exit.
    fn extend_claim<'a>(
        &'a self,
        queue: &'a str,
        run_id: Uuid,
        extend_by_secs: i64,
    ) -> BoxFut<'a, Result<()>>;
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
            // `max_attempts: 3` lets absurd retry transient failures (e.g. a cargo
            // build timeout on a slow network) within-session. The Phase 1 engine
            // is a retry-claim loop: after `fail_run` it re-`claim_task`s the retry
            // run absurd schedules + resumes from the last `COMPLETED` checkpoint,
            // so retries no longer orphan (the Phase 0b one-shot worker did). After
            // 3 failures the task goes terminal `failed` -> MetaMach takes over
            // (cold-start / Task 4.4 / manual re-dispatch).
            let task_id: Uuid = sqlx::query_scalar(
                "SELECT task_id FROM absurd.spawn_task($1, $2, $3, '{\"max_attempts\": 3}'::jsonb)",
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

    fn extend_claim<'a>(
        &'a self,
        queue: &'a str,
        run_id: Uuid,
        extend_by_secs: i64,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move {
            sqlx::query("SELECT absurd.extend_claim($1, $2, $3)")
                .bind(queue)
                .bind(run_id)
                .bind(extend_by_secs)
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
/// without PG. Tracks queues / tasks / runs / checkpoints / events in a
/// `Mutex<FakeState>`. Faithful enough to assert the engine's call sequence
/// (`create_queue` -> `spawn_task` -> `claim_task` -> `set_checkpoint` ->
/// `complete_run`/`fail_run`) and inspect the resulting state.
///
/// `claim_task` mints a fresh `run_id` (UUIDv4) and moves the task `pending` ->
/// `running`, mirroring absurd's lease semantics. There are no `.await`s inside
/// the boxed futures, so the `std::sync::Mutex` guard never crosses an await
/// point (Send-safe).
#[cfg(test)]
pub struct FakeEngine {
    state: std::sync::Mutex<FakeState>,
    /// `max_attempts` applied to subsequently spawned tasks (default 3; tests use
    /// [`with_max_attempts`](FakeEngine::with_max_attempts) for fewer).
    default_max_attempts: usize,
}

#[cfg(test)]
impl Default for FakeEngine {
    fn default() -> Self {
        Self {
            state: std::sync::Mutex::new(FakeState::default()),
            default_max_attempts: 3,
        }
    }
}

#[cfg(test)]
#[derive(Default)]
struct FakeState {
    queues: std::collections::HashSet<String>,
    /// task_id -> task record. Insertion order is preserved per-queue via
    /// `queue_order` so `claim_task` drains FIFO (as absurd does).
    tasks: std::collections::HashMap<Uuid, FakeTask>,
    queue_order: std::collections::HashMap<String, Vec<Uuid>>,
    /// task_id -> checkpoints in insertion order (last = most recent).
    checkpoints: std::collections::HashMap<Uuid, Vec<(String, Value)>>,
    /// (queue, event_name) -> emitted payloads (await_event drains FIFO).
    events: std::collections::HashMap<(String, String), Vec<Value>>,
}

#[cfg(test)]
#[derive(Clone)]
struct FakeTask {
    task_id: Uuid,
    task_name: String,
    params: Value,
    /// `pending` (initial) | `running` (claimed) | `sleeping` (retry pending) |
    /// `completed` | `failed` (terminal).
    state: String,
    max_attempts: usize,
    attempts: usize,
    /// The currently-claimed run id (`None` between attempts).
    current_run: Option<Uuid>,
    /// Run ids waiting to be claimed: the initial run at spawn + a retry run
    /// after each `fail_run` while `attempts < max_attempts`.
    pending_runs: std::collections::VecDeque<Uuid>,
}

#[cfg(test)]
impl FakeEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the `max_attempts` applied to subsequently spawned tasks (for tests
    /// that want fewer retries, e.g. exhaustion in 2 attempts instead of 3).
    pub fn with_max_attempts(max_attempts: usize) -> Self {
        Self {
            state: std::sync::Mutex::new(FakeState::default()),
            default_max_attempts: max_attempts,
        }
    }

    /// Seed a checkpoint for `task_id` as if `step` had `COMPLETED` in a prior
    /// attempt - used to test the resume path ([`run_workflow`](crate::workflow)
    /// skips the step via [`resume_index`](crate::workflow)).
    pub fn seed_checkpoint(&self, task_id: Uuid, step: &str) {
        self.state
            .lock()
            .expect("fake engine mutex")
            .checkpoints
            .entry(task_id)
            .or_default()
            .push((
                step.to_string(),
                serde_json::json!({"step": step, "status": "COMPLETED"}),
            ));
    }

    /// Snapshot of a task's state (`pending`/`running`/`sleeping`/`completed`/
    /// `failed`), for engine unit-test assertions.
    pub fn task_state(&self, task_id: Uuid) -> Option<String> {
        self.state
            .lock()
            .expect("fake engine mutex")
            .tasks
            .get(&task_id)
            .map(|t| t.state.clone())
    }

    /// Most-recent checkpoint `(step, state)` for a task.
    pub fn last_checkpoint(&self, task_id: Uuid) -> Option<(String, Value)> {
        self.state
            .lock()
            .expect("fake engine mutex")
            .checkpoints
            .get(&task_id)
            .and_then(|cs| cs.last().cloned())
    }
}

#[cfg(test)]
impl DurableEngine for FakeEngine {
    fn create_queue<'a>(&'a self, queue: &'a str) -> BoxFut<'a, Result<()>> {
        let q = queue.to_string();
        Box::pin(async move {
            self.state
                .lock()
                .expect("fake engine mutex")
                .queues
                .insert(q);
            Ok(())
        })
    }

    fn spawn_task<'a>(
        &'a self,
        queue: &'a str,
        task_name: &'a str,
        params: &'a Value,
    ) -> BoxFut<'a, Result<Uuid>> {
        let q = queue.to_string();
        let name = task_name.to_string();
        let p = params.clone();
        let max_attempts = self.default_max_attempts;
        Box::pin(async move {
            let task_id = Uuid::new_v4();
            let run_id = Uuid::new_v4();
            let mut st = self.state.lock().expect("fake engine mutex");
            st.queues.insert(q.clone());
            st.tasks.insert(
                task_id,
                FakeTask {
                    task_id,
                    task_name: name,
                    params: p,
                    state: "pending".to_string(),
                    max_attempts,
                    attempts: 0,
                    current_run: None,
                    pending_runs: [run_id].into(),
                },
            );
            st.queue_order.entry(q).or_default().push(task_id);
            Ok(task_id)
        })
    }

    fn claim_task<'a>(
        &'a self,
        queue: &'a str,
        _worker_id: &'a str,
    ) -> BoxFut<'a, Result<Option<ClaimedTask>>> {
        let q = queue.to_string();
        Box::pin(async move {
            let mut st = self.state.lock().expect("fake engine mutex");
            // First task in this queue that's pending/sleeping with a claimable run.
            let task_id = st
                .queue_order
                .get(&q)
                .into_iter()
                .flatten()
                .copied()
                .find(|id| {
                    st.tasks
                        .get(id)
                        .map(|t| {
                            matches!(t.state.as_str(), "pending" | "sleeping")
                                && !t.pending_runs.is_empty()
                        })
                        .unwrap_or(false)
                });
            let Some(task_id) = task_id else {
                return Ok(None);
            };
            let task = st.tasks.get_mut(&task_id).expect("task present");
            let run_id = task.pending_runs.pop_front().expect("pending run present");
            task.attempts += 1;
            task.current_run = Some(run_id);
            task.state = "running".to_string();
            Ok(Some(ClaimedTask {
                run_id,
                task_id,
                task_name: task.task_name.clone(),
                params: task.params.clone(),
            }))
        })
    }

    fn extend_claim<'a>(
        &'a self,
        _queue: &'a str,
        _run_id: Uuid,
        _extend_by_secs: i64,
    ) -> BoxFut<'a, Result<()>> {
        // No-op: the in-memory FakeEngine doesn't enforce lease expiry, so
        // extension is a successful no-op (the engine still calls it).
        Box::pin(async move { Ok(()) })
    }

    fn complete_run<'a>(
        &'a self,
        _queue: &'a str,
        run_id: Uuid,
        _state: &'a Value,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move {
            let mut st = self.state.lock().expect("fake engine mutex");
            for t in st.tasks.values_mut() {
                if t.current_run == Some(run_id) {
                    t.current_run = None;
                    t.state = "completed".to_string();
                    break;
                }
            }
            Ok(())
        })
    }

    fn fail_run<'a>(
        &'a self,
        _queue: &'a str,
        run_id: Uuid,
        _reason: &'a Value,
    ) -> BoxFut<'a, Result<()>> {
        Box::pin(async move {
            let mut st = self.state.lock().expect("fake engine mutex");
            for t in st.tasks.values_mut() {
                if t.current_run == Some(run_id) {
                    t.current_run = None;
                    if t.attempts < t.max_attempts {
                        // absurd schedules a retry run (immediately claimable in
                        // the fake - no backoff delay, for fast deterministic tests).
                        t.pending_runs.push_back(Uuid::new_v4());
                        t.state = "sleeping".to_string();
                    } else {
                        t.state = "failed".to_string();
                    }
                    break;
                }
            }
            Ok(())
        })
    }

    fn set_checkpoint<'a>(
        &'a self,
        _queue: &'a str,
        task_id: Uuid,
        step: &'a str,
        state: &'a Value,
        _owner_run: Uuid,
    ) -> BoxFut<'a, Result<()>> {
        let s = state.clone();
        Box::pin(async move {
            self.state
                .lock()
                .expect("fake engine mutex")
                .checkpoints
                .entry(task_id)
                .or_default()
                .push((step.to_string(), s));
            Ok(())
        })
    }

    fn get_last_checkpoint<'a>(
        &'a self,
        _queue: &'a str,
        task_id: Uuid,
    ) -> BoxFut<'a, Result<Option<(String, Value)>>> {
        Box::pin(async move {
            Ok(self
                .state
                .lock()
                .expect("fake engine mutex")
                .checkpoints
                .get(&task_id)
                .and_then(|cs| cs.last().cloned()))
        })
    }

    fn non_terminal_tasks<'a>(&'a self, queue: &'a str) -> BoxFut<'a, Result<Vec<TaskInfo>>> {
        let q = queue.to_string();
        Box::pin(async move {
            let st = self.state.lock().expect("fake engine mutex");
            let ids = st.queue_order.get(&q).cloned().unwrap_or_default();
            Ok(ids
                .into_iter()
                .filter_map(|id| {
                    let t = st.tasks.get(&id)?;
                    if matches!(t.state.as_str(), "pending" | "sleeping" | "running") {
                        Some(TaskInfo {
                            task_id: t.task_id,
                            task_name: t.task_name.clone(),
                            state: t.state.clone(),
                        })
                    } else {
                        None
                    }
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
        let p = payload.clone();
        Box::pin(async move {
            self.state
                .lock()
                .expect("fake engine mutex")
                .events
                .entry((queue.to_string(), event_name.to_string()))
                .or_default()
                .push(p);
            Ok(())
        })
    }

    fn await_event<'a>(
        &'a self,
        queue: &'a str,
        _task_id: Uuid,
        _run_id: Uuid,
        _step: &'a str,
        event_name: &'a str,
        _timeout_secs: Option<i64>,
    ) -> BoxFut<'a, Result<Value>> {
        // The Phase 0b engine does NOT call await_event (the resume loop is the
        // follow-on). For trait completeness: drain an emitted payload if present,
        // else return Null (never blocks the test).
        Box::pin(async move {
            Ok(self
                .state
                .lock()
                .expect("fake engine mutex")
                .events
                .get_mut(&(queue.to_string(), event_name.to_string()))
                .and_then(|v| v.first().cloned())
                .unwrap_or(Value::Null))
        })
    }
}
