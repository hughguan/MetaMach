# MetaMach 0.1.0 — Engineering Feature Specification

> Cognitive scheduler, agent sandbox (janus-sh), durable workflows, data contracts, and fault matrix.

## 1. Module Architecture Overview

Following Herdr 0.7.3 plugin specifications and the system's independent resident process design, MetaMach 0.1.0 software is composed of four core functional layers. This specification strictly adheres to the Immutable ROOT vs. Mutable State separation and pixel-level defines behavioral boundaries, data flows, and exception handling for each feature.

```
+-----------------------------------------------------------------------------------------+
|                                METAMACH CORE LAYERS                                      |
+-----------------------------------------------------------------------------------------+
|  1. CONTROL:   janus-daemon (resident UDS service) & herdr-janus (Popup shadow client)   |
|  2. SANDBOX:   janus-sh (proxy shell) & Event-Driven Tool Guard (synchronous kernel guard) |
|  3. WORKFLOW:  Absurd Postgres transaction engine & Cross-Host Tether (remain-on-exit)   |
|  4. KNOWLEDGE: Federated OpenWiki Skill & Auto-Pruning Smelter (Melt DB Cache)           |
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

### Feature 2.2: In-Memory Agent Sandbox (janus-sh & Tool Guard)

- **Description:** Provide an out-of-process, independently audited security gate that performs synchronous physical interception before Agent intent reaches Bash, supporting Dry-Run redirection for special operations.

- **Technical Spec:**
    - **Shell Interception Proxy (janus-sh):** When Tether launches a pane, it forcibly injects the `SHELL` environment variable to point to the **absolute path** of `janus-sh` (`${HERDR_PLUGIN_ROOT}/bin/janus-sh`, installed by `make bootstrap`, see Deployment-Spec §5.1). **Never use the relative path `target/release/janus-sh`**—it fails when CWD is not the repo root, the binary is not yet compiled, or the pane is started from a different directory.
    - **Synchronous UDS Reconciliation:** When an Agent sends any command-line string, `janus-sh` synchronously suspends execution, packaging the raw `argv` array and dispatching it to `janus.sock`.
    - **Timeout & Deadlock Prevention:** `janus-sh` blocks synchronously waiting for the Daemon verdict with a configurable timeout (default 30s). If the Daemon crashes or the UDS breaks causing a timeout, **fail-closed**: return an error to the Agent without executing the command (never let through), preventing the Agent's shell from hanging indefinitely due to Daemon unreachability. Verdict response format and semantics are defined in Contract 3.4.
    - **Dry-Run Redirection:** The Daemon's Tool Guard module performs security review against core allowlist commands (e.g., commands that modify system-level configuration or execute financial transaction orders). Before receiving the Factory Director's digitally signed authorization (Correlation ID) via Teams/TUI, the Daemon forcibly rewrites command parameters in memory (e.g., appending `--dry-run`) or returns an interception error—never passing through to the real physical host shell.

### Feature 2.3: Multi-Agent Cross-Host Durable Workflows

- **Description:** Based on Absurd Postgres strong transaction engine and Tether cross-host tmux PTY, supporting idempotent handoff and breakpoint self-healing for pipeline steps across multiple stations and host boundaries.

- **Technical Spec:**
    - **Absurd Transaction Primitives:** Before each Workflow Step executes, the Daemon must commit an `UPDATE` in the physical database to lock the transition state (`STARTING`). Upon successful execution, output data is packaged as `JSONB result_cache` in an atomic Commit (state transitions to `COMPLETED`).
    - **Physical Non-Destruction (Remain-on-Exit):** Tether-launched local or OpenSSH BatchMode-connected remote tmux sessions must be explicitly injected with `remain-on-exit on`. **To avoid polluting the Factory Director's personal tmux global config and unrelated sessions**, Tether uses an **independent tmux server** (`tmux -L metamach-tether ...`) and sets `set remain-on-exit on` per-session within that server (never using the `-g` global flag). When cross-host cross-compilation or firmware flashing fails with non-zero exit code (`Exit Code != 0`), the physical PTY pane is never killed, preserving the error scene. The independent server also allows disaster recovery drills to safely execute `tmux -L metamach-tether kill-server` without harming the director's other sessions.
    - **Cold-Start Reconciliation:** After a system power-cycle restart, the Daemon scans all tasks in non-terminal states. `tmux-resurrect` usage is forbidden. It directly extracts the last valid `COMPLETED` Checkpoint from Postgres, assigns a brand-new Tether Session UUID, and seamlessly resumes at the physical breakpoint.

### Feature 2.4: Human-in-the-Loop Gate & Notification

- **Description:** When a pipeline encounters a compile break or privilege violation, trigger system-level non-destructive suspension and pop up high-density reconciliation cards on external async communication gateways for Factory Director approval.

- **Technical Spec:**
    - **Non-Destructive Suspension:** Mark task state as `SUSPENDED`. The Daemon holds the current Tether physical PTY, protecting memory context.
    - **Async Bidirectional Gateway (Telegram primary, Teams secondary):** **Telegram is the primary notification backend** (open protocol, mobile-native, Bot API + Inline Keyboard natively implements `[Resume]` buttons); MS Teams is the secondary adapter (MessageCard format, API entirely different from Telegram, requires independent adapter). The Daemon constructs only an **abstract Webhook Payload** (task UUID, interception cause, 16KB-truncated stdout scene, `Resume` trigger key + Correlation ID), which each backend adapter then translates into native format (Telegram `sendMessage` + `inline_keyboard` / Teams Actionable MessageCard). Adding a new backend only requires implementing an adapter without modifying Daemon core logic.
    - **Remote HITL Response (redesigned; no longer relies on `Ctrl+C`):** `SUSPENDED` means the command was intercepted and the pane is idle—**there is no process that needs to be released by `Ctrl+C`.** The correct recovery closed loop is:
        1. The Factory Director `attach`-es into that idle pane via Herdr TUI and **fixes in-place** (edits code, rewrites config, corrects pin definitions, etc.);
        2. The director types `metamach-resume` in the pane (or taps the `Resume` card callback on mobile) to signal completion;
        3. The Daemon verifies the Correlation ID signature, transitions the state from `SUSPENDED` back to `RUNNING`, and **dispatches the command for the next step**—**never blindly re-executing the intercepted original command**, which would overwrite the director's just-applied in-place fix.

### Feature 2.5: Federated Lifecycle Smelter (Onboard / Offboard & Auto-Pruning)

- **Description:** Manage the hot/cold data state transitions of product blueprints. On product offboarding, smelt the full lifecycle execution traces into cold experience deposits and wipe large-volume caches.

- **Technical Spec:**
    - **Federated Wiki Design:** `configs/global_rules.md` resides in Immutable ROOT as globally injected System Lines. Each blueprint owns a dedicated `blueprints/<name>/openwiki/` subdirectory for project-specific AST knowledge graph storage.

    - **Onboard Registration Mechanism:** When the Factory Director executes `janus onboard --blueprint <name>`, the Daemon takes over with the following atomic sequence (any critical step failure triggers full rollback—no half-activated state):
        1. **Recipe Validation:** Read `blueprints/<name>/janus.toml`, validate required fields (`blueprint.name`, `blueprint.default_workflow`, `openwiki.scope`, optional `remote`). Confirm `workflows/<default_workflow>.toml` exists and conforms to the workflow file schema. Validation failure returns a clear error without writing to the database.
        2. **Pre-Ignition Self-Check:** Probe Absurd Postgres connectivity and tmux engine readiness. If `[remote]` is declared, perform a best-effort `BatchMode` connectivity probe against the remote SSH target (`-o ConnectTimeout=5`); unreachable only logs `WARN`, does not block Onboard (allows offline Onboard first, fill in target later).
        3. **Tenant Registration (Idempotent):** Execute `INSERT INTO blueprints (name, status, default_workflow, config, openwiki_scope, remote_host, onboarded_at) VALUES (…) ON CONFLICT (name) DO UPDATE SET status='ACTIVE', onboarded_at=NOW(), offboarded_at=NULL, …`. Use `blueprint_id` as the physical partition key for logical multi-tenant isolation. Repeated Onboard has no side effects; re-onboarding an already `OFFBOARDED` blueprint reactivates it.
        4. **Workflow Binding:** Persist `default_workflow` into blueprint metadata and precompile-validate that workflow's Step sequence, ensuring instant ignition on dispatch.
        5. **Knowledge Graph Loading & Experience Inheritance:** Index `blueprints/<name>/openwiki/` into OpenWiki retrieval scope. **If `production_report.md` exists at that path** (prior Offboard artifact), the Daemon prioritizes parsing its structured blocks (compile error history / pin conflicts / Tool Guard interception logs / successful Patches) and appends critical failure patterns as `## Previous Incidents` few-shot examples into that blueprint's Agent System Prompt template, achieving cross-generational immune inheritance.
        6. **Onboarding Ready:** Transaction commits; status set to `ACTIVE`. Daemon broadcasts a `blueprint_registered` event to `herdr-janus` via UDS; the Popup dispatch menu instantly refreshes to show the new product.

    - **Offboard Smelting Mechanism:** When the Factory Director executes `janus offboard --blueprint <name>`:
        1. Daemon scans Absurd DB, extracting all historical Task and Step execution traces and Tool Guard interception logs for that blueprint.
        2. Calls the configured LLM (see "Offboard LLM Integration Spec" below) to compress the above execution snapshots, pin errors, and avoidance patches into a high-density Markdown file, atomically writing to `./blueprints/<name>/openwiki/production_report.md`.
        3. **Database Anti-Bloat Degradation (Melt Cache):** Calls the backend SQL stored procedure `melt_blueprint_data('<name>')`, **completely DELETEs rows** (not NULL-ifies) for all Step `result_cache` JSON large fields and stdout log rows belonging to that blueprint—NULL-ification does not free TOAST physical space; full-row deletion allows autovacuum / `VACUUM FULL` to reclaim. Simultaneously writes one row of audit metadata statistics (Task ID, elapsed time) into a separate `absurd_audit_log` table for later audit.
        4. Via the shadow client, invokes local Git to auto-incrementally Commit and Push `production_report.md`, completing the self-propagation of silicon experience.

    - **Offboard LLM Integration Spec:** The LLM used for smelting is a configurable external dependency with the following specification:
        - **Endpoint Config:** `configs/offboard.toml` declares `endpoint`, `api_key_env` (environment variable name; key never touches disk), `model`, `max_input_tokens`.
        - **Input Budget:** Only takes the most recent N Steps (default N=50), each `result_cache` truncated to 16KB; total input constrained by `max_input_tokens`; excess discarded in reverse chronological order.
        - **Prompt Template:** Forces structured output of four blocks—[Compile Error History], [Pin Conflict Details], [Tool Guard Interception Logs], [Successful Patches Applied].
        - **Degradation Fallback:** When the LLM is unavailable (API key invalid, rate-limited, air-gapped offline) or exceeds 120s timeout, **does not block Offboard**; instead writes a raw JSON snapshot `production_report.raw.json` and logs `WARN`.
        - **Async Execution:** The Offboard command immediately returns "Smelting in progress"; LLM summarization runs in the background; when the report is ready, a UDS event notifies `herdr-janus` and completes the Git commit.

