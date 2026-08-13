# 🛡️ MetaMach

<p align="center">
  <a href="https://github.com/hughguan/MetaMach/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/hughguan/MetaMach/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-0f766e.svg"></a>
  <a href="https://github.com/ogulcancelik/herdr"><img alt="Herdr 0.7.3+" src="https://img.shields.io/badge/Herdr-0.7.3%2B%20(Optional)-172033.svg"></a>
  <img alt="macOS & Linux" src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux-475569.svg">
  <img alt="Tests: 206" src="https://img.shields.io/badge/tests-206%20(CI%20green)-22c55e.svg">
  <img alt="Version: 0.7.0-candidate" src="https://img.shields.io/badge/version-0.7.0--candidate-6366f1.svg">
</p>

> **MetaMach is a durable AI Software Factory OS —**
> a bare-metal safety harness and execution engine that orchestrates autonomous AI agents (Claude Code, Codex, Aider, etc.) inside survivable engineering workflows with checkpoints, Human-in-the-Loop (HITL) approval gates, and sandboxed execution.

---

## Quick Start

### 1. Prerequisites

| Dependency | Requirement | Purpose |
|---|---|---|
| **Rust** | 1.88+ (Edition 2024) | Required — compiles MetaMach binaries |
| **PostgreSQL** | 16+ (host-native, no Docker) | Required — durable state machine & task queue |
| **tmux** | 3.3+ | Required — isolated PTY session engine |
| **Herdr** | 0.7.3+ | *Optional* — terminal TUI dashboard (`herdr-janus`) |

> **Note:** MetaMach runs 100% standalone via `janus` CLI and Web Studio (`janus studio`). Herdr is an optional plugin host if you want a terminal overlay.

### 2. Installation & Bootstrap (30-second setup)

```bash
git clone https://github.com/hughguan/MetaMach.git
cd MetaMach
make bootstrap          # One command: checks prereqs, compiles binaries, initializes host PG
```

### 3. Initialize & Run Your First Project

```bash
cd my-project
janus init              # Scaffolds .janus/, auto-starts daemon, validates & registers
janus start             # Runs the workflow with checkpointing & safety gates
```

For quick single-command executions without a TOML workflow file:
```bash
janus start --inline "cargo test"       # Auto-generates a transient single-step workflow
```

---

## Workflow Lifecycle

Every project managed by MetaMach follows a simple 7-verb lifecycle:

`init → plan → dry-run → start → monitor → harvest → offboard`

| Step | Command | Description |
|---|---|---|
| **init** | `janus init` | Scaffold `.janus/`, validate blueprint, register project database with daemon |
| **plan** | `janus plan --description "..."` | Generate or edit a workflow definition (`.janus/workflows/*.toml`) |
| **dry-run** | `janus start --dry-run` | Preview execution plan & DAG topology without running commands |
| **start** | `janus start` | Execute workflow with automatic step checkpointing & tool reconciliation |
| **monitor** | `janus status` / `stop` / `continue` | Check real-time progress, pause, or resume interrupted workflows |
| **harvest** | `janus harvest list` / `apply` | Inspect and apply outputs from sandboxed execution tasks |
| **offboard** | `janus offboard --blueprint <name>` | Archive audit log, generate production report, purge transient runtime data |

> **LLM-Assisted Planning:** Generate a workflow directly from natural language:  
> `janus plan --blueprint my-project --description "Build Rust binary, run test suite, deploy"`

---

## Workflow DSL & Safety Rail Examples

MetaMach uses a unified **Workflow DSL** (`.janus/workflows/<name>.toml`) supporting both linear sequential steps and multi-node DAG workflows.

### 1. Linear Workflow (Sequential Steps)

```toml
# .janus/workflows/ci.toml
[workflow]
name = "ci"
description = "Build and test suite"

[[steps]]
name = "build"
agent = "builder"
command = "cargo build --release"

[[steps]]
name = "test"
agent = "tester"
command = "cargo test"
max_correction_attempts = 3      # re-dispatches with error context on failure
```

### 2. Minimal DAG Workflow (Parallel Execution)

