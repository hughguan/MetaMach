# Repository Guidelines

## Project Structure

MetaMach is currently a **specification-first repository** — no implementation code exists yet. The intended monorepo layout (defined in `docs/ARCH.md` §5):

```
metamach/
├── docs/                # ✅ English specs (source of truth)
├── docs/CH/             # ❌ gitignored — Chinese translations & audit artifacts
├── janus/               # ⬜ Rust workspace (daemon, Herdr plugin, TUI, proxy shell)
├── configs/             # ⬜ Agent pool & deployment configs
├── workflows/           # ⬜ Declarative .toml pipelines
├── blueprints/          # ⬜ Product blueprints
├── openwiki/            # ⬜ RAG knowledge base
├── CLAUDE.md            # AI agent guidance for Claude Code
└── AGENTS.md            # This file
```

## Spec Source of Truth

- **`docs/` (English) is the sole version-controlled spec source.** The seven authoritative files: `ARCH.md`, `PRD.md`, `Feature-Spec.md`, `Project-Plan.md`, `Review-Spec.md`, `Test-Spec.md`, `Deployment-Spec.md`.
- `docs/CH/` is **gitignored** and not authoritative. When English and Chinese disagree, English wins. Sync direction is always **from `docs/` to `docs/CH/`**, never the reverse.

## Build, Test & Development Commands

No commands are runnable today (repo is docs-only). The intended toolchain, per the specs:

| Command | Purpose |
|---|---|
| `cargo build --release --locked` | Build the Rust workspace |
| `cargo fmt --all -- --check` | Enforce Rust 2024 Edition formatting |
| `cargo clippy --all-targets -- -D warnings` | Lint (fail on warnings) |
| `cargo test --workspace` | Run all tests |
| `make bootstrap` | Full bootstrap: symlinks → compile → db-up |

**Toolchain:** Rust 1.88+ (Edition 2024), Docker Compose v2.20+, tmux 3.2+, Herdr v1.

## Coding Style & Naming

- **Rust 2024 Edition** with `rustfmt` defaults. All code must pass `cargo fmt` and `cargo clippy`.
- Binaries use kebab-case: `janus-daemon`, `herdr-janus`, `janus-sh`.
- Config files are TOML (`agents.toml`, `janus.toml`, `workflows/*.toml`).

## Testing Guidelines

- Unit tests in `#[cfg(test)]` modules alongside source; integration tests in `tests/` per crate.
- CI gates: `cargo fmt`, `cargo clippy`, `cargo test --workspace`. All must pass before merge.

## Commit & Pull Request Guidelines

- **Commit messages** follow Conventional Commits: `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`.
- **PR descriptions** must reference the spec(s) being implemented (e.g., "Implements ARCH.md §3 CLI architecture").
- **Spec changes** must update the English `docs/` files. Do not modify `docs/CH/` directly unless syncing from `docs/`.
- Keep PRs focused — one logical change per PR.

## Architecture Overview

MetaMach is a durable AI software factory OS. Core components:

- **`janus-daemon`** — control-plane daemon (Rust), sole owner of state and DB connection pool.
- **`herdr-janus`** — Herdr plugin (shadow client), UI rendering only.
- **`janus-sh`** — UDS proxy shell that reconciles agent commands with the daemon before execution.
- **`herdr-tether`** (external) — tmux-based cross-host execution with session durability.
- **Absurd Postgres** — transactional single-DB, multi-tenant by `blueprint_id`.

Three customization dimensions: **Agent Pool**, **Workflows**, **Blueprints**. Lifecycle: Onboard ↔ Offboard.

## External Dependencies

`herdr-tether`, `absurd`, and `openwiki` are separate repos — fetched and built by `make bootstrap`, not committed here.
