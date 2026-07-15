# 🛡️ MetaMach 2.0

> **An industrial-grade, durable AI Software Factory OS powered by Janus Daemon and distributed physical execution sessions.**

MetaMach 2.0 orchestrates specialized AI agents (Claude Code, Codex, Pi) as isolated, ephemeral function nodes inside robust, survivable engineering pipelines—managed straight from your pocket via Telegram or TUI.

---

## 🪐 Core Pillars

- **🧠 Brain-as-a-Daemon (Janus Daemon)**  
  The control plane is a standalone Rust daemon (`janus-daemon`) that owns the entire state machine, database connection pool, and event gateway. The Herdr plugin runs as a lightweight shadow client (`herdr-janus`) responsible only for rendering and interaction—UI crashes never lose engineering context.

- **🔌 Cross-Host Session Durability (Tether Engine)**  
  All physical agent execution sessions are backed by `remain-on-exit` tmux sessions via `herdr-tether`, surviving network blips, SSH drops, or terminal closes across local and remote machines. Re-attach and resume in milliseconds.

- **🛡️ Durable Workflows & HITL (Self-Healing)**  
  Workflow state is transactional and atomic. When an AI hits a blocking failure (compile error, permission denied), the pipeline auto-suspends, preserves the terminal live, and signals a **Human-in-the-Loop** approval via Telegram/Teams. Resume at the exact breakpoint.

---

## 📐 Three-Dimensional Customization

| Dimension | Description |
|-----------|-------------|
| **Agent Pool & Stack** (生产要素) | Global registration of AI resources—API keys and SSH credentials decrypted in `/dev/shm` (RAM disk, never leaked to disk). Fine-grained role-based permission levels (Scout / Coder / Deployer). |
| **Workflows** (工艺流水线) | Declarative `.toml` pipelines (`workflows/*.toml`). Chain multiple agent stations across local and remote SSH hosts. |
| **Blueprints** (产品蓝图) | Physical project containers under `blueprints/<name>/`. Each binds a custom `janus.toml` recipe, a multi-tenant database partition, OpenWiki knowledge scope, and optional remote compilation targets. On offboarding, auto-compacts the database and generates a `production_report.md`. |

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
│   ├── herdr-plugin.toml     #   Herdr v1 plugin manifest
│   └── src/
│       ├── bin/
│       │   ├── janus_daemon.rs  # 🪐 Control-plane daemon
│       │   └── herdr_janus.rs   # 🔌 Herdr shadow client
│       ├── tool_guard/          # janus-sh UDS proxy shell
│       ├── absurd/              # Postgres transaction ledger
│       └── tui/                 # Ratatui popup interface
├── openwiki/                 # Shared RAG knowledge base
├── workflows/                # Declarative pipeline SOPs
│   ├── dev-flow.toml
│   ├── debug-flow.toml
│   └── firmware-deploy.toml
├── provisioning/             # Bootstrap, init scripts
├── docker-compose.yml        # Unified Postgres container
└── Makefile                  # Factory master switch
```

---

## ⚡ Quick Start

### Prerequisites
- Ubuntu 22.04+ or macOS 13+
- Rust 1.88+ (Edition 2024), Tmux 3.2+, Docker & Compose
- Herdr with `metamach.janus` plugin installed

### One-Command Bootstrap
```bash
make bootstrap
```
This single command **auto-provisions** everything:
1. Creates immutable/mutable directory separation with symlinks
2. Compiles `janus-daemon`, `herdr-janus`, and `janus-sh` in release mode
3. Spins up and initializes the Unified Postgres database via Docker Compose

After bootstrap, press `prefix+j` inside Herdr to open the Dispatcher console and dispatch a workflow.

### Shutdown
```bash
make db-down   # Stop the Postgres container
make clean     # Clean build artifacts and unmount RAM disk
```

---

## 🛡️ Resilience Invariants

- **Remain-on-Exit**: Every Tether-powered session is 100% crash-proof. AI process segfaults? Syntax errors? The tmux terminal stays alive, preserving full context.
- **16KB Budget**: Step checkpoints and stdout capture are strictly capped at 16KB. Database `Janus GC` prunes expired entries every 24 hours—no unbounded bloat.
- **janus-sh Proxy Shell**: Agent commands are intercepted via a UDS sync protocol before reaching bash. High-risk operations are physically suspended until HITL approval via Teams/Telegram.
- **Stateless Cold Start**: Postgres is the sole source of truth. After a full power loss, `janus-daemon` reconnects, identifies the last completed checkpoint, and resumes execution at the breakpoint—no `tmux-resurrect` reliance.