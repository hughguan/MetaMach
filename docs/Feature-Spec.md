# MetaMach 0.3.0 — Engineering Feature Specification

> Cognitive scheduler, agent sandbox (janush), durable workflows, data contracts, and fault matrix.

## 1. Module Architecture Overview

Following Herdr 0.7.3 plugin specifications and the system's independent resident process design, MetaMach 0.3.0 software is composed of four core functional layers. This specification strictly adheres to the Immutable ROOT vs. Mutable State separation and pixel-level defines behavioral boundaries, data flows, and exception handling for each feature.

```
+-----------------------------------------------------------------------------------------+
|                                METAMACH CORE LAYERS                                      |
+-----------------------------------------------------------------------------------------+
|  1. CONTROL:   janus-daemon (resident UDS service) & herdr-janus (Popup shadow client)   |
|  2. SANDBOX:   janush (proxy shell) & Event-Driven Tool Guard (synchronous kernel guard) |
|  3. WORKFLOW:  Absurd Postgres (One PG, Multi-DB) & janus::tmux (remain-on-exit)       |
|  4. KNOWLEDGE: Federated OpenWiki Skill & Trace Purge & Audit Archive (DELETE + archive)  |
+-----------------------------------------------------------------------------------------+
```

## 2. Core Feature Specifications

### Feature 2.1: Singleton Resident Control Hub (Janus Daemon & Twin-Client UI)

- **Description:** Implement a long-running background daemon `janus-daemon` for centralized state management, and a lightweight Herdr interaction shell `herdr-janus` responsible for Popup rendering.

- **Technical Spec:**
    - `janus-daemon` binds a unique Unix Domain Socket listener at `${HERDR_PLUGIN_STATE_DIR}/janus.sock` on startup.
    - **Lazy-Start Self-Healing:** When `herdr-janus` is awakened, if `janus.sock` is absent or connection times out, it must seamlessly auto-launch the background daemon. **Use `std::process::Command::spawn()` with explicit detach** (`stdin/stdout/stderr` redirected to `/dev/null`, `pre_exec` calls `setsid` to detach from controlling terminal)—not raw `fork()`+`exec()`, which is explicitly discouraged on macOS (see `man fork`) and not cross-platform. On spawn failure (resource exhaustion, etc.), report error to the Factory Director rather than silently crashing.
    - **UI Popup Constraints:** Via `herdr-plugin.toml`'s `[[panes]] placement = "overlay"` (validated Herdr 0.7.3 directive; `popup`/`width`/`height` are **not** valid manifest fields - see `docs/herdr-v1-contract.md`), open the `herdr-janus` shadow client in a Herdr overlay pane. Pane sizing is managed by Herdr; the ratatui app renders within it. Use `ratatui` as pure keyboard UI rendering engine; input focus auto-locked; pressing `Esc` reports to Daemon via the shadow client then safely pops the stack.
    - **PG-Unreachable Startup Self-Healing:** Daemon retries Absurd Postgres connection with exponential backoff on startup (5 attempts, 2s interval). If still unreachable, enters **degraded mode**: writes only to local `fallback.db` (ring buffer), logs `WARN`, and notifies the Factory Director via Popup: "DB offline, running degraded." All Step state changes during this period land in `fallback.db`; upon detecting PG recovery, auto-triggers batch Log Replay merging into the primary database (consistent with §4 fault matrix), keeping the workshop running.

### Feature 2.2: In-Memory Agent Sandbox (janush & Tool Guard)

- **Description:** Provide an out-of-process, independently audited security gate that performs synchronous physical interception before Agent intent reaches Bash, supporting Dry-Run redirection for special operations.

- **Technical Spec:**
    - **Shell Interception Proxy (janush):** When tmux launches a pane, it forcibly injects the `SHELL` environment variable to point to the **absolute path** of `janush` (`${HERDR_PLUGIN_ROOT}/bin/janush`, installed by `make bootstrap`, see Deployment-Spec §5.1). **Never use the relative path `target/release/janush`**—it fails when CWD is not the repo root, the binary is not yet compiled, or the pane is started from a different directory.
    - **Synchronous UDS Reconciliation:** When an Agent sends any command-line string, `janush` synchronously suspends execution, packaging the raw `argv` array and dispatching it to `janus.sock`.
    - **Timeout & Deadlock Prevention:** `janush` blocks synchronously waiting for the Daemon verdict with a configurable timeout (default 30s). If the Daemon crashes or the UDS breaks causing a timeout, **fail-closed**: return an error to the Agent without executing the command (never let through), preventing the Agent's shell from hanging indefinitely due to Daemon unreachability. Verdict response format and semantics are defined in Contract 3.4.
    - **Dry-Run Redirection:** The Daemon's Tool Guard module performs security review against core allowlist commands (e.g., commands that modify system-level configuration or flash hardware). For financial-class high-risk operations, forces rewriting `argv` to `--action dry-run` before delivering to the host shell. This redirection is synchronous and transparent to the Agent.

### Feature 2.3: Durable Workflow State Machine (tmux & Absurd Engine)

