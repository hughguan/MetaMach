-- MetaMach M4 Task 4.2 - Offboard anti-bloat pruning (Feature-Spec §2.5.3).
--
-- melt_blueprint_data(name) physically DELETEs (not NULL-ifies) the large
-- result_cache JSON for a blueprint's steps, so autovacuum / VACUUM FULL can
-- reclaim TOAST space (NULL-ification would not). One audit-stats row is written
-- per affected task into absurd_audit_log. Idempotent (CREATE OR REPLACE).

CREATE TABLE IF NOT EXISTS absurd_audit_log (
    id          SERIAL PRIMARY KEY,
    blueprint   VARCHAR(100) NOT NULL,
    task_id     INTEGER,
    elapsed_sec INTEGER,
    melted_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE OR REPLACE FUNCTION melt_blueprint_data(p_name VARCHAR)
RETURNS INTEGER AS $$
DECLARE
    deleted INTEGER;
BEGIN
    -- Audit the tasks whose step caches are about to be melted (one row each).
    INSERT INTO absurd_audit_log (blueprint, task_id)
        SELECT DISTINCT p_name, t.id
        FROM absurd_steps s
        JOIN absurd_tasks t ON t.id = s.task_id
        JOIN blueprints b ON b.id = t.blueprint_id
        WHERE b.name = p_name
          AND s.result_cache IS NOT NULL;

    -- Physically DELETE the large JSON cells.
    DELETE FROM absurd_steps s
        USING absurd_tasks t, blueprints b
        WHERE s.task_id = t.id
          AND t.blueprint_id = b.id
          AND b.name = p_name
          AND s.result_cache IS NOT NULL;
    GET DIAGNOSTICS deleted = ROW_COUNT;

    RETURN deleted;
END;
$$ LANGUAGE plpgsql;
