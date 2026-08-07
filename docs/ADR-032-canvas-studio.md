# ADR-032: MetaMach Studio — Visual Workflow DAG Editor & Web Observer (0.6.0 Candidate)

| Field | Value |
|---|---|
| **Context** | MetaMach 0.5.0/0.5.1 unified workflows and pipelines under `.janus/workflows/` (ADR-031). While CLI tools (`janus`, `janush`, `herdr-janus`) provide full control plane capabilities, complex multi-node DAG workflows and human-in-the-loop (HITL) safety gate approvals benefit from visual graph editing, real-time node state visualization, and physical PTY terminal streaming. |
| **Options Considered** | (1) Embed Web UI directly into `janus-daemon` (axum inside core daemon), (2) Build a decoupled sidecar binary (`janus-studio`) communicating over UDS (`janus.sock`), (3) Remain CLI/TUI-only (status quo). |
| **Decision** | **Adopted as Candidate ADR (0.6.0)** — Option (2): Decoupled `janus-studio` sidecar process. Keep `janus-daemon` zero-web-dependency. Embed static assets into `janus-studio` via `rust-embed` (React Flow / XYFlow). Proxy all control plane queries over existing UDS socket (`janus.sock`). Expose REST + WebSocket endpoints to the browser on `127.0.0.1:8443`. |
| **Rationale** | Maintains Core MM-CORE isolation (no web vulnerabilities or heavy async web framework code inside the resident control daemon). Provides visual DAG drag-and-drop authoring, real-time step monitoring, and web-based HITL safety interlock approval without impacting daemon stability or memory footprint. |
| **Status** | 📋 Candidate ADR — Targeted for 0.6.0 (Amends ADR-020, ADR-029, ADR-031). |

---

## 🏗️ 1. Architecture & Integration Topology

MetaMach Studio is an **optional** Web UI running as an independent sidecar binary (`janus-studio`). It connects to `janus-daemon` via Unix Domain Socket (`janus.sock`) and serves HTTP/REST + WebSocket endpoints to web browsers.

```text
 ┌────────────────────────────────────────────────────────────────────────┐
 │ 🌐 Browser / Mobile / Desktop Client                                   │
 │                                                                        │
 │  【 MetaMach Studio (Visual Canvas & Web Observer UI) 】              │
 │    - Unified Workflow DAG Drag-and-Drop Editor                         │
 │    - Real-time Node Status & PTY 16 KiB Log Output Stream              │
 │    - Web-based Safety Interlock Approval (HITL Gate)                   │
 └───────────────────▲────────────────────────────────────────────────────┘
                     │ REST API / WebSocket (127.0.0.1:8443)
                     │ Auth: X-Janus-Studio-Token
                     ▼
 ┌───────────────────┴────────────────────────────────────────────────────┐
 │ 🕸️ janus-studio (Standalone Rust Binary, Axum + rust-embed)            │
 │                                                                        │
 │  - Embedded Static UI Assets (React Flow / XYFlow via rust-embed)      │
 │  - REST API: /api/v1/workflows, /api/v1/gates                         │
 │  - WS API:   /runs/:id/stream (WebSocket Status & PTY Stream)        │
 │                                                                        │
 │  ── UDS Connection (0600 socket permissions) ──►                      │
 └────────────────────────────────────┬───────────────────────────────────┘
                                      │ UDS (janus.sock)
                                      ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │ 🧠 janus-daemon (MM-CORE Resident Daemon)                              │
 │                                                                        │
 │  - janus::tmux (Physical Execution Engine)                             │
 │  - janus::gateway (HITL Callback Listener & HMAC Verdict Awaiter)      │
 │  - DurableEngine + Absurd PG (Catalog & Blueprint DBs)                 │
 └────────────────────────────────────────────────────────────────────────┘
```

> **Core Isolation Principle:** `janus-studio` is completely decoupled from `janus-daemon`. If `janus-studio` crashes, web dependencies fail, or port 8443 is blocked, the daemon core, Tool Guard, and Absurd PG execution engine remain 100% operational.