- **Description:** Provide a durable, self-healing multi-station pipeline engine combining the internalized `janus::tmux` (physical session immortality) and the Absurd Postgres transaction engine (state checkpointing).

- **Technical Spec:**
    - **Task Lifecycle:** On dispatch, the Daemon creates a task via `absurd.spawn_task` with `status = 'PENDING'`. Before the first Step begins, it transitions to `STARTING`. When the first Step starts executing, the task transitions to `RUNNING`. Terminal states are `COMPLETED`, `FAILED`, or `SUSPENDED`.
    - **Step Lifecycle:** Each Step independently transitions through `PENDING → STARTING → RUNNING → COMPLETED | FAILED | SUSPENDED`. The `STARTING` state is a brief transitional gate (establishes tmux session, writes pre-flight log); the `RUNNING` state indicates the Agent's command is actively executing in the tmux pane.
    - **Multi-DB Fan-Out:** At dispatch, `janus-daemon` connects to the target blueprint's dedicated database (`metamach_blueprint_<name>`) and spawns a task via `absurd.spawn_task`, writes step checkpoints via `set_task_checkpoint_state`, and adds a `metamach_step_meta` overlay row per step. The global catalog DB (`metamach_db`) tracks only `blueprints` metadata and `absurd_audit_log` entries.
    - **janus::tmux (Internalized):** Physical session management is handled by `janus::tmux`, a native Rust module inside `janus-daemon`. It directly creates and manages tmux `remain-on-exit` sessions (socket `-L metamach-tmux`) and cross-host SSH transport. The external `herdr-tether` plugin is deprecated and no longer required.
    - **Optimistic Locking (target_sha):** At dispatch, the Daemon pins the blueprint repo's current Git `HEAD` SHA-1 into `target_sha` and sends it as `dispatch_sha` in the step payload. On remote report return, the Daemon compares `report.dispatch_sha == current HEAD` — a mismatch means `HEAD` advanced, so the report is stale: discard, mark `SUSPENDED`, emit `CONCURRENCY_RACE_ALERT`, and auto-reschedule. See Contract 3.1 and Contract 3.5 for the full payload contracts.

### Feature 2.4: Human-in-the-Loop (HITL) Gate

- **Description:** Provide non-destructive suspension and asynchronous bidirectional approval for steps that encounter insurmountable obstacles (compile errors, privilege violations, pin conflicts).

- **Technical Spec:**
    - **Non-Destructive Suspension:** When a step fails or triggers a Tool Guard fuse, the Daemon transitions the step to `SUSPENDED` in the database. The physical tmux session is **never killed** — the terminal scene (error messages, memory variables, console cache) is preserved intact.
    - **Multi-Endpoint HITL:** The Daemon sends a mobile alert card via Telegram and/or Teams webhooks. The card includes a Correlation ID, task context, and an interactive `[Resume]` button. Tapping `[Resume]` sends a signed callback to the Daemon, which verifies the Correlation ID and dispatches the next step.
    - **In-Place Fix Workflow:** The Factory Director can also `attach` into the still-alive tmux pane, fix the issue in-place, and type `metamach-resume` (or click `[Resume]` in TUI) to signal completion. The Daemon verifies the step is `SUSPENDED` for the current blueprint, transitions it to `COMPLETED`, and dispatches the next step.

### Feature 2.5: Federated Lifecycle Smelter (Onboard / Offboard & Trace Purge)

- **Description:** Provide a complete product lifecycle engine: Onboard (CREATE DATABASE + tenant registration), Offboard (DELETE + audit archive + LLM smelting), and re-Onboard with experience inheritance.

