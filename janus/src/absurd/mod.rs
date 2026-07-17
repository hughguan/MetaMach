//! Absurd DB layer (Feature-Spec §2.1, §4; Contract 3.1/3.3/3.8).
//!
//! Owns the sqlx Postgres connection pool. The Daemon opens the DB in
//! **degraded mode** immediately (fallback.db only, reads return empty) and
//! spawns a background task that retries PG with backoff; once PG is reachable
//! the pool is upgraded in place and queries start hitting Postgres. This keeps
//! the UDS server serving without blocking on a 10s PG retry at startup
//! (Feature-Spec §2.1 PG-Unreachable Self-Healing).

pub mod fallback;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::protocol::{ActiveTask, BlueprintInfo, StepStatus};

/// Physical size budget for Step result_cache / stdout (Feature-Spec §4, UTC-05-01).
pub const SIZE_BUDGET: usize = 16 * 1024;
const BUDGET_TAG: &str = "[MetaMach Log Budget Exceeded]";
const PG_RETRY_ATTEMPTS: u32 = 5;
const PG_RETRY_INTERVAL: Duration = Duration::from_secs(2);

pub struct AbsurdDb {
    /// `None` while PG is unreachable (degraded); upgraded in place by the
    /// background connect task.
    pg: RwLock<Option<PgPool>>,
    fallback: Mutex<fallback::FallbackDb>,
}

impl AbsurdDb {
    /// Open in degraded mode (fallback.db ready, PG pending). Synchronous so the
    /// Daemon can start serving UDS immediately.
    pub fn open_degraded(fallback_path: &Path) -> Result<Self> {
        let fallback = fallback::FallbackDb::open(fallback_path)?;
        Ok(Self {
            pg: RwLock::new(None),
            fallback: Mutex::new(fallback),
        })
    }

    /// Spawn the background PG-connect task (5 attempts, 2s interval). On success
    /// the pool is stored; on exhaustion the DB stays degraded.
    pub fn spawn_connect(self: &Arc<Self>, opts: PgConnectOptions) {
        let db = Arc::clone(self);
        tokio::spawn(async move {
            for attempt in 1..=PG_RETRY_ATTEMPTS {
                match PgPoolOptions::new()
                    .max_connections(8)
                    .connect_with(opts.clone())
                    .await
                {
                    Ok(pool) => {
                        *db.pg.write().await = Some(pool);
                        info!("connected to Absurd Postgres");
                        return;
                    }
                    Err(e) => warn!("PG connect {attempt}/{PG_RETRY_ATTEMPTS} failed: {e}"),
                }
                tokio::time::sleep(PG_RETRY_INTERVAL).await;
            }
            warn!(
                "Absurd Postgres unreachable after {PG_RETRY_ATTEMPTS} attempts - degraded mode (fallback.db only)"
            );
        });
    }

    pub async fn pg_online(&self) -> bool {
        self.pg.read().await.is_some()
    }

