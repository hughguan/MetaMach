# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Specification source of truth

The **English specs directly under `docs/` are the sole version-controlled spec source**:

- `docs/ARCH.md`, `docs/PRD.md`, `docs/Feature-Spec.md`, `docs/Project-Plan.md`, `docs/Review-Spec.md`, `docs/Test-Spec.md`, `docs/Deployment-Spec.md`
- `docs/CH/` (Chinese translations + the `*-Review.md` audit deep-dives) is **gitignored** (see `.gitignore`) and is **not authoritative**. When the English specs and the Chinese translations disagree, the English `docs/` wins. Do not edit `docs/CH/` as the source of truth; if asked to translate/sync, port **from `docs/` to `docs/CH/`**, never the reverse.
- The `docs/CH/*-Review.md` files are point-in-time audit artifacts with resolution logs — useful history, but not the spec.

## Repository status

This is currently a **documentation-only repository**. The 7 English specs describe the MetaMach 1.0 system that is *to be built*. None of the implementation exists yet — there is no `janus/` Rust workspace, no `Makefile`, no `docker-compose.yml`, no `configs/`, `blueprints/`, or `workflows/` directories. The monorepo tree in `docs/ARCH.md` §5 and the commands in `docs/Deployment-Spec.md` §5 are **targets**, not present files.

When asked to implement code, consult the specs first: the intended layout, data contracts, and CLI surface are all defined there.

## Intended build & toolchain (defined in specs, not yet implemented)

Per `docs/Deployment-Spec.md` §1 and `docs/Project-Plan.md` (Check-in Gates):

- **Rust 1.88+ (Edition 2024)** — `cargo build --release --locked`; CI gates: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`.
- **Docker Compose v2.20+** — `docker compose up -d` brings up the Absurd Postgres container (Unix socket only, no TCP).
- **tmux 3.3+**, **Herdr 0.7.3** (plugin host; M0-validated contract in `docs/herdr-v1-contract.md`).
- **External dependencies** (separate repos, fetched/built by `make bootstrap`, NOT in this repo): `herdr-tether` (tmux/SSH execution engine), `absurd` (Postgres engine / `melt_blueprint_data`), `openwiki` (RAG knowledge engine).
- The intended bootstrap entrypoint is `make bootstrap` (symlinks → compile → db-up). The intended binaries are `janus-daemon`, `herdr-janus`, `janush`, `herdr-tether`.

There are no commands to run today. For doc work, edit the Markdown directly.

## High-level architecture

MetaMach 1.0 is a durable AI "software factory" OS. The core mental model (spread across `ARCH.md` + `Feature-Spec.md`):

- **`janus-daemon` (resident brain):** the sole owner of state, the DB connection pool, and the UDS gateway. All Step state transitions are transactional in Absurd Postgres. Exposes a read-only `progress` primitive for the dashboard.
- **`herdr-janus` (shadow client):** a lightweight Herdr plugin that only renders the Popup (two views: **Dispatch** and **Progress**). Crashes never lose state — it just re-attaches. Lazy-starts the Daemon via `std::process::Command::spawn()` + detach.
- **`janush` (proxy shell):** tmux injects this as `SHELL` (absolute path `${HERDR_PLUGIN_ROOT}/bin/janush`). Every Agent command is synchronously reconciled with the Daemon over UDS **before** reaching bash. Verdict: `ALLOW` / `BLOCK` / `REWRITE` (Contract 3.4). 30s timeout = fail-closed `BLOCK`.
- **`herdr-tether` (physical execution, external):** tmux `remain-on-exit` sessions (dedicated server `tmux -L metamach-tether`), cross-host SSH. Sessions survive network drops/power loss.
- **Absurd Postgres (Absurd DB):** single-DB, multi-tenant by `blueprint_id`. Sole source of truth; cold start reads the last `COMPLETED` checkpoint (never `tmux-resurrect`).
- **OpenWiki (external):** federated RAG; `production_report.md` from Offboard is recycled as few-shot `## Previous Incidents` on the next Onboard.

