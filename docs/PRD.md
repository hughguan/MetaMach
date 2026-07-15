# MetaMach 1.0 — Product Requirements

> A business guide for the Factory Director: blueprint onboarding, workflow dispatch, HITL gates, and production reports.

## Director's Note

This specification is crafted for the **Factory Director** (business end-user). You need not understand Rust, UDS sockets, or PostgreSQL internals. Your core responsibilities are: **registering new products (Blueprints), dispatching SOP workflows, approving high-risk operations (HITL Gate), and reviewing final quality inspection reports (Production Report)**. This document dissects the MetaMach 1.0 feature landscape from a business perspective.

## 1. Business Vision & Core Pain Points

In traditional AI-assisted R&D and automated assembly, the Factory Director frequently faces three pain points:

1. **Opaque & Uncontrollable (Safety Line):** AI agents act like runaway horses in black-box terminals, arbitrarily writing code and commands that could directly delete databases or trigger physical hardware pin conflicts—lacking safety guardrails and manual confirmation gates.

2. **Fragile & Non-Durable (Runtime Line):** A slight network jitter, SSH disconnect, or laptop sleep instantly crashes hours-long heavy compilation and deployment tasks with no recovery path—total loss of progress.

3. **Non-Accumulating Knowledge (Evolution Line):** Every error, pin conflict, and fix the AI encounters vanishes when the session ends, never converted into permanent factory assets—leading to repeated mistakes in subsequent development.

**MetaMach 1.0** delivers a digital silicon pipeline as deterministic, safe, controllable, and self-evolving as a physical factory, through its design of "resident guardian brain + durable physical sessions + shared knowledge graph."

## 2. Core Business Feature Modules

### 2.1 Blueprint Onboarding & Offboarding

A Blueprint is the smallest business unit of factory production (e.g., `joyrobots` education robot or `gatemetric` BMX attitude evaluation system).

- **One-Click Onboard:** The sole entrypoint for the Factory Director to introduce a new product line on Day 0. Executing `janus onboard --blueprint <name>` (or selecting "Onboard New Product" via the Popup console) triggers the system to take over with the standard onboarding process:

    1. **Recipe Validation:** Read `blueprints/<name>/janus.toml`, validate product name, default workflow (`default_workflow`), remote SSH target, and OpenWiki knowledge graph index scope; confirm `workflows/<default_workflow>.toml` exists.
    2. **Pre-Ignition Self-Check:** Probe Absurd DB reachability and tmux engine readiness; for cross-host blueprints (e.g., `gatemetric`), best-effort probe remote SSH target reachability (unreachable = warn only, does not block Onboard).
    3. **Tenant Registration:** Allocate an independent logical tenant isolation space in Absurd DB (partitioned by `blueprint_id`), writing one row of `ACTIVE` blueprint metadata. This operation is **idempotent**—repeated execution produces no dirty data; re-onboarding a previously offboarded blueprint reactivates it from `OFFBOARDED`.
    4. **Workflow Binding:** Bind the default SOP workflow declared in `janus.toml`, making it immediately dispatchable.
    5. **Knowledge Graph Loading & Experience Inheritance:** Load the product's dedicated `blueprints/<name>/openwiki/` knowledge graph. **If this blueprint was previously offboarded and generated a `production_report.md`**, the system prioritizes indexing that whitepaper and injects critical avoidance patterns (pin conflicts, compile errors) as `## Previous Incidents` few-shot examples into the next-generation Agent's System Prompt—achieving cross-generational silicon experience inheritance.
    6. **Onboarding Ready:** Status set to `ACTIVE`; the product line instantly appears in the Popup dispatch menu, ready for workflow dispatch.

- **Lossless Offboard:** When product R&D concludes and the line needs sealing, the Factory Director executes `janus offboard --blueprint <name>`. The system automatically extracts all Step execution traces from the project's development period, smelts them via the configured LLM into a latest "Quality Inspection Report & Production Whitepaper (`production_report.md`)" deposited into the knowledge graph, and simultaneously wipes expired redundant caches from the database for lossless slimming. On the next Onboard of this blueprint, this whitepaper is automatically recycled as immune antibodies for the next-generation Agent (see "Knowledge Graph Loading & Experience Inheritance" above).

### 2.2 SOP Workflow Dispatcher

Workflows are standardized, declarative collections of process steps. The Factory Director need not manually intervene in each step—only dispatch at the whole-pipeline level:

- **Visual Dispatch Menu:** Pressing the hotkey `prefix+j` in the Herdr terminal pops up an elegant **Popup control panel** at screen center. The Factory Director can view all available products here and dispatch standard SOPs (e.g., "R&D Assembly Line," "Diagnostic & Debug Line," or "Firmware Cross-Device Flash Line") with one key.
- **Cross-Physical-Host Coordination:** Workflows support multi-station collaboration. For example, Step 1 locally launches AI to write filter algorithms; Step 2 automatically tunnels the code via secure channel to a remote heavy cross-compilation server—zero manual terminal switching or SSH configuration by the Factory Director.

### 2.3 Physical Session "Immortality" Guarantee (Tether Engine)

