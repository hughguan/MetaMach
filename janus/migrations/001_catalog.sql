-- 001_catalog.sql
-- Global catalog DB (metamach_db): blueprint registry + audit log.
-- Applied at `make bootstrap` / first janus-daemon startup to the catalog DB.
-- This DB is MetaMach-owned; the absurd engine is NOT installed here (absurd
-- lives in each per-blueprint DB - see 002_blueprint.sql / Contract 3.1b).
-- See docs/Feature-Spec.md Contract 3.1.

-- Blueprint tenant registry (Onboard writes / Offboard sets OFFBOARDED).
CREATE TABLE IF NOT EXISTS blueprints (
    id               SERIAL PRIMARY KEY,
    name             VARCHAR(100) UNIQUE NOT NULL,
    status           VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',  -- ACTIVE | OFFBOARDED
    default_workflow VARCHAR(100) NOT NULL,
    config           JSONB,        -- janus.toml verbatim
    openwiki_scope   JSONB,        -- [openwiki].scope index range
    remote_host      VARCHAR(100), -- [remote].host (NULL = local-only)
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    onboarded_at     TIMESTAMPTZ,  -- most recent Onboard time
    offboarded_at    TIMESTAMPTZ   -- most recent Offboard time
);

-- Global audit log: full traces for every offboarded task (never pruned).
-- task_id is UUID to match absurd.spawn_task() output (M0.5 spike F1).
CREATE TABLE IF NOT EXISTS absurd_audit_log (
    id              SERIAL PRIMARY KEY,
    task_id         UUID NOT NULL,
    blueprint_name  VARCHAR(100) NOT NULL,
    workflow_name   VARCHAR(100) NOT NULL,
    step_count      INTEGER NOT NULL,
    elapsed_seconds DOUBLE PRECISION,
    offboarded_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    trace_summary   JSONB         -- full trace metadata: step names, statuses, timestamps
);
CREATE INDEX IF NOT EXISTS idx_audit_blueprint ON absurd_audit_log(blueprint_name);
CREATE INDEX IF NOT EXISTS idx_audit_task      ON absurd_audit_log(task_id);
