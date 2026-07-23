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

pub mod adapter;
pub mod fallback;
pub mod schema;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock as StdRwLock};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use crate::protocol::{ActiveTask, BlueprintInfo, StepStatus};
// 0.4.0: SIZE_BUDGET / truncate_16k / BUDGET_TAG moved to `protocol` (the leaf
// module) so `WebhookPayload` can share them without an absurd<->protocol cycle
// (`absurd` already imports `protocol`). Re-exported here to keep
// `crate::absurd::{SIZE_BUDGET, truncate_16k}` and `super::truncate_16k`
// (fallback.rs + tests) working unchanged.
pub use crate::protocol::{BUDGET_TAG, SIZE_BUDGET, truncate_16k};
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
                        // M4 Task 4.1: replay any fallback events buffered
                        // during the outage into their per-blueprint overlays.
                        let db2 = Arc::clone(&db);
                        tokio::spawn(async move {
                            match db2.replay_fallback().await {
                                Ok(n) if n > 0 => {
                                    info!(replayed = n, "PG recovery: fallback log replay")
                                }
                                Ok(_) => {}
                                Err(e) => warn!("PG recovery: fallback replay failed: {e}"),
                            }
                        });
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
    /// caller skips that blueprint). Public so the Dispatch handler can build an
    /// `AbsurdPgAdapter` against the dispatched blueprint's pool.
    pub async fn blueprint_pool(&self, name: &str) -> Result<Option<PgPool>> {
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

    /// Per-blueprint overlay migrations (002 `metamach_step_meta` + 003
    /// `hitl_verdict`). `absurd.sql` (the external absurd engine's
    /// task/checkpoint tables) is NOT applied here - it lands with the
    /// absurd-engine integration (M4); the overlay alone backs onboard/offboard/
    /// progress (they read `metamach_step_meta`, not absurd's tables).
    const BLUEPRINT_MIGRATION_002: &str = include_str!("../../migrations/002_blueprint.sql");
    const BLUEPRINT_MIGRATION_003: &str = include_str!("../../migrations/003_hitl_verdict.sql");

    /// Create `metamach_blueprint_<name>` (if absent) + apply the per-blueprint
    /// overlay migrations (002 + 003). Called by `janus onboard` so offboard/
    /// progress can read the blueprint's `metamach_step_meta`. Idempotent: a
    /// re-onboard re-applies the IF NOT EXISTS / ADD COLUMN IF NOT EXISTS
    /// migrations (no-op on an existing DB).
    pub async fn ensure_blueprint_db(&self, name: &str) -> Result<()> {
        let Some(catalog) = self.catalog_pool().await else {
            bail!("catalog DB offline - cannot create blueprint DB for {name}");
        };
        let db_name = format!("metamach_blueprint_{}", sanitize_ident(name));
        // CREATE DATABASE can't run in a transaction block; sqlx auto-commits DDL.
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
                .bind(&db_name)
                .fetch_one(&catalog)
                .await?;
        if !exists {
            // CREATE DATABASE is racy: two concurrent onboards of the same
            // blueprint can both see EXISTS=false, then race on the insert into
            // `pg_database`. The loser gets either SQLSTATE 42P04
            // (duplicate_database, friendly "already exists") or 23505
            // (unique_violation on `pg_database_datname_index`, the raw index
            // hit when both pass the pre-check). In a CREATE DATABASE both mean
            // "the DB now exists" - treat as success and fall through to
            // re-apply the idempotent IF NOT EXISTS migrations. Any other
            // error propagates.
            match sqlx::query(&format!("CREATE DATABASE {db_name}"))
                .execute(&catalog)
                .await
            {
                Ok(_) => info!(%db_name, "created blueprint DB"),
                Err(e) => {
                    let code = e
                        .as_database_error()
                        .and_then(|d| d.code().map(|c| c.into_owned()));
                    let raced = matches!(code.as_deref(), Some("42P04") | Some("23505"));
                    if !raced {
                        return Err(e.into());
                    }
                    info!(%db_name, ?code, "blueprint DB created concurrently; race resolved");
                }
            }
        }
        // Apply the overlay migrations to the blueprint DB (multi-statement,
        // incl. the dollar-quoted trigger function - sqlx::raw_sql runs the
        // whole script via the simple query protocol).
        let opts = self
            .base_opts
            .read()
            .expect("base_opts lock")
            .clone()
            .context("base_opts not set")?
            .database(&db_name);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;
        // Load the vendored absurd.sql (spawn_task/claim_task/...) BEFORE the
        // MetaMach overlays - this IS the "absurdctl init" the 002 header
        // references. Idempotent (create ... if not exists).
        schema::init_absurd_schema(&pool)
            .await
            .with_context(|| format!("init absurd schema in {db_name}"))?;
        sqlx::raw_sql(Self::BLUEPRINT_MIGRATION_002)
            .execute(&pool)
            .await
            .with_context(|| format!("apply 002_blueprint.sql to {db_name}"))?;
        sqlx::raw_sql(Self::BLUEPRINT_MIGRATION_003)
            .execute(&pool)
            .await
            .with_context(|| format!("apply 003_hitl_verdict.sql to {db_name}"))?;
        self.blueprint_pools
            .write()
            .expect("bp lock")
            .insert(name.to_string(), Arc::new(pool));
        Ok(())
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
                        blueprint_name, workflow_name, session_name \
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
                // The current step's tmux session - the daemon's second-pass
                // `has_session` check reads this to set `tmux_alive` (Contract 3.3).
                let session_name = steps
                    .iter()
                    .rev()
                    .find(|s| matches!(s.status.as_str(), "STARTING" | "RUNNING"))
                    .and_then(|s| s.session_name.clone());
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
                    tmux_alive: false, // flipped by the daemon's second pass (§0.5).
                    session_name,
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
    /// outage; replayed into Postgres on recovery - Feature-Spec §4). The
    /// `blueprint_name` routes the event to the correct per-blueprint DB on
    /// replay (Contract 3.8).
    pub fn record_fallback_event(
        &self,
        blueprint: &str,
        task_id: Uuid,
        step_name: &str,
        status: &str,
        result_cache: Option<&str>,
    ) -> Result<()> {
        self.fallback
            .lock()
            .expect("fallback mutex poisoned")
            .record(&task_id, blueprint, step_name, status, result_cache)
    }

    /// Current fallback ring depth (health/observability + tests).
    pub fn fallback_count(&self) -> Result<i64> {
        self.fallback
            .lock()
            .expect("fallback mutex poisoned")
            .count()
    }

    /// M4 Task 4.1 - Log Replay: drain the fallback ring buffer (populated
    /// during the PG outage by `suspend_step` / `record_hitl_resolution`) and
    /// merge each event into the routed per-blueprint `metamach_step_meta`
    /// overlay. Called on PG recovery. The ring is truncated regardless of
    /// per-event outcome (a bad event is warned + dropped, not re-queued) so the
    /// ring can't grow unbounded across recoveries. Returns the count merged.
    pub async fn replay_fallback(&self) -> Result<usize> {
        let events = {
            let fb = self.fallback.lock().expect("fallback mutex poisoned");
            fb.drain()?
        };
        if events.is_empty() {
            return Ok(0);
        }
        let mut replayed = 0usize;
        for ev in events {
            let task_id = match Uuid::parse_str(&ev.task_id) {
                Ok(u) => u,
                Err(e) => {
                    warn!(task_id = %ev.task_id, "replay: bad task_id ({e}); dropping");
                    continue;
                }
            };
            // Ensure the blueprint DB + overlay exist (idempotent); the event
            // may target a blueprint whose pool was lost during the outage.
            if let Err(e) = self.ensure_blueprint_db(&ev.blueprint_name).await {
                warn!(blueprint = %ev.blueprint_name, "replay: ensure_blueprint_db failed ({e}); dropping");
                continue;
            }
            let Some(pool) = self.blueprint_pool(&ev.blueprint_name).await? else {
                warn!(blueprint = %ev.blueprint_name, "replay: no blueprint pool; dropping");
                continue;
            };
            if let Err(e) = sqlx::query(
                "INSERT INTO metamach_step_meta (task_id, step_name, blueprint_name, status) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (task_id, step_name) DO UPDATE \
                 SET status = EXCLUDED.status, updated_at = NOW()",
            )
            .bind(task_id)
            .bind(&ev.step_name)
            .bind(&ev.blueprint_name)
            .bind(&ev.status)
            .execute(&pool)
            .await
            {
                warn!(task_id = %task_id, "replay: overlay upsert failed ({e}); dropping");
                continue;
            }
            replayed += 1;
        }
        if replayed > 0 {
            info!(replayed, "fallback log replay merged events into overlays");
        }
        Ok(replayed)
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
            self.record_fallback_event(blueprint, task_id, step_name, "SUSPENDED", None)
        }
    }

    /// 0.4.0: record a resolved HITL verdict on the suspended step's overlay row
    /// (Contract 4.3c). `verdict` is `APPROVED` / `REJECTED` / `OVERRIDDEN` -
    /// written to the `hitl_verdict` column (migration 003) so the M4 resume loop
    /// can read it. Falls back to a fallback event if the blueprint DB (or the
    /// column, pre-migration) is unavailable, so a recording failure never panics
    /// the gateway's verdict thread.
    pub async fn record_hitl_resolution(
        &self,
        blueprint: &str,
        task_id: Uuid,
        step_name: &str,
        verdict: &str,
    ) -> Result<()> {
        if let Some(pool) = self.blueprint_pool(blueprint).await? {
            sqlx::query(
                "UPDATE metamach_step_meta SET hitl_verdict = $3 \
                 WHERE task_id = $1 AND step_name = $2",
            )
            .bind(task_id)
            .bind(step_name)
            .bind(verdict)
            .execute(&pool)
            .await?;
            Ok(())
        } else {
            self.record_fallback_event(blueprint, task_id, step_name, verdict, None)
        }
    }

    /// M4 Task 4.1 Phase 0b - engine write path: upsert a step's `STARTING`
    /// row (pins `target_sha` + records the tmux `session_name`). Idempotent
    /// (`ON CONFLICT` resets to `STARTING` for a re-dispatch of the same
    /// task_id+step). Degrades to a fallback event if the blueprint DB is
    /// unreachable (consistent with [`suspend_step`]).
    pub async fn upsert_step_start(
        &self,
        blueprint: &str,
        task_id: Uuid,
        step_name: &str,
        workflow_name: &str,
        target_sha: &str,
        session_name: &str,
    ) -> Result<()> {
        if let Some(pool) = self.blueprint_pool(blueprint).await? {
            sqlx::query(
                "INSERT INTO metamach_step_meta \
                    (task_id, step_name, blueprint_name, workflow_name, status, target_sha, session_name) \
                 VALUES ($1, $2, $3, $4, 'STARTING', $5, $6) \
                 ON CONFLICT (task_id, step_name) DO UPDATE \
                 SET status = 'STARTING', workflow_name = EXCLUDED.workflow_name, \
                     target_sha = EXCLUDED.target_sha, session_name = EXCLUDED.session_name, \
                     exit_code = NULL, started_at = NULL, updated_at = NOW()",
            )
            .bind(task_id)
            .bind(step_name)
            .bind(blueprint)
            .bind(workflow_name)
            .bind(target_sha)
            .bind(session_name)
            .execute(&pool)
            .await?;
            Ok(())
        } else {
            self.record_fallback_event(blueprint, task_id, step_name, "STARTING", None)
        }
    }

    /// Phase 0b: flip a step to `RUNNING` and stamp `started_at` (Contract 3.3
    /// - `started_at` is set on the STARTING->RUNNING transition).
    pub async fn set_step_running(
        &self,
        blueprint: &str,
        task_id: Uuid,
        step_name: &str,
    ) -> Result<()> {
        if let Some(pool) = self.blueprint_pool(blueprint).await? {
            sqlx::query(
                "UPDATE metamach_step_meta SET status = 'RUNNING', started_at = NOW() \
                 WHERE task_id = $1 AND step_name = $2",
            )
            .bind(task_id)
            .bind(step_name)
            .execute(&pool)
            .await?;
            Ok(())
        } else {
            self.record_fallback_event(blueprint, task_id, step_name, "RUNNING", None)
        }
    }

    /// Phase 0b: finalize a step (`COMPLETED`/`FAILED`/`SUSPENDED`) with its
    /// exit code + 16 KiB-truncated stdout tail. `stdout_tail` is truncated by
    /// the caller via [`truncate_16k`] before this write (the authoritative
    /// budget enforcement point).
    pub async fn finalize_step(
        &self,
        blueprint: &str,
        task_id: Uuid,
        step_name: &str,
        status: &str,
        exit_code: Option<i32>,
        stdout_tail: Option<&str>,
    ) -> Result<()> {
        if let Some(pool) = self.blueprint_pool(blueprint).await? {
            sqlx::query(
                "UPDATE metamach_step_meta SET status = $3, exit_code = $4, stdout_tail = $5 \
                 WHERE task_id = $1 AND step_name = $2",
            )
            .bind(task_id)
            .bind(step_name)
            .bind(status)
            .bind(exit_code)
            .bind(stdout_tail)
            .execute(&pool)
            .await?;
            Ok(())
        } else {
            self.record_fallback_event(blueprint, task_id, step_name, status, stdout_tail)
        }
    }

    /// Phase 0b: re-read a step's `status` after the tmux pane dies. The engine
    /// uses this to detect that the daemon's `GuardCheck` handler flipped the
    /// step to `SUSPENDED` (HITL `require_approval`/`blacklist`) - in which case
    /// the task stops suspended regardless of the pane exit code. Returns `None`
    /// if the blueprint DB is unreachable or the row is absent (treated as
    /// "not suspended" -> the exit code decides).
    pub async fn step_status(
        &self,
        blueprint: &str,
        task_id: Uuid,
        step_name: &str,
    ) -> Result<Option<String>> {
        let Some(pool) = self.blueprint_pool(blueprint).await? else {
            return Ok(None);
        };
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT status FROM metamach_step_meta WHERE task_id = $1 AND step_name = $2",
        )
        .bind(task_id)
        .bind(step_name)
        .fetch_optional(&pool)
        .await?;
        Ok(row.map(|(s,)| s))
    }

    /// HITL: read a step's recorded verdict (`APPROVED`/`REJECTED`/`OVERRIDDEN`)
    /// or `None`. Used by the `GuardCheck` handler to ALLOW an already-approved
    /// step on re-run, so the engine's HITL resume doesn't re-block infinitely.
    pub async fn hitl_verdict(
        &self,
        blueprint: &str,
        task_id: Uuid,
        step_name: &str,
    ) -> Result<Option<String>> {
        let Some(pool) = self.blueprint_pool(blueprint).await? else {
            return Ok(None);
        };
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT hitl_verdict FROM metamach_step_meta WHERE task_id = $1 AND step_name = $2",
        )
        .bind(task_id)
        .bind(step_name)
        .fetch_optional(&pool)
        .await?;
        Ok(row.and_then(|(v,)| v))
    }

    /// Read a step's `workflow_name` (denormalized on the overlay) so the HITL
    /// verdict sink can build the absurd queue name (`<blueprint>_<workflow>`)
    /// for `emit_event`.
    pub async fn step_workflow_name(
        &self,
        blueprint: &str,
        task_id: Uuid,
        step_name: &str,
    ) -> Result<Option<String>> {
        let Some(pool) = self.blueprint_pool(blueprint).await? else {
            return Ok(None);
        };
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT workflow_name FROM metamach_step_meta WHERE task_id = $1 AND step_name = $2",
        )
        .bind(task_id)
        .bind(step_name)
        .fetch_optional(&pool)
        .await?;
        Ok(row.and_then(|(w,)| w))
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
    session_name: Option<String>,
}

