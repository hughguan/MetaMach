-- MetaMach 1.0 - Absurd Postgres initial schema (Feature-Spec Contract 3.1)
--
-- Run automatically on first container init via /docker-entrypoint-initdb.d,
-- and re-runnable via `make db-migrate` (all statements are idempotent).
--
-- M1 scope: the three core tables only (blueprints, absurd_tasks, absurd_steps).
--   - fallback_events (Contract 3.8, SQLite) lands in M2+.
--   - melt_blueprint_data() stored proc (Feature-Spec §2.5) lands in M4 Offboard.
--   - The 16KiB result_cache budget is enforced in the Daemon's absurd module
--     (Feature-Spec §4 fault matrix), NOT at the DB layer, so no size CHECK here.

-- Blueprint tenant registry (Onboard writes / Offboard sets OFFBOARDED)
CREATE TABLE IF NOT EXISTS blueprints (
    id              SERIAL PRIMARY KEY,
    name            VARCHAR(100) UNIQUE NOT NULL,
    status          VARCHAR(20)    NOT NULL DEFAULT 'ACTIVE',  -- ACTIVE | OFFBOARDED
    default_workflow VARCHAR(100)  NOT NULL,
    config          JSONB,                                    -- janus.toml verbatim
    openwiki_scope  JSONB,                                    -- [openwiki].scope index range
    remote_host     VARCHAR(100),                             -- [remote].host (NULL = local-only)
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    onboarded_at    TIMESTAMPTZ,                              -- most recent Onboard time
    offboarded_at   TIMESTAMPTZ,                              -- most recent Offboard time
    CONSTRAINT blueprints_status_chk
        CHECK (status IN ('ACTIVE', 'OFFBOARDED'))
);

-- Workflow task table (one dispatch = one Task row)
CREATE TABLE IF NOT EXISTS absurd_tasks (
    id           SERIAL PRIMARY KEY,
    blueprint_id INTEGER      NOT NULL REFERENCES blueprints(id) ON DELETE CASCADE,
    workflow_name VARCHAR(100) NOT NULL,
    status       VARCHAR(20)  NOT NULL,  -- PENDING | STARTING | RUNNING | COMPLETED | SUSPENDED | FAILED
    started_at   TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT tasks_status_chk
        CHECK (status IN ('PENDING', 'STARTING', 'RUNNING', 'COMPLETED', 'SUSPENDED', 'FAILED'))
);

-- Step checkpoint table (with Size Budget enforcement at the Daemon layer)
CREATE TABLE IF NOT EXISTS absurd_steps (
    task_id      INTEGER REFERENCES absurd_tasks(id) ON DELETE CASCADE,
    step_name    VARCHAR(100) NOT NULL,
    status       VARCHAR(20)  NOT NULL,  -- PENDING | STARTING | RUNNING | COMPLETED | SUSPENDED | FAILED
    result_cache JSONB,                  -- strictly capped at 16KB by the Daemon before INSERT
    updated_at   TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (task_id, step_name),
    CONSTRAINT steps_status_chk
        CHECK (status IN ('PENDING', 'STARTING', 'RUNNING', 'COMPLETED', 'SUSPENDED', 'FAILED'))
);

-- Backs the Contract 3.3 progress query (filter non-terminal tasks per blueprint)
CREATE INDEX IF NOT EXISTS idx_tasks_blueprint ON absurd_tasks(blueprint_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status    ON absurd_tasks(status);
