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

use anyhow::Result;
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

        let mut active = Vec::with_capacity(tasks.len());
        for t in tasks {
            let steps: Vec<StepRow> = sqlx::query_as(
                "SELECT step_name, status, result_cache FROM absurd_steps \
                 WHERE task_id = $1 ORDER BY step_name",
            )
            .bind(t.id)
            .fetch_all(&pg)
            .await?;

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