// 0.4.0: `truncate_16k` moved to `protocol` and re-exported above; the
// authoritative 16 KiB enforcement point is still `AbsurdDb` (Feature-Spec §4).

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
            session_name: None,
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

    /// Mirror the daemon's `pg_connect_options` for tests: TCP via DATABASE_URL
    /// (CI) or the Unix socket via METAMACH_PG_SOCKET_DIR (local `make db-init`,
    /// where the `postgres://...?host=` URL form is rejected by `from_str`).
    fn pg_opts_for_test() -> PgConnectOptions {
        use std::str::FromStr;
        if let Ok(socket) = std::env::var("METAMACH_PG_SOCKET_DIR") {
            return PgConnectOptions::new()
                .socket(socket)
                .username("metamach_admin")
                .database("metamach_db");
        }
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        PgConnectOptions::from_str(&url).expect("parse DATABASE_URL")
    }

    /// M4 Task 4.1 - Log Replay integration (PG-gated). Buffers SUSPENDED
    /// transitions in the fallback ring (as `suspend_step` does during an
    /// outage), then verifies `replay_fallback` merges them into the routed
    /// per-blueprint `metamach_step_meta` overlay and drains the ring.
    #[tokio::test]
    #[ignore = "requires PostgreSQL"]
    async fn replay_fallback_merges_events_into_overlay() {
        let bp = "replaytest";
        let tmp = tempfile::tempdir().expect("tmp");
        let db = std::sync::Arc::new(
            AbsurdDb::open_degraded(&tmp.path().join("fallback.db")).expect("open"),
        );
        db.spawn_connect(pg_opts_for_test());
        let start = std::time::Instant::now();
        while !db.pg_online().await && start.elapsed() < std::time::Duration::from_secs(12) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(db.pg_online().await, "PG did not come online");

        db.ensure_blueprint_db(bp).await.expect("ensure bp db");

        // Buffer two SUSPENDED transitions (as suspend_step would during outage).
        let t1 = Uuid::new_v4();
        let t2 = Uuid::new_v4();
        db.record_fallback_event(bp, t1, "scout", "SUSPENDED", None)
            .expect("record");
        db.record_fallback_event(bp, t2, "code", "SUSPENDED", None)
            .expect("record");
        assert_eq!(db.fallback_count().unwrap(), 2, "two events buffered");

        // Replay merges + drains.
        let n = db.replay_fallback().await.expect("replay");
        assert_eq!(n, 2, "both events replayed");
        assert_eq!(db.fallback_count().unwrap(), 0, "ring drained");

        // Overlay rows landed with the replayed status.
        let pool = db
            .blueprint_pool(bp)
            .await
            .expect("pool lookup")
            .expect("some pool");
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT status FROM metamach_step_meta WHERE task_id = $1 AND step_name = $2",
        )
        .bind(t1)
        .bind("scout")
        .fetch_optional(&pool)
        .await
        .expect("query t1");
        assert_eq!(row.expect("t1 row").0, "SUSPENDED");
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT status FROM metamach_step_meta WHERE task_id = $1 AND step_name = $2",
        )
        .bind(t2)
        .bind("code")
        .fetch_optional(&pool)
        .await
        .expect("query t2");
        assert_eq!(row.expect("t2 row").0, "SUSPENDED");

        // Replaying again is a no-op (ring empty).
        assert_eq!(db.replay_fallback().await.unwrap(), 0);
    }
}