```toml
# .janus/workflows/parallel_ci.toml
[workflow]
name = "parallel_ci"
description = "Run linter and tests concurrently after build"

[[nodes]]
id = "build"
workflow = "wf_build"

[[nodes]]
id = "lint"
needs = ["build"]
steps = [{ name = "clippy", command = "cargo clippy" }]

[[nodes]]
id = "test"
needs = ["build"]
steps = [{ name = "cargo_test", command = "cargo test" }]
```

### 3. Advanced DAG Workflow (Dual-Track Isolation & Path Whitelisting)

Nodes can declare **sandbox isolation** (runs in an isolated Git worktree) and **allowed write paths**:

```toml
# .janus/workflows/fullstack_iot.toml
[workflow]
name = "fullstack_iot"
description = "Parallel web build and firmware flashing"

[[nodes]]
id = "build_web_ui"
isolation = "sandbox"            # runs in an isolated Git worktree & sandbox tmux
writes = ["apps/web/dist/"]      # whitelist: post-execution guard flags writes outside this directory
steps = [{ name = "install", command = "bun install && bun run build" }]

[[nodes]]
id = "flash_esp32"
isolation = "bare_metal"         # host-native, janush-protected physical execution
needs = ["build_web_ui"]
steps = [{ name = "flash", command = "esptool.py write_flash 0x0 target/firmware.bin" }]
```

---

## Monitoring, Web Studio & Harvest

### CLI Monitoring & Task Control

```bash
janus status                             # Snapshot active task status
janus status --json                      # Machine-readable output
janus stop --blueprint my-project        # Pause active tasks for a project
janus continue --blueprint my-project    # Resume paused or interrupted tasks from last checkpoint
```

### MetaMach Studio (Visual Canvas & Web Observer)

Launch the interactive web observer sidecar:

```bash
janus studio                             # Starts Web Studio at http://127.0.0.1:8444
janus studio -d                          # Launch detached in the background
```

- 🎨 **Visual DAG Canvas**: Drag-and-drop workflow editor and execution plan graph.
- 📡 **Real-Time Stream**: Live WebSocket observer streaming step state transitions.
- 🛡️ **HITL Web Interlocks**: One-click human-in-the-loop approval center for high-risk operations.

### Sandbox Harvest Pipeline

Review and apply outputs from sandboxed steps without risking main workspace contamination:

```bash
janus harvest list                                       # List all harvested sandbox refs
janus harvest apply --ref-name refs/sandbox/<id>-<step>   # Apply approved sandbox output to working directory
```

---

## Architecture & Project Structure

### System Architecture

```
                         ═══════════════════════════════════════════
                          CLI Agent (Claude Code / Codex / Pi / ...)
                         ═══════════════════════════════════════════
                                        │
                                        │ spawns on user command
                                        ▼
                         ┌──────────────────────────────┐
                         │  janush  (Proxy Shell)       │
                         │  • Tool Guard reconciliation │
                         │  • 30s fail-closed timeout   │
                         │  • 16KB streaming truncation │
                         └──────────────┬───────────────┘
                                        │ UDS (Unix Domain Socket)
                                        ▼
                         ┌──────────────────────────────────────────┐
                         │        janus-daemon  (MM-CORE)           │
                         │  • Master state machine & UDS router     │
                         │  • Workflow engine & durable checkpoints │
                         │  • DAG engine (topological sort)         │
                         │  • HITL Gateway & Webhooks               │
                         │  • Cold-start resume & self-healing      │
                         └──┬─────────────┬─────────────┬──────────┘
                            │             │             │
                   ┌────────┘             │             └──────────┐
                   ▼                      ▼                        ▼
     ┌─────────────────────┐  ┌───────────────────┐  ┌──────────────────────┐
     │  janus::tmux         │  │  PostgreSQL       │  │  janus::gateway       │
     │  • tmux -L mm-tmux   │  │  • catalog DB     │  │  • HITL dispatch      │
     │  • remain-on-exit    │  │  • per-blueprint DB│  │  • Webhook ingress    │
     │  • SSH reverse tunnel│  │  • SQLite fallback │  │  • HMAC verification  │
     └──────────────────────┘  └───────────────────┘  └──────────────────────┘
                   ▲                                           ▲
                   │ (reattach view)                           │ UDS proxying
     ┌──────────────────────┐                  ┌──────────────────────────────┐
     │  herdr-janus (TUI)    │                  │  janus-studio (Web Observer) │
     │  • Terminal dashboard│                  │  • Visual DAG Canvas Editor  │
     └──────────────────────┘                  │  • Real-Time WS Streamer     │
                                               └──────────────────────────────┘
```
> **TL;DR:** `janus-daemon` owns all state in PostgreSQL while `janush` interceptively checks every agent command before bare-metal execution. Decoupled Web Studio sidecar runs on `http://127.0.0.1:8444`.

