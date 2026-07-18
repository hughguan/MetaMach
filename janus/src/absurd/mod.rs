//! Absurd DB layer (Feature-Spec §2.1, §4; Contract 3.1/3.1b/3.3/3.8).
//!
//! Owns the sqlx Postgres connection pools. The Daemon opens the **catalog** DB
//! (`metamach_db`) in degraded mode immediately (fallback.db only) and spawns a
//! background task that retries PG; once reachable the catalog pool is upgraded
//! in place. Per-blueprint pools (`metamach_blueprint_<name>`) are resolved on
//! demand and cached, so step-meta reads fan out across active blueprints
//! (Contract 3.3 progress, cold-start, offboard).
//!
//! Absurd owns the task/checkpoint/event tables in each per-blueprint DB; MetaMach
//! reads its thin `metamach_step_meta` overlay for the dashboard/audit fields it
//! needs (status, exit_code, stdout_tail, started_at, target_sha) without dynamic
//! SQL or a `list_tasks` absurd function (M0.5 spike F1). Absurd's write-side
//! functions (`spawn_task`/`set_task_checkpoint_state`/`cleanup_tasks`) are called
//! by the workflow engine (M2.4 tmux + M4); this module is the read/audit path.

pub mod fallback;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock as StdRwLock};
use std::time::Duration;

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use crate::protocol::{ActiveTask, BlueprintInfo, StepStatus};

/// Physical size budget for Step stdout_tail / result_cache (Feature-Spec §4, UTC-05-01).
pub const SIZE_BUDGET: usize = 16 * 1024;
const BUDGET_TAG: &str = "[MetaMach Log Budget Exceeded]";
const PG_RETRY_ATTEMPTS: u32 = 5;
const PG_RETRY_INTERVAL: Duration = Duration::from_secs(2);

pub struct AbsurdDb {
    /// Catalog DB pool (`metamach_db`): `blueprints` + `absurd_audit_log`.
    /// `None` while PG is unreachable (degraded).
    catalog: RwLock<Option<PgPool>>,
    /// Base connect options (socket+user+password, database=`metamach_db`).
    /// Cloned per blueprint with the database swapped to `metamach_blueprint_<name>`.
    /// Std lock: written once (sync `spawn_connect`), read briefly in async paths.
    base_opts: StdRwLock<Option<PgConnectOptions>>,
    /// Cached per-blueprint pools (lazy, never evicted - bounded by # blueprints).
    blueprint_pools: StdRwLock<HashMap<String, Arc<PgPool>>>,
    fallback: Mutex<fallback::FallbackDb>,
}

impl AbsurdDb {
    /// Open in degraded mode (fallback.db ready, PG pending). Synchronous so the
    /// Daemon can start serving UDS immediately.
    pub fn open_degraded(fallback_path: &Path) -> Result<Self> {
        let fallback = fallback::FallbackDb::open(fallback_path)?;
        Ok(Self {
            catalog: RwLock::new(None),
            base_opts: StdRwLock::new(None),
            blueprint_pools: StdRwLock::new(HashMap::new()),
            fallback: Mutex::new(fallback),
        })
    }