    /// All `ACTIVE` blueprints (Dispatch view). Empty in degraded mode.
    pub async fn active_blueprints(&self) -> Result<Vec<BlueprintInfo>> {
        let Some(pg) = self.pool().await else {
            return Ok(vec![]);
        };
        let rows: Vec<BlueprintRow> = sqlx::query_as(
            "SELECT name, default_workflow, remote_host, status \
             FROM blueprints WHERE status = 'ACTIVE' ORDER BY name",
        )
        .fetch_all(&pg)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// Contract 3.3 progress snapshot - non-terminal tasks joined with their steps.
    /// Empty in degraded mode. Uses an independent read path (never contends with
    /// workflow write transactions - Feature-Spec §2.6).
    pub async fn progress(&self, blueprint: Option<&str>) -> Result<Vec<ActiveTask>> {
        let Some(pg) = self.pool().await else {
            return Ok(vec![]);
        };
        let tasks: Vec<TaskRow> = sqlx::query_as(
            "SELECT t.id, b.name AS blueprint, t.workflow_name, t.status, t.started_at \
             FROM absurd_tasks t JOIN blueprints b ON b.id = t.blueprint_id \
             WHERE t.status IN ('STARTING','RUNNING','SUSPENDED') \
             AND ($1::text IS NULL OR b.name = $1) \
             ORDER BY t.id",
        )
        .bind(blueprint)
        .fetch_all(&pg)
        .await?;

        if tasks.is_empty() {
            return Ok(vec![]);
        }

        // Batch-fetch all steps for the task set in one query (avoids N+1
        // round-trips - one SELECT per task under load). Ordered by task_id,
        // step_name so each task's steps stay in step_name order after partition.
        let task_ids: Vec<i64> = tasks.iter().map(|t| t.id).collect();
        let all_steps: Vec<StepRow> = sqlx::query_as(
            "SELECT task_id, step_name, status, result_cache FROM absurd_steps \
             WHERE task_id = ANY($1) ORDER BY task_id, step_name",
        )
        .bind(task_ids)
        .fetch_all(&pg)
        .await?;
        let mut steps_by_task: std::collections::HashMap<i64, Vec<StepRow>> =
            std::collections::HashMap::new();
        for s in all_steps {
            steps_by_task.entry(s.task_id).or_default().push(s);
        }

        let mut active = Vec::with_capacity(tasks.len());
        for t in tasks {
            let steps = steps_by_task.remove(&t.id).unwrap_or_default();
            let current_step = steps
                .iter()
                .rev()
                .find(|s| matches!(s.status.as_str(), "STARTING" | "RUNNING"))
                .map(|s| s.step_name.clone());
            let elapsed = t.started_at.map(|s| (Utc::now() - s).num_seconds().max(0));

            active.push(ActiveTask {
                task_id: t.id,
                blueprint_id: t.blueprint,
                workflow_name: t.workflow_name,
                status: t.status.clone(),
                started_at: t.started_at.map(|s| s.to_rfc3339()),
                elapsed_seconds: elapsed,
                current_step,
                tether_alive: false, // Tether liveness lands with Task 2.4.
                suspended_reason: if t.status == "SUSPENDED" {
                    Some("awaiting HITL".to_string())
                } else {
                    None
                },
                steps: steps
                    .into_iter()
                    .map(|s| StepStatus {
                        name: s.step_name,
                        status: s.status,
                        stdout_tail: s.result_cache.map(|j| truncate_16k(&j.to_string())),
                    })
                    .collect(),
            });
        }
        Ok(active)
    }

    /// Snapshot the current pool (cheap clone - `PgPool` is `Arc`-backed) without
    /// holding the lock across an `.await`.
    async fn pool(&self) -> Option<PgPool> {
        self.pg.read().await.as_ref().cloned()
    }

    /// Append a Step transition to the fallback ring buffer (used during PG
    /// outage; replayed into Postgres on recovery - Feature-Spec §4).
    pub fn record_fallback_event(
        &self,
        task_id: i64,
        step_name: &str,
        status: &str,
        result_cache: Option<&str>,
    ) -> Result<()> {
        self.fallback
            .lock()
            .expect("fallback mutex poisoned")
            .record(task_id, step_name, status, result_cache)
    }

    /// Mark a Step `SUSPENDED` (non-destructive HITL - Feature-Spec §2.4). If PG
    /// is online, UPDATEs the step row (no-op if the step doesn't exist yet, e.g.
    /// M3 with no running workflow); in degraded mode, records a fallback event.
    pub async fn suspend_step(&self, task_id: i64, step_name: &str, reason: &str) -> Result<()> {
        if let Some(pg) = self.pool().await {
            sqlx::query(
                "UPDATE absurd_steps SET status = 'SUSPENDED', \
                 result_cache = COALESCE(result_cache, $1) \
                 WHERE task_id = $2 AND step_name = $3",
            )
            .bind(serde_json::json!({ "suspended_reason": reason }))
            .bind(task_id)
            .bind(step_name)
            .execute(&pg)
            .await?;
            Ok(())
        } else {
            let cache = serde_json::json!({ "suspended_reason": reason }).to_string();
            self.record_fallback_event(task_id, step_name, "SUSPENDED", Some(&cache))
        }
    }

    /// Task 4.3: idempotent tenant registration (Feature-Spec §2.5.3). Returns
    /// `true` if an existing (e.g. OFFBOARDED) row was reactivated, `false` if a
    /// fresh row was inserted.
    pub async fn register_blueprint(
        &self,
        recipe: &crate::recipe::ValidatedRecipe,
    ) -> Result<bool> {
        let Some(pg) = self.pool().await else {
            bail!("pg offline");
        };
        let scope_json = serde_json::to_value(&recipe.openwiki_scope)?;
        let config_json: serde_json::Value =
            serde_json::from_str(&recipe.config_text).unwrap_or(serde_json::Value::Null);
        // `(xmax = 0)` is true for a freshly INSERTed row, false for one touched
        // by ON CONFLICT DO UPDATE (i.e. a reactivation of an existing row).
        let row: (bool,) = sqlx::query_as(
            "INSERT INTO blueprints \
                (name, status, default_workflow, config, openwiki_scope, remote_host, onboarded_at) \
             VALUES ($1, 'ACTIVE', $2, $3, $4, $5, NOW()) \
             ON CONFLICT (name) DO UPDATE \
             SET status='ACTIVE', default_workflow=EXCLUDED.default_workflow, \
                 config=EXCLUDED.config, openwiki_scope=EXCLUDED.openwiki_scope, \
                 remote_host=EXCLUDED.remote_host, onboarded_at=NOW(), offboarded_at=NULL \
             RETURNING (xmax = 0) AS inserted",
        )
        .bind(&recipe.name)
        .bind(&recipe.default_workflow)
        .bind(&config_json)
        .bind(&scope_json)
        .bind(recipe.remote_host.as_deref())
        .fetch_one(&pg)
        .await?;
        Ok(!row.0) // reactivated = NOT freshly inserted
    }

    /// Task 4.2: scan the most-recent step traces (with result_cache) for the
    /// blueprint, for Offboard smelting.
    pub async fn scan_offboard_traces(&self, name: &str, limit: usize) -> Result<Vec<StepTrace>> {
        let Some(pg) = self.pool().await else {
            bail!("pg offline");
        };
        let rows: Vec<StepTrace> = sqlx::query_as(
            "SELECT s.task_id, s.step_name, s.status, s.result_cache::text AS result_cache \
             FROM absurd_steps s \
             JOIN absurd_tasks t ON t.id = s.task_id \
             JOIN blueprints b ON b.id = t.blueprint_id \
             WHERE b.name = $1 \
             ORDER BY s.updated_at DESC \
             LIMIT $2",
        )
        .bind(name)
        .bind(limit as i64)
        .fetch_all(&pg)
        .await?;
        Ok(rows)
    }

    /// Task 4.2: physically DELETE result_cache JSON for the blueprint via the
    /// `melt_blueprint_data` stored proc (migration 002). Returns rows deleted.
    pub async fn melt_blueprint_data(&self, name: &str) -> Result<i64> {
        let Some(pg) = self.pool().await else {
            bail!("pg offline");
        };
        let row: (i64,) = sqlx::query_as("SELECT melt_blueprint_data($1)")
            .bind(name)
            .fetch_one(&pg)
            .await?;
        Ok(row.0)
    }

    /// Task 4.2: mark a blueprint OFFBOARDED.
    pub async fn set_blueprint_offboarded(&self, name: &str) -> Result<()> {
        let Some(pg) = self.pool().await else {
            bail!("pg offline");
        };
        sqlx::query("UPDATE blueprints SET status='OFFBOARDED', offboarded_at=NOW() WHERE name=$1")
            .bind(name)
            .execute(&pg)
            .await?;
        Ok(())
    }

    /// Task 4.1: non-terminal tasks at cold start, each with its last COMPLETED
    /// step (the resume breakpoint). Empty in degraded mode (PG offline).
    pub async fn cold_start_running_tasks(&self) -> Result<Vec<ColdStartTask>> {
        let Some(pg) = self.pool().await else {
            return Ok(vec![]);
        };
        let rows: Vec<ColdStartRow> = sqlx::query_as(
            "SELECT t.id, b.name AS blueprint, t.workflow_name, t.status, \
                    (SELECT s.step_name FROM absurd_steps s \
                     WHERE s.task_id = t.id AND s.status = 'COMPLETED' \
                     ORDER BY s.updated_at DESC LIMIT 1) AS last_completed_step \
             FROM absurd_tasks t JOIN blueprints b ON b.id = t.blueprint_id \
             WHERE t.status IN ('STARTING','RUNNING','SUSPENDED') \
             ORDER BY t.id",
        )
        .fetch_all(&pg)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// ARCH §6.2: Janus GC - NULL-ify `result_cache` for steps of tasks
    /// COMPLETED more than 3 days ago. Returns rows affected.
    pub async fn gc_old_caches(&self) -> Result<i64> {
        let Some(pg) = self.pool().await else {
            return Ok(0);
        };
        let res = sqlx::query(
            "UPDATE absurd_steps s SET result_cache = NULL \
             FROM absurd_tasks t \
             WHERE s.task_id = t.id \
               AND t.status = 'COMPLETED' \
               AND t.completed_at IS NOT NULL \
               AND t.completed_at < NOW() - INTERVAL '3 days' \
               AND s.result_cache IS NOT NULL",
        )
        .execute(&pg)
        .await?;
        Ok(res.rows_affected() as i64)
    }
}

/// A historical step trace for Offboard smelting (Task 4.2).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StepTrace {
    pub task_id: i64,
    pub step_name: String,
    pub status: String,
    pub result_cache: Option<String>,
}

/// A non-terminal task found at cold start + its last COMPLETED step (Task 4.1).
#[derive(Debug, Clone)]
pub struct ColdStartTask {
    pub task_id: i64,
    pub blueprint: String,
    pub workflow_name: String,
    pub status: String,
    pub last_completed_step: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ColdStartRow {
    id: i64,
    blueprint: String,
    workflow_name: String,
    status: String,
    last_completed_step: Option<String>,
}

impl From<ColdStartRow> for ColdStartTask {
    fn from(r: ColdStartRow) -> Self {
        Self {
            task_id: r.id,
            blueprint: r.blueprint,
            workflow_name: r.workflow_name,
            status: r.status,
            last_completed_step: r.last_completed_step,
        }
    }
}

#[derive(sqlx::FromRow)]
struct BlueprintRow {
    name: String,
    default_workflow: String,
    remote_host: Option<String>,
    status: String,
}

impl From<BlueprintRow> for BlueprintInfo {
    fn from(r: BlueprintRow) -> Self {
        Self {
            name: r.name,
            default_workflow: r.default_workflow,
            remote_host: r.remote_host,
            status: r.status,
        }
    }
}

#[derive(sqlx::FromRow)]
struct TaskRow {
    id: i64,
    blueprint: String,
    workflow_name: String,
    status: String,
    started_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct StepRow {
    task_id: i64,
    step_name: String,
    status: String,
    result_cache: Option<serde_json::Value>,
}

/// Truncate to the 16 KiB budget, appending the budget-exceeded tag if cut.
/// The authoritative enforcement point is here (Feature-Spec §4 fault matrix).
pub fn truncate_16k(s: &str) -> String {
    if s.len() <= SIZE_BUDGET {
        return s.to_string();
    }
    let target = SIZE_BUDGET.saturating_sub(BUDGET_TAG.len());
    let mut cut = target;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(SIZE_BUDGET);
    out.push_str(&s[..cut]);
    out.push_str(BUDGET_TAG);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_under_budget_is_unchanged() {
        let s = "x".repeat(100);
        assert_eq!(truncate_16k(&s).len(), 100);
        assert!(!truncate_16k(&s).contains(BUDGET_TAG));
    }

    #[test]
    fn truncate_over_budget_caps_and_tags() {
        let s = "x".repeat(SIZE_BUDGET * 2);
        let out = truncate_16k(&s);
        assert!(out.len() <= SIZE_BUDGET, "len {} > budget", out.len());
        assert!(out.ends_with(BUDGET_TAG));
    }

    #[test]
    fn truncate_respects_char_boundary() {
        // multibyte content that would split a UTF-8 sequence at a naive byte cut
        let s = "é".repeat(SIZE_BUDGET); // é = 2 bytes
        let out = truncate_16k(&s);
        assert!(out.len() <= SIZE_BUDGET);
        // must remain valid UTF-8 (no panic on from_str)
        let _: String = out.parse().unwrap();
    }
}
