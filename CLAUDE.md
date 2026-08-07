# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Specification source of truth

The **English specs directly under `docs/` are the sole version-controlled spec source**:

- 5 fundamental core specs: `docs/PRD.md`, `docs/ARCH.md`, `docs/ADR.md`, `docs/SPEC.md`, `docs/PLAN.md`
- Dependency contracts under `docs/contracts/`: `docs/contracts/herdr.md`, `docs/contracts/absurd.md`, `docs/contracts/tmux.md`
- `docs/CH/` (Chinese translations) and `docs/bak/` (historical audit/design backups) are not authoritative. When English specs and translations disagree, English `docs/` wins.

## Repository status

This is an **implemented Rust workspace plus specs** — not documentation-only. The codebase is at **version 0.5.0** (~11,000 LOC, 178 tests, CI-green). All milestones M0–M4 plus M5 integration tests are complete, including the 0.3.0 de-containerization consensus and the 0.4.0 gateway/ecosystem delta.

**Implemented and tested:**
- **M0:** Herdr 0.7.3 plugin contract validated.
- **M1:** Native Absurd Postgres (Unix socket, no Docker), catalog + blueprint migrations, `herdr-janus` shadow TUI.
- **M2:** `janus-daemon` resident brain, UDS twin-process protocol, `progress` primitive, `janus::tmux` (internalized from `herdr-tether`), F1 multi-DB fan-out.
- **M3:** `janush` proxy shell + Tool Guard rule engine (ALLOW/BLOCK/REWRITE, hot-reload, 30s fail-closed timeout).
- **M4:** Init/Offboard lifecycle, LLM-smelt `production_report.md`, cold-start resume, `target_sha` optimistic locking, workflow engine (absurd pull-mode, checkpointing, retry-claim loop), HITL resume loop, cross-host SSH reverse tunnel transport (ADR-017).
- **M5:** Integration test suite (8 files, 178 tests), PG-gated blocking CI gate.
- **0.4.0:** HITL Gateway (Teams Adaptive Cards, HMAC-SHA256), Cognitive Provider SPI (MCP), loopback HTTP callback listener.
- **0.4.5–0.4.9:** Pipeline DAG engine (Kahn's topological sort), stream filter (ANSI stripping), configurable agents, observer panel TUI, environmental snapshot, dual-path log pipeline, hardware pre-flight probes, E2E pipeline tests.
- **0.5.0 (ADR-029):** Project-based templates — `janus init` scaffolds `.janus/` from `templates/`. Blueprint config moved from `blueprints/<name>/janus.toml` → `.janus/blueprint.toml`. 15 workflow templates (linear + DAG), 3 agent role templates.

The four binaries — `janus`, `janus-daemon`, `herdr-janus`, `janush` — all exist under `janus/src/bin/`. Zero `todo!`/`unimplemented!`/`FIXME`/`HACK` stubs in the codebase.

## Build & toolchain

Per `docs/SPEC.md` and `docs/PLAN.md`:

- **Rust 1.88+ (Edition 2024)** - build with `cargo build --release --locked` (run from `janus/`). CI gates (all green): `cargo fmt --all --manifest-path janus/Cargo.toml -- --check`, `cargo clippy --manifest-path janus/Cargo.toml --all-targets -- -D warnings`, `cargo test --workspace --manifest-path janus/Cargo.toml`.
- **Native PostgreSQL 16+** (NOT Docker) - the 0.3.0 consensus de-containerized the DB. `make db-init` runs `initdb` + `pg_ctl` + `createdb` + the catalog migration against `$(METAMACH_DB_DIR)` (default `~/.metamach/db`), Unix socket only. Per-blueprint migrations (`002_blueprint.sql`, `003_hitl_verdict.sql`, `004_env_snapshot.sql`) run on `janus init`.
- **tmux 3.3+**, **Herdr 0.7.3** (plugin host; contract in `docs/contracts/herdr.md`).
- **`herdr-tether` was internalized as `janus::tmux`** (ADR-006 / 0.3.0) — it is no longer an external dependency. The remaining external engine is `openwiki` (RAG), whose per-blueprint content lives under `.janus/openwiki/`. `absurd` is the branded name for the Postgres execution layer; `janus::absurd` is the in-repo sqlx pool/audit module (contract in `docs/contracts/absurd.md`).
- Bootstrap entrypoint is `make bootstrap` = `prereq` -> `symlinks` -> `compile` -> `db-init`. Other Make targets: `db-down`, `db-backup`, `db-restore`, `db-migrate`, `health`, `logs`, `ram-disk`, `uninstall`, `clean`.

## High-level architecture

MetaMach 0.5.0 is a durable AI "software factory" OS. The core mental model (spread across `ARCH.md` + `SPEC.md`):

- **`janus-daemon` (resident brain):** the sole owner of state, the DB connection pool, and the UDS gateway. All Step state transitions are transactional in Absurd Postgres. Exposes a read-only `progress` primitive for the dashboard. Hosts the HITL Gateway loopback HTTP listener and workflow engine.
- **`herdr-janus` (shadow client):** a lightweight Herdr plugin that only renders the TUI (ratatui: Dispatch and Progress views). Crashes never lose state — it just re-attaches. Lazy-starts the Daemon via `std::process::Command::spawn()` + detach.
- **`janush` (proxy shell):** tmux injects this as `SHELL` (absolute path `${HERDR_PLUGIN_ROOT}/bin/janush`). Every Agent command is synchronously reconciled with the Daemon over UDS **before** reaching bash. Verdict: `ALLOW` / `BLOCK` / `REWRITE` (Contract 3.4). 30s timeout = fail-closed `BLOCK`.
- **`janus::tmux` (physical execution, internalized):** the former external `herdr-tether` engine, now a native module. Manages `remain-on-exit` tmux sessions on an isolated server (`tmux -L metamach-tmux`); cross-host SSH `-R` reverse tunnel transport maps the local `janus.sock` to remote hosts (ADR-017). Sessions survive process exit, SSH drop, or frontend destruction (ARCH §6.1).
- **Absurd Postgres (Absurd DB):** a catalog DB (`metamach_db`) plus one DB per active blueprint (`metamach_blueprint_<name>`) — the F1 multi-DB fan-out. SQLite fallback ring buffer (ADR-004) keeps the workshop alive during PG outages. Sole source of truth; cold start reads the last `COMPLETED` checkpoint (never `tmux-resurrect`). The `progress` query unions across per-blueprint DBs in Rust (no cross-DB `JOIN`).
- **`janus::gateway` (HITL Gateway):** Teams Adaptive Card dispatch, non-blocking verdict thread, HMAC-SHA256 loopback HTTP callback listener. Unified `JANUS_HITL_TIMEOUT_SECS` deadline; late callbacks get `410 Gone`.
- **`janus::cognitive` (Cognitive Provider SPI):** opt-in per-blueprint `[cognitive]` config, `McpProvider` (`codebase-memory-mcp` over stdio JSON-RPC; 2s advisory timeout), `NoopProvider` fail-open default.
- **`janus::workflow` (Workflow engine):** drives blueprint steps via absurd pull-mode queue. Per-step tmux sessions under `janush`, exit-code capture, per-step checkpoints, lease renewal (10s), retry-claim loop (`max_attempts: 3`), HITL resume.
- **`janus::pipeline` (Pipeline DAG engine):** `.janus/workflows/<name>.toml` with `[nodes]` + `needs` edges. Kahn's algorithm topological sort, parallel level execution.
- **OpenWiki (external):** federated RAG; `production_report.md` from Offboard is recycled as few-shot `## Previous Incidents` on the next Init.

Three customization dimensions: **Agent Pool** (`configs/agents.toml` + `.janus/agents/`), **Workflows** (`templates/workflows/` + `.janus/workflows/`), **Blueprints** (`.janus/blueprint.toml`). Lifecycle: **Init** (scaffold + validate + register) ↔ **Offboard** (LLM-smelt `production_report.md` -> `melt_blueprint_data` deletes large JSON rows).

**Immutable-vs-Mutable isolation** (critical; see `docs/SPEC.md`): `${HERDR_PLUGIN_ROOT}` (read-only checkout/binaries), `${HERDR_PLUGIN_CONFIG_DIR}` (mutable config: `agents.toml`), `${HERDR_PLUGIN_STATE_DIR}` (mutable state: `janus.sock`, `janus.pid`, `fallback.db`, PG socket). `make bootstrap` must never wipe state on plugin updates.

## Spec map & cross-doc conventions

| Doc | Scope | Key anchors |
|---|---|---|
| `PRD.md` | Product requirements, director journey, functional matrix | User persona, business goals, functional requirement matrix |
| `ARCH.md` | Architecture, topology, monorepo tree, resilience invariants | §3 CLI & binary architecture; §5 directory tree; §6 invariants |
| `ADR.md` | 31 Architecture Decision Records (ADR-001 through ADR-031) | De-containerization, multi-DB, tmux internalization, fail-closed timeout, SSH transport, pipeline DAG, project-based templates, CI & pre-push hook, unified workflow DSL |
| `SPEC.md` | Technical specifications, test catalog, test report & deployment/CI ops | Feature Contracts 3.x/4.x, UTC test catalog, 178-test report, system deployment & CI pipeline |
| `PLAN.md` | Execution plan & milestone history (M0 through M5) | Milestone roadmap, physical check-in units, verification gates |
| `contracts/` | Dependency contracts (`herdr.md`, `absurd.md`, `tmux.md`) | External plugin, database engine, and physical PTY execution contracts |

Cross-doc identifiers to keep consistent when editing:
- **Data contracts:** `blueprints`, `absurd_tasks`, `absurd_steps` (SPEC.md Part 1 §1); `fallback_events` SQLite ring buffer (`fallback.db`, Contract 3.9).
- **Status enum:** `PENDING -> STARTING -> RUNNING -> COMPLETED | FAILED | SUSPENDED | STOPPED` (tasks/steps); `ACTIVE <-> OFFBOARDED` (blueprints).
- **CLI:** unified `janus` CLI with subcommands `janus init` / `offboard` / `start` / `status` / `plan` / `daemon` / `tmux` (all require the Daemon running — they are UDS clients, never direct DB access). tmux session commands are `janus tmux open|attach|list` (native `janus::tmux`; the old `herdr-tether <subcommand>` surface was internalized).
- **Naming:** database is "Absurd Postgres" (formal) / "Absurd DB" (shorthand) — not "Unified DB/PG". Project is branded **MetaMach 0.5.0**. tmux socket is `metamach-tmux` (renamed from the prior `metamach-tether`).
- **Safety tests:** never prescribe literal `rm -rf /`; use the `/tmp/metamach-*-guard-$(uuidgen)` sentinel pattern (see SPEC.md Part 2 UTC-02-02).

When changing a spec, check the related docs — e.g., a schema change in SPEC.md Part 1 §1 typically affects SPEC.md Part 2 UTC cases and PLAN.md milestone tasks. The contracts, test IDs, and milestone units are the cross-referencing fabric.
