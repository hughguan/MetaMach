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

- **🧠 Brain-as-a-Daemon (janus-daemon)**
  The control plane is a standalone Rust daemon that owns the entire state machine, database connection pool, and event gateway. The Herdr plugin runs as a lightweight shadow client (`herdr-janus`) responsible only for rendering and interaction — UI crashes never lose engineering context.

- **🔌 Cross-Host Session Durability (janus::tmux)**
  All physical agent execution sessions are backed by `remain-on-exit` tmux sessions via the internalized `janus::tmux` native Rust module. The external `herdr-tether` plugin has been fully deprecated and replaced. tmux sessions survive frontend popup destruction (SIGHUP immunity) and operate at microsecond-level IPC latency.

- **🛡️ Durable Workflows & HITL (Self-Healing)**
  Workflow state is transactional and atomic. When an AI hits a blocking failure (compile error, permission denied), the pipeline auto-suspends, preserves the terminal live, and signals a **Human-in-the-Loop** approval via Teams/Telegram through `janus::gateway`. Resume at the exact breakpoint.

- **🧱 De-containerized (No Docker)**
  Postgres runs as a host-native process, not a Docker container. `make bootstrap` compiles, symlinks, and launches PG directly — no Docker, no Compose, no container network overhead. Physical persistence at `~/.metamach/db/` survives power-cycle restarts.

- **📊 Dual-Track Data Survival**
  Normal writes go through `janush → UDS → janus-daemon → Absurd PG`. If PG goes down, the SQLite ring buffer (`fallback.db`) keeps the workshop alive. On PG recovery, the ring buffer replays into PG automatically.

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