Ensures the physical robustness of any R&D task:

- **No-Drop-On-Disconnect (Remain-on-Exit):** All underlying tasks (whether local compilation or SSH remote flashing) are locked inside independent physical Sessions. Even if the Factory Director unplugs the network cable, closes the terminal, or the computer crashes, tasks continue running smoothly on the server backend.
- **Scene Preservation & Instant Re-Attach:** When a task completes or errors out mid-way, the scene is 100% preserved. The Factory Director can at any moment one-click "Attach" into the error terminal's physical scene, manually tune, and seamlessly resume.

### 2.4 Intelligent Security Guard & HITL Approval Gate

MetaMach 1.0 adheres to a "safety first" principle, keeping AI within a secure institutional cage:

- **Proactive High-Risk Command Interception:** When the AI attempts operations exceeding its qualification level (e.g., modifying critical system configuration files, executing high-risk deletion commands, or triggering hardware pin conflicts), the underlying proxy shell (`janus-sh`) intercepts at kernel level and forcibly suspends the task.
- **Multi-Endpoint Approval (Teams / TUI):** When the security gate triggers, the Factory Director's **MS Teams / Telegram mobile** instantly receives a card with error scene context (containing cause, affected module, and `[Resume]` / `[Reject]` buttons). The Factory Director can remotely control the workshop assembly line start/stop with a mobile tap.

### 2.5 Workflow Progress Dashboard (Workflow Monitor)

After dispatch, the Factory Director needs a "factory production big screen" to answer three core questions: **Is it still running? Which station is it stuck on? Is it progressing normally or deadlocked?**

- **Real-Time Progress Dashboard:** After pressing `prefix+j` to wake the Popup, the Factory Director can toggle between "Dispatch" and "Progress" views with one key. The Progress view lists all in-flight workflows in matrix form: owning blueprint, workflow name, current step, each step's status (`PENDING` / `RUNNING` / `COMPLETED` / `SUSPENDED` / `FAILED`), elapsed time, and the most recent truncated terminal output (stdout summary). The dashboard auto-refreshes every 1–2 seconds.
- **Instant Suspend Highlight:** When a step triggers a safety fuse and enters `SUSPENDED`, that row highlights red within 1 second with direct `[Attach Scene]` / `[Resume]` entry points—the Factory Director need not hunt across multiple terminals for the stuck task.
- **Multi-Blueprint Parallel Operation:** When the workshop has multiple blueprints in production simultaneously (e.g., `joyrobots` and `gatemetric` in parallel), the dashboard groups and independently displays each blueprint's workflow progress with zero cross-contamination.
- **Headless Environment Fallback:** In SSH-only, non-TUI environments (e.g., CI or remote servers), the Factory Director can execute `janus status` for an equivalent plain-text/JSON snapshot to grasp workshop status in seconds.

### 2.6 Workflow Error Handling

Pipelines do not have only two outcomes—"successful progression" and "HITL suspend." The Factory Director must be able to distinguish three error categories from a business perspective and know the handling for each:

- **Transient Errors (auto-retry, no director intervention):** Recoverable faults such as SSH timeouts, network jitter, transient disk-full. The Daemon auto-retries with exponential backoff up to 3 times; the progress dashboard shows a `RETRYING (n/3)` badge on that step. Recovery within 3 attempts → continue; director takes no action.
- **Blocking Errors (HITL card, director decides):** Faults requiring human judgment such as compile errors, privilege violations, pin conflicts. The step enters `SUSPENDED`; the director receives a card with scene context and can choose `[Resume]` (fix in-place then dispatch next step), `[Skip Step]` (skip current step with a record), or `[Abort]` (terminate the entire pipeline).
- **Fatal Errors (terminate & archive, director notified):** Unrecoverable faults such as corrupted blueprint recipe, OpenWiki engine completely unavailable with no fallback. The Daemon terminates the task as `FAILED`, generates a partial `production_report.md` based on completed steps, and notifies the director via Popup + notification backend to await manual intervention.

## 3. Business Functional Matrix & Priorities

| Feature Module | Business Scenario & User Value | UAT Criteria | Priority |
|---|---|---|---|
| **Popup Dispatch Console** | Factory Director dispatches workflows in seconds via modal popup within Herdr, no view switching. | Warm path (Daemon running): render ≤100ms; lazy-start path (Daemon not running): ≤3s; focus auto-locked, keyboard highlight supported. | **High (P0)** |
| **Workflow Progress Dashboard** | Real-time view of all in-flight workflow step progress, current step, elapsed time, and suspend reasons; quickly locate stuck tasks. | Dashboard refreshes within 2s; SUSPENDED steps highlight within 1s with Resume entry; multi-blueprint parallel display supported. | **High (P0)** |
| **Proxy Sandbox Interception (janus-sh)** | Intercept AI privilege-escalation commands; prevent accidental database deletion, unauthorized network egress, or pin conflicts. | ≥99.9% of unauthorized commands intercepted and suspended in test suite (N=10,000), never reaching the real shell. | **High (P0)** |
| **Multi-Endpoint HITL** | Factory Director remotely approves via mobile (Teams/TG); pipeline seamlessly resumes. | Webhook POST completes locally ≤500ms (end-to-end delivery depends on external service); suspended terminal scene resumes ≤1s after tap. | **High (P0)** |
| **Physical Process Protection (Tether)** | Physical sessions persist after local network disconnect or SSH restart. | Manually kill Herdr foreground process; backend compilation script does not halt; re-Attach restores scene data intact. | **Medium (P1)** |
| **Blueprint One-Click Onboard** | Day 0: register new product, allocate independent tenant space, bind default workflow, load dedicated knowledge graph (incl. historical experience inheritance). | After `janus onboard`, Popup menu instantly shows the new product and can immediately dispatch; repeated Onboard is idempotent with no side effects. | **High (P0)** |
| **Offboard Smelting** | R&D complete, one-click archive. Generate quality report and回流 knowledge into OpenWiki shared RAG. | After Offboard triggers, md document with debugging records auto-generated in local directory; database redundancy wiped. | **Low (P2)** |

