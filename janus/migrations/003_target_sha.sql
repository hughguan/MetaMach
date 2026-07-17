-- MetaMach 1.0 - Git SHA optimistic-lock column (Feature-Spec Contract 3.1, ARCH §6.5).
--
-- Adds absurd_steps.target_sha: the blueprint repo's Git HEAD SHA-1 pinned at
-- Step dispatch time. Lets the Daemon detect and discard stale remote test
-- reports whose dispatch SHA no longer matches the current HEAD - see the
-- Optimistic Locking note in Feature-Spec Contract 3.1.
--
-- The all-zeros sentinel (DEFAULT) marks a non-git blueprint; the lock check is
-- skipped for sentinel rows. NOT NULL with a default so existing rows backfill
-- cleanly. Idempotent (IF NOT EXISTS) so `make db-migrate` is safe to re-run.
--
-- Preparatory: dispatch-time pinning, the remote-report `dispatch_sha` contract,
-- and the auto-reschedule engine arrive with Task 2.4 (herdr-tether). Until then
-- the column is persisted but not enforced.

ALTER TABLE absurd_steps
    ADD COLUMN IF NOT EXISTS target_sha VARCHAR(40) NOT NULL
    DEFAULT '0000000000000000000000000000000000000000';
