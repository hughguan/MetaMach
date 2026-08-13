# MetaMach 0.6.0 — Target Specifications & Quality Matrix

> **Scope:** Unified technical specification converging Feature Specifications, Test Suite Specifications, Test Report, and Deployment/CI Specifications.  
> **Status:** Fully Implemented.  
> **Test Status:** ✅ **205 tests — 205 passed, 0 failed, 0 ignored** (139 unit + 66 integration across 9 test files).

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
| **Contract 3.2** | `GuardCheck` / `GuardVerdict` | `janush` → Daemon | `agent`, `command`, `session_name`, `target_sha`, `env_snapshot`. |
| **Contract 3.3** | `RegisterTenant` | `janus init` → Daemon | Registers blueprint name, default workflow, and validates PostgreSQL schema. |
| **Contract 3.4** | `Dispatch` | CLI/TUI → Daemon | Dispatches linear or DAG workflow by name onto Absurd PG. Returns `task_id`. |
| **Contract 3.5** | `Stop` / `Continue` | CLI/TUI → Daemon | Kills active tmux sessions for task and marks task `STOPPED` / resumes task. |
| **Contract 3.6** | `ProgressQuery` | TUI → Daemon | Queries active task execution state across per-blueprint databases. |
| **Contract 3.7** | Blueprint Recipe | CLI/Daemon | Parsing and validation of `.janus/blueprint.toml` configuration. |
| **Contract 3.8** | Step Workflow | Engine/Daemon | Execution of workflow steps, step status transitions, and Absurd queue naming. |
| **Contract 3.9** | SQLite Fallback Ring | Adapter/Daemon | Ring buffer persistence (`fallback.db`) during PostgreSQL outages. |
| **Contract 3.10** | Offboard & Smelt | CLI/Daemon | Archival of audit trail, database smelting, and `production_report.md` git commit. |
| **Contract 3.11** | Cold-Start Reconcile | Daemon Boot | Recovery and rescheduling of interrupted tasks on daemon startup. |

---

## 2. Tool Guard Security Rules & Webhook Dispatch

`janus::tool_guard` enforces role-based permission rules on every agent command before execution:

- **Rules Evaluation**:
  - `ALLOW`: Command passes through to `janush` and executes bare-metal in PTY.
  - `BLOCK`: Command is rejected with exit code `126` (fail-closed timeout = 30s `BLOCK`).
  - `REWRITE`: Command is safely modified (e.g. converting financial command to `--dry-run`).
- **HITL Interception**: Commands matching high-risk patterns (e.g. production deploy, `rm -rf /`, database drop) trigger `SUSPEND` state in Absurd PG and dispatch a Teams Adaptive Card via `janus::gateway`. Execution freezes until human approval or 30-minute default deadline expiry (`410 Gone`, configurable via `JANUS_HITL_TIMEOUT_SECS`).

---

## 3. Advanced Engine & Infrastructure Features (Contracts 4.1 – 4.6)

### Contract 4.1 — Cognitive Provider SPI (`janus::cognitive`)
- Advisory-only SPI for external Model Context Protocol (MCP) servers (e.g. `codebase-memory-mcp`).
- Standard 2-second fail-open timeout: provider timeouts or unreachability automatically fall back to standard Tool Guard rules without blocking execution.

### Contract 4.2 — Model Context Protocol Integration & OpenWiki RAG
- Connects MCP tool discovery and semantic codebase context to agent execution pipelines.
- Supplemented by `.janus/openwiki/` RAG knowledge scopes.

### Contract 4.3 — HITL Gateway & Webhook Integration (`janus::gateway`)
- Microsoft Teams Adaptive Card (1.4 JSON schema) dispatch for high-risk human approval requests.
- HMAC-SHA256 signature verification (`X-MetaMach-Signature`) and loopback HTTP callback listener (`:9090` / `/api/v1/hitl/verdict`).
- Idempotent handling: duplicate verdicts return `409 Conflict`.

### Contract 4.4 — Init & Offboard Lifecycle
- `janus init`: Idempotent scaffold of `.janus/`, blueprint recipe validation, PG blueprint database initialization, tenant registration.
- `janus offboard`: Commits `production_report.md` to git, executes `melt_blueprint_data` (purges heavy JSON rows while retaining step audit logs), updates catalog status to `OFFBOARDED`.

### Contract 4.5 — Environmental Snapshot & Stream Filter
- **Environmental Snapshot (`004_env_snapshot.sql`)**: Captures `JANUS_ENV_TIMESTAMP` (ISO-8601 UTC) and `JANUS_ENV_TTY_DEVICES` on step start.
- **ANSI Stream Filter (`janus::workflow::filter`)**: Strips ANSI terminal escape sequences and collapses progress bar redrawing to enforce 16 KiB truncation budgets.

### Contract 4.6 — Pre-Flight Hardware Probes & Dual-Path Logging
- **Pre-Flight Probes**: Probes hardware interfaces (Serial, USB, SSH) before executing target hardware steps. Outcome: `Bypass`, `RequireApproval`, or `NoProbe`.
- **Dual-Path Log Pipeline**: Raw PTY output written to `/tmp/metamach/logs/<task_id>.log` (7-day GC); 16 KiB truncated tail stored transactionally in PostgreSQL `metamach_step_meta`.

### Contract 4.7 — Typed Context Envelopes (ADR-034)
- **Typed Checkpoint Envelope**: Enforces compile-time type safety and Serde schema validation (`TypedEnvelope`, `CheckpointEnvelope`, `EnvelopeBase`, `BuildEnvelope`, `TestEnvelope`) for PostgreSQL task checkpoints.
- **Legacy Checkpoint Fallback**: Automatically decodes pre-0.7.0 unconstrained JSON checkpoints for backward compatibility.
- **Scene Isolation**: Envelopes govern persistent Absurd PG task checkpoints (`absurd.c_<queue>`), while progress scene rendering continues using truncated `stdout_tail` under the 16 KiB budget (ADR-008).