## 4. Factory Director Daily User Journey

### Day 0: Onboarding a Brand-New Blueprint

> The Factory Director faces a freshly powered-on, zero-product-line empty workshop. The first task is not dispatching, but "onboarding" a new product line.

1. **Prepare Recipe:** The Factory Director places product source and a `janus.toml` under `blueprints/gatemetric/` (declaring default workflow `firmware-deploy`, remote SSH compilation target, OpenWiki knowledge graph index scope).
2. **One-Click Onboard:** Execute `janus onboard --blueprint gatemetric` (or select "Onboard New Product" in Popup).
3. **System Auto-Takeover:** System validates recipe → self-checks DB & tmux → registers `ACTIVE` tenant in Absurd DB → binds `firmware-deploy` workflow → loads dedicated knowledge graph (if prior `production_report.md` exists, auto-recycled as immune antibodies).
4. **Instantly Available:** Terminal prints Onboard success; `gatemetric` immediately appears in Popup dispatch menu. The workshop transitions from zero products to production-ready state.

### In Practice: GateMetric BMX Attitude Evaluation Board R&D

1. **09:00 — Dispatch Work Order:** Factory Director enters the Richmond Hill workshop, opens terminal, presses `prefix+j`. In the popup, selects product `gatemetric` and dispatches `dev-flow` (R&D assembly SOP).
2. **09:05 — Check Progress:** After dispatching, the director does not wait blind; switches to "Progress" view in the Popup. The dashboard shows `dev-flow` has entered Step 1 (Scout scan, `RUNNING`), with real-time scrolling of recent terminal output. Director confirms normal pipeline progression and leaves the station.
3. **09:15 — AI Auto-Assembly & Safety Interception:** AI enters, writing ESP32 filter algorithms in the local sandbox. The AI attempts to directly modify the board's core pin configuration via command line to debug peripherals. At this moment, `janus-sh` security guard raises alarm: _"Pin conflict detected—risk of physical board burn!"_ The task is auto-suspended non-destructively; local and remote physical sessions remain alive. The progress dashboard highlights that step red as `SUSPENDED` within 1 second, popping up `[Attach Scene]` / `[Resume]` entries.
4. **09:16 — Multi-Endpoint HITL (two realistic paths, no contradiction):**
    - **Path A (Mobile — director in a meeting, only phone available):** Director reads error context on phone, taps **`[Mark for Manual Fix]`**. System replies "Awaiting director return to terminal for in-place fix"; step remains `SUSPENDED`, physical session alive. After meeting, director returns to station, `attach`-es into the error pane, fixes the C++ header pin definition in-place, types `metamach-resume` as completion signal; Daemon dispatches the next step.
    - **Path B (Desktop — director already at terminal):** Director receives the same card directly in TUI, immediately `attach`-es to the error pane, fixes in-place, types `metamach-resume` (or clicks **`[Resume]`**) to recover; pipeline seamlessly hands off.
    Both paths **never blindly re-execute the intercepted original command**; they fix in-place then dispatch the next step, avoiding overwriting the manual fix.
5. **14:00 — Auto-Evolution & Line Sealing:** Cross-compilation and remote deployment flashing pass smoothly. Director executes `janus offboard --blueprint gatemetric` at the console. The system instantly wipes local database cache large fields to prevent bloat, and auto-deposits a `production_report.md` under the OpenWiki directory documenting this pin conflict and its resolution.
6. **Future:** The next time the director executes `janus onboard` for `gatemetric`, the system prioritizes indexing that `production_report.md` and injects the pin conflict avoidance pattern as `## Previous Incidents` few-shot into the next-generation Agent's System Prompt, making it immune to the same class of errors from the moment it enters.

## 5. Delivery Standards & Quality Red Lines

- **Absolute Data Footprint Anti-Bloat:** Any step log capture is forcibly capped at the **16KB budget**. Excess is truncated before database insertion, and after Offboard, large JSONs must be completely physically wiped to prevent "meltdown" from database bloat.
- **Zero-Dependency Out-of-the-Box:** One-command `make bootstrap` auto-starts the PG database container, completes symlink configuration, completely shielding the Factory Director from complex underlying infrastructure setup.