### Feature 2.6: Workflow Progress Dashboard (Workflow Monitor & Status Query)

- **Description:** Provide the Factory Director with real-time visibility into in-flight workflows, answering "Is it still running? / Which step is it stuck on? / Normal or deadlocked?" Aggregates authoritative state from Absurd Postgres via a read-only bypass without interfering with workflow execution channels.

- **Technical Spec:**
    - **Dual-View Popup:** `herdr-janus`'s Popup adds a "Progress" view alongside the existing "Dispatch" view, toggled via the `Tab` key after waking with `prefix+j`. The Progress view renders the in-flight task matrix grouped by blueprint using `ratatui` tables: owning blueprint · workflow name · current step · per-step status (`PENDING` / `STARTING` / `RUNNING` / `COMPLETED` / `SUSPENDED` / `FAILED`) · elapsed time · most recent stdout summary (truncated to 1KB).
    - **Polling Cadence:** While the Progress view is open, `herdr-janus` sends a `progress` query to the Daemon via UDS at a fixed 1–2s cadence and re-renders. Closing the view stops polling—zero idle overhead.
    - **`progress` Query Primitive (Daemon side):** Upon receiving the query, the Daemon executes a read-only `SELECT` aggregating `absurd_tasks JOIN absurd_steps` (filtering non-terminal tasks), overlaid with Tether physical Session liveness signals (`tmux has-session`). This query uses an independent read-only transaction, **never contending** with workflow execution write transactions.
    - **Suspend Instant Highlight:** When a step status is `SUSPENDED`, the dashboard highlights that row red within 1s and renders `[A]ttach Scene` / `[R]esume` shortcut entries at the row end, directly reusing the existing Tether attach and HITL resume pathways.
    - **`janus status` CLI Fallback:** In non-TUI environments (SSH / CI), `janus status [--blueprint <name>] [--json]` uses the same `progress` primitive, outputting plain-text or JSON snapshots for scriptable inspection.