- **Technical Spec:**
    - **Onboard Registration Mechanism:** When the Factory Director executes `janus onboard --blueprint <name>`, the Daemon takes over with the following sequence:
        1. **Recipe Validation:** Read `blueprints/<name>/janus.toml`, validate required fields. Confirm `workflows/<default_workflow>.toml` exists. Validation failure returns a clear error without writing to the database.
        2. **Pre-Ignition Self-Check:** Probe Absurd Postgres connectivity and tmux engine readiness. If `[remote]` is declared, perform a best-effort `BatchMode` connectivity probe (`-o ConnectTimeout=5`); unreachable only logs `WARN`, does not block Onboard.
        3. **Per-Blueprint Database Creation:** Execute `CREATE DATABASE metamach_blueprint_<name>` to allocate an independent logical database for the new blueprint (One PG, Multi-DB topology). Blueprint name is validated (max 60 chars, alphanumeric + underscore). On `42P04` (duplicate database), treat as idempotent — the database already exists from a prior Onboard. Since `CREATE DATABASE` cannot run inside a transaction block, the Daemon orchestrates compensation: if a later step fails, it executes `DROP DATABASE metamach_blueprint_<name>` as cleanup.
        4. **Tenant Registration (Idempotent):** `INSERT INTO blueprints … ON CONFLICT (name) DO UPDATE SET status='ACTIVE' …` in the global catalog DB. Repeated Onboard has no side effects; re-onboarding an already `OFFBOARDED` blueprint reactivates it.
        5. **Workflow Binding:** Persist `default_workflow` and precompile-validate the Step sequence.
        6. **Knowledge Graph Loading & Experience Inheritance:** Index `blueprints/<name>/openwiki/` into OpenWiki. If `production_report.md` exists, parse its structured blocks and append critical failure patterns as `## Previous Incidents` into the Agent System Prompt template.

    - **Offboard Trace Purge & Audit Archive:** When the Factory Director executes `janus offboard --blueprint <name>`:
        1. **Trace Extraction:** Daemon scans the blueprint's dedicated database, extracting all historical Task and Step execution traces and Tool Guard interception logs.
        2. **LLM Smelting (async):** Calls the configured LLM (see "Offboard LLM Integration Spec" below) to compress execution snapshots into `production_report.md`. When the LLM is unavailable, writes `production_report.raw.json` and logs `WARN` — does not block Offboard.
        3. **DELETE + Audit Archive (8a):** The Daemon orchestrates a multi-DB sequence: (a) in the blueprint's dedicated database, call absurd `cleanup_tasks` to purge task/checkpoint data and `DELETE FROM metamach_step_meta` for the blueprint overlay rows; (b) in the global catalog DB, `INSERT INTO absurd_audit_log` one row per offboarded task with full trace metadata (task_id, blueprint_name, workflow_name, step_count, elapsed_seconds, offboarded_at); (c) `UPDATE blueprints SET status = 'OFFBOARDED', offboarded_at = NOW() WHERE name = <name>`. The per-blueprint database is retained (not dropped) — schema and audit history survive for forensic review.
        4. **Git Commit (8c):** Via the shadow client, invokes local Git to auto-incrementally commit and push `production_report.md`.

    - **Offboard LLM Integration Spec:** The LLM used for smelting is a configurable external dependency:
        - **Endpoint Config:** `configs/offboard.toml` declares `endpoint`, `api_key_env` (environment variable name; key never touches disk), `model`, `max_input_tokens`.
        - **Input Budget:** Only takes the most recent N Steps (default N=50), each `result_cache` truncated to 16KB; total input constrained by `max_input_tokens`.
        - **Prompt Template:** Forces structured output of four blocks — [Compile Error History], [Pin Conflict Details], [Tool Guard Interception Logs], [Successful Patches Applied].
        - **Degradation Fallback:** When the LLM is unavailable or exceeds 120s timeout, writes `production_report.raw.json` and logs `WARN` — does not block Offboard.
        - **Async Execution:** The Offboard command immediately returns "Smelting in progress"; LLM summarization runs in the background; when ready, a UDS event notifies `herdr-janus` and completes the Git commit.

### Feature 2.6: Workflow Progress Dashboard (Workflow Monitor & Status Query)

- **Description:** Provide the Factory Director with real-time visibility into in-flight workflows, answering "Is it still running? / Which step is it stuck on? / Normal or deadlocked?" Aggregates authoritative state from Absurd Postgres via a read-only bypass without interfering with workflow execution channels.

- **Technical Spec:**
    - **Dual-View Popup:** `herdr-janus`'s Popup adds a "Progress" view alongside the existing "Dispatch" view, toggled via the `Tab` key after waking with `prefix+j`. The Progress view renders the in-flight task matrix grouped by blueprint using `ratatui` tables: owning blueprint · workflow name · current step · per-step status (`PENDING` / `STARTING` / `RUNNING` / `COMPLETED` / `SUSPENDED` / `FAILED`) · elapsed time · most recent stdout summary (truncated to 1KB).
    - **`janus status` CLI Fallback:** In non-TUI environments (SSH / CI), `janus status [--blueprint <name>] [--json]` uses the same `progress` primitive, outputting plain-text or JSON snapshots for scriptable inspection.

## 3. System Data Exchange Contracts

### Contract 3.1: Global Catalog Schema (`metamach_db`)

The global catalog database (`metamach_db`) is the single source of truth for blueprint metadata and audit trails. Per-blueprint operational data resides in independent databases (`metamach_blueprint_<name>`) — see Contract 3.1b.

```sql
-- =================================================================
-- GLOBAL CATALOG DB (metamach_db): blueprint registry + audit log
-- =================================================================

-- Blueprint tenant registry (Onboard writes / Offboard sets OFFBOARDED)
CREATE TABLE blueprints (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) UNIQUE NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',  -- ACTIVE | OFFBOARDED
    default_workflow VARCHAR(100) NOT NULL,
    config JSONB,                                   -- janus.toml verbatim
    openwiki_scope JSONB,                           -- [openwiki].scope index range
    remote_host VARCHAR(100),                       -- [remote].host (NULL = local-only)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    onboarded_at TIMESTAMPTZ,                       -- most recent Onboard time
    offboarded_at TIMESTAMPTZ                       -- most recent Offboard time
);

-- Global audit log: full traces for every offboarded task
-- (written by Daemon during Offboard; never pruned). task_id is UUID to match
-- absurd.spawn_task() output (M0.5 spike F1).
CREATE TABLE absurd_audit_log (
    id SERIAL PRIMARY KEY,
    task_id UUID NOT NULL,
    blueprint_name VARCHAR(100) NOT NULL,
    workflow_name VARCHAR(100) NOT NULL,
    step_count INTEGER NOT NULL,
    elapsed_seconds DOUBLE PRECISION,
    offboarded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    trace_summary JSONB       -- full trace metadata: step names, statuses, timestamps
);
CREATE INDEX idx_audit_blueprint ON absurd_audit_log(blueprint_name);
CREATE INDEX idx_audit_task ON absurd_audit_log(task_id);
```

