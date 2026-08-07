# 🛡️ MetaMach

<p align="center">
  <a href="https://github.com/hughguan/MetaMach/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/hughguan/MetaMach/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-0f766e.svg"></a>
  <a href="https://github.com/ogulcancelik/herdr"><img alt="Herdr 0.7.3+" src="https://img.shields.io/badge/Herdr-0.7.3%2B-172033.svg"></a>
  <img alt="macOS & Linux" src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux-475569.svg">
  <img alt="Tests: 182" src="https://img.shields.io/badge/tests-182%20(CI%20green)-22c55e.svg">
  <img alt="Version: 0.6.0" src="https://img.shields.io/badge/version-0.6.0-6366f1.svg">
</p>

> **MetaMach is NOT an AI agent framework. It is a durable AI Software Factory OS —**
> a bare-metal safety harness and execution engine that orchestrates autonomous agents
> inside survivable engineering pipelines with physical hardware access.

---

## Architecture

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
                         │  • Workflow engine (absurd pull-mode)    │
                         │  • DAG engine (topological sort)        │
                         │  • HITL Gateway + Teams Adaptive Cards   │
                         │  • Cold-start resume & checkpoint/recover│
                         └──┬─────────────┬─────────────┬──────────┘
                            │             │             │
                   ┌────────┘             │             └──────────┐
                   ▼                      ▼                        ▼
     ┌─────────────────────┐  ┌───────────────────┐  ┌──────────────────────┐
     │  janus::tmux         │  │  Absurd Postgres  │  │  janus::gateway       │
     │  • tmux -L mm-tmux   │  │  • catalog DB     │  │  • HITL dispatch      │
     │  • remain-on-exit    │  │  • per-blueprint DB│  │  • Hermes /v1/runs     │
     │  • SSH reverse tunnel│  │  • SQLite fallback │  │  • HMAC webhooks      │
     └──────────────────────┘  └───────────────────┘  └──────────────────────┘
                   ▲                                           ▲
                   │ (reattach view)                           │ UDS proxying
     ┌──────────────────────┐                  ┌──────────────────────────────┐
     │  herdr-janus (TUI)    │                  │  janus-studio (Web Observer) │  ← http://127.0.0.1:8443
     │  • Dispatch / Progress│                  │  • Visual DAG Canvas Editor  │     Decoupled Axum sidecar
     └──────────────────────┘                  │  • Real-Time WS Streamer     │     Zero daemon web dep
                                               │  • HITL Gateway Interlocks   │
                                               └──────────────────────────────┘
