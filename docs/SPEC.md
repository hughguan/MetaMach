# MetaMach 0.5.0 — Target Specifications & Quality Matrix

> **Scope:** Unified technical specification converging Feature Specifications, Test Suite Specifications, Test Report, and Deployment/CI Specifications.  
> **Status:** Fully Implemented.  
> **Test Status:** ✅ **178 tests — 178 passed, 0 failed, 0 ignored** (131 unit + 47 integration).

---

# Part 1 — Feature Specifications & Data Contracts

## 1. UDS JSON Protocol Contracts (Contracts 3.1 – 3.11)

All communication between MetaMach binaries (`janush` proxy shell, `herdr-janus` TUI, `janus` CLI) and `janus-daemon` occurs over the Unix Domain Socket (`janus.sock`) via newline-delimited JSON.

```json
{"type": "GuardCheck", "agent": "builder", "command": "cargo build", "session_name": "metamach-task-123"}
{"type": "GuardVerdict", "verdict": "ALLOW", "rewritten_command": null}
```

| Contract | Message Type | Sender → Receiver | Payload / Description |
|---|---|---|---|
| **Contract 3.1** | `Ping` / `Pong` | Client → Daemon | Daemon liveness check + version info. |
| `GuardCheck` | `janush` → Daemon | `agent`, `command`, `session_name`, `target_sha`, `env_snapshot`. |
| `GuardVerdict` | Daemon → `janush` | Verdict (`ALLOW`, `BLOCK`, `REWRITE`), `rewritten_command`, `cognitive_context`. |
| `RegisterTenant` | `janus init` → Daemon | Registers blueprint name, default workflow, and validates PostgreSQL schema. |
| `Dispatch` | CLI/TUI → Daemon | Dispatches linear or DAG workflow by name onto Absurd PG. Returns `task_id`. |
| `Stop` | CLI/TUI → Daemon | Kills active tmux sessions for task and marks task `STOPPED`. |
| `Continue` | CLI/TUI → Daemon | Resumes stopped/crashed tasks via cold-start reconciliation. |
| `ProgressQuery` | TUI → Daemon | Queries active task execution state across per-blueprint databases. |

---

## 2. Tool Guard Security Rules & Webhook Dispatch

`janus::tool_guard` enforces role-based permission rules on every agent command before execution:

- **Rules Evaluation**:
  - `ALLOW`: Command passes through to `janush` and executes bare-metal in PTY.
  - `BLOCK`: Command is rejected with exit code `126` (fail-closed timeout = 30s `BLOCK`).
  - `REWRITE`: Command is safely modified (e.g. converting financial command to `--dry-run`).
- **HITL Interception**: Commands matching high-risk patterns (e.g. production deploy, `rm -rf /`, database drop) trigger `SUSPEND` state in Absurd PG and dispatch a Teams Adaptive Card via `janus::gateway`. Execution freezes until human approval or 15-minute deadline expiry (`410 Gone`).

---

## 3. Advanced Engine & Infrastructure Features (Contracts 4.1 – 4.3)

### Contract 4.1 — Init & Offboard Lifecycle
- `janus init`: Idempotent scaffold of `.janus/`, blueprint recipe validation, PG blueprint database initialization, tenant registration.
- `janus offboard`: Commits `production_report.md` to git, executes `melt_blueprint_data` (purges heavy JSON rows while retaining step audit logs), updates catalog status to `OFFBOARDED`.

### Contract 4.2 — Environmental Snapshot & Stream Filter
- **Environmental Snapshot (`004_env_snapshot.sql`)**: Captures `JANUS_ENV_TIMESTAMP` (ISO-8601 UTC) and `JANUS_ENV_TTY_DEVICES` on step start.
- **ANSI Stream Filter (`janus::workflow::filter`)**: Strips ANSI terminal escape sequences and collapses progress bar redrawing to enforce 16 KiB truncation budgets.

### Contract 4.3 — Pre-Flight Hardware Probes & Dual-Path Logging
- **Pre-Flight Probes**: Probes hardware interfaces (Serial, USB, SSH) before executing target hardware steps. Outcome: `Bypass`, `RequireApproval`, or `NoProbe`.
- **Dual-Path Log Pipeline**: Raw PTY output written to `/tmp/metamach/logs/<task_id>.log` (7-day GC); 16 KiB truncated tail stored transactionally in PostgreSQL `metamach_step_meta`.

---

# Part 2 — Validation & Test Suite Specifications

## 1. Integration Test Catalog (UTC Suite)

MetaMach maintains 178 automated tests across 8 integration test files and inline unit tests:

| Test ID | Module / File | Description | Target Contract | Severity |
|---|---|---|---|---|
| **UTC-01-01** | `uds_contract.rs` | Daemon binds physical socket `janus.sock` and PID lock file `janus.pid`; second launch refuses lock. | §3.1 Daemon socket | Blocker |
| **UTC-02-02** | `uds_contract.rs` | `janush` proxy shell intercepts ALLOW/BLOCK commands and enforces 30s fail-closed timeout. | §3.4 Fail-Closed | Blocker |
| **UTC-03-01** | `step_workflow.rs` | Step state transitions (`STARTING` → `RUNNING` → `COMPLETED`/`FAILED`). | §3.3 Step Workflow | Blocker |
| **UTC-03-03** | `step_workflow.rs` | Cold-start reconciliation: daemon restart resumes from last `COMPLETED` checkpoint. | §4.4 Cold-Start | Blocker |
| **UTC-04-01** | `onboard_lifecycle.rs` | HITL suspend preserves guard verdict scene; resume executes follow-on step. | §2.4 HITL Gateway | Critical |
| **UTC-05-01** | `onboard_lifecycle.rs` | Dual 16 KiB budget truncation at `janush` and `janus-daemon`. | §4.2 16KB Budget | Critical |
| **UTC-05-02** | `onboard_lifecycle.rs` | `janus offboard` smelts operational data and archives audit trail. | Contract 4.1 Offboard | Major |
| **UTC-10-02** | `gateway.rs` | HITL Teams Adaptive Card webhook callback validation and duplicate rejection (`409 Conflict`). | §2.4 HITL Gateway | Critical |
| **UTC-10-04** | `gateway.rs` | HMAC-SHA256 constant-time webhook signature verification. | §2.4 Security | Blocker |