---

## 🎨 2. Frontend Technology Selection

To adhere to MetaMach's zero-bloat philosophy:

- **Primary Stack:** React Flow / SvelteFlow packaged as static bundles and embedded into the `janus-studio` binary via `rust-embed`.
- **Alternative Size-Budget Stack:** HTML5 Canvas engine (XYFlow / native SVG renderer) if binary budget must strictly remain under 1 MB.

---

## 🛠️ 3. Visual Interface Modules

```text
+-----------------------------------------------------------------------------------+
| 🪐 MetaMach Studio v0.6.0                     [Blueprint: metamach_demo ▾] 🟢     |
+------------------------------------+----------------------------------------------+
| 🎨 Workflow Canvas (DAG Editor)    | 📊 Run Live Monitor                          |
|                                    |                                              |
|  [ Level 0 Barrier ]               | 🚀 Task: 019fd9f3-e6ba-704c-b2a0             |
|  ┌───────────────────┐             | ┌──────────────────────────────────────────┐ |
|  │wf_architect_design│             | │ wf_architect_design [ COMPLETED 🟢 ]      │ |
|  └─────────┬─────────┘             | │ wf_builder_implement[ RUNNING 🔵 ]        │ |
|            │ (needs)               | │ wf_tester_verify    [ PENDING ⚪ ]        │ |
|            ▼                       | └──────────────────────────────────────────┘ |
|  [ Level 1 Barrier ]               | -------------------------------------------- |
|  ┌───────────────────┐             | 🚨 SAFETY GATE INTERCEPTION!                  |
|  │wf_builder_implement│            | Command: rm -rf /tmp/metamach-sentinel-test  |
|  └─────────┬─────────┘             | Time Remaining: 24s [ Fail-Closed Timeout ]  |
|            │ (needs)               |                                              |
|            ▼                       |   [  APPROVE (合闸)  ]   [ REJECT (断电) ]   |
|  [ Level 2 Barrier ]               | -------------------------------------------- |
|  ┌───────────────────┐             | 📜 Live PTY Terminal Output Stream:          |
|  │ ⚠️ wf_tester_verify│ (HITL Gate)| [02:15:01] Compiling janus v0.5.0...         |
|  └───────────────────┘             | [02:15:05] test result: ok. 178 passed       |
+------------------------------------+----------------------------------------------+
```

### 1. Workflow DAG Visual Editor
- **Unified Workflow Support (ADR-031):** Supports both Linear Mode (sequential steps) and DAG Mode (multi-node dependency graphs with Kahn topological barriers).
- **Interactive Editing:** Drag workflows from side panel onto the canvas, wire `needs` dependency edges, and export directly to `.janus/workflows/*.toml`.

### 2. Real-Time Execution Monitoring
Step state lifecycle mapped to upper-case status enums:

| Status Enum | Color | Description |
|---|---|---|
| `PENDING` | ⚪ Gray/White | Waiting for predecessor level completion |
| `RUNNING` | 🔵 Pulsing Blue | Executing step command inside `janus::tmux` |
| `COMPLETED` | 🟢 Emerald Green | Absurd Checkpoint successfully committed |
| `FAILED` | 🔴 Rose Red | Non-zero exit code or timeout |
| `SUSPENDED` | 🟡 Amber Flash | Intercepted by Tool Guard (Awaiting HITL Approval) |

### 3. Web Safety Interlock Approval
- High-risk operations trigger a visual alert card with configurable countdown timer (via `Gateway::await_verdict`).
- Clicking **[ APPROVE ]** issues an HMAC-signed `GateAction { task_id, approve: true }` UDS request to `janus-daemon`.

---

## 📡 4. UDS & REST API Contracts

### A. Reused Existing UDS Endpoints (`janus/src/protocol.rs`)

