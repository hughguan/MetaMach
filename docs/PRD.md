# MetaMach 0.3.0 — Product Requirements

> A business guide for the Factory Director: blueprint onboarding, workflow dispatch, HITL gates, and production reports.

## Director's Note

This specification is crafted for the **Factory Director** (business end-user). You need not understand Rust, UDS sockets, or PostgreSQL internals. Your core responsibilities are: **registering new products (Blueprints), dispatching SOP workflows, approving high-risk operations (HITL Gate), and reviewing final quality inspection reports (Production Report)**. This document dissects the MetaMach 0.3.0 feature landscape from a business perspective.

## 1. Business Vision & Core Pain Points

In traditional AI-assisted R&D and automated assembly, the Factory Director frequently faces three pain points:

1. **Opaque & Uncontrollable (Safety Line):** AI agents act like runaway horses in black-box terminals, arbitrarily writing code and commands that could directly delete databases or trigger physical hardware pin conflicts — lacking safety guardrails and manual confirmation gates.

2. **Fragile & Non-Durable (Runtime Line):** A slight network jitter, SSH disconnect, or laptop sleep instantly crashes hours-long heavy compilation and deployment tasks with no recovery path — total loss of progress.

3. **Non-Accumulating Knowledge (Evolution Line):** Every error, pin conflict, and fix the AI encounters vanishes when the session ends, never converted into permanent factory assets — leading to repeated mistakes in subsequent development.

**MetaMach 0.3.0** delivers a digital silicon pipeline as deterministic, safe, controllable, and self-evolving as a physical factory, through its design of "resident guardian brain + durable physical sessions + shared knowledge graph."

## 2. Core Business Feature Modules

### 2.1 Blueprint Onboarding & Offboarding

A Blueprint is the smallest business unit of factory production (e.g., `joyrobots` education robot or `gatemetric` BMX attitude evaluation system).

- **One-Click Onboard:** The sole entrypoint for the Factory Director to introduce a new product line on Day 0. Executing `janus onboard --blueprint <name>` (or selecting "Onboard New Product" via the Popup console) triggers the system to take over with the standard onboarding process:

    1. **Recipe Validation:** Read `blueprints/<name>/janus.toml`, validate product name, default workflow (`default_workflow`), remote SSH target, and OpenWiki knowledge graph index scope; confirm `workflows/<default_workflow>.toml` exists.
    2. **Pre-Ignition Self-Check:** Probe Absurd DB reachability and tmux engine readiness; for cross-host blueprints (e.g., `gatemetric`), best-effort probe remote SSH target reachability (unreachable = warn only, does not block Onboard).
    3. **Tenant Registration:** Allocate an independent dedicated database (`CREATE DATABASE metamach_blueprint_<name>`) in the host-native Postgres instance for complete data isolation. Write one row of `ACTIVE` blueprint metadata to the global catalog. This operation is **idempotent** — repeated execution produces no dirty data; re-onboarding a previously offboarded blueprint reactivates it from `OFFBOARDED`.
    4. **Workflow Binding:** Bind the default SOP workflow declared in `janus.toml`, making it immediately dispatchable.
    5. **Knowledge Graph Loading & Experience Inheritance:** Load the product's dedicated `blueprints/<name>/openwiki/` knowledge graph. **If this blueprint was previously offboarded and generated a `production_report.md`**, the system prioritizes indexing that whitepaper and injects critical avoidance patterns (pin conflicts, compile errors) as `## Previous Incidents` few-shot examples into the next-generation Agent's System Prompt — achieving cross-generational silicon experience inheritance.
    6. **Onboarding Ready:** Status set to `ACTIVE`; the product line instantly appears in the Popup dispatch menu, ready for workflow dispatch.

- **Lossless Offboard:** When product R&D concludes and the line needs sealing, the Factory Director executes `janus offboard --blueprint <name>`. The system automatically extracts all Step execution traces from the project's development period, smelts them via the configured LLM into a latest "Quality Inspection Report & Production Whitepaper (`production_report.md`)" deposited into the knowledge graph. Operational data is purged (DELETEd) from the blueprint's dedicated database, while a complete audit trail is written to the global `absurd_audit_log` for forensic traceability. The per-blueprint database is retained (not dropped). On the next Onboard of this blueprint, the whitepaper is automatically recycled as immune antibodies for the next-generation Agent.

### 2.2 SOP Workflow Dispatcher

Workflows are standardized, declarative collections of process steps. The Factory Director need not manually intervene in each step — only dispatch at the whole-pipeline level:

- **Visual Dispatch Menu:** Pressing the hotkey `prefix+j` in the Herdr terminal pops up an elegant **Popup control panel** at screen center. The Factory Director can view all available products here and dispatch workflows with a single keypress.

- **Progress Dashboard:** After dispatching, the director can switch to the "Progress" view (via `Tab` key) to see real-time step progression, elapsed time, and the most recent terminal output for each in-flight task. `SUSPENDED` steps are highlighted in red with `[Attach Scene]` / `[Resume]` shortcut entries.

- **CLI Fallback:** In non-TUI environments (SSH / CI), `janus status` provides a plain-text or JSON snapshot of all in-flight tasks — the same data source as the dashboard.

### 2.3 Durable Physical Sessions (Tether Engine)

The system's underlying physical execution layer is powered by `janus::tether`, a native engine inside the resident daemon that directly manages tmux `remain-on-exit` sessions. Key business guarantees:

- **Session Never Dies:** Even if the Factory Director closes the terminal, sleeps the laptop, or the SSH connection drops, the background compilation, flashing, or testing process continues uninterrupted. Re-attaching restores the scene 100% in milliseconds.
- **Cross-Host Deployment:** For blueprints with remote targets (e.g., `gatemetric` deploying to a remote ESP32 compile server), the system automatically tunnels the instruction via SSH and maintains session durability on the remote host.

### 2.4 Intelligent Security Guard & HITL Approval Gate

The system intercepts every command the AI attempts to execute before it reaches the real shell (via `janus-sh` proxy shell). The security guard (`Tool Guard`) evaluates each command against the Agent's role permissions, blacklist, and approval requirements:

- **Blacklisted Commands:** Immediately blocked; the physical shell never sees them.
- **High-Risk Operations:** Automatically redirected to dry-run mode (e.g., financial trade orders).
- **Requiring Approval:** The step is non-destructively suspended; the Factory Director receives a mobile alert card with scene context.

Pipelines do not have only two outcomes — "successful progression" and "HITL suspend." The Factory Director must be able to distinguish three error categories from a business perspective and know the handling for each:

- **Blocking Errors (HITL card, director decides):** Faults requiring human judgment such as compile errors, privilege violations, pin conflicts. The step enters `SUSPENDED`; the director receives a card with scene context and can choose `[Resume]` (fix in-place then dispatch next step), `[Skip Step]` (skip current step with a record), or `[Abort]` (terminate the entire pipeline).

- **Non-Blocking Warnings (Auto-pass, but logged):** The system detects a potential issue (e.g., dependency version mismatch warning) but the Agent can continue; the director is notified but no action is required.

- **Terminal Failures (Auto-abort, no recovery):** The system encounters a fatal error (e.g., remote host permanently unreachable, disk full); the pipeline terminates and the director is notified with the root cause.

### 2.5 Production Report & Knowledge Graph

After Offboard, the system automatically generates a `production_report.md` containing four structured blocks: [Compile Error History], [Pin Conflict Details], [Tool Guard Interception Logs], and [Successful Patches Applied]. This report is committed to the blueprint's OpenWiki directory and indexed into the federated knowledge graph, serving as immune antibodies for the next-generation Agent.

## 3. Key Business Metrics

| Metric | Description | Pass Criteria | Priority |
|--------|-------------|--------------|----------|
| **Visual Dispatch & Progress** | One-key popup: view products, dispatch workflows, real-time step status. | Popup renders ≤100ms; step status refresh ≤2s; `SUSPENDED` highlight ≤1s. | **High (P0)** |
| **Multi-Endpoint HITL** | Factory Director remotely approves via mobile (Teams/TG) or desktop TUI; pipeline seamlessly resumes. | Webhook POST completes locally ≤500ms; suspended terminal scene resumes ≤1s after tap or `metamach-resume`. | **High (P0)** |
| **Durable Session Resurrection** | tmux session survives network drop, terminal close, or laptop sleep. | Session alive after all disconnect scenarios; re-attach restores scene ≤1s. | **High (P0)** |
| **Onboard Registration** | One-click register new product, load dedicated knowledge graph (incl. historical experience inheritance). | After `janus onboard`, Popup menu instantly shows the new product and can immediately dispatch; repeated Onboard is idempotent with no side effects. | **High (P0)** |
| **Offboard Trace Purge & Audit** | R&D complete, one-click archive. Generate quality report, purge operational data, write full audit trail. | After Offboard triggers, `production_report.md` auto-generated; `absurd_audit_log` has one row per task; per-blueprint database retained for forensic review. | **Medium (P1)** |

