-- 002_blueprint.sql
-- Per-blueprint DB (metamach_blueprint_<name>): MetaMach step-meta overlay.
-- Applied on `janus onboard` AFTER `absurdctl init` has installed absurd v0.4.0
-- (sql/absurd.sql) into the `absurd` schema of this DB.
--
-- Absurd owns the task / checkpoint / event tables and the durable-execution
-- functions (spawn_task, claim_task, complete_run, fail_run,
-- set_task_checkpoint_state = result_cache, get_task_checkpoint_state,
-- await_event / emit_event = HITL SUSPENDED / resume, cleanup_tasks = Janus GC,
-- create_queue / drop_queue). MetaMach does NOT redefine those tables - parallel
-- absurd_tasks/absurd_steps tables would conflict with absurd's functions
-- (M0.5 spike F1). This migration adds only the thin MetaMach overlay carrying
-- fields absurd has no concept of (target_sha, exit_code, stdout_tail,
-- started_at). status + result_cache live in absurd's checkpoint state JSONB.
-- See docs/Feature-Spec.md Contract 3.1b.
--
-- One absurd queue per blueprint/workflow is created by `janus onboard` via
-- absurd.create_queue('<blueprint>.<workflow>'); not done here so this migration
-- stays idempotent and workflow-agnostic.

-- MetaMach step-meta overlay, keyed by absurd's UUID task_id + step_name.
-- No hard FK to absurd's dynamic per-queue tables (partitioned, dynamically
-- named); the Daemon guarantees consistency at the application layer.
CREATE TABLE IF NOT EXISTS metamach_step_meta (
    task_id         UUID NOT NULL,                       -- matches absurd.spawn_task().task_id
    step_name       VARCHAR(100) NOT NULL,
    blueprint_name  VARCHAR(100) NOT NULL,               -- denormalized; no cross-DB FK
    target_sha      VARCHAR(64) NOT NULL DEFAULT '0000000000000000000000000000000000000000',
                                                          -- Optimistic lock: Git HEAD pinned at dispatch.
                                                          -- All-zeros sentinel = non-git blueprint (lock skipped).
                                                          -- VARCHAR(64) supports SHA-256.
    exit_code       INTEGER,                             -- NULL until step completes
    stdout_tail     TEXT,                                -- most recent ~1KB terminal snapshot
    started_at      TIMESTAMPTZ,                         -- when the step transitioned to RUNNING
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (task_id, step_name)
);
CREATE INDEX IF NOT EXISTS idx_step_meta_blueprint ON metamach_step_meta(blueprint_name);

-- Auto-touch updated_at on row change.
CREATE OR REPLACE FUNCTION metamach_touch_updated_at() RETURNS trigger AS $$
BEGIN
    NEW.updated_at := NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS trg_step_meta_touch ON metamach_step_meta;
CREATE TRIGGER trg_step_meta_touch BEFORE UPDATE ON metamach_step_meta
    FOR EACH ROW EXECUTE FUNCTION metamach_touch_updated_at();
