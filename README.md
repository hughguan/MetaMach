# 🛡️ MetaMach 0.4.0

> $$\text{MetaMach} = \mathbf{META}\text{-Control } + \text{ Bare-}\mathbf{MACH}\text{ine Engine}$$
>
> **MetaMach is NOT an AI agent framework. It is a Bare-Metal, High-Availability Safety Harness & Execution Engine for Autonomous Agents in the Physical World.**

MetaMach 0.4.0 orchestrates specialized AI agents (Claude Code, Codex, Pi) as isolated, ephemeral function nodes inside robust, survivable engineering pipelines — managed straight from your pocket via Teams, Telegram, or TUI.

---

## 🪐 Industrial Suite Architecture

```
                       ┌─────────────────────────────────────────────────────────┐
                       │          🪐 MetaMach 0.4.0 Industrial Suite              │
                       └────────────────────────────┬────────────────────────────┘
                                                    │
    ════════════════════ 1. PHYSICAL INTERCEPTION & EXECUTION ════════════════════
                                                    │
      【 Terminal / Any CLI Agent 】 ──(Spawns)──► 【 janush 】 (Interception Shell)
      (Aider, Roo Code, Claude Code...)              │
                                                     │ ── 16KB Streaming Truncation
                                                     │ ── Synchronous UDS
                                                     ▼
    ═══════════════════════ 2. MM-CORE CONTROL PLANE ═════════════════════════════

                                 ┌───────────────────────────────────────┐
                                 │     🧠 janus-daemon (MM-CORE)        │
                                 │  - Master State Machine & UDS Router  │
                                 │  - 30s Fail-Closed Timeout Engine     │
                                 └───────────┬───────────────┬───────────┘
                                             │               │
                     ┌───────────────────────┘               └───────────────────────┐
                     ▼                                                               ▼
   【 janus::tmux 】 (PTY Sandbox Engine)                            【 Storage & Data Survival 】
   - Isolated `tmux -L metamach-tmux`                                - Primary: Native Absurd PG (`~/.metamach/db/`)
   - SIGHUP Immunity & Physical Keep-Alive                           - Fallback: SQLite Ring Buffer (`fallback.db`)
   - Bare-Metal Hardware Access (USB/GPIO)                           - Authoritative 16KB Pre-Insert Truncation
                     │
                     ▲ (Reattach View)
                     │
         【 herdr-janus 】 (Shadow TUI)
         Transient Renderer / Zero State

    ═══════════════════ 3. DECOUPLED ECOSYSTEM INTEGRATIONS ═══════════════════════
                                                     │
                                                     ▼
                                       【 janus::gateway 】 (HITL Gateway)
                                       - Payload-Complete HTTP/UDS Proxy
                                       - Hermes Run API Schema (/v1/runs)
                                                     │
                             ┌───────────────────────┴───────────────────────┐
                             ▼                                               ▼
         【 Human-in-the-Loop Channels 】                   【 Opt-in Cognitive Services (SPI) 】
         - Microsoft Teams (Adaptive Cards)                - OpenWiki (Contextual Markdown)
         - Telegram / Out-of-band Webhooks                 - codebase-memory-mcp (Tree-Sitter / MCP)
         - Remote Approve / Reject / Override

```

### 🛠️ Layer-by-Layer Physical Logic

**1. Physical Interception Layer.** Any CLI Agent running in a shell issues commands — `janush` acts as the first interception line, performing transparent capture, 16KB real-time streaming truncation, and synchronous reporting to `janus-daemon` via Unix Domain Socket (UDS).

**2. MM-CORE Control & Durability Layer.**
- **`janus-daemon` → `janus::tmux`**: After confirming the command is safe, the Daemon drives the underlying `janus::tmux` engine to execute the instruction inside the isolated `metamach-tmux` server. Even if the foreground UI crashes or the SSH connection drops, the underlying process keeps running uninterrupted.
- **`janus-daemon` → Storage**: State writes are preferentially committed to the host-native Absurd PG. If PG crashes, writes seamlessly fail over to the SQLite ring buffer for degraded-mode survival; PG automatically replays on recovery.
- **`herdr-janus` → `janus::tmux`**: A pure shadow TUI renderer — only mounts and interacts with the physical screen; holds zero state.