## 4. Factory Director Daily User Journey

### Day 0: Onboarding a Brand-New Blueprint

> The Factory Director faces a freshly powered-on, zero-product-line empty workshop. The first task is not dispatching, but "onboarding" a new product line.

1. **Prepare Recipe:** The Factory Director places product source and a `janus.toml` under `blueprints/gatemetric/` (declaring default workflow `firmware-deploy`, remote SSH compilation target, OpenWiki knowledge graph index scope).
2. **One-Click Onboard:** Execute `janus onboard --blueprint gatemetric` (or select "Onboard New Product" in Popup).
3. **System Auto-Takeover:** System validates recipe → self-checks DB & tmux → creates dedicated database `metamach_blueprint_gatemetric` → registers `ACTIVE` tenant in global catalog → binds `firmware-deploy` workflow → loads dedicated knowledge graph (if prior `production_report.md` exists, auto-recycled as immune antibodies).
4. **Instantly Available:** Terminal prints Onboard success; `gatemetric` immediately appears in Popup dispatch menu. The workshop transitions from zero products to production-ready state.

### In Practice: GateMetric BMX Attitude Evaluation Board R&D

1. **09:00 — Dispatch Work Order:** Factory Director enters the Richmond Hill workshop, opens terminal, presses `prefix+j`. In the popup, selects product `gatemetric` and dispatches `dev-flow` (R&D assembly SOP).
2. **09:05 — Check Progress:** After dispatching, the director does not wait blind; switches to "Progress" view in the Popup. The dashboard shows `dev-flow` has entered Step 1 (Scout scan, `RUNNING`), with real-time scrolling of recent terminal output. Director confirms normal pipeline progression and leaves the station.
3. **09:15 — AI Auto-Assembly & Safety Interception:** AI enters, writing ESP32 filter algorithms in the local sandbox. The AI attempts to directly modify the board's core pin configuration via command line to debug peripherals. At this moment, `janus-sh` security guard raises alarm: _"Pin conflict detected — risk of physical board burn!"_ The task is auto-suspended non-destructively; local and remote physical sessions remain alive. The progress dashboard highlights that step red as `SUSPENDED` within 1 second, popping up `[Attach Scene]` / `[Resume]` entries.
4. **09:16 — Multi-Endpoint HITL (two realistic paths, no contradiction):**
    - **Path A (Mobile — director in a meeting, only phone available):** Director reads error context on phone, taps **`[Mark for Manual Fix]`**. System replies "Awaiting director return to terminal for in-place fix"; step remains `SUSPENDED`, physical session alive. After meeting, director returns to station, `attach`-es into the error pane, fixes the C++ header pin definition in-place, types `metamach-resume` as completion signal; Daemon dispatches the next step.
    - **Path B (Desktop — director already at terminal):** Director receives the same card directly in TUI, immediately `attach`-es to the error pane, fixes in-place, types `metamach-resume` (or clicks **`[Resume]`**) to recover; pipeline seamlessly hands off.
    Both paths **never blindly re-execute the intercepted original command**; they fix in-place then dispatch the next step, avoiding overwriting the manual fix.
5. **14:00 — Auto-Evolution & Line Sealing:** Cross-compilation and remote deployment flashing pass smoothly. Director executes `janus offboard --blueprint gatemetric` at the console. The system purges operational data from the blueprint's dedicated database (DELETE, not DROP), writes a full audit trail to the global `absurd_audit_log`, and auto-deposits a `production_report.md` under the OpenWiki directory documenting this pin conflict and its resolution.
6. **Future:** The next time the director executes `janus onboard` for `gatemetric`, the system prioritizes indexing that `production_report.md` and injects the pin conflict avoidance pattern as `## Previous Incidents` few-shot into the next-generation Agent's System Prompt, making it immune to the same class of errors from the moment it enters.

## 5. Delivery Standards & Quality Red Lines

- **Absolute Data Footprint Anti-Bloat:** Any step log capture is forcibly capped at the **16KB budget**. Excess is truncated before database insertion. After Offboard, operational data is fully purged (DELETEd) with a complete audit trail written to `absurd_audit_log` — no bloat, no data loss, full forensic traceability.
- **Zero-Dependency Out-of-the-Box:** One-command `make bootstrap` auto-initializes the host-native Postgres instance, compiles binaries, and completes symlink configuration, completely shielding the Factory Director from complex underlying infrastructure setup. No Docker required.
- **Fail-Closed Safety:** If the security daemon becomes unreachable, commands are never executed (fail-closed, 30s timeout). The factory defaults to safety, not convenience.
