# Repository Guidelines

## Project Structure

MetaMach is a **specification-first repository with a working Rust implementation**. Version 0.6.0 spans M0–M4 + the 0.3.0 de-containerization consensus + the 0.4.0 gateway/ecosystem delta + ADR-029 project-based templates + ADR-031 DSL unification + ADR-032 Studio. 190 tests, CI-green. Current layout:

``` metamach/
├── docs/                # ✅ English specs (source of truth) + ADR.md (32 decisions)
├── docs/CH/             # ❌ gitignored - Chinese translations & audit artifacts
├── janus/               # ✅ Rust workspace (5 binaries + shared lib, ~12,000 LOC)
│   ├── src/bin/         #   janus, janus-daemon, herdr-janus, janush, janus-studio
│   ├── src/absurd/      #   Postgres adapter + SQLite fallback ring
│   ├── src/tmux/        #   PTY session engine (remain-on-exit)
│   ├── src/tool_guard/  #   Rule engine + webhook dispatch
│   ├── src/gateway/     #   HITL Gateway (Teams Adaptive Cards, HMAC)
│   ├── src/cognitive/   #   Cognitive Provider SPI (MCP)
│   ├── src/workflow/    #   Workflow engine + stream filter
│   ├── src/studio_assets/#  Embedded HTML, CSS, JS Canvas Studio assets
│   ├── src/{agent,coldstart,lifecycle,paths,pipeline,protocol,recipe,spawn,uds}.rs
│   ├── migrations/      #   001_catalog, 002_blueprint, 003_hitl_verdict, 004_env_snapshot
│   └── tests/           #   10 integration test files (190 tests total)
├── templates/           # ✅ `janus init` scaffolds from here
│   ├── blueprint.toml   #   Default project recipe
│   ├── agents/          #   Architect, Builder, Tester role templates
│   └── workflows/       #   15 workflow templates (linear + DAG)
├── configs/             # ✅ agents.toml, global_rules.md, offboard.toml
├── scripts/             # ✅ pre-push git hook (fmt + clippy + test + PG E2E)
├── bin/                 # ✅ compiled plugin binaries (gitignored build output)
├── .github/workflows/   # ✅ ci.yml (native PG + tmux + Herdr, all 178 tests)
├── Makefile             # ✅ bootstrap/db-init/db-backup/health/uninstall/...
├── CLAUDE.md            # AI agent guidance for Claude Code
└── AGENTS.md            # This file
```

**Per-project layout** (after `janus init` in a target project):
```
my-project/
├── .janus/              # All MetaMach config for this project (ADR-029/ADR-031)
│   ├── blueprint.toml   #   [blueprint] name, default_workflow, [remote], [openwiki]
│   ├── agents/          #   Project-specific agent role overrides
│   ├── workflows/       #   Unified workflow definitions (linear + DAG)
│   └── openwiki/        #   RAG knowledge scope; production_report.md on offboard
└── src/                 # Your project source
```

## Spec Source of Truth

- **`docs/` (English) is the sole version-controlled spec source.** Authoritative structure: 5 fundamental specs (`PRD.md`, `ARCH.md`, `ADR.md`, `SPEC.md`, `PLAN.md`) plus dependency contracts under `docs/contracts/` (`herdr.md`, `absurd.md`, `tmux.md`).
- `docs/CH/` is **gitignored** and not authoritative. When English and Chinese disagree, English wins. Sync direction is always **from `docs/` to `docs/CH/`**, never the reverse.

## Build, Test & Development Commands

The Rust workspace lives under `janus/` - either `cd janus` first or pass `--manifest-path janus/Cargo.toml`. CI runs all of these and is green.

| Command | Purpose |
|---|---|
| `cargo build --release --locked --manifest-path janus/Cargo.toml` | Build the workspace |
| `cargo fmt --all --manifest-path janus/Cargo.toml -- --check` | Enforce Rust 2024 Edition formatting |
| `cargo clippy --manifest-path janus/Cargo.toml --all-targets -- -D warnings` | Lint (fail on warnings) |
| `cargo test --workspace --manifest-path janus/Cargo.toml` | Run all tests (lib + integration) |
| `make bootstrap` | Full bootstrap: prereq → symlinks → compile → db-init |
| `make db-init` | Initialize native Postgres + catalog migration |
| `make health` | PG liveness + daemon socket check |

