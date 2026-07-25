-- 004_env_snapshot.sql
-- Per-blueprint DB (metamach_blueprint_<name>): 0.5.0 environmental snapshot
-- column (ADR-024). Stores the physical environment at step dispatch time so
-- replay/resume can detect changed conditions (different system time, missing
-- USB devices, etc.).
-- Applied on `janus onboard` alongside 002/003.

ALTER TABLE metamach_step_meta ADD COLUMN IF NOT EXISTS env_snapshot JSONB;