### Contract 4.8 — Pluggable Credential Provisioning SPI (ADR-036 Phase 1)
- **CredentialProvider SPI (`janus::credential`)**: Vendor-agnostic trait (`provision`, `revoke`, `cleanup_sweep`) for dynamic, scoped API keys and token lifecycle management.
- **BoxFut Async Conventions**: Implements `Pin<Box<dyn Future>>` manual desugaring for codebase consistency with `DurableEngine`.
- **Cold-Start Sweep (`janus::coldstart::reconcile_credentials`)**: Daemon cold start scans active credential records and revokes orphaned keys from dead or expired tasks.

### Contract 4.9 — Augmented Cold Retry with Correction Context (ADR-035)
- **Correction Context Injection**: On step failure or envelope validation error, re-dispatches step in a fresh tmux session with `METAMACH_CORRECTION_CONTEXT` environment variable.
- **Configurable Attempt Limit**: Step DSL supports `max_correction_attempts: Option<u32>` (defaults to 3 if omitted).
- **Agent Self-Healing**: Prompts reference `$METAMACH_CORRECTION_CONTEXT` for targeted self-correction without persistent interactive session dependencies.

### Contract 4.10 — Dual-Track Execution Isolation & Post-Execution Writes Guard (ADR-033)
- **DSL Dual-Track Additions**: `WorkflowStep`, `DagNodeDef`, and `PipelineNode` support optional `isolation` (`"sandbox"` / `"bare_metal"`), `best_of_n`, and `writes` whitelist paths.
- **Post-Execution Writes Guard (`verify_post_execution_writes`)**: Verifies post-execution workspace diffs against allowed `writes` scope.
- **HITL Escalation & Recovery Ref**: Unauthorized file modifications snapshot to `refs/metamach/rollback/<task_id>-<step_name>`, suspend the step, and trigger HITL escalation without destructive auto-rollback.

### Contract 4.11 — Herdr TUI Harvest Pipeline (ADR-036 Phase 2)
- **Sandbox Diff Collection (`harvest_sandbox_output`)**: Collects working directory diffs into stash/tree commits under Git ref `refs/sandbox/<task_id>-<step_name>`.
- **Harvest Ref Management (`list_harvest_refs` & `merge_harvest_ref`)**: Discovers harvested sandbox refs and merges approved sandbox outputs back into `HEAD`.

---

# Part 2 — Validation & Test Suite Specifications

## 1. Integration Test Catalog (UTC Suite)

MetaMach maintains 205 automated tests across 9 integration test files and inline unit tests:

| Test ID | Module / File | Description | Target Contract | Severity |
|---|---|---|---|---|
| **UTC-01-01** | `uds_contract.rs` | Daemon binds physical socket `janus.sock` and PID lock file `janus.pid`; second launch refuses lock. | Contract 3.1 | Blocker |
| **UTC-02-02** | `uds_contract.rs` | `janush` proxy shell intercepts ALLOW/BLOCK commands and enforces 30s fail-closed timeout. | Contract 3.2 | Blocker |
| **UTC-03-01** | `step_workflow.rs` | Step state transitions (`STARTING` → `RUNNING` → `COMPLETED`/`FAILED`). | Contract 3.8 | Blocker |
| **UTC-03-03** | `step_workflow.rs` | Cold-start reconciliation: daemon restart resumes from last `COMPLETED` checkpoint. | Contract 3.11 | Blocker |
| **UTC-04-01** | `onboard_lifecycle.rs` | HITL suspend preserves guard verdict scene; resume executes follow-on step. | Contract 4.3 | Critical |
| **UTC-05-01** | `onboard_lifecycle.rs` | Dual 16 KiB budget truncation at `janush` and `janus-daemon`. | Contract 4.5 | Critical |
| **UTC-05-02** | `onboard_lifecycle.rs` | `janus offboard` smelts operational data and archives audit trail. | Contract 4.4 | Major |
| **UTC-10-02** | `gateway.rs` | HITL Teams Adaptive Card webhook callback validation and duplicate rejection (`409 Conflict`). | Contract 4.3 | Critical |
| **UTC-10-04** | `gateway.rs` | HMAC-SHA256 constant-time webhook signature verification. | Contract 4.3 | Blocker |
| **UTC-33-01** | `protocol_contract.rs` | Dual-track execution isolation, WorkflowStep writes parsing, and post-execution writes guard contract. | Contract 4.10 | Major |
| **UTC-34-01** | `protocol_contract.rs` | Typed checkpoint envelope roundtrip serialization, legacy fallback, and domain envelope validation. | Contract 4.7 | Major |
| **UTC-35-01** | `step_workflow.rs` | Augmented Cold Retry WorkflowStep max_correction_attempts parsing, defaulting, and retry contract. | Contract 4.9 | Major |
| **UTC-36-01** | `protocol_contract.rs` | CredentialProvider SPI lifecycle, provisioning, revocation, and cold-start sweep contract. | Contract 4.8 | Major |
| **UTC-36-02** | `protocol_contract.rs` | Herdr TUI harvest pipeline sandbox diff collection, ref listing, and merge contract. | Contract 4.11 | Major |

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

- **Total Workspace Tests**: **205 passed, 0 failed, 0 ignored**
- **Test Breakdown**:
  - **Unit Tests (`janus/src/`)**: 139 tests (129 lib + 10 binary unit tests)
  - **Integration Tests (`janus/tests/`)**: 66 tests across 9 files
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
