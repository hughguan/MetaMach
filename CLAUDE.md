# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Specification source of truth

The **English specs directly under `docs/` are the sole version-controlled spec source**:

- `docs/ARCH.md`, `docs/PRD.md`, `docs/Feature-Spec.md`, `docs/Project-Plan.md`, `docs/Review-Spec.md`, `docs/Test-Spec.md`, `docs/Deployment-Spec.md`
- `docs/ARCH-0.2.0.md`, `docs/ARCH-0.3.0.md`, `docs/ARCH-0.4.0.md` are incremental architecture **delta** specs layered on `ARCH.md`. 0.3.0 (de-containerization, native PG, F1 multi-DB, `janus::tmux`) and 0.4.0 (gateway & ecosystem) are both implemented.
- `docs/CH/` (Chinese translations + the `*-Review.md` audit deep-dives) is **gitignored** (see `.gitignore`) and is **not authoritative**. When the English specs and the Chinese translations disagree, the English `docs/` wins. Do not edit `docs/CH/` as the source of truth; if asked to translate/sync, port **from `docs/` to `docs/CH/`**, never the reverse.
- The `docs/CH/*-Review.md` files are point-in-time audit artifacts with resolution logs - useful history, but not the spec.

## Repository status

This is an **implemented Rust workspace plus specs** - not documentation-only. Milestones M0–M3 and M5 plus the 0.3.0 and 0.4.0 rearchitectures are built. **M4 is partial**: Task 4.2 (Offboard) + Task 4.3 (Onboard) are implemented and tested, and Task 4.1's **SQLite Log Replay is implemented** (`AbsurdDb::replay_fallback` drains the ring buffer into the routed per-blueprint `metamach_step_meta` overlay on PG recovery). Still deferred: Task 4.1's cross-host SSH transport + checkpoint-driven cold-start re-exec (`coldstart::reconcile` only *logs* resume plans, no re-exec; `janus::tmux` is local-only) and Task 4.4's `target_sha` optimistic-lock enforcement (schema column without enforcement). `janus/` (~6,100 LOC src + ~1,500 LOC tests, zero `todo!`/`unimplemented!` stubs), `Makefile`, `configs/`, `blueprints/`, `workflows/`, `bin/`, and `.github/workflows/ci.yml` (CI is green). The four binaries - `janus`, `janus-daemon`, `herdr-janus`, `janush` - all exist under `janus/src/bin/`.

The **0.4.0 delta** (`docs/ARCH-0.4.0.md`: Cognitive Provider SPI, `codebase-memory-mcp`, stateless HITL Gateway, Teams Active Cards) is **implemented** - `janus/src/gateway/` (`mod.rs` + `teams.rs`) and `janus/src/cognitive/mod.rs` exist and are wired into the daemon (the daemon constructs `HitlGateway`, spawns its loopback HTTP callback listener, and passes it to connection handlers). HITL dispatch lives in `gateway::Gateway::dispatch`; `tool_guard::webhook` retains only the sender adapters (Telegram/Logging), reused as the gateway's channels. **M5** (integration testing & release gate) is complete: the `janus/tests/` suite covers UTC-01/02/03/04/05/08/10, the PG-gated tests are a **blocking** CI gate (with `001_catalog.sql` applied to the `postgres:16` service), and `v0.4.0` is tagged (GPG-signed). When asked to implement code, consult the specs first: layout, data contracts, and CLI surface are all defined there.

## Build & toolchain

Per `docs/Deployment-Spec.md` §1 and `docs/Project-Plan.md` (Check-in Gates):

- **Rust 1.88+ (Edition 2024)** - build with `cargo build --release --locked` (run from `janus/`). CI gates (all green): `cargo fmt --all --manifest-path janus/Cargo.toml -- --check`, `cargo clippy --manifest-path janus/Cargo.toml --all-targets -- -D warnings`, `cargo test --workspace --manifest-path janus/Cargo.toml`.
- **Native PostgreSQL 16+** (NOT Docker) - the 0.3.0 consensus de-containerized the DB. `make db-init` runs `initdb` + `pg_ctl` + `createdb` + the catalog migration against `$(METAMACH_DB_DIR)` (default `~/.metamach/db`), Unix socket only. Per-blueprint migrations (`002_blueprint.sql`) run on `janus onboard`.
- **tmux 3.3+**, **Herdr 0.7.3** (plugin host; M0-validated contract in `docs/herdr-v1-contract.md`).
- **`herdr-tether` was internalized as `janus::tmux`** (Task 2.4 / 0.3.0) - it is no longer an external dependency. The remaining external engine is `openwiki` (RAG), whose per-blueprint content lives under `blueprints/<name>/openwiki/`. `absurd` is the branded name for the Postgres execution layer; `janus::absurd` is the in-repo sqlx pool/audit module, and schema lives in `janus/migrations/`.
- Bootstrap entrypoint is `make bootstrap` = `prereq` -> `symlinks` -> `compile` -> `db-init`. Other Make targets: `db-down`, `db-backup`, `db-restore`, `db-migrate`, `health`, `logs`, `ram-disk`, `uninstall`, `clean`.

## High-level architecture

MetaMach 0.4.0 is a durable AI "software factory" OS. The core mental model (spread across `ARCH.md` + `Feature-Spec.md`):