## 3. System Data Exchange Contracts

### Contract 3.1: Core Schema (Absurd DB)

```sql
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

-- Workflow task table (one dispatch = one Task row)
CREATE TABLE absurd_tasks (
    id SERIAL PRIMARY KEY,
    blueprint_id INTEGER NOT NULL REFERENCES blueprints(id) ON DELETE CASCADE,
    workflow_name VARCHAR(100) NOT NULL,
    status VARCHAR(20) NOT NULL,  -- PENDING | STARTING | RUNNING | COMPLETED | SUSPENDED | FAILED
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Step checkpoint table (with Size Budget enforcement & Optimistic Locking)
CREATE TABLE absurd_steps (
    task_id INTEGER REFERENCES absurd_tasks(id) ON DELETE CASCADE,
    step_name VARCHAR(100) NOT NULL,
    status VARCHAR(20) NOT NULL,  -- PENDING | STARTING | RUNNING | COMPLETED | SUSPENDED | FAILED
    target_sha VARCHAR(40) NOT NULL DEFAULT '0000000000000000000000000000000000000000',
                                  -- Optimistic lock: Git HEAD SHA-1 pinned at Step dispatch.
                                  -- The all-zeros sentinel marks a non-git blueprint (lock skipped).
    result_cache JSONB,           -- strictly capped at 16KB physical storage
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (task_id, step_name)
);
```

