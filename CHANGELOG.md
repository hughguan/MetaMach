# Changelog

All notable changes to MetaMach are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