**3. Decoupled Gateway & Cognition Layer.**
- **`janus-daemon` ↔ `janus::gateway`**: When a high-risk operation triggers the 30s Fail-Closed suspension, the Daemon dispatches the event to the payload-complete `janus::gateway`.
- **`janus::gateway` → External World**:
  - **Human Circuit-Breaker**: Pushes Rich Adaptive Cards to **Microsoft Teams** via the Hermes `/v1/runs` compatible protocol. The Factory Director remotely taps Approve to energize the circuit; the signal returns through the Gateway to the Daemon, unfreezing `janush`.
  - **Cognitive Enhancement**: Asynchronously queries `codebase-memory-mcp` and `OpenWiki` context via MCP on demand; the MM-CORE Daemon maintains a minimal memory footprint (< 50MB).

---

## 🧱 Core Pillars

> **Architecture Invariant:** MetaMach is built on four non-negotiable industrial principles: Safety, Stability, Decoupling, and Reusability. It provides a bare-metal, fail-closed execution harness for autonomous AI agents operating in high-risk engineering environments.

### 🛡️ 1. Safety First

**Fail-Closed Gatekeeping (`janush`).** Every shell command passes through the synchronous `janush` interceptor. Any unverified or high-risk operation triggers a strict 30-second timeout — if unapproved, execution hard-fails closed, preventing catastrophic hardware or system mutations.

**Dual-Tier Flow Budgeting.** Strict 16KB log truncation enforced both at the streaming shell boundary (`janush`) and pre-database insertion (`janus-daemon`), shielding local storage from agent infinite loops.

**Out-of-Band HITL Guard.** High-risk actions automatically freeze state and fan out interactive Adaptive Cards to Microsoft Teams/Telegram via `janus::gateway`. Human operators can approve, modify, or terminate execution remotely without granting the network layer write access to local PTY sandboxes.

### ⚙️ 2. Uncompromising Stability

**Brain-as-a-Daemon (`janus-daemon`).** The core control plane is an independent, host-native Rust daemon owning the transactional state machine (Absurd PG) and UDS event router. The TUI (`herdr-janus`) is merely a transient view — UI crashes or terminal closures never lose engineering state.

**Dual-Track Data Survival (SQLite Ring Buffer).** Production state is atomically backed by Postgres. In the event of a host PG crash (OOM or disk pressure), execution seamlessly fails over to a local SQLite ring buffer (`fallback.db`), keeping the workshop running in Degraded Mode until PG automatically replays and recovers.

**De-containerized Physical Persistence.** No Docker or Compose overhead. Postgres runs as a native host sandbox writing directly to `~/.metamach/db/`, ensuring absolute state preservation across power-cycle restarts.

### 🔌 3. Pure Decoupling

**Execution vs. UI Decoupling.** PTY session keep-alive is isolated inside the native `janus::tmux` engine (`tmux -L metamach-tmux`). Sessions possess SIGHUP immunity and survive complete disconnection of the developer's laptop or frontend interface.

**Control vs. Cognition Decoupling (Opt-in SPIs).** Heavy symbol indexing (`codebase-memory-mcp`) and contextual knowledge mapping (OpenWiki) are completely isolated into asynchronous, opt-in Model Context Protocol (MCP) plugins, maintaining a minimal core daemon footprint.

**Payload-Complete HITL Gateway.** The notification routing layer (`janus::gateway`) is completely decoupled from PTY lifecycle management; network latency or external webhook failures will never deadlock or crash ongoing physical execution.

### 🔄 4. Universal Reusability

**Hermes Protocol Convergence.** The gateway exposes native compatibility with the Hermes Run API Schema (`/v1/runs`), allowing MetaMach to instantly reuse pre-existing open-source agent dashboards, webhooks, and multi-channel notification bots out-of-the-box.

**Agent & Model Agnostic.** Operates directly at the OS PTY boundary. Works seamlessly with any CLI agent (Claude Code, Aider, Roo Code, Codex) without requiring custom prompts, specific vendor models, or API wrappers.

