-- 003_hitl_verdict.sql
-- Per-blueprint DB (metamach_blueprint_<name>): 0.4.0 HITL verdict column.
-- Applied on `janus onboard` alongside 002_blueprint.sql (same per-blueprint
-- migration path). Adds the durable record the gateway's verdict thread writes
-- (Contract 4.3c) and the M4 resume loop reads.
--
-- `hitl_verdict` is NULL until a Teams/TUI callback resolves a SUSPENDED step;
-- then it is one of APPROVED / REJECTED / OVERRIDDEN. It is distinct from
-- `status` (the absurd task-state mirror) - a step can be SUSPENDED with
-- hitl_verdict = APPROVED pending the M4 re-dispatch.

ALTER TABLE metamach_step_meta ADD COLUMN IF NOT EXISTS hitl_verdict VARCHAR(32);
