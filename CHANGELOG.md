# Changelog

All notable changes to MetaMach are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.2] - 2026-07-23

M4 Task 4.1 complete — workflow engine + cold-start resume + HITL resume loop +
cross-host SSH transport + target_sha enforcement.

### Added
- **Phase 0a absurd adapter** (`init_absurd_schema` + `DurableEngine` trait +
  `AbsurdPgAdapter`): vendored `absurd.sql` loaded on `janus onboard`, exposing
  `spawn_task`/`claim_task`/`complete_run`/`fail_run`/`set_checkpoint`/
  `await_event`/`emit_event`/`extend_claim` behind a Rust trait.
- **Phase 0b workflow engine** (`workflow.rs`): `run_workflow` drives a
  blueprint's steps via absurd pull-mode (`create_queue`/`spawn_task`/
  `claim_task`), creating tmux sessions under `janush` as SHELL (Tool Guard
  interception on every Agent command), `pane_dead_status` exit-code capture,
  lease extension every 10s for long steps, `session_name` on
  `metamach_step_meta` + `tmux_alive` wired in the daemon Progress handler.
- **`Request::Dispatch`** (Contract 3.11): dispatches a blueprint's workflow;
  returns the absurd-minted `task_id` immediately; the step loop runs detached.
- **Phase 1 cold-start resume**: `coldstart::reconcile` no longer just logs —
  it validates the recipe, builds the engine + backend, and spawns
  `run_workflow` detached for each `STARTING`/`RUNNING` task from its last
  `COMPLETED` checkpoint.
- **Retry-claim loop**: `max_attempts: 3` on absurd's `spawn_task`; after
  `fail_run` the engine re-claims the retry run absurd schedules and resumes
  from the last checkpoint (transient failures retry within-session).
- **HITL resume loop** (M4 §3.3): `AwaitOutcome { Suspended, Resolved }`;
  `resume_point()` reads checkpoint state; `hitl_await_and_rerun()` calls
  `await_event` — on APPROVED re-runs the step (GuardCheck ALLOWs via
  `hitl_verdict=APPROVED`), on REJECTED `fail_run`.
- **Phase 2 cross-host SSH transport** (ADR-017): `TmuxFactory` produces
  local or remote `TmuxBackend` per step host; SSH `-R` reverse tunnel maps
  the local `janus.sock` to `/tmp/mm-<host>.sock` on the remote host so
  remote `janush` reaches the local daemon. No separate backend type — same
  `TmuxBackend` with `ssh <host>` prefix.
- **Task 4.4 `target_sha` enforcement**: after a step exits successfully,
  the engine compares the pinned `target_sha` against current `git HEAD`;
  mismatch → step marked `SUSPENDED` with `CONCURRENCY_RACE_ALERT` →
  `fail_run` → absurd retry re-runs against the new HEAD.

### Changed
- `run_workflow` generic over `F: BackendFactory` (not `B: DurableBackend`);
  per-step backend resolved via `factory.get(host)`.
- `ValidatedRecipe` carries `remote_user` (from `[remote] user`).
- `step_command` takes `host: Option<&str>` — sets `JANUS_SOCK_PATH` for
  remote sessions (the reverse-tunnel socket).

## [0.4.1] - 2026-07-20

Patch release: M5 release-gate hardening + test coverage. The 0.4.0 `--ignored`
CI step was silently failing (the `postgres:16` service never had `001_catalog.sql`
applied); 0.4.1 makes the PG-gated tests a real, passing, blocking gate and
fills the remaining automation-feasible test gaps.

### Fixed
- **PG-gated CI tests were silently failing**: CI's `postgres:16` service created
  an empty `metamach_db` with no catalog schema, so every onboard/offboard test
  errored on `relation "blueprints" does not exist` - masked by
  `continue-on-error`. Added an "Apply catalog migration" step (`001_catalog.sql`)
  and made the `--ignored` step a **blocking** gate.
- **`CREATE DATABASE` race** in `absurd::ensure_blueprint_db`: two concurrent
  onboards of the same blueprint raced on the `pg_database` insert (loser got
  SQLSTATE `23505`, not the friendly `42P04`). Now catches both codes
  (idempotent); any other error propagates.
- **Stale 0.4.0 docs**: `CLAUDE.md` claimed 0.4.0 was unimplemented (no
  `gateway/`/`cognitive/` modules) and M5 was unstarted - all false.
  `ARCH-0.4.0.md` status `Proposal / Under Review` -> `Implemented`.
  `tool_guard::webhook.rs` module doc self-contradiction resolved.

### Added
- **Blueprint name validation** (UTC-05-04b / Feature-Spec §2.5): `recipe::validate`
  rejects names that are empty, >60 chars, or contain chars outside
  `[a-zA-Z0-9_]` (before any file/DB access).
- **UTC-05-05** (experience inheritance): a prior Offboard's
  `production_report.md` is inherited as `## Previous Incidents` on re-onboard.
- **UTC-05-03** (git experience inheritance): Offboard best-effort `git commit`s
  `production_report.md`; covers the previously-untested `git_commit_report` path.
- **UTC-06-03** (`janus status` CLI): Contract 3.3 `--json`/text snapshot.

### Changed
- PG-gated integration tests use **uniquely-named blueprints** + isolated temp
  repo roots, so they run in parallel without racing on `CREATE DATABASE`,
  the migration trigger, or the shared catalog. Offboard writes land in the
  temp repo (no `production_report.md` pollution of the real repo).