**Single-Binary Zero-Dependency Bootstrap.** A clean, host-native Rust binary architecture. `make bootstrap` compiles, symlinks, and points to `~/.metamach/db/` for immediate bare-metal deployment.

---

## 📐 Three-Dimensional Customization

| Dimension | Description |
|-----------|-------------|
| **Agent Pool & Stack** | Global registration of AI resources — API keys and SSH credentials decrypted in `/dev/shm` (RAM disk, never leaked to disk). Fine-grained role-based permission levels (Scout / Coder / Deployer). |
| **Workflows** | Declarative `.toml` pipelines (`workflows/*.toml`). Chain multiple agent stations across local and remote SSH hosts. |
| **Blueprints** | Physical project containers under `blueprints/<name>/`. Each binds a custom `janus.toml` recipe, a dedicated per-blueprint database (`metamach_blueprint_<name>`), OpenWiki knowledge scope, and optional remote compilation targets. On offboarding, purges operational data with full audit trail and generates a `production_report.md`. |

---

## 🗂️ Project Structure

```
metamach/
├── blueprints/               # Product blueprints (joyrobots, gatemetric...)
│   ├── joyrobots/            #   Modular education robot platform
│   └── gatemetric/           #   BMX attitude evaluation system
├── integrations/             # 🔌 Opt-in external services (SPI / MCP)
│   ├── openwiki/             #   Shared RAG knowledge base
│   └── codebase-memory-mcp/  #   Tree-sitter symbol index (MCP transport)
├── docs/                     # Full design specs, PRD, test, deployment
├── janus/                    # 🛡️ Janus Core (Rust)
│   ├── Cargo.toml            #   Rust workspace
│   ├── herdr-plugin.toml     #   Herdr 0.7.3 plugin manifest
│   └── src/
│       ├── bin/
│       │   ├── janus_daemon.rs  # 🪐 Control-plane daemon (MM-CORE Brain)
│       │   ├── herdr_janus.rs   # 🔌 Herdr shadow client (TUI only)
│       │   ├── janush.rs        # 🛡️ Proxy shell (30s Fail-Closed fuse)
│       │   └── janus.rs         # 📋 Unified CLI entrypoint
│       ├── gateway/             # 🌐 janus::gateway — HITL dispatch (Hermes/Teams)
│       ├── cognitive/           # 🔌 Cognitive Provider SPI
│       ├── tmux/                # 🔌 janus::tmux — PTY session engine
│       ├── tool_guard/          # 🛡️ Rule engine + webhook dispatch
│       ├── absurd/              # 📊 Postgres transaction ledger + SQLite fallback
│       ├── lifecycle.rs         # 🔄 Onboard/Offboard lifecycle
│       ├── coldstart.rs         # ❄️ Cold-start self-healing
│       ├── protocol.rs          # 📜 Shared types + Contracts 3.x/4.x
│       └── lib.rs               # 📚 Crate root
├── configs/                  # Agent pool, tmux config, global rules
├── workflows/                # Declarative pipeline SOPs
│   ├── dev-flow.toml
│   ├── debug-flow.toml
│   └── firmware-deploy.toml
├── provisioning/             # Bootstrap, init scripts
└── Makefile                  # Factory master switch (no Docker)
```

---

---

## ⚡ Quick Start

### Prerequisites
- Linux or macOS
- Rust 1.88+ (Edition 2024), Tmux 3.3+
- **Postgres 16+** (host-native — no Docker required)
- Herdr with `metamach.janus` plugin installed

### Bootstrap
```bash
make prereq      # Check for pg_config, tmux, cargo (fails fast with instructions)
make bootstrap   # Compile + symlink + init DB
```
`make bootstrap` **auto-provisions** everything:
1. Creates immutable/mutable directory separation with symlinks (`~/.metamach/db/`)
2. Compiles `janus-daemon`, `herdr-janus`, `janush`, and `janus` in release mode
3. Launches host-native Postgres and runs catalog migration (`001_catalog.sql`)

After bootstrap, press `prefix+j` inside Herdr to open the Dispatcher console and dispatch a workflow.

### Shutdown
```bash
make db-down   # Gracefully stop the Postgres instance
make clean     # Clean build artifacts and unmount RAM disk
```