| Request Variant | Purpose |
|---|---|
| `Ping` / `Pong` | Liveness check between `janus-studio` and `janus-daemon` |
| `Progress { blueprint: Option<String> }` | Query in-flight task progress and node statuses |
| `Dispatch { blueprint, workflow, inline_command }` | Trigger workflow or DAG execution |
| `Stop { blueprint, task_id }` | Cancel/halt running task execution |
| `Continue { blueprint, task_id }` | Resume suspended task from Absurd PG checkpoint |
| `GateAction { task_id: Uuid, approve: bool }` | Submit HITL gate verdict (ADR-020, already delivered) |
| `GuardCheck { agent_role, capability, argv }` | Test Tool Guard rules |
| `Onboard` / `Offboard` | Lifecycle management |

### B. Proposed New 0.6.0 Endpoints

| Endpoint | Protocol / Route | Description |
|---|---|---|
| `ListWorkflows` | UDS `Request::ListWorkflows` | List workflow TOML definitions in `.janus/workflows/` |
| `GetWorkflow { name }` | UDS `Request::GetWorkflow` | Fetch single workflow definition |
| `SaveWorkflow { name, toml }` | UDS `Request::SaveWorkflow` | Validate & save workflow TOML into `.janus/workflows/` |
| WS `/runs/:id/stream` | WebSocket (Studio HTTP) | Real-time step state transitions & PTY output stream |

---

## 🔒 5. Authentication, Error Handling & State Synchronization

### A. Authentication & Security Model
1. **Studio ↔ Daemon (UDS):** File permissions enforced by Unix Domain Socket (`janus.sock`, mode `0600`).
2. **Browser ↔ Studio (HTTP/WS):**
   - Default bind to loopback interface `127.0.0.1:8443` (preventing external network exposure).
   - Header auth via `X-Janus-Studio-Token`. On startup, `janus-studio` generates a 256-bit random token saved to `~/.metamach/studio.token` (permissions `0600`).
   - Optional TLS configuration (`--tls-cert`, `--tls-key`).

### B. Error Handling & Resiliency
1. **Daemon UDS Disconnect:** `janus-studio` enters exponential backoff reconnect loop (1s, 2s, 4s, max 10s). UI displays non-blocking banner: `"Daemon Disconnected — Retrying UDS..."`.
2. **WebSocket Drop:** Client automatically reconnects with `?last_event_id=` to prevent lost logs.
3. **Workflow TOML Validation Failure:** `SaveWorkflow` validates Kahn DAG cycles before writing. On failure, returns structured error (`{ line, column, message }`) for inline editor syntax highlighting.

### C. State Synchronization on Connect
Upon initial WebSocket connection or page refresh:
1. `janus-studio` executes `Request::Progress` over UDS.
2. Studio transmits a `SNAPSHOT` message over WebSocket containing all active tasks and node status states.
3. Subsequent updates are delivered as lightweight `DELTA` streaming events.

---

## 🌐 6. Operational & Multi-Blueprint Design

### A. Multi-Blueprint Workspace Navigation
Studio top bar features a global Blueprint Selector dropdown (`metamach_demo`, `spec2software`, etc.). Switching blueprints filters `Progress` queries and `.janus/workflows/` targets dynamically.

### B. CLI Integration & Launcher
`janus-studio` is managed via the main CLI:

```bash
# Launch Studio sidecar on default port 8443:
janus studio

# Launch on custom port with custom token path:
janus studio --port 9090 --token-file ~/.metamach/studio.token
```

### C. Testing Strategy
- **Unit Testing:** Axum route handlers tested via `tower::ServiceExt`.
- **Integration Testing:** UDS client tested against mock daemon socket.
- **Visual E2E Testing:** Playwright headless browser suite validating DAG drag-and-drop and HITL verdict dispatch.

---

## 🏁 7. Dependency Chain & Milestone Placement

```text
Workflow DAG (0.5.0 - ADR-031)  ──►  0.5.0 Engine Stabilization  ──►  MetaMach Studio (0.6.0)
   .janus/workflows/*.toml               Cargo Test 178 Green               janus-studio Binary
```

MetaMach Studio is scheduled for implementation in milestone **0.6.0** following 0.5.0 stabilization.