```

---

## Project Structure

### MetaMach source repo

```
metamach/
├── docs/                        # English specs (5 core fundamental specs + contracts)
│   ├── PRD.md                   #   Product Requirements Document
│   ├── ARCH.md                  #   High-level Architecture
│   ├── ADR.md                   #   31 Architecture Decision Records
│   ├── SPEC.md                  #   Target Specifications, Test Catalog & Deployment
│   ├── PLAN.md                  #   Execution Plan & Milestone History
│   └── contracts/               #   Dependency Contracts (herdr.md, absurd.md)
├── janus/                       # Rust workspace (~2,800 LOC)
│   ├── Cargo.toml
│   ├── herdr-plugin.toml        #   Herdr 0.7.3 plugin manifest
│   ├── src/
│   │   ├── bin/
│   │   │   ├── janus_daemon.rs  #   Control-plane daemon
│   │   │   ├── herdr_janus.rs   #   Herdr shadow TUI client
│   │   │   ├── janush.rs        #   Proxy shell (Tool Guard)
│   │   │   ├── janus_studio.rs  #   MetaMach Studio sidecar (Axum REST/WS, ADR-032)
│   │   │   └── janus.rs         #   CLI: init, start, status, studio, stop, continue, offboard
│   │   ├── studio_assets/       #   Embedded HTML/CSS/JS Canvas UI
│   │   ├── absurd/              #   Postgres adapter + SQLite fallback
│   │   ├── tmux/                #   PTY session engine
│   │   ├── tool_guard/          #   Rule engine + webhook dispatch
│   │   ├── gateway/             #   HITL Gateway (Teams, HMAC)
│   │   ├── cognitive/           #   Cognitive Provider SPI (MCP)
│   │   ├── workflow/            #   Workflow engine + stream filter
│   │   ├── pipeline.rs          #   DAG engine (topological sort, ADR-031)
│   │   ├── lifecycle.rs         #   Onboard / Offboard lifecycle
│   │   ├── coldstart.rs         #   Cold-start self-healing
│   │   ├── recipe.rs            #   Blueprint recipe validation
│   │   ├── agent.rs             #   Agent pool & provisioning
│   │   ├── paths.rs             #   Path resolution (Herdr + standalone)
│   │   └── protocol.rs          #   Shared UDS types (Contracts 3.x/4.x)
│   ├── migrations/              #   SQL (001_catalog, 002_blueprint, ...)
│   └── tests/                   #   Integration & E2E tests (177 total)
├── templates/                   # `janus init` scaffolds from here
│   ├── blueprint.toml           #   Default project recipe
│   ├── agents/                  #   Architect, Builder, Tester roles
│   └── workflows/               #   15 workflow templates (12 linear + 3 DAG)
├── configs/                     # Factory defaults
│   ├── agents.toml              #   Global agent pool
│   ├── offboard.toml            #   Offboard configuration
│   └── global_rules.md          #   Shared rules
├── scripts/
│   └── pre-push                 #   Git hook: fmt + clippy + test + PG E2E
├── .github/workflows/ci.yml     #   CI: PG + tmux + Herdr, all 178 tests
└── Makefile                     #   bootstrap, db-init, health, clean
```

### Per-project (after `janus init`)

```
my-project/
├── .janus/                      # All MetaMach config for this project
│   ├── blueprint.toml           #   [blueprint] name, default_workflow, [remote]
│   ├── agents/                  #   Project-specific agent overrides
│   │   ├── architecture.toml
│   │   ├── builder.toml
│   │   └── tester.toml
│   └── workflows/               #   Workflow definitions (linear + DAG)
│       ├── wf_architect_design.toml
│       ├── wf_builder_implement.toml
│       ├── wf_tester_validate.toml
│       ├── req2spec.toml
│       └── spec2software.toml
│   └── openwiki/                #   RAG knowledge scope (per-blueprint)
│       └── production_report.md #   Generated on offboard
└── src/                         # Your project source
```

---

## Quick Start

### Prerequisites

| Dependency | Version | Check |
|---|---|---|
| Rust | 1.88+ (Edition 2024) | `rustc --version` |
| PostgreSQL | 16+ (host-native, no Docker) | `pg_config --version` |
| tmux | 3.3+ | `tmux -V` |
| Herdr | 0.7.3+ (plugin host) | `herdr --version` |

### Bootstrap

```bash
git clone https://github.com/hughguan/MetaMach.git
cd MetaMach
make bootstrap          # prereq → symlinks → compile → db-init
```

`make bootstrap` auto-provisions:
1. Checks prerequisites (`pg_config`, `tmux`, `cargo`)
2. Compiles 4 binaries in release mode
3. Initializes native Postgres at `~/.metamach/db/`
4. Applies catalog migration (`001_catalog.sql`)

### Lifecycle

Every blueprint follows a consistent lifecycle. Each verb maps to one `janus` command:

```
init ──→ plan ──→ dry-run ──→ start ──→ monitor ──→ offboard
 │                                      │
 └── LLM-assisted ──────────────────────┘
      (janus plan --description "...")
