# 🛡️ MetaMach 0.4.0

> $$\text{MetaMach} = \mathbf{META}\text{-Control } + \text{ Bare-}\mathbf{MACH}\text{ine Engine}$$
>
> **MetaMach is NOT an AI agent framework. It is a Bare-Metal, High-Availability Safety Harness & Execution Engine for Autonomous Agents in the Physical World.**

MetaMach 0.4.0 orchestrates specialized AI agents (Claude Code, Codex, Pi) as isolated, ephemeral function nodes inside robust, survivable engineering pipelines — managed straight from your pocket via Teams, Telegram, or TUI.

---

## 🪐 The 0.4.0 Triad

```
       [ MetaMach 0.4.0 Industrial Suite ]
                       │
       ┌───────────────┼───────────────┐
       ▼               ▼               ▼
  【 janush 】    【 janus-daemon 】 【 janus::gateway 】
 (Interception)    (MM-CORE Brain)  (Hermes/Teams HITL)
```

| Component | Role | Key Invariant |
|-----------|------|---------------|
| **`janush`** | Invisible safety shell — PTY 30s Fail-Closed fuse | Synchronous UDS interception before any command reaches Bash; never lets through on timeout |
| **`janus-daemon`** | Background control brain — Absurd PG state ignition, `janus::tmux` PTY keep-alive, SQLite degraded-mode fallback | Sole owner of state and DB connection pool; survives frontend crash |
| **`janus::gateway`** | Physical portal connecting the human workshop (Teams/Web/Mobile) to the bare-metal machine | Payload-complete HITL dispatch; Hermes Run API envelope; non-blocking — tmux session never frozen |

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

## 📚 Documentation

Full design specifications live directly under `docs/` (English) — this is the **sole version-controlled spec source**. Chinese translations and audit artifacts live under `docs/CH/`, which is gitignored (local-only, non-authoritative); when the two disagree, `docs/` wins.

| Doc | Scope |
|-----|-------|
| `ARCH.md` | System architecture, topology, component interactivity |
| `ARCH-0.2.0.md` | Database layer evolution exploration (0.1.0 → 0.2.0 proposals) |
| `ARCH-0.3.0.md` | **Architecture consensus baseline** — final arbitration of all 0.3.0 proposals |
| `ARCH-0.4.0.md` | **0.4.0 Delta** — Gateway, Cognitive Provider SPI, Teams HITL |
| `PRD.md` | Product requirements & Factory Director user journey |
| `Feature-Spec.md` | Engineering feature specs, data contracts (Contracts 3.1–4.3c), fault matrix |
| `Project-Plan.md` | Milestone roadmap (M0–M5) & check-in units |
| `Review-Spec.md` | Audit/review standards & sign-off criteria (REV-SEC, REV-STB, REV-DIS, REV-EVO, REV-GW, REV-COG) |
| `Test-Spec.md` | Test cases (UTC-01 through UTC-10) & QA strategy |
| `Deployment-Spec.md` | Physical deployment, bootstrap, directory mapping |

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