- Degraded-mode tests strip both `DATABASE_URL` and `METAMACH_PG_SOCKET_DIR`
  (hermetic against a local `make db-init`).

## [0.4.0] - 2026-07-19

The 0.4.0 gateway & ecosystem delta (`docs/ARCH-0.4.0.md`) - implementation +
spec, plus the M5 integration test suite.

### Added
- **`janus::gateway` HITL Gateway** (Contracts 4.3a-c): payload-complete
  dispatch, non-blocking verdict thread, loopback HTTP callback listener
  (`127.0.0.1:8443`) with HMAC-SHA256 validation (200/401/409/410), Microsoft
  Teams Adaptive Card adapter. One unified HITL deadline
  (`JANUS_HITL_TIMEOUT_SECS`): a late callback gets `410 Gone`; the awaiter gets
  `Err(Timeout)` -> BLOCK.
- **`janus::cognitive` Cognitive Provider SPI** (Contracts 4.1/4.2): opt-in
  per-blueprint `[cognitive]` config, `NoopProvider` (fail-open default),
  `McpProvider` (`codebase-memory-mcp` over stdio JSON-RPC; 2s advisory
  `validate_command` timeout; `extract_knowledge` offboard supplement).
- `Response::GuardVerdict.cognitive_context` (Contract 4.1) - a cognitive
  provider's BLOCK reason, carried back to `janush`.
- `hitl_verdict` column on `metamach_step_meta` (migration `003_hitl_verdict`) -
  the durable record the M4 resume loop will read.
- `docs/Deployment-Spec.md` §7 (Gateway Ingress): loopback listener, tunnel /
  reverse-proxy model, HMAC provisioning, verification procedure.
- **M5 integration tests** (Tasks 5.1/5.2): `janush`<->daemon UDS contract
  round-trip (Contract 3.2/3.4), singleton PID-lock refusal (UTC-01-01),
  degraded-mode resilience (UTC-08-01), UDS protocol robustness (UTC-02-04),
  `janush` proxy-shell interception (UTC-02-02), HITL gateway HTTP ingress
  (UTC-10-02/04/08).
- `rust-toolchain.toml` pinning Rust 1.88 (CI floor) - local verification now
  matches CI exactly.

### Changed
- `WebhookPayload` relocated to `protocol` (the leaf module) and enriched with
  `blueprint`, `step`, `stdout_tail`, `expires_at`; `scene` kept as a legacy
  alias. Breaks the would-be `absurd <-> protocol` cycle.
- `tool_guard::webhook` dispatch now delegates to `gateway::dispatch`
  (non-blocking); the gateway's verdict thread records the resolution via a
  `VerdictSink` (the daemon wires a DB-backed sink; the gateway stays
  payload-complete - no DB).
- `lifecycle::offboard` appends the cognitive provider's `extract_knowledge`
  artifact to the LLM-smelt `production_report.md` (supplement, not replacement).

### Fixed
- Pending-verdict leak + false-200 on `await_verdict` timeout (P1): dead entries
  removed on timeout; `tx.send` result checked so a late callback whose awaiter
  already timed out returns `410 Gone`, not a false `200`.
- `JANUS_HITL_TIMEOUT_SECS` validation drift (P2): centralized
  `protocol::hitl_timeout_secs()` with the `>0` guard, shared by
  `WebhookPayload::build` and the gateway.
- CI `clippy::uninlined_format_args` failure (local 1.95 vs CI 1.88 drift),
  resolved by inlining format args + pinning the toolchain.

## [0.3.0] - de-containerized, Multi-DB, internalized tmux

The implemented baseline: M0-M4 + the 0.3.0 consensus (native PG, F1 multi-DB,
`janus::tmux` internalization).

### Added
- **M0:** Herdr 0.7.3 plugin contract validated (`docs/herdr-v1-contract.md`).
- **M1:** native Absurd Postgres (Unix socket, no Docker), `001_catalog` +
  `002_blueprint` migrations, `herdr-janus` shadow shell, `configs/`.
- **M2:** `janus-daemon` resident brain, twin-process UDS, `progress`
  primitive, `janus::tmux` (internalized from `herdr-tether`), F1 multi-DB
  fan-out (catalog DB + one DB per blueprint).
- **M3:** `janush` proxy shell + Tool Guard rule engine (Contract 3.4
  ALLOW/BLOCK/REWRITE; hot-reload `agents.toml`).
- **M4:** Onboard/Offboard lifecycle, LLM-smelt `production_report.md`,
  cold-start self-heal, `target_sha` optimistic locking, Janus GC.
- CI pipeline (`.github/workflows/ci.yml`): `fmt` + `clippy -D warnings` +
  `test --workspace`, native PG service, SSH-gated tests `#[ignore]`.
- `Makefile`: `bootstrap` (prereq -> symlinks -> compile -> db-init),
  `db-backup` / `db-restore` / `health` / `uninstall`.

### Changed
- De-containerized: native PostgreSQL replaces Docker Compose (ARCH-0.3.0
  consensus).
- `janus::tether` -> `janus::tmux`; `janus-sh` -> `janush`; tmux socket
  `metamach-tether` -> `metamach-tmux`.
- Single-DB -> multi-DB (catalog + per-blueprint), `task_id` UUID-keyed.

---

Earlier spec-only revisions (the initial docs, ARCH-0.2.0, the rebrand to
0.1.0) predate the implemented 0.3.0 baseline and are not enumerated here; see
`git log` for the full history.