```

| Step | Command | What it does |
|---|---|---|
| **init** | `janus init` | Scaffold `.janus/`, validate blueprint, register with daemon |
| **plan** | edit `.toml` or `janus plan --description "..."` | Define the workflow — linear `[[steps]]` or DAG `[[nodes]]` |
| **dry-run** | `janus start --dry-run` | Preview the execution plan without dispatching — verify steps, agents, and DAG topology |
| **start** | `janus start` | Execute the workflow — every step is checkpointed for crash recovery |
| **monitor** | `janus status` / `stop` / `continue` | Observe progress, pause, resume, or inspect task output |
| **offboard** | `janus offboard --blueprint <name>` | Archive audit trail, commit `production_report.md`, mark inactive |

---

### Initialize a project

```bash
cd my-project
janus init
```

Scaffolds `.janus/` (if new), validates the blueprint and default workflow,
creates a dedicated PG database, and registers the blueprint with the daemon —
all in one step. Idempotent: re-running on an existing project re-validates
without overwriting files.

```bash
janus init --dry-run                  # scaffold + validate, skip daemon registration
```

The scaffold includes:
- `blueprint.toml` — edit the `name` and `default_workflow` fields
- `agents/` — architect, builder, tester role templates
- `workflows/` — 15 workflow templates (12 linear + 3 DAG: `req2spec`, `spec2software`, `adr-process`)

### Plan a workflow

MetaMach uses a **unified Workflow DSL** (ADR-031) — a single file format for both
simple linear steps and complex multi-agent DAGs. All workflows live under
`.janus/workflows/`. Two ways to create one:

**Manual** — edit a `.toml` file directly. Linear mode for sequential steps,
DAG mode for multi-node dependency graphs:

```toml
# .janus/workflows/ci.toml
[workflow]
name = "ci"
description = "Build, test, and lint"

[[steps]]
name = "build"
agent = "builder"
command = "cargo build --release"