---

## 2. End-to-End Workflow & Pipeline Test Specifications (UTC-E2E)

| ID | Test Scenario | Description | Key Verifications | Severity |
|---|---|---|---|---|
| **UTC-E2E-01** | `req2spec` Pipeline | 3-agent cross-review producing Architecture, Feature Spec, and Test Spec. | All steps reach `COMPLETED`, review loops converge, git commit created. | Blocker |
| **UTC-E2E-02** | `spec2software` Pipeline | Per-unit implementation cycle (BUILDER implements → TESTER validates → ARCHITECT reviews). | Milestone boundaries pause for Director approval, cold-start resume works mid-unit. | Blocker |
| **UTC-E2E-03** | `adr-process` Pipeline | Injection of mid-development Architecture Decision Records. | ADR transitions `PENDING` → `CLOSED:APPROVED`, docs updated and committed. | Critical |

---

# Part 3 — Test Report & Quality Verification

## 1. Test Execution Summary

- **Total Workspace Tests**: **178 passed, 0 failed, 0 ignored**
- **Test Breakdown**:
  - **Unit Tests (`janus/src/`)**: 131 tests
  - **Integration Tests (`janus/tests/`)**: 47 tests across 8 files
- **Execution Speed**: ~3.5 seconds total workspace test runtime.
- **Coverage Strategy**:
  - PG-gated tests use **runtime-skip** (detects PostgreSQL liveness, avoiding hard failures on environment mismatch).
  - All test waits use bounded polling loops (`wait_ready()`, 100ms interval, 5–15s max timeout). Hardcoded `sleep(12)` calls have been completely eliminated.

## 2. Integration Suite File Breakdown

| File | Test Count | Scope |
|---|---|---|
| `uds_contract.rs` | 9 | Daemon binding, PID lock, 30s timeout, protocol fuzzing, status CLI |
| `onboard_lifecycle.rs` | 8 | Init/Offboard, multi-DB fanout, incident inheritance, budget truncation |
| `step_workflow.rs` | 7 | Step state transitions, cold-start reconcile, concurrent workflow isolation |
| `e2e_pipeline.rs` | 6 | Multi-step workflows, DAG level barriers, failing node abortion, stop/continue |
| `config_contract.rs` | 6 | Herdr plugin manifest parsing, fallback paths, min version check |
| `protocol_contract.rs` | 5 | JSON serialization tag conventions, GuardCheck/Verdict round-tripping |
| `tmux.rs` | 4 | Tmux PTY session creation, remain-on-exit survival, pane output capture |
| `gateway.rs` | 2 | Webhook HTTP callbacks, constant-time HMAC validation |

---

# Part 4 — Deployment & System Operations

## 1. Prerequisites & Toolchain

| Dependency | Minimum Version | Requirement Details |
|---|---|---|
| **Rust** | 1.88+ (Edition 2024) | Compiled via `cargo build --release --locked`. |
| **PostgreSQL** | 16+ (Host-Native) | Unix socket only (`make db-init`). No Docker required. |
| **tmux** | 3.3+ | Isolated server `tmux -L metamach-tmux`. |
| **Herdr** | 0.7.3+ | Optional for daemon; required for `herdr-janus` TUI. |

---

## 2. Directory Isolation Architecture

MetaMach enforces strict separation between immutable binaries/code and mutable config/state:

```
${HERDR_PLUGIN_ROOT}         # Immutable (repo checkout / release build)
├── bin/                     #   janus, janus-daemon, herdr-janus, janush
└── templates/               #   scaffold templates (blueprint.toml, agents/, workflows/)

${HERDR_PLUGIN_CONFIG_DIR}   # Mutable Config (~/.config/herdr/plugins/config/metamach.janus)
├── agents.toml              #   global agent pool
└── offboard.toml            #   offboard policy

${HERDR_PLUGIN_STATE_DIR}    # Mutable State (~/.local/state/herdr/plugins/metamach.janus)
├── janus.sock               #   UDS socket (0600)
├── janus.pid                #   daemon process PID lock
├── fallback.db              #   SQLite ring buffer fallback
└── pg_socket/               #   PostgreSQL Unix socket
```

---

## 3. Makefile Operations & Pre-Push Hook

- `make bootstrap`: Full zero-dependency setup: prerequisites check → symlinks → compile release binaries → PostgreSQL native init (`db-init`).
- `make health`: Verifies PostgreSQL Unix socket liveness and daemon socket connectivity.
- **Pre-Push Git Hook (`scripts/pre-push`)**:
  - Automatically runs `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test`.
  - Includes **docs-only detection**: if a commit contains only markdown/documentation edits (`docs/`, `*.md`), heavy database test execution is skipped automatically.