> **Optimistic Locking - `target_sha` (preparatory; enforcement lands with Task 2.4):** At dispatch the Daemon pins the blueprint repo's current Git `HEAD` SHA-1 into `target_sha` (the all-zeros sentinel is used for non-git blueprints, which skip the lock) and sends it as `dispatch_sha` with the step payload; the remote report echoes `dispatch_sha` back. On return the Daemon compares `report.dispatch_sha == current HEAD` - a mismatch means `HEAD` advanced since dispatch (concurrent commit), so the report is stale: the Daemon discards it, marks the step `SUSPENDED`, emits a `CONCURRENCY_RACE_ALERT` through the existing HITL channel (Telegram/Teams), and auto-reschedules the step against the new `HEAD`. The apply itself is `UPDATE absurd_steps SET status = 'COMPLETED', result_cache = $1 WHERE task_id = $2 AND step_name = $3 AND target_sha = $4` (`$4 = report.dispatch_sha`); the `target_sha = $4` clause is the reschedule guard. **Reschedule semantics:** a reschedule writes a *new* `absurd_tasks` row (new `task_id`) rather than mutating the existing row's `target_sha`, preserving the old task's audit trail and ensuring stale pre-reschedule reports zero-row correctly. The column is added by `003_target_sha.sql`; dispatch-time pinning, the remote-report contract, and the auto-reschedule engine arrive with Task 2.4 (herdr-tether).

> **Unified Status Enumeration:** The system-wide Step/Task state machine is `PENDING -> STARTING -> RUNNING -> COMPLETED | FAILED | SUSPENDED`. The Blueprint-level state machine is `ACTIVE <-> OFFBOARDED` (Onboard activates / Offboard archives).

### Contract 3.2: Proxy Shell Sync UDS Request Payload (janus-sh → Daemon)

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
    "SHELL": "${HERDR_PLUGIN_ROOT}/bin/janus-sh"
  }
}
```

### Contract 3.3: Workflow Progress Query Response Payload (Daemon → herdr-janus / `janus status`)

```json
{
  "active_tasks": [
    {
      "task_id": 1042,
      "blueprint_id": "gatemetric",
      "workflow_name": "dev-flow",
      "status": "RUNNING",
      "started_at": "2026-07-15T09:00:00Z",
      "elapsed_seconds": 945,
      "current_step": "cross_compile",
      "tether_alive": true,
      "suspended_reason": null,
      "steps": [
        {"name": "scout",         "status": "COMPLETED"},
        {"name": "code",          "status": "COMPLETED"},
        {"name": "cross_compile", "status": "RUNNING",  "stdout_tail": "…latest 1KB terminal summary…"}
      ]
    }
  ]
}
```

> `active_tasks` only includes non-terminal tasks (`STARTING` / `RUNNING` / `SUSPENDED`). `stdout_tail` is the most recent terminal output truncated to 1KB summary; `tether_alive` reflects whether the corresponding Tether physical Session is alive. This payload drives both Popup progress dashboard rendering and `janus status --json` CLI output.

### Contract 3.4: Proxy Shell Sync UDS Response (Daemon → janus-sh)

```json
{
  "execution_id": "0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a",
  "verdict": "ALLOW",
  "reason": "financial_trade_requires_approval",
  "rewritten_argv": ["hi5bot", "--action", "dry-run"],
  "correlation_id": "0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a"
}
```

> **Verdict Semantics:** `ALLOW` = execute as-is on the host shell; `BLOCK` = `janus-sh` returns non-zero exit code to Agent without execution, Step transitions to `SUSPENDED` triggering HITL; `REWRITE` = replace original argv with `rewritten_argv` then execute (e.g., Dry-Run redirection). **Timeout:** If Daemon does not respond within `janus-sh`'s synchronous blocking window (default 30s), `janus-sh` **fail-closed** treats it as `BLOCK` and returns an error—never letting the command through.

### Contract 3.5: Agent Pool Qualification & Tool Guard Rules Schema (`configs/agents.toml`)

```toml
# Global Agent role qualifications & Tool Guard decision matrix
# (Loaded by Daemon on startup; hot-reload supported)

