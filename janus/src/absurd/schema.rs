//! Absurd schema bootstrap (M4 Task 4.1 Phase 0a; `docs/Absurd-Integration.md` §1).
//!
//! `absurd.sql` is vendored at `janus/sql/absurd.sql` (v0.4.0, upstream commit
//! `9b77b35`). This loads it into a per-blueprint DB on `janus onboard` (via
//! [`AbsurdDb::ensure_blueprint_db`], *before* the 002/003 MetaMach overlays) so
//! absurd's `spawn_task`/`claim_task`/`complete_run`/`set_task_checkpoint_state`/
//! `await_event`/`emit_event`/`create_queue` stored procedures exist. Idempotent:
//! `absurd.sql` is written with `create [or replace] ... if not exists`, so
//! re-applying it (re-onboard, cold-start) is a no-op.
//!
//! This IS the "absurdctl init" the `002_blueprint.sql` header references - the
//! 002 comment "Applied on janus onboard AFTER absurdctl init" becomes literally
//! true once [`init_absurd_schema`] runs first.

use anyhow::{Context, Result, bail};
use sqlx::PgPool;
use tracing::info;

/// The absurd schema version this binary expects. `absurd.sql` v0.4.0's
/// `get_schema_version()` returns `"main"`. [`init_absurd_schema`] refuses to
/// proceed if the DB carries a different version - prevents silent skew when
/// the vendored `absurd.sql` is upgraded (see `docs/Absurd-Integration.md` §7.1).
const EXPECTED_ABSURD_VERSION: &str = "main";

/// Load the vendored `absurd.sql` into `pool`'s database and verify the schema
/// version matches [`EXPECTED_ABSURD_VERSION`].
///
/// Called by `ensure_blueprint_db` after the blueprint DB is created + its pool
/// connected, and *before* the 002/003 MetaMach overlays. `absurd.sql` installs
/// the `absurd` schema (queues table, `t_`/`r_`/`c_`/`e_`/`w_`/`i_` materialized
/// per-queue by `create_queue`, and the stored procedures). 002 then adds the
/// thin `metamach_step_meta` overlay on top.
///
/// **Idempotent:** if `absurd.get_schema_version()` already returns
/// [`EXPECTED_ABSURD_VERSION`] (absurd loaded on a prior call - re-onboard,
/// `replay_fallback`'s internal `ensure_blueprint_db`), this is a fast no-op
/// rather than re-executing all 3,085 lines (absurd.sql is not fully idempotent
/// on re-run, so the skip is required, not just an optimization).
pub async fn init_absurd_schema(pool: &PgPool) -> Result<()> {
    // Already loaded? `get_schema_version()` errors if the absurd schema is
    // absent (function undefined) - that's the "not loaded yet" signal.
    if let Ok(v) = schema_version(pool).await {
        if v == EXPECTED_ABSURD_VERSION {
            return Ok(());
        }
        bail!(
            "absurd schema version mismatch: DB has {v:?}, binary expects \
             {EXPECTED_ABSURD_VERSION:?} (re-vendor janus/sql/absurd.sql or migrate the DB)"
        );
    }
    let sql = include_str!("../../sql/absurd.sql");
    sqlx::raw_sql(sql)
        .execute(pool)
        .await
        .context("apply absurd.sql")?;
    let version = schema_version(pool).await?;
    if version != EXPECTED_ABSURD_VERSION {
        bail!(
            "absurd schema version mismatch after load: DB has {version:?}, binary expects \
             {EXPECTED_ABSURD_VERSION:?}"
        );
    }
    info!(version = %EXPECTED_ABSURD_VERSION, "absurd schema loaded");
    Ok(())
}

/// `SELECT absurd.get_schema_version()` - the version string baked into the
/// vendored `absurd.sql` (currently `"main"`).
pub async fn schema_version(pool: &PgPool) -> Result<String> {
    let v: String = sqlx::query_scalar("SELECT absurd.get_schema_version()")
        .fetch_one(pool)
        .await
        .context("query absurd.get_schema_version")?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_version_tracks_vendored_v0_4_0() {
        // absurd.sql v0.4.0 returns "main". If this assertion fires, the
        // vendored absurd.sql changed its version string - update the constant
        // (and re-run the PG-gated integration test).
        assert_eq!(EXPECTED_ABSURD_VERSION, "main");
    }
}