    /// Spawn the background PG-connect task (5 attempts, 2s interval). Stores the
    /// base connect options for per-blueprint pool derivation. On success the
    /// catalog pool is stored; on exhaustion the DB stays degraded.
    pub fn spawn_connect(self: &Arc<Self>, opts: PgConnectOptions) {
        *self.base_opts.write().expect("base_opts lock") = Some(opts.clone());
        let db = Arc::clone(self);
        tokio::spawn(async move {
            for attempt in 1..=PG_RETRY_ATTEMPTS {
                match PgPoolOptions::new()
                    .max_connections(8)
                    .connect_with(opts.clone())
                    .await
                {
                    Ok(pool) => {
                        *db.catalog.write().await = Some(pool);
                        info!("connected to Absurd Postgres (catalog)");
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
        self.catalog.read().await.is_some()
    }

    /// Snapshot the catalog pool (cheap clone - `PgPool` is `Arc`-backed) without
    /// holding the lock across an `.await`.
    async fn catalog_pool(&self) -> Option<PgPool> {
        self.catalog.read().await.as_ref().cloned()
    }

    /// Resolve (and cache) the per-blueprint DB pool. Returns `None` if the
    /// catalog is offline or the blueprint DB cannot be reached (graceful - the
    /// caller skips that blueprint).
    async fn blueprint_pool(&self, name: &str) -> Result<Option<PgPool>> {
        if let Some(p) = self.blueprint_pools.read().expect("bp lock").get(name) {
            return Ok(Some(p.as_ref().clone()));
        }
        let Some(base) = self.base_opts.read().expect("base_opts lock").clone() else {
            return Ok(None);
        };
        let db_name = format!("metamach_blueprint_{}", sanitize_ident(name));
        let opts = base.database(&db_name);
        match PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
        {
            Ok(pool) => {
                let arc = Arc::new(pool.clone());
                self.blueprint_pools
                    .write()
                    .expect("bp lock")
                    .insert(name.to_string(), arc);
                Ok(Some(pool))
            }
            Err(e) => {
                warn!(blueprint = name, "per-blueprint DB connect failed: {e}");
                Ok(None)
            }
        }
    }

    /// All `ACTIVE` blueprints (Dispatch view). Empty in degraded mode.
    pub async fn active_blueprints(&self) -> Result<Vec<BlueprintInfo>> {
        let Some(pg) = self.catalog_pool().await else {
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

    /// Contract 3.3 progress snapshot - non-terminal steps grouped by task, fanned
    /// out across the caller's (or all ACTIVE) blueprint DBs. Empty in degraded
    /// mode. Uses an independent read path (never contends with workflow writes).
    pub async fn progress(&self, blueprint: Option<&str>) -> Result<Vec<ActiveTask>> {
        let names: Vec<String> = match blueprint {
            Some(n) => vec![n.to_string()],
            None => self
                .active_blueprints()
                .await?
                .into_iter()
                .map(|b| b.name)
                .collect(),
        };
        let mut all = Vec::new();
        for name in names {
            let Some(pool) = self.blueprint_pool(&name).await? else {
                continue;
            };
            let rows: Vec<MetaRow> = sqlx::query_as(
                "SELECT task_id, step_name, status, exit_code, stdout_tail, started_at, \
                        blueprint_name, workflow_name \
                 FROM metamach_step_meta \
                 WHERE status IN ('STARTING','RUNNING','SUSPENDED') \
                 ORDER BY task_id, step_name",
            )
            .fetch_all(&pool)
            .await?;
            if rows.is_empty() {
                continue;
            }
            let mut by_task: HashMap<Uuid, Vec<MetaRow>> = HashMap::new();
            for r in rows {
                by_task.entry(r.task_id).or_default().push(r);
            }
            for (task_id, mut steps) in by_task {
                let blueprint_name = steps
                    .first()
                    .map(|s| s.blueprint_name.clone())
                    .unwrap_or_else(|| name.clone());
                let workflow_name = steps
                    .first()
                    .and_then(|s| s.workflow_name.clone())
                    .unwrap_or_default();
                let task_status = derive_task_status(&steps);
                let suspended = task_status == "SUSPENDED";
                let current_step = steps
                    .iter()
                    .rev()
                    .find(|s| matches!(s.status.as_str(), "STARTING" | "RUNNING"))
                    .map(|s| s.step_name.clone());
                let started_at = steps.iter().filter_map(|s| s.started_at).min();
                let elapsed = started_at.map(|s| (Utc::now() - s).num_seconds().max(0));
                all.push(ActiveTask {
                    task_id,
                    blueprint_id: blueprint_name,
                    workflow_name,
                    status: task_status,
                    started_at: started_at.map(|s| s.to_rfc3339()),
                    elapsed_seconds: elapsed,
                    current_step,
                    tmux_alive: false, // Tether liveness lands with Task 2.4.
                    suspended_reason: if suspended {
                        Some("awaiting HITL".to_string())
                    } else {
                        None
                    },
                    steps: steps
                        .drain(..)
                        .map(|s| StepStatus {
                            name: s.step_name,
                            status: s.status,
                            exit_code: s.exit_code,
                            stdout_tail: s.stdout_tail.map(|t| truncate_16k(&t)),
                        })
                        .collect(),
                });
            }
        }
        Ok(all)
    }

    /// Append a Step transition to the fallback ring buffer (used during PG
    /// outage; replayed into Postgres on recovery - Feature-Spec §4).
    pub fn record_fallback_event(
        &self,
        task_id: Uuid,
        step_name: &str,
        status: &str,
        result_cache: Option<&str>,
    ) -> Result<()> {
        self.fallback
            .lock()
            .expect("fallback mutex poisoned")
            .record(&task_id, step_name, status, result_cache)
    }

    /// Mark a Step `SUSPENDED` (non-destructive HITL - Feature-Spec §2.4) in the
    /// blueprint's overlay. If the blueprint DB is unreachable, records a fallback
    /// event instead so the transition is not lost.
    pub async fn suspend_step(
        &self,
        blueprint: &str,
        task_id: Uuid,
        step_name: &str,
        _reason: &str,
    ) -> Result<()> {
        if let Some(pool) = self.blueprint_pool(blueprint).await? {
            sqlx::query(
                "UPDATE metamach_step_meta SET status = 'SUSPENDED' \
                 WHERE task_id = $1 AND step_name = $2",
            )
            .bind(task_id)
            .bind(step_name)
            .execute(&pool)
            .await?;
            Ok(())
        } else {
            self.record_fallback_event(task_id, step_name, "SUSPENDED", None)
        }
    }

    /// Task 4.3: idempotent tenant registration (Feature-Spec §2.5.3). Catalog DB
    /// only - the per-blueprint DB + absurd queue are created by `janus onboard`
    /// (M4 Task 4.3). Returns `true` if an existing row was reactivated.
    pub async fn register_blueprint(
        &self,
        recipe: &crate::recipe::ValidatedRecipe,
    ) -> Result<bool> {
        let Some(pg) = self.catalog_pool().await else {
            bail!("pg offline");
        };
        let scope_json = serde_json::to_value(&recipe.openwiki_scope)?;
        let config_json = serde_json::Value::String(recipe.config_text.clone());
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
        Ok(!row.0)
    }

    /// Task 4.2: scan the most-recent step traces for the blueprint, for Offboard
    /// smelting. Reads the overlay's `stdout_tail` as the trace snapshot (full
    /// absurd checkpoint retrieval via `get_task_checkpoint_states` lands with M4).
    pub async fn scan_offboard_traces(&self, name: &str, limit: usize) -> Result<Vec<StepTrace>> {
        let Some(pool) = self.blueprint_pool(name).await? else {
            bail!("blueprint DB unreachable for {name}");
        };
        let rows: Vec<StepTrace> = sqlx::query_as(
            "SELECT task_id, step_name, status, \
                    COALESCE(stdout_tail, '')::text AS result_cache \
             FROM metamach_step_meta WHERE blueprint_name = $1 \
             ORDER BY updated_at DESC LIMIT $2",
        )
        .bind(name)
        .bind(limit as i64)
        .fetch_all(&pool)
        .await?;
        Ok(rows)
    }

    /// Task 4.2: Offboard trace purge + audit archive (F2: NOT a `melt` proc -
    /// absurd has generic `cleanup_tasks`; MetaMach orchestrates). DELETEs the
    /// overlay rows in the blueprint DB and archives a summary row to the global
    /// `absurd_audit_log` (catalog). Per-task archiving lands with M4.
    pub async fn offboard_blueprint_data(&self, name: &str) -> Result<i64> {
        // Catalog must be reachable so the audit row can be written - refuse to
        // purge without an audit trail (F2: DELETE + archive). A TOCTOU catalog
        // drop between the lifecycle pg_online() check and here aborts the
        // offboard BEFORE any overlay rows are deleted.
        let Some(catalog) = self.catalog_pool().await else {
            bail!("catalog DB offline - cannot offboard {name} (audit log unreachable)");
        };
        let purged = if let Some(pool) = self.blueprint_pool(name).await? {
            let count: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM metamach_step_meta WHERE blueprint_name = $1")
                    .bind(name)
                    .fetch_one(&pool)
                    .await?;
            sqlx::query("DELETE FROM metamach_step_meta WHERE blueprint_name = $1")
                .bind(name)
                .execute(&pool)
                .await?;
            count.0
        } else {
            0
        };
        sqlx::query(
            "INSERT INTO absurd_audit_log \
                (task_id, blueprint_name, workflow_name, step_count, elapsed_seconds, trace_summary) \
             VALUES ($1, $2, '', $3, NULL, $4)",
        )
        .bind(Uuid::nil()) // blueprint-level summary; per-task rows land with M4
        .bind(name)
        .bind(purged)
        .bind(serde_json::json!({ "purged_overlay_rows": purged }))
        .execute(&catalog)
        .await?;
        Ok(purged)
    }

    /// Task 4.2: mark a blueprint OFFBOARDED (catalog).
    pub async fn set_blueprint_offboarded(&self, name: &str) -> Result<()> {
        let Some(pg) = self.catalog_pool().await else {
            bail!("pg offline");
        };
        sqlx::query("UPDATE blueprints SET status='OFFBOARDED', offboarded_at=NOW() WHERE name=$1")
            .bind(name)
            .execute(&pg)
            .await?;
        Ok(())
    }

    /// Task 4.1: non-terminal tasks at cold start, each with its last `COMPLETED`
    /// step (the resume breakpoint). Fans out across ACTIVE blueprint DBs. Empty
    /// in degraded mode.
    pub async fn cold_start_running_tasks(&self) -> Result<Vec<ColdStartTask>> {
        let blueprints = self.active_blueprints().await?;
        let mut all = Vec::new();
        for b in blueprints {
            let Some(pool) = self.blueprint_pool(&b.name).await? else {
                continue;
            };
            let rows: Vec<ColdStartRow> = sqlx::query_as(
                "SELECT DISTINCT ON (task_id) task_id, blueprint_name, workflow_name, status, \
                        (SELECT s2.step_name FROM metamach_step_meta s2 \
                         WHERE s2.task_id = m.task_id AND s2.status = 'COMPLETED' \
                         ORDER BY s2.updated_at DESC LIMIT 1) AS last_completed_step \
                 FROM metamach_step_meta m \
                 WHERE m.status IN ('STARTING','RUNNING','SUSPENDED') \
                 ORDER BY task_id, CASE m.status WHEN 'SUSPENDED' THEN 0 \
                                                WHEN 'RUNNING' THEN 1 \
                                                WHEN 'STARTING' THEN 2 \
                                                ELSE 3 END",
            )
            .fetch_all(&pool)
            .await?;
            for r in rows {
                all.push(ColdStartTask {
                    task_id: r.task_id,
                    blueprint: r.blueprint_name,
                    workflow_name: r.workflow_name.unwrap_or_default(),
                    status: r.status,
                    last_completed_step: r.last_completed_step,
                });
            }
        }
        Ok(all)
    }

    /// ARCH §6.2: Janus GC - clear `stdout_tail` for terminal steps not updated in
    /// 3 days (reclaims the large field; the row + status are retained for audit).
    /// Fans out across ACTIVE blueprint DBs.
    pub async fn gc_old_caches(&self) -> Result<i64> {
        let blueprints = self.active_blueprints().await?;
        let mut total = 0i64;
        for b in blueprints {
            let Some(pool) = self.blueprint_pool(&b.name).await? else {
                continue;
            };
            let res = sqlx::query(
                "UPDATE metamach_step_meta SET stdout_tail = NULL \
                 WHERE status IN ('COMPLETED','FAILED') \
                   AND stdout_tail IS NOT NULL \
                   AND updated_at < NOW() - INTERVAL '3 days'",
            )
            .execute(&pool)
            .await?;
            total += res.rows_affected() as i64;
        }
        Ok(total)
    }
}

/// Derive a task-level status from its steps: SUSPENDED beats RUNNING beats
/// STARTING beats terminal. A task with any non-terminal step is itself
/// non-terminal.
fn derive_task_status(steps: &[MetaRow]) -> String {
    let mut has_suspended = false;
    let mut has_running = false;
    let mut has_starting = false;
    for s in steps {
        match s.status.as_str() {
            "SUSPENDED" => has_suspended = true,
            "RUNNING" => has_running = true,
            "STARTING" => has_starting = true,
            _ => {}
        }
    }
    if has_suspended {
        "SUSPENDED".to_string()
    } else if has_running {
        "RUNNING".to_string()
    } else if has_starting {
        "STARTING".to_string()
    } else {
        "COMPLETED".to_string()
    }
}

/// Postgres identifier sanitizer for blueprint DB names (`metamach_blueprint_<name>`).
/// Allows alphanumerics + underscore; replaces everything else with `_`. The
/// caller (Onboard) is expected to have validated the name already.
fn sanitize_ident(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A historical step trace for Offboard smelting (Task 4.2).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StepTrace {
    pub task_id: Uuid,
    pub step_name: String,
    pub status: String,
    pub result_cache: Option<String>,
}

/// A non-terminal task found at cold start + its last `COMPLETED` step (Task 4.1).
#[derive(Debug, Clone)]
pub struct ColdStartTask {
    pub task_id: Uuid,
    pub blueprint: String,
    pub workflow_name: String,
    pub status: String,
    pub last_completed_step: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ColdStartRow {
    task_id: Uuid,
    blueprint_name: String,
    workflow_name: Option<String>,
    status: String,
    last_completed_step: Option<String>,
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
struct MetaRow {
    task_id: Uuid,
    step_name: String,
    status: String,
    exit_code: Option<i32>,
    stdout_tail: Option<String>,
    started_at: Option<DateTime<Utc>>,
    blueprint_name: String,
    workflow_name: Option<String>,
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
        let s = "é".repeat(SIZE_BUDGET); // é = 2 bytes
        let out = truncate_16k(&s);
        assert!(out.len() <= SIZE_BUDGET);
        let _: String = out.parse().unwrap();
    }

    #[test]
    fn derive_status_priority() {
        let mk = |status: &str| MetaRow {
            task_id: Uuid::nil(),
            step_name: "s".into(),
            status: status.into(),
            exit_code: None,
            stdout_tail: None,
            started_at: None,
            blueprint_name: "b".into(),
            workflow_name: None,
        };
        assert_eq!(
            derive_task_status(&[mk("SUSPENDED"), mk("RUNNING")]),
            "SUSPENDED"
        );
        assert_eq!(
            derive_task_status(&[mk("RUNNING"), mk("STARTING")]),
            "RUNNING"
        );
        assert_eq!(derive_task_status(&[mk("STARTING")]), "STARTING");
        assert_eq!(
            derive_task_status(&[mk("COMPLETED"), mk("COMPLETED")]),
            "COMPLETED"
        );
    }

    #[test]
    fn sanitize_ident_replaces_invalid() {
        assert_eq!(sanitize_ident("gatemetric"), "gatemetric");
        assert_eq!(sanitize_ident("gate-metric.9"), "gate_metric_9");
    }
}