[[steps]]
name = "test"
agent = "tester"
command = "cargo test"
```

For complex pipelines, use `[[nodes]]` with `needs` edges for parallel execution.
Nodes can reference external workflow files (`workflow = "..."`) or define inline
`steps = [...]`. See `templates/workflows/req2spec.toml` for a full DAG example.

**LLM-assisted** — generate a workflow from natural language:

```bash
janus plan --blueprint my-project --description "Build, test, and deploy via SSH"
```

This produces a validated workflow `.toml` file (ADR-023). Edit it further, then
dry-run to verify the execution plan.

### Dry-run a workflow

Validate and preview a workflow's execution plan without starting:

```bash
janus start --dry-run                               # preview default workflow
janus start --workflow spec2software --dry-run      # preview DAG execution plan
```

For DAG workflows, this prints the topologically sorted execution levels
and per-node step breakdown — useful for verifying `needs` edges before
committing to execution.

### Start a workflow

Execute a workflow. The daemon auto-detects the shape and routes accordingly:

```bash
janus start                                         # uses .janus/blueprint.toml defaults
janus start --blueprint my-project                  # explicit blueprint, default workflow
janus start --workflow ci                           # override workflow (linear or DAG)
```

Linear mode runs directly via `handle_dispatch → spawn_workflow`. DAG mode
runs via topological sort with level-parallel execution. Every step is
checkpointed in Absurd PG for crash recovery.

```bash
# Inline command: quick one-off step
janus start --inline "cargo test"                   # auto-generates transient workflow
```

### Monitor

```bash
janus status                           # all active blueprints
janus status --blueprint my-project    # one blueprint
janus status --json                    # machine-readable
```

Pause and resume running workflows:

```bash
janus stop --blueprint my-project                      # stop all active tasks for blueprint
janus stop --task-id <uuid>                            # stop specific task
janus continue --blueprint my-project                  # resume stopped/crashed tasks via cold-start
```

> **Herdr TUI:** `herdr plugin link ./janus` then `prefix+j` opens the
> Dispatcher overlay for a terminal progress view.

### Launch MetaMach Studio (Web Observer & Visual Canvas)

MetaMach 0.6.0 includes a zero-dependency web dashboard (ADR-032) running as a standalone `janus-studio` sidecar binary over UDS (`janus.sock`):

```bash
janus studio                           # launch interactive sidecar on http://127.0.0.1:8443
janus studio -d                        # launch detached in background
janus studio --port 9000               # custom HTTP port
```

**Key Features**:
- 🎨 **Visual DAG Canvas**: Interactive drag-and-drop workflow authoring, node dependencies, and execution plan visualization.
- 📡 **Real-Time Web Observer**: Live WebSocket stream (`/runs/:id/stream`) delivering step state transitions (`SNAPSHOT` and diff `DELTA` events).
- 🛡️ **HITL Gateway Approval Center**: Web-based one-click interlock approval (`/api/v1/gates/:task_id/verdict`) for high-risk agent operations.
- 🔐 **Token Security**: Auto-generates `~/.metamach/studio.token` with header validation (`X-Janus-Studio-Token`).
- ⚡ **Zero-Dependency Core**: Decoupled Axum sidecar leaves core `janus-daemon` zero-web-dependency.

### Offboard a blueprint

```bash
janus offboard --blueprint my-project
```

Purges operational data from the per-blueprint database, archives the audit
trail, git-commits `production_report.md`, and marks the blueprint `OFFBOARDED`.

---

## Customization Dimensions

| Dimension | Location | Description |
|---|---|---|
| **Agent Pool** | `configs/agents.toml` + `.janus/agents/` | Permissions, provisioning, quota, fallback chains, pre-flight probes |
| **Workflows** | `templates/workflows/` + `.janus/workflows/` | Linear step sequences or DAG node graphs with `needs` dependencies (unified DSL, ADR-031) |
| **Blueprints** | `.janus/blueprint.toml` | Per-project recipe: name, default workflow, openwiki scope, remote host |

---

## Core Principles

### 🛡️ Safety First
- **Fail-closed**: 30s timeout → BLOCK (never pass-through on uncertainty)
- **Dual 16KB budget**: truncation at both `janush` (stream) and `janus-daemon` (DB insert)
- **Tool Guard**: ALLOW / BLOCK / REWRITE rules per agent role, with hot-reload
- **HITL Gateway**: high-risk ops freeze → Teams Adaptive Card → Approve/Reject → resume/fail

### ⚙️ Uncompromising Stability
- **Daemon-owned state**: TUI is transient; state survives UI crashes and SSH drops
- **Dual-track survival**: primary Absurd PG → SQLite fallback ring on PG outage → auto-replay
- **Cold-start resume**: daemon restart picks up from last `COMPLETED` checkpoint
- **De-containerized**: native PostgreSQL at `~/.metamach/db/`, no Docker

### 🔌 Pure Decoupling
- **tmux isolation**: `tmux -L metamach-tmux`, sessions survive agent disconnection
- **Cognitive SPI**: opt-in MCP plugins (`codebase-memory-mcp`, OpenWiki) — async, advisory only
- **HITL Gateway**: external webhook latency never blocks PTY execution

### 🔄 Universal Reusability
- **Agent-agnostic**: works with any CLI agent (Claude Code, Codex, Pi, Aider, Roo Code)
- **Cross-host**: SSH `-R` reverse tunnels for remote compilation targets (ADR-017)
- **Single-binary bootstrap**: `make bootstrap` → ready to deploy

---

## CI & Testing

- **178 tests**: all pass, 0 ignored
- **CI gates**: `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace` (178 tests)
- **E2E tests**: init → start → multi-step workflow completion, DAG parallel execution, stop/continue, Tool Guard interception
- **Herder contract tests**: manifest parse, version check, E2E smoke (PG + tmux + Herdr)
- **Pre-push hook**: `scripts/pre-push` auto-provisions PG and runs E2E tests

---

## License

MIT — see [LICENSE](LICENSE).