### MetaMach Workspace Layout

```
metamach/
├── docs/                        # Technical specifications & ADR index
├── janus/                       # Core Rust workspace (~14,100 LOC)
│   ├── src/bin/                 #   5 Binaries: janus, janus-daemon, herdr-janus, janush, janus-studio
│   ├── src/                     #   Engine modules (absurd, tmux, tool_guard, gateway, cognitive, workflow)
│   └── tests/                   #   Integration test suite (9 files, 206 tests)
├── templates/                   # Default templates for `janus init`
│   ├── blueprint.toml           #   Default blueprint recipe
│   ├── agents/                  #   architecture.toml, builder.toml, tester.toml
│   └── workflows/               #   15 workflow templates (linear + DAG)
└── Makefile                     # Bootstrap, database setup, health check
```

### Per-Project Layout (after `janus init`)

```
my-project/
├── .janus/                      # Project-specific MetaMach configuration
│   ├── blueprint.toml           #   Project recipe & default workflow selection
│   ├── agents/                  #   Agent role overrides (architect, builder, tester)
│   │   ├── architecture.toml
│   │   ├── builder.toml
│   │   └── tester.toml
│   ├── workflows/               #   Project workflows (linear + DAG)
│   │   ├── wf_architect_design.toml
│   │   ├── wf_builder_implement.toml
│   │   ├── req2spec.toml
│   │   └── spec2software.toml
│   └── openwiki/                #   RAG knowledge scope & production reports
└── src/                         # Your project source code
```

---

## Core Principles

- 🛡️ **Safety First**: Fail-closed 30s timeouts, post-execution file write whitelisting, sandboxed Git worktree isolation, and human approval gates for high-risk operations.
- ⚙️ **Uncompromising Stability**: Daemon-owned state machine, transactional PostgreSQL checkpoints with SQLite fallback, and automatic cold-start task recovery.
- 🔌 **Pure Decoupling**: Agent-agnostic integration with any CLI tool, decoupled Web Studio sidecar, and host-native execution without container overhead.

---

## 📚 Specifications & Documentation

The authoritative technical specifications live under `docs/`:

- [PRD.md](docs/PRD.md) — Product Requirements & Persona Specifications
- [ARCH.md](docs/ARCH.md) — High-Level Architecture & Component Topology
- [ADR.md](docs/ADR.md) — Architecture Decision Records Index (36 Decision Records)
- [SPEC.md](docs/SPEC.md) — Technical Contracts, Test Catalog & Deployment Matrix
- [PLAN.md](docs/PLAN.md) — Execution Roadmap & Milestone History

---

## Troubleshooting & FAQ

<details>
<summary><b>Daemon not reachable (janus-daemon)</b></summary>

If `janus` commands report `janus-daemon not reachable`, start the resident background daemon:
```bash
janus daemon                            # Launch daemon in foreground (Ctrl+C to stop)
make health                             # Verify daemon socket & PG connection
```
</details>

<details>
<summary><b>PostgreSQL database connection issues</b></summary>

MetaMach uses a host-native PostgreSQL instance (no Docker). If the database is down or uninitialized:
```bash
make db-init                           # Initialize PG cluster at ~/.metamach/db/ & run catalog migrations
```
</details>

<details>
<summary><b>Is MetaMach a replacement for Docker or CI runners?</b></summary>

No. MetaMach is an **AI Software Factory OS** designed to safely host autonomous AI coding agents on local or remote dev machines with hardware access, strong checkpoints, and human approval gates.
</details>

---

## CI & Testing

- **206 Workspace Tests**: 100% passing (140 unit tests + 66 integration tests across 9 files).
- **CI Gates**: Every commit passes `cargo fmt`, `cargo clippy -D warnings`, and `cargo test --workspace`.
- **Pre-Push Hook**: `./scripts/pre-push` auto-provisions local PostgreSQL and validates E2E workflow dispatches.

---

## License

MIT — see [LICENSE](LICENSE).
