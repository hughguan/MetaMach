# Repository Guidelines

## Project Structure

MetaMach is a **specification-first repository with a working Rust implementation**. The 0.3.0 consensus (M0–M4 + de-containerization, native PG, F1 multi-DB, `janus::tmux` internalization) is built and CI-green. Current layout:

```
metamach/
├── docs/                # ✅ English specs (source of truth) + ARCH-0.2.0/0.3.0/0.4.0 deltas
├── docs/CH/             # ❌ gitignored - Chinese translations & audit artifacts
├── janus/               # ✅ Rust workspace (4 binaries + shared lib, ~2,800 LOC)
│   ├── src/bin/         #   janus, janus-daemon, herdr-janus, janush
│   ├── src/{absurd,tmux,tool_guard,lifecycle,protocol,uds,recipe,coldstart,spawn,paths}.rs
│   ├── migrations/      #   001_catalog.sql, 002_blueprint.sql
│   └── tests/           #   integration tests (tmux.rs)
├── configs/             # ✅ agents.toml, global_rules.md, offboard.toml, tmux.conf
├── workflows/           # ✅ Declarative .toml pipelines (dev-flow, firmware-deploy)
├── blueprints/          # ✅ Product blueprints (gatemetric, joyrobots) + per-blueprint openwiki/
├── bin/                 # ✅ compiled plugin binaries (gitignored build output)
├── .github/workflows/   # ✅ ci.yml (native PG service, fmt + clippy -D warnings + test)
├── Makefile             # ✅ bootstrap/db-init/db-backup/health/uninstall/...
├── CLAUDE.md            # AI agent guidance for Claude Code
└── AGENTS.md            # This file
```

## Spec Source of Truth

- **`docs/` (English) is the sole version-controlled spec source.** The seven authoritative files: `ARCH.md`, `PRD.md`, `Feature-Spec.md`, `Project-Plan.md`, `Review-Spec.md`, `Test-Spec.md`, `Deployment-Spec.md`. Plus the incremental `ARCH-0.2.0/0.3.0/0.4.0.md` deltas.
- `docs/CH/` is **gitignored** and not authoritative. When English and Chinese disagree, English wins. Sync direction is always **from `docs/` to `docs/CH/`**, never the reverse.

## Build, Test & Development Commands

The Rust workspace lives under `janus/` - either `cd janus` first or pass `--manifest-path janus/Cargo.toml`. CI runs all of these and is green.

| Command | Purpose |
|---|---|
| `cargo build --release --locked --manifest-path janus/Cargo.toml` | Build the workspace |
| `cargo fmt --all --manifest-path janus/Cargo.toml -- --check` | Enforce Rust 2024 Edition formatting |
| `cargo clippy --manifest-path janus/Cargo.toml --all-targets -- -D warnings` | Lint (fail on warnings) |
| `cargo test --workspace --manifest-path janus/Cargo.toml` | Run all tests (lib + integration) |
| `make bootstrap` | Full bootstrap: prereq -> symlinks -> compile -> db-init |
| `make db-init` | Initialize native Postgres + catalog migration |
| `make health` | PG liveness + daemon socket check |

**Toolchain:** Rust 1.88+ (Edition 2024), native PostgreSQL 16+ (pg_config/pg_ctl/initdb - NOT Docker), tmux 3.3+, Herdr 0.7.3. Tests that need PG use `DATABASE_URL=postgres://metamach_admin@/metamach_db` over the Unix socket; tmux-using tests require a real `tmux` server.

## Coding Style & Naming

- **Rust 2024 Edition** with `rustfmt` defaults. All code must pass `cargo fmt` and `cargo clippy -D warnings`.
- Binaries use kebab-case: `janus-daemon`, `herdr-janus`, `janush`, `janus`.
- Config files are TOML (`agents.toml`, `janus.toml`, `workflows/*.toml`).
- The physical execution module is `janus::tmux` (internalized from the former external `herdr-tether`); its isolated tmux server is `tmux -L metamach-tmux`.

## Testing Guidelines

- Unit tests in `#[cfg(test)]` modules alongside source; integration tests in `janus/tests/` per crate.
- CI gates: `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace`. All must pass before merge.
- SSH-gated tests are marked `#[ignore]` and run separately (`--ignored`), continue-on-error in CI.

## Commit & Pull Request Guidelines

- **Commit messages** follow Conventional Commits: `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`. Milestone-scoped work uses `feat(mN):` / `fix(mN):` (e.g., `feat(m2): ...`).
- **PR descriptions** must reference the spec(s) being implemented (e.g., "Implements ARCH.md §3 CLI architecture").
- **Spec changes** must update the English `docs/` files. Do not modify `docs/CH/` directly unless syncing from `docs/`.
- Keep PRs focused - one logical change per PR.

## Architecture Overview

MetaMach 0.3.0 is a durable AI software factory OS. Core components:

- **`janus-daemon`** - control-plane daemon (Rust), sole owner of state and DB connection pool.
- **`herdr-janus`** - Herdr 0.7.3 plugin (shadow client), UI rendering only.
- **`janush`** - UDS proxy shell that reconciles agent commands with the daemon before execution.
- **`janus::tmux`** (internalized from the former external `herdr-tether`) - native module managing `remain-on-exit` tmux sessions on `tmux -L metamach-tmux`; cross-host SSH transport lands with M4.
- **Absurd Postgres** - catalog DB (`metamach_db`) plus one DB per active blueprint (`metamach_blueprint_<name>`); the F1 multi-DB fan-out.

Three customization dimensions: **Agent Pool**, **Workflows**, **Blueprints**. Lifecycle: Onboard ↔ Offboard.

## External Dependencies

`openwiki` (RAG knowledge engine) is a separate repo whose per-blueprint content is consumed under `blueprints/<name>/openwiki/`. The physical execution engine formerly known as `herdr-tether` has been **internalized as `janus::tmux`** and is no longer external. Herdr 0.7.3 is the external plugin host (M0-validated contract in `docs/herdr-v1-contract.md`).