- **`janus-daemon` (resident brain):** the sole owner of state, the DB connection pool, and the UDS gateway. All Step state transitions are transactional in Absurd Postgres. Exposes a read-only `progress` primitive for the dashboard.
- **`herdr-janus` (shadow client):** a lightweight Herdr plugin that only renders the Popup (two views: **Dispatch** and **Progress**). Crashes never lose state - it just re-attaches. Lazy-starts the Daemon via `std::process::Command::spawn()` + detach.
- **`janush` (proxy shell):** tmux injects this as `SHELL` (absolute path `${HERDR_PLUGIN_ROOT}/bin/janush`). Every Agent command is synchronously reconciled with the Daemon over UDS **before** reaching bash. Verdict: `ALLOW` / `BLOCK` / `REWRITE` (Contract 3.4). 30s timeout = fail-closed `BLOCK`.
- **`janus::tmux` (physical execution, internalized):** the former external `herdr-tether` engine, now a native module. Manages `remain-on-exit` tmux sessions on an isolated server (`tmux -L metamach-tmux`); cross-host SSH transport lands with M4. Sessions survive process exit, SSH drop, or frontend destruction (ARCH §6.1).
- **Absurd Postgres (Absurd DB):** a catalog DB (`metamach_db`) plus one DB per active blueprint (`metamach_blueprint_<name>`) - the F1 multi-DB fan-out. Sole source of truth; cold start reads the last `COMPLETED` checkpoint (never `tmux-resurrect`). The `progress` query unions across per-blueprint DBs in Rust (no cross-DB `JOIN`).
- **OpenWiki (external):** federated RAG; `production_report.md` from Offboard is recycled as few-shot `## Previous Incidents` on the next Onboard.

Three customization dimensions: **Agent Pool** (`configs/agents.toml`), **Workflows** (`workflows/*.toml`), **Blueprints** (`blueprints/<name>/janus.toml`). Lifecycle: **Onboard** (validate recipe -> register tenant -> bind workflow -> load knowledge) ↔ **Offboard** (LLM-smelt `production_report.md` -> `melt_blueprint_data` deletes large JSON rows).

**Immutable-vs-Mutable isolation** (critical; see `Deployment-Spec.md` §2): `${HERDR_PLUGIN_ROOT}` (read-only checkout/binaries), `${HERDR_PLUGIN_CONFIG_DIR}` (mutable config: `agents.toml`), `${HERDR_PLUGIN_STATE_DIR}` (mutable state: `janus.sock`, `janus.pid`, `fallback.db`, PG socket). `make bootstrap` must never wipe state on plugin updates.

## Spec map & cross-doc conventions

| Doc | Scope | Key anchors |
|---|---|---|
| `ARCH.md` | Architecture, topology, monorepo tree, resilience invariants | §3 CLI & binary architecture; §5 directory tree; §6 invariants |
| `ARCH-0.2.0/0.3.0/0.4.0.md` | Incremental architecture deltas | 0.3.0 + 0.4.0 implemented (native PG, F1 multi-DB, `janus::tmux`, gateway/cognitive/Teams) |
| `PRD.md` | Product requirements, director journey, functional matrix | §3 matrix (priorities + measurable UAT); §4 Day-0 Onboard + user journey |
| `Feature-Spec.md` | Feature specs + data contracts + fault matrix | **Contracts 3.1–3.8**; §2.4 HITL; §2.5 Onboard/Offboard+LLM; §4 fault matrix |
| `Project-Plan.md` | Milestones M0–M5 + check-in units + CI gates | M0–M3 + M5 + 0.3.0/0.4.0 implemented; M4 partial (4.2/4.3 + 4.1-LogReplay done; 4.1-re-exec/SSH + 4.4 deferred); Check-in Gates |
| `Review-Spec.md` | Audit domains + sign-off sheet | `REV-SEC/STB/DIS/EVO/OPS-NN` items; §3 dependency ordering |
| `Test-Spec.md` | Test cases + environment | `UTC-XX-YY` IDs (Suites 2.1–2.7); §1 severity gates |
| `Deployment-Spec.md` | Directory topology, Makefile, secrets | §5 Makefile (bootstrap/db-init/db-backup/health/uninstall); §4 RAM-disk secrets |
| `herdr-v1-contract.md` | **M0-validated** Herdr 0.7.3 plugin contract (authoritative for Herdr integration; supersedes "Herdr v1" assumptions) | manifest schema; `overlay` placement (not `popup`); injected `HERDR_PLUGIN_ROOT/CONFIG_DIR/STATE_DIR` + `HERDR_SOCKET_PATH`; dir mapping |

Cross-doc identifiers to keep consistent when editing:
- **Data contracts:** `blueprints`, `absurd_tasks`, `absurd_steps` (Feature-Spec Contract 3.1); `fallback_events` SQLite ring buffer (Contract 3.8).
- **Status enum:** `PENDING -> STARTING -> RUNNING -> COMPLETED | FAILED | SUSPENDED` (tasks/steps); `ACTIVE <-> OFFBOARDED` (blueprints).
- **CLI:** unified `janus` CLI with subcommands `janus onboard` / `offboard` / `status` / `daemon` / `tmux` (all require the Daemon running - they are UDS clients, never direct DB access). tmux session commands are `janus tmux open|attach|list` (native `janus::tmux`; the old `herdr-tether <subcommand>` surface was internalized).
- **Naming:** database is "Absurd Postgres" (formal) / "Absurd DB" (shorthand) - not "Unified DB/PG". Project is branded **MetaMach 0.4.0**. tmux socket is `metamach-tmux` (renamed from the prior `metamach-tether`).
- **Safety tests:** never prescribe literal `rm -rf /`; use the `/tmp/metamach-*-guard-$(uuidgen)` sentinel pattern (see `Review-Spec.md` REV-SEC-02, `Test-Spec.md` UTC-02-02).

When changing a spec, check the related docs - e.g., a schema change in Feature-Spec Contract 3.1 typically affects Test-Spec UTC cases, Review-Spec REV items, and Project-Plan milestone tasks. The contracts, test IDs, and milestone units are the cross-referencing fabric.