**Toolchain:** Rust 1.88+ (Edition 2024), native PostgreSQL 16+ (pg_config/pg_ctl/initdb - NOT Docker), tmux 3.3+, Herdr 0.7.3. Tests that need PG use `DATABASE_URL=postgres://metamach_admin@/metamach_db` over the Unix socket; tmux-using tests require a real `tmux` server.

## Coding Style & Naming

- **Rust 2024 Edition** with `rustfmt` defaults. All code must pass `cargo fmt` and `cargo clippy -D warnings`.
- Binaries use kebab-case: `janus-daemon`, `herdr-janus`, `janush`, `janus`, `janus-studio`.
- Config files are TOML (`agents.toml`, `blueprint.toml`, `workflows/*.toml`).
- The physical execution module is `janus::tmux` (internalized from the former external `herdr-tether`); its isolated tmux server is `tmux -L metamach-tmux`.

## Testing Guidelines

- Unit tests in `#[cfg(test)]` modules alongside source; integration tests in `janus/tests/` (10 files, 190 tests total).
- CI gates: `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace`. All must pass before merge.
- PG-gated tests use **runtime-skip** (check `DATABASE_URL` at test start) rather than `#[ignore]` — they run automatically when PG is available (CI) and skip gracefully when it is not (local dev without `make db-init`).
- Test names are prefixed with UTC IDs mapped to `SPEC.md` (e.g., `utc_03_03_cold_start_reconcile`).

## Commit & Pull Request Guidelines

- **Commit messages** follow Conventional Commits: `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`. Milestone-scoped work uses `feat(mN):` / `fix(mN):` (e.g., `feat(m2): ...`).
- **PR descriptions** must reference the spec(s) being implemented (e.g., "Implements ARCH.md §3 CLI architecture").
- **Spec changes** must update the English `docs/` files. Do not modify `docs/CH/` directly unless syncing from `docs/`.
- Keep PRs focused - one logical change per PR.

## Architecture Overview

MetaMach 0.5.0 is a durable AI software factory OS. Core components:

- **`janus-daemon`** - control-plane daemon (Rust), sole owner of state and DB connection pool. Hosts UDS listener, workflow engine, HITL gateway, and cold-start reconciliation.
- **`herdr-janus`** - Herdr 0.7.3 plugin (shadow client), TUI rendering only (ratatui). Crashes never lose state.
- **`janush`** - UDS proxy shell that reconciles agent commands with the daemon's Tool Guard before execution. Fail-closed 30s timeout.
- **`janus::tmux`** - native module managing `remain-on-exit` tmux sessions on `tmux -L metamach-tmux`. Cross-host SSH reverse tunnel transport (ADR-017).
- **`janus::gateway`** - HITL Gateway with Teams Adaptive Card dispatch, HMAC-SHA256 webhook validation, loopback HTTP callback listener.
- **`janus::cognitive`** - Cognitive Provider SPI: opt-in MCP plugins (`codebase-memory-mcp`), advisory command validation.
- **`janus::workflow`** - Workflow engine driving blueprint steps via absurd pull-mode queue. Checkpointing, lease renewal, retry-claim loop.
- **`janus::pipeline`** - Pipeline DAG engine with Kahn's algorithm topological sort, parallel level execution.
- **Absurd Postgres** - catalog DB (`metamach_db`) plus one DB per active blueprint (`metamach_blueprint_<name>`); the F1 multi-DB fan-out. SQLite fallback ring buffer for PG outage survival.

Three customization dimensions: **Agent Pool** (`configs/agents.toml` + `.janus/agents/`), **Workflows** (`templates/workflows/` + `.janus/workflows/`), **Blueprints** (`.janus/blueprint.toml`). Lifecycle: Init ↔ Offboard.

## External Dependencies

`openwiki` (RAG knowledge engine) is a separate repo whose per-blueprint content is consumed under `.janus/openwiki/`. The physical execution engine formerly known as `herdr-tether` has been **internalized as `janus::tmux`** and is no longer external. Herdr 0.7.3 is the external plugin host (contract in `docs/contracts/herdr.md`).