### Contract 3.1b: Per-Blueprint Schema (`metamach_blueprint_<name>`)

Each blueprint gets its own dedicated database. **The durable-execution engine is [Absurd](https://github.com/earendil-works/absurd) (v0.4.0)**, installed into the per-blueprint DB via `absurdctl init` (applies `absurd.sql` into the `absurd` schema). Absurd owns the task / checkpoint / event tables and the state-machine functions; MetaMach **does not** redefine them - parallel `absurd_tasks`/`absurd_steps` tables would conflict with absurd's functions (M0.5 spike F1). MetaMach adds only a thin overlay table for fields absurd has no concept of.

**Concept mapping (MetaMach -> Absurd primitive):**

| MetaMach concept | Absurd primitive |
|---|---|
| a workflow dispatch | `spawn_task(queue, task_name, params)` -> `task_id uuid`, `run_id uuid`; `claim_task` / `complete_run` / `fail_run` |
| `result_cache` (16KB) + step `status` | `set_task_checkpoint_state(queue, task_id, step_name, state jsonb, owner_run)` / `get_task_checkpoint_state` |
| `SUSPENDED` (HITL gate) | `await_event` -> `should_suspend=true`; `emit_event` resumes |
| cold-start self-heal (no tmux-resurrect) | native: checkpoints re-read via `get_task_checkpoint_states` |
| Janus GC (3-day cleanup) | `cleanup_tasks` / `cleanup_all_queues` |
| Onboard tenant | `create_queue('<blueprint>.<workflow>')` per workflow |
| `CONCURRENCY_RACE_ALERT` reschedule | `retry_task` / new `spawn_task` |

`task_id` is a **UUID** (absurd's `spawn_task` output), not an integer. No cross-DB foreign keys - the Daemon resolves `blueprint_name` at the application layer. `status` + `result_cache` live in absurd's checkpoint `state` JSONB; the overlay below carries only MetaMach-specific fields.

```sql
-- =================================================================
-- PER-BLUEPRINT DB (metamach_blueprint_<name>)
-- =================================================================
-- 1. `absurdctl init` applies absurd.sql (absurd schema: task/checkpoint/event
--    tables + durable-execution functions). External engine install - not shown.
-- 2. This migration adds only the MetaMach overlay (002_blueprint.sql):

-- MetaMach step-meta overlay, keyed by absurd's UUID task_id + step_name.
-- Carries fields absurd has no concept of. status + result_cache live in
-- absurd's checkpoint state JSONB (set_task_checkpoint_state).
CREATE TABLE metamach_step_meta (
    task_id         UUID NOT NULL,                       -- absurd.spawn_task().task_id
    step_name       VARCHAR(100) NOT NULL,
    blueprint_name  VARCHAR(100) NOT NULL,               -- denormalized; no cross-DB FK
    target_sha      VARCHAR(64) NOT NULL DEFAULT '0000000000000000000000000000000000000000',
                                                          -- Optimistic lock: Git HEAD pinned at dispatch.
                                                          -- All-zeros sentinel = non-git blueprint (lock skipped).
                                                          -- VARCHAR(64) supports SHA-256.
    exit_code       INTEGER,                             -- NULL until step completes
    stdout_tail     TEXT,                                -- most recent ~1KB terminal snapshot
    started_at      TIMESTAMPTZ,                         -- when the step transitioned to RUNNING
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (task_id, step_name)
);
CREATE INDEX idx_step_meta_blueprint ON metamach_step_meta(blueprint_name);
```

> **Optimistic Locking - `target_sha` (app-layer; F4):** At dispatch, the Daemon pins the blueprint repo's current Git `HEAD` into `metamach_step_meta.target_sha` and sends it as `dispatch_sha` in the step payload (see Contract 3.5). The remote report echoes `dispatch_sha` back. On return, the Daemon compares `report.dispatch_sha == current HEAD` - a mismatch means `HEAD` advanced since dispatch (concurrent commit), so the report is stale: discard it, mark the step `SUSPENDED`, emit a `CONCURRENCY_RACE_ALERT` via the HITL channel, and auto-reschedule against the new `HEAD`. Absurd has no SHA-lock concept (its checkpoint write is its own upsert), so the guard is an application-level `UPDATE metamach_step_meta SET exit_code = $2, stdout_tail = $3, started_at = $4 WHERE task_id = $5 AND step_name = $6 AND target_sha = $7` (`$7 = report.dispatch_sha`) executed by the Rust daemon **before** calling absurd's `complete_run` / `set_task_checkpoint_state`. **Reschedule semantics:** a reschedule calls `spawn_task` again (new UUID `task_id`) rather than mutating the existing row's `target_sha`, preserving the old task's audit trail. The `target_sha` column ships in `002_blueprint.sql`; dispatch-time pinning, the remote-report contract, and the auto-reschedule engine are provided by `janus::tmux` and enforced in M4 Task 4.4.

> **Unified Status Enumeration:** The system-wide Step/Task state machine is `PENDING -> STARTING -> RUNNING -> COMPLETED | FAILED | SUSPENDED`. The `STARTING` state is a brief transitional gate (tmux session creation, pre-flight log); the `RUNNING` state indicates the Agent's command is actively executing. The Blueprint-level state machine is `ACTIVE <-> OFFBOARDED` (Onboard activates / Offboard archives).

### Contract 3.2: Proxy Shell Sync UDS Request Payload (janush → Daemon)

```json
{
  "execution_id": "0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a",
  "blueprint_id": "gatemetric",
  "task_id": 1042,
  "step_name": "cross_compile",
  "cwd": "/workspaces/metamach/blueprints/gatemetric/firmware",
  "argv": ["esptool.py", "--chip", "esp32", "write_flash", "0x1000", "firmware.bin"],
  "env_snapshot": {
    "USER": "factory_agent",
    "SHELL": "${HERDR_PLUGIN_ROOT}/bin/janush"
  }
}
```

### Contract 3.3: Workflow Progress Query Response Payload (Daemon → herdr-janus / `janus status`)

```json
{
  "active_tasks": [
    {
      "task_id": "0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a",
      "blueprint_id": "gatemetric",
      "workflow_name": "dev-flow",
      "status": "RUNNING",
      "started_at": "2026-07-15T09:00:00Z",
      "elapsed_seconds": 945,
      "current_step": "cross_compile",
      "tmux_alive": true,
      "suspended_reason": null,
      "steps": [
        {"name": "scout",         "status": "COMPLETED", "exit_code": 0},
        {"name": "code",          "status": "COMPLETED", "exit_code": 0},
        {"name": "cross_compile", "status": "RUNNING",  "stdout_tail": "…latest 1KB terminal summary…"}
      ]
    }
  ]
}
```

> `active_tasks` only includes non-terminal tasks (`STARTING` / `RUNNING` / `SUSPENDED`). `stdout_tail` is the most recent terminal output truncated to 1KB summary; `tmux_alive` reflects whether the corresponding tmux physical Session is alive. This payload drives both Popup progress dashboard rendering and `janus status --json` CLI output.

### Contract 3.4: Proxy Shell Sync UDS Response (Daemon → janush)

```json
{
  "execution_id": "0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a",
  "verdict": "ALLOW",
  "reason": "financial_trade_requires_approval",
  "rewritten_argv": null
}
```

```json
{
  "execution_id": "0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a",
  "verdict": "REWRITE",
  "reason": "financial_trade_requires_dry_run",
  "rewritten_argv": ["hi5bot", "--action", "dry-run"]
}
```

```json
{
  "execution_id": "0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a",
  "verdict": "BLOCK",
  "reason": "blacklist_match: rm -rf /",
  "rewritten_argv": null
}
```

> **Verdict semantics:** `ALLOW` — execute the original argv as-is; `REWRITE` — execute `rewritten_argv` instead; `BLOCK` — return an error to the Agent, do not execute anything. The 30s fail-closed timeout applies to the entire round-trip.

### Contract 3.5: Step Dispatch & Remote Report Payloads (Daemon ↔ janus::tmux)

```json
// Dispatch payload (Daemon → tmux → remote host)
{
  "dispatch_sha": "a1b2c3d4e5f6...",
  "task_id": 1042,
  "step_name": "cross_compile",
  "blueprint_name": "gatemetric",
  "command": "make cross-compile",
  "cwd": "/workspaces/metamach/blueprints/gatemetric/firmware",
  "env": {
    "SHELL": "/path/to/bin/janush",
    "METAMACH_TASK_ID": "1042"
  }
}

// Remote report payload (remote host → tmux → Daemon)
{
  "dispatch_sha": "a1b2c3d4e5f6...",
  "task_id": 1042,
  "step_name": "cross_compile",
  "exit_code": 0,
  "stdout_tail": "…last 1KB of terminal output…",
  "result_cache": { "binary_size": 123456, "warnings": 0 },
  "completed_at": "2026-07-15T09:12:00Z"
}
```

> The `dispatch_sha` field is the optimistic-locking token. The remote host echoes it back verbatim in the report. The Daemon verifies `report.dispatch_sha == current HEAD` before applying the result. Mismatch → stale report → discard + reschedule.

### Contract 3.6: Agent Pool Registration Schema (`configs/agents.toml`)

```toml
[agent.scout]
permissions      = ["read", "grep", "find"]
allow_network    = false
bash_safe        = true
bash_blacklist   = []

[agent.coder]
permissions      = ["read", "write", "edit", "bash-safe", "git-commit"]
allow_network    = false
bash_safe        = true
bash_blacklist   = ["rm -rf /", "> /dev/sd*", "mkfs.*", "dd if=* of=/dev/*"]

[agent.deployer]
permissions      = ["read", "write", "bash-full", "ssh", "git-push"]
allow_network    = true
require_approval = ["esptool.py write_flash", "make flash", "*production*"]
```

> **Decision Priority:** Tool Guard evaluates each `janush`-reported argv in order — (1) `bash_blacklist` hit → `BLOCK`; (2) `require_approval` hit → `BLOCK` and set `SUSPENDED` awaiting HITL; (3) command capability not in current role `permissions` allowlist → `BLOCK`; (4) financial-class high-risk command → `REWRITE` to Dry-Run; (5) remainder → `ALLOW`. Rules are configurable (not hardcoded Rust); Daemon loads via `configs/agents.toml` symlink (Mutable Config zone).

### Contract 3.7: Blueprint Recipe Schema (`blueprints/<name>/janus.toml`)

```toml
[blueprint]
name = "gatemetric"
default_workflow = "firmware-deploy"

[remote]                           # Optional; omit for local-only blueprints (e.g., joyrobots)
host = "192.168.1.100"
user = "builder"

[openwiki]
scope = ["mpu6050", "esp32-timers", "i2c-conflicts"]
```

> On Onboard, the Daemon strictly validates `blueprint.name`, `blueprint.default_workflow` (corresponding file must exist), and `openwiki.scope`; `[remote]` absent = local-only blueprint. Validation failure prevents Onboard.

### Contract 3.8: Workflow Pipeline Schema (`workflows/<name>.toml`)

```toml
[workflow]
name = "firmware-deploy"
description = "Standard firmware cross-compile & flash pipeline"

[[steps]]                          # Ordered station chain
name = "scout"
agent = "scout"                    # References configs/agents.toml role
toolset = ["read", "grep", "find"]

[[steps]]
name = "code"
agent = "coder"
toolset = ["read", "write", "edit", "bash-safe"]

[[steps]]
name = "cross-compile"
agent = "deployer"
command = "make cross-compile"
host = "remote"                    # References janus.toml [remote]; default = local
toolset = ["bash-full", "ssh"]
```

> Each `[[steps]]` declares at minimum `name`, `agent`, `toolset`; `command` is the concrete instruction executed at that station; `host` references the blueprint's `[remote]` for cross-host steps. The Daemon dispatches steps sequentially in array order, writing one checkpoint per step via `set_task_checkpoint_state` (plus a `metamach_step_meta` overlay row).

### Contract 3.9: Offboard Configuration Schema (`configs/offboard.toml`)

```toml
[offboard]
endpoint = "https://api.openai.com/v1/chat/completions"
api_key_env = "OPENAI_API_KEY"
model = "gpt-4o"
max_input_tokens = 128000
max_steps = 50
timeout_seconds = 120
```

> The `api_key_env` field references an environment variable name; the actual key never touches disk. If the file is absent, Offboard skips LLM smelting and writes `production_report.raw.json`.

### Contract 3.10: Disaster Recovery Ring Buffer Schema (`fallback.db`, SQLite)

```sql
-- fallback.db: local ring buffer hosting transition states during PG unreachability
-- (${HERDR_PLUGIN_STATE_DIR}/fallback.db)
CREATE TABLE fallback_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    blueprint_name VARCHAR(100) NOT NULL,   -- denormalized for replay routing
    step_name TEXT NOT NULL,
    status TEXT NOT NULL,       -- STARTING | RUNNING | COMPLETED | SUSPENDED | FAILED
    target_sha VARCHAR(40) NOT NULL DEFAULT '0000000000000000000000000000000000000000',
    exit_code INTEGER,
    result_cache TEXT,          -- JSON text, also 16KB truncated
    stdout_tail TEXT,           -- latest 1KB terminal output snapshot
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_fe_task ON fallback_events(task_id);
CREATE INDEX idx_fe_blueprint ON fallback_events(blueprint_name);
```

> Ring buffer: max 1000 entries or 50MB, whichever comes first; oldest entries evicted when limit reached. On PG recovery, batch Log Replay replays `fallback_events` into absurd's checkpoint state (`set_task_checkpoint_state`) and the `metamach_step_meta` overlay in the appropriate per-blueprint database, then truncates the ring buffer. `blueprint_name` is used to route replay to the correct per-blueprint database.

### Contract 3.11: Workflow Dispatch Request/Response (Daemon ↔ `janus` CLI / Dispatch view)

```rust
// Client -> Daemon: dispatch a blueprint's workflow onto the absurd engine.
pub enum Request {
    Dispatch { blueprint: String, workflow: Option<String> },
}
// Daemon -> client: the absurd-minted task_id (synchronous; the step loop runs detached).
pub enum Response {
    Dispatch { task_id: Uuid },
}
```

> `workflow` overrides the blueprint's `default_workflow`; `None` uses the default. The handler validates the recipe (`recipe::validate`), ensures the per-blueprint DB + absurd schema exist, calls `spawn_workflow` (absurd `create_queue` + `spawn_task` with `max_attempts: 1` - no auto-retry; MetaMach reschedules via cold-start / Task 4.4) to mint the `task_id`, returns it, then spawns `run_workflow` detached to claim + execute the steps. Each step runs under `janush` as the tmux session workload (`janush -c "<command>"` with `JANUS_AGENT` / `JANUS_BLUEPRINT` / `JANUS_TASK_ID` / `JANUS_STEP` / `JANUS_WORKFLOW` / `HERDR_PLUGIN_STATE_DIR` env set) so every Agent command is Tool-Guard-reconciled. The engine pins `target_sha` = git HEAD per step (all-zeros for non-git), transitions `metamach_step_meta` `PENDING -> STARTING -> RUNNING -> COMPLETED | FAILED | SUSPENDED`, captures the exit code (`tmux display-message '#{pane_dead_status}'`) + 16 KiB `stdout_tail`, and writes one absurd checkpoint per step (`set_task_checkpoint_state`). `complete_run` is called once after all steps (one absurd run = one dispatch - absurd's `complete_run` ends the *task*, so it is NOT per-step); `fail_run` on the first failing step; a HITL `SUSPENDED` step (the daemon's `GuardCheck` handler already set `step_meta.status = SUSPENDED`) leaves the run non-terminal - the `await_event` / `emit_event` resume loop is a follow-on. The absurd lease (30s) is renewed every ~10s via `extend_claim` so long steps don't auto-fail. `Progress.tmux_alive` (Contract 3.3) is wired: the daemon holds a `DurableBackend` and runs a second-pass `has_session` check on each task's current-step `session_name`.

### Contract 4.1: Cognitive Provider SPI (`janus::cognitive`)

```rust
/// 0.4.0 — Cognitive Provider SPI (ARCH-0.4.0 §III).
///
/// Implementations are opt-in and communicate via local IPC (Unix socket
/// or stdio). The daemon holds at most one active provider per blueprint;
/// providers are lazily started on first query and terminated on Offboard.
pub trait CognitiveProvider: Send + Sync {
    /// Validate whether a command is consistent with the blueprint's
    /// domain constraints. Returns `None` when the provider has no opinion
    /// (pass-through); returns `Some(reason)` to recommend a BLOCK verdict.
    /// Advisory only — timeout is 2s; on timeout, the standard Tool Guard
    /// verdict proceeds without cognitive input.
    fn validate_command(
        &self,
        blueprint: &str,
        argv: &[String],
        cwd: Option<&str>,
    ) -> Result<Option<String>, CognitiveError>;

    /// On Offboard, produce a condensed knowledge artifact. The output is
    /// appended to the LLM smelt `production_report.md` — a supplement,
    /// not a replacement (Feature-Spec §2.5 Offboard).
    fn extract_knowledge(
        &self,
        blueprint: &str,
    ) -> Result<String, CognitiveError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CognitiveError {
    #[error("provider not reachable: {0}")]
    Unreachable(String),
    #[error("query timeout")]
    Timeout,
}
```

> **Design invariants:** (1) Cannot block the tmux session — `validate_command` timeout is 2s. (2) Cannot read database state — receives only `argv` + `cwd` + `blueprint` name. (3) Opt-in per blueprint — configured in `blueprints/<name>/janus.toml` under a `[cognitive]` section.

### Contract 4.2: MCP Symbol Indexing (codebase-memory-mcp)

The daemon spawns the `codebase-memory-mcp` MCP server as a child process on first use per blueprint. Communication is via stdin/stdout JSON-RPC (MCP transport protocol). The server indexes only the blueprint's own source tree (`blueprints/<name>/`). It has no access to the daemon's source, other blueprints, or the host filesystem outside the blueprint root.

### Contract 4.3a: HITL Gateway — Hermes Run API Envelope

The gateway exposes a local HTTP listener on `127.0.0.1:<port>` (default `8443`, configurable via `JANUS_GATEWAY_LISTEN_PORT`). External Teams reachability requires a tunnel or reverse proxy (cloudflared / nginx+TLS) — the daemon never handles TLS or exposes a public port.

**Outbound (Gateway → Teams):**
```
POST /v1/runs
{
  "run_id": "<correlation_id>",
  "status": "requires_action",
  "action": {
    "type": "hitl_approval",
    "payload": {
      "blueprint": "gatemetric",
      "task_id": "<uuid>",
      "step": "cross-compile",
      "command": "make CROSS_COMPILE=arm-none-eabi-",
      "cause": "require_approval: cross-compile on remote",
      "stdout_tail": "<16KB truncated>",
      "expires_at": "2026-07-18T00:00:00Z"
    }
  }
}
```

**Callback (Teams → Gateway):**
```
POST /v1/runs/{run_id}/actions
{
  "action": "approve" | "reject" | "override",
  "override_command": "<optional rewritten argv>",
  "approved_by": "hughguan@contoso.com",
  "timestamp": "2026-07-17T15:04:05Z"
}
```

> `expires_at` = now + `JANUS_HITL_TIMEOUT_SECS` (default 30 min). Callbacks after expiry return `410 Gone`.

### Contract 4.3b: Microsoft Teams Active Cards

The `TeamsSender` adapter translates the enriched `WebhookPayload` (Contract 4.3c) into an Adaptive Card with `Approve` / `Reject` / `Override` actions. Authentication uses HMAC payload signing with a pre-shared secret (`JANUS_TEAMS_HMAC_SECRET`, provisioned in `/dev/shm` on Linux, `$TMPDIR` on macOS). Replay protection: each `run_id` accepts exactly one callback (duplicates return `409 Conflict`).

### Contract 4.3c: Enriched WebhookPayload & Gateway Trait

```rust
/// Shared HITL card type (in `protocol.rs`).
/// Enriched with Hermes Run API fields for Teams adapter compatibility.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    pub task_id: Option<Uuid>,
    pub execution_id: String,
    pub correlation_id: String,        // == Hermes run_id; single source of truth
    pub cause: String,
    pub command: String,
    pub reason: String,
    pub scene: String,                 // 16KB-truncated; legacy alias for stdout_tail
    pub resume_key: String,            // "metamach-resume:{correlation_id}"
    pub blueprint: String,             // 0.4.0: owning blueprint name
    pub step: String,                  // 0.4.0: current step name
    pub stdout_tail: String,           // 0.4.0: canonical field (Hermes naming)
    pub expires_at: String,            // 0.4.0: ISO 8601; now + HITL_TIMEOUT
}

/// 0.4.0 — HITL Gateway dispatch trait.
pub trait HitlGateway: Send + Sync {
    /// Dispatch a HITL card to all configured channels. Non-blocking:
    /// spawns a verdict thread and returns immediately. The correlation_id
    /// is already in `payload.correlation_id` (the gateway never mints it).
    fn dispatch(&self, payload: &WebhookPayload) -> Result<(), GatewayError>;

    /// Block until a verdict arrives for the given correlation_id, or
    /// until the timeout expires (fail-closed: timeout = BLOCK).
    /// Called from the gateway's dedicated verdict thread, never from
    /// the tmux control thread — the tmux session is never frozen.
    fn await_verdict(
        &self,
        correlation_id: &str,
        timeout: Duration,
    ) -> Result<GatewayVerdict, GatewayError>;
}

#[derive(Debug, Clone)]
pub enum GatewayVerdict {
    Approve,
    Reject,
    Override { rewritten_argv: Vec<String> },
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("channel unavailable: {0}")]
    ChannelError(String),
    #[error("verdict timeout")]
    Timeout,
    #[error("invalid callback signature")]
    AuthFailed,
}
```

> `GatewayVerdict` is distinct from `tool_guard::Verdict` (ALLOW | BLOCK | REWRITE). The gateway's verdict is about HITL approval; the tool guard's verdict is about command interception.

## 4. UAT & Fault Matrix

| Fault Boundary | Physical Behavior | System-Level Fault Tolerance & Convergence |
|---|---|---|
| **tmux Physical Network/SSH Drop** | Remote compile server offline; `std::process` pipe read/write hangs. | **Session Freeze:** Janus Daemon captures the immortal underlying handle via `janus::tmux`, marks Step `SUSPENDED` in Postgres. Never kills the background tmux pane. On network recovery, re-establish SSH pipe and execute `janus tmux attach` to wake the scene in seconds. |
| **Agent Hallucination, Infinite Log Spam** | Terminal stdout stream generates megabytes of garbage text per second. | **Physical Size Budget Fuse (single authoritative enforcement point):** `janush` has an in-memory streaming counter — when single-Step stdout exceeds **16 KiB**, it early-streaming truncates (optimization only, reducing UDS transfer). The **authoritative 16KiB enforcement point is at `janus-daemon`'s `absurd` module, before the `INSERT` transaction commits** — Daemon re-validates and hard-truncates before database write, appending `[MetaMach Log Budget Exceeded]` tag. Two defense lines targeting the same 16KiB cap; the DB write is the final gate. |
| **Absurd DB Connection Pool Crash** | Host Postgres encounters extreme physical OOM or native process crash. | **State Machine Anti-Blast Degradation:** Janus Daemon internally has a local in-memory SQLite ring buffer. During PG disconnection, all transition-state Step changes are atomically written to local `fallback.db` first. Upon detecting host PG recovery, auto-triggers batch merge replay (Log Replay), ensuring workshop production never halts. |
| **Fail-Closed 30s UDS Timeout** | Daemon crashes or UDS socket breaks during janush command check. | **Fail-Closed:** `janush` blocks synchronously with a 30s timeout. If the Daemon does not respond, returns an error to the Agent without executing the command. Never lets through. Agent's shell does not hang indefinitely. |
| **HITL Gateway Channel Failure** | Teams/Telegram webhook unreachable or returns 5xx. | **Non-Blocking Degradation:** `gateway::dispatch()` is non-blocking; the tmux session is never frozen. `LoggingSender` (null channel) always fires, guaranteeing an audit trail. The verdict thread times out after `JANUS_HITL_TIMEOUT_SECS` (fail-closed: timeout = BLOCK). |
| **Cognitive Provider Timeout** | CognitiveProvider::validate_command exceeds 2s deadline. | **Advisory Pass-Through:** The cognitive check is advisory, not gating. On timeout, the standard Tool Guard verdict proceeds without cognitive input. The tmux session is never blocked waiting for a cognitive provider. |