[agent.scout]
permissions      = ["read", "grep", "find", "git-log"]
allow_network    = false
bash_safe        = true

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

> **Decision Priority:** Tool Guard evaluates each `janus-sh`-reported argv in order—(1) `bash_blacklist` hit → `BLOCK`; (2) `require_approval` hit → `BLOCK` and set `SUSPENDED` awaiting HITL; (3) command capability not in current role `permissions` allowlist → `BLOCK`; (4) financial-class high-risk command → `REWRITE` to Dry-Run; (5) remainder → `ALLOW`. Rules are configurable (not hardcoded Rust); Daemon loads via `configs/agents.toml` symlink (Mutable Config zone).

### Contract 3.6: Blueprint Recipe Schema (`blueprints/<name>/janus.toml`)

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

> On Onboard, the Daemon strictly validates `blueprint.name`, `blueprint.default_workflow` (corresponding file must exist), and `openwiki.scope`; `[remote]` absent = local-only blueprint. Validation failure prevents Onboard (see §2.5 Onboard step 1).

### Contract 3.7: Workflow Pipeline Schema (`workflows/<name>.toml`)

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

> Each `[[steps]]` declares at minimum `name`, `agent`, `toolset`; `command` is the concrete instruction executed at that station; `host` references the blueprint's `[remote]` for cross-host steps. The Daemon dispatches steps sequentially in array order, writing one `absurd_steps` Checkpoint per step.

### Contract 3.8: Disaster Recovery Ring Buffer Schema (`fallback.db`, SQLite)

```sql
-- fallback.db: local ring buffer hosting transition states during PG unreachability
-- (${HERDR_PLUGIN_STATE_DIR}/fallback.db)
CREATE TABLE fallback_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    step_name TEXT NOT NULL,
    status TEXT NOT NULL,       -- STARTING | RUNNING | COMPLETED | SUSPENDED | FAILED
    result_cache TEXT,          -- JSON text, also 16KB truncated
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_fe_task ON fallback_events(task_id);
```

> Ring buffer: max 1000 entries or 50MB, whichever comes first; oldest entries evicted when limit reached. On PG recovery, batch Log Replay merges all `fallback_events` into `absurd_steps` and truncates the ring buffer.

## 4. UAT & Fault Matrix

| Fault Boundary | Physical Behavior | System-Level Fault Tolerance & Convergence |
|---|---|---|
| **Tether Physical Network/SSH Drop** | Remote compile server offline; `std::process` pipe read/write hangs. | **Session Freeze:** Janus Daemon captures the immortal underlying handle, marks Step `SUSPENDED` in Postgres. Never kills the background tmux pane. On network recovery, re-establish SSH pipe and execute `herdr-tether attach` to wake the scene in seconds. |
| **Agent Hallucination, Infinite Log Spam** | Terminal stdout stream generates megabytes of garbage text per second. | **Physical Size Budget Fuse (single authoritative enforcement point):** `janus-sh` has an in-memory streaming counter—when single-Step stdout exceeds **16 KiB**, it **early-streaming truncates** (optimization only, reducing UDS transfer). The **authoritative 16KiB enforcement point is at `janus-daemon`'s `absurd` module, before the `INSERT` transaction commits**—Daemon re-validates and hard-truncates before database write, appending `[MetaMach Log Budget Exceeded]` tag. Two defense lines targeting the same 16KiB cap; the DB write is the final gate, ensuring dirty data never reaches Postgres. |
| **Absurd DB Connection Pool Crash** | Host Postgres encounters extreme physical OOM or container unexpected exit. | **State Machine Anti-Blast Degradation:** Janus Daemon internally has a local in-memory SQLite ring buffer. During PG disconnection, all transition-state Step changes are atomically written to local `HERDR_PLUGIN_STATE_DIR/fallback.db` first. Upon detecting host PG container recovery, auto-triggers batch merge replay (Log Replay), ensuring workshop production never halts. |