Three customization dimensions: **Agent Pool** (`configs/agents.toml`), **Workflows** (`workflows/*.toml`), **Blueprints** (`blueprints/<name>/janus.toml`). Lifecycle: **Onboard** (validate recipe → register tenant → bind workflow → load knowledge) ↔ **Offboard** (LLM-smelt `production_report.md` → `melt_blueprint_data` deletes large JSON rows).

**Immutable-vs-Mutable isolation** (critical; see `Deployment-Spec.md` §2): `${HERDR_PLUGIN_ROOT}` (read-only checkout/binaries), `${HERDR_PLUGIN_CONFIG_DIR}` (mutable config: `agents.toml`), `${HERDR_PLUGIN_STATE_DIR}` (mutable state: `janus.sock`, `janus.pid`, `fallback.db`, PG socket). `make bootstrap` must never wipe state on plugin updates.

## Spec map & cross-doc conventions

| Doc | Scope | Key anchors |
|---|---|---|
| `ARCH.md` | Architecture, topology, monorepo tree, resilience invariants | §3 CLI & binary architecture; §5 directory tree; §6 invariants |
| `PRD.md` | Product requirements, director journey, functional matrix | §3 matrix (priorities + measurable UAT); §4 Day-0 Onboard + user journey |
| `Feature-Spec.md` | Feature specs + data contracts + fault matrix | **Contracts 3.1–3.8**; §2.4 HITL; §2.5 Onboard/Offboard+LLM; §4 fault matrix |
| `Project-Plan.md` | Milestones M0–M4 + check-in units + CI gates | M0 Herdr validation (✅ done); M2 Tasks 2.3/2.4/2.5; M4 Task 4.2a/b/c split; Check-in Gates |
| `Review-Spec.md` | Audit domains + sign-off sheet | `REV-SEC/STB/DIS/EVO/OPS-NN` items; §3 dependency ordering |
| `Test-Spec.md` | Test cases + environment | `UTC-XX-YY` IDs (Suites 2.1–2.7); §1 severity gates |
| `Deployment-Spec.md` | Directory topology, docker-compose, Makefile, secrets | §3 compose (Unix socket); §5 Makefile (bootstrap/db-backup/health/uninstall); §4 RAM-disk secrets |
| `herdr-v1-contract.md` | **M0-validated** Herdr 0.7.3 plugin contract (authoritative for Herdr integration; supersedes "Herdr v1" assumptions) | manifest schema; `overlay` placement (not `popup`); injected `HERDR_PLUGIN_ROOT/CONFIG_DIR/STATE_DIR` + `HERDR_SOCKET_PATH`; dir mapping |

Cross-doc identifiers to keep consistent when editing:
- **Data contracts:** `blueprints`, `absurd_tasks`, `absurd_steps` (Feature-Spec Contract 3.1); `fallback_events` SQLite ring buffer (Contract 3.8).
- **Status enum:** `PENDING -> STARTING -> RUNNING -> COMPLETED | FAILED | SUSPENDED` (tasks/steps); `ACTIVE <-> OFFBOARDED` (blueprints).
- **CLI:** unified `janus` CLI with subcommands `janus onboard` / `offboard` / `status` / `daemon` (all require the Daemon running — they are UDS clients, never direct DB access). tmux commands are always `herdr-tether <subcommand>`.
- **Naming:** database is "Absurd Postgres" (formal) / "Absurd DB" (shorthand) — not "Unified DB/PG". Project is branded **MetaMach 1.0**.
- **Safety tests:** never prescribe literal `rm -rf /`; use the `/tmp/metamach-*-guard-$(uuidgen)` sentinel pattern (see `Review-Spec.md` REV-SEC-02, `Test-Spec.md` UTC-02-02).

When changing a spec, check the related docs — e.g., a schema change in Feature-Spec Contract 3.1 typically affects Test-Spec UTC cases, Review-Spec REV items, and Project-Plan milestone tasks. The contracts, test IDs, and milestone units are the cross-referencing fabric.
