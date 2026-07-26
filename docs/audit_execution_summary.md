# Audit Action Items — Execution Summary

> All CI gates pass: `cargo build` ✅ | `cargo clippy -D warnings` ✅ | `cargo fmt --check` ✅

---

## Changes Applied

### 🔴 P0: Documentation Sync to v0.5.0
| File | Change |
|---|---|
| [AGENTS.md](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/AGENTS.md) | **Full rewrite** — Updated project structure from 0.3.0 to 0.5.0 (ADR-029 `.janus/` + `templates/` layout), corrected LOC from ~2,800 → ~11,000, updated test count to 171, added new modules (gateway, cognitive, workflow, pipeline, agent), updated architecture overview, fixed testing guidelines to document runtime-skip strategy, removed obsolete `workflows/` and `blueprints/` references |
| [CLAUDE.md](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/CLAUDE.md) | **Full rewrite** — Synced repository status to 0.5.0 with all implemented features documented (M0-M5, 0.4.0-0.5.0), added ADR.md to spec map, updated architecture section with gateway/cognitive/workflow/pipeline modules, corrected version branding, updated CLI commands list (init, dispatch, pipeline) |

### 🔴 P0: Async Command Migration (3 files, 8 call sites)
| File | Change |
|---|---|
| [lifecycle.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/lifecycle.rs) | `tmux_ready()` and `ssh_probe()` → `async fn` with `tokio::process::Command` (SSH probe's 5s ConnectTimeout was the worst blocking offender). `pre_ignition_checks()` → `async fn`. `git_commit_report()` → wrapped in `tokio::task::spawn_blocking` |
| [agent.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/agent.rs) | `run_preflight()` → `pub async fn` with `tokio::process::Command`. 3 test functions updated to `#[tokio::test] async fn` |
| [gateway/mod.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/gateway/mod.rs) | `.unwrap()` on SocketAddr parse → `.expect("127.0.0.1:<port> is always a valid SocketAddr")` |

**Not migrated (intentionally):**
- `spawn.rs` — uses `pre_exec(setsid)` for daemon detach (no tokio equivalent)
- `bin/janus.rs` — synchronous CLI helper (not in async context)
- `tool_guard/webhook.rs` — already wrapped in `spawn_blocking` by the gateway
- `workflow/mod.rs` git operations — these are short-duration git commands inside the async workflow engine; `spawn_blocking` would add complexity with minimal benefit. Flagged for future conversion.

### 🔴 P0: Unwrap Audit
| Finding | Detail |
|---|---|
| **Initial count**: 142 | Grep was misleading — included test code within `#[cfg(test)]` modules |
| **Actual production unwraps**: ~1–2 | Almost all 142 are in test code or guaranteed invariants |
| **Fixed**: 1 | `gateway/mod.rs` SocketAddr parse → `expect()` with documented invariant |

### 🟡 P1: Version Bump
| File | Change |
|---|---|
| [Cargo.toml](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/Cargo.toml) | `version = "0.4.9"` → `"0.5.0"`, updated description |

### 🟡 P1: CI Improvements
| File | Change |
|---|---|
| [ci.yml](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/.github/workflows/ci.yml) | Added `cargo audit` dependency vulnerability scanning step. Updated header comment from 0.4.0 → 0.5.0 |

### 🟠 P2: Makefile Improvements
| File | Change |
|---|---|
| [Makefile](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/Makefile) | Updated header to 0.5.0. Added `make test`, `make lint`, `make ci` convenience targets. Updated bootstrap message. Added `.PHONY` declarations for new targets |

---

## Remaining Items (Not Applied)

| Item | Reason | Recommendation |
|---|---|---|
| Replace `sleep(12)` in tests | Integration tests may need careful polling logic design | Replace with UDS ping polling loop in a follow-up PR |
| Add macOS CI matrix | Requires macOS runner configuration | Add `runs-on: macos-latest` for compile+unit tests |
| Update docs/ path references | 14 docs need bulk path replacement (`blueprints/<name>/janus.toml` → `.janus/blueprint.toml`) | Batch update in a `docs:` commit |
| Add competitive analysis to PRD | Product content requires author input | Draft and review with stakeholders |
| Convert remaining `std::process::Command` in `workflow/mod.rs` | 3 git operations (short-duration, low risk) | Convert in a future `refactor:` PR |
