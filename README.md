# 🛡️ MetaMach

<p align="center">
  <a href="https://github.com/hughguan/MetaMach/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/hughguan/MetaMach/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-0f766e.svg"></a>
  <a href="https://github.com/ogulcancelik/herdr"><img alt="Herdr 0.7.3+" src="https://img.shields.io/badge/Herdr-0.7.3%2B-172033.svg"></a>
  <img alt="macOS & Linux" src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux-475569.svg">
  <img alt="Tests: 171" src="https://img.shields.io/badge/tests-171%20(CI%20green)-22c55e.svg">
  <img alt="Version: 0.5.0" src="https://img.shields.io/badge/version-0.5.0-6366f1.svg">
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
                         │  • Pipeline DAG (topological sort)       │
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
                   ▲
                   │ (reattach view)
     ┌──────────────────────┐
     │  herdr-janus (TUI)    │  ← Herdr overlay pane (prefix+j)
     │  • Dispatch / Progress│     Shadow client — zero state, zero logic
     └──────────────────────┘
```

---

## Project Structure

### MetaMach source repo

```
metamach/
├── docs/                        # English specs (source of truth)
│   ├── ARCH.md                  #   Architecture (0.5.0 converged)
│   ├── ADR.md                   #   29 Architecture Decision Records
│   ├── PRD.md, Feature-Spec.md  #   Product & feature specs
│   ├── Test-Spec.md             #   Test specifications
│   └── Deployment-Spec.md       #   Deployment guide
├── janus/                       # Rust workspace (~2,800 LOC)
│   ├── Cargo.toml
│   ├── herdr-plugin.toml        #   Herdr 0.7.3 plugin manifest
│   ├── src/
│   │   ├── bin/
│   │   │   ├── janus_daemon.rs  #   Control-plane daemon
│   │   │   ├── herdr_janus.rs   #   Herdr shadow TUI client
│   │   │   ├── janush.rs        #   Proxy shell (Tool Guard)
│   │   │   └── janus.rs         #   CLI: init, onboard, dispatch, pipeline
│   │   ├── absurd/              #   Postgres adapter + SQLite fallback
│   │   ├── tmux/                #   PTY session engine
│   │   ├── tool_guard/          #   Rule engine + webhook dispatch
│   │   ├── gateway/             #   HITL Gateway (Teams, HMAC)
│   │   ├── cognitive/           #   Cognitive Provider SPI (MCP)
│   │   ├── workflow/            #   Workflow engine + stream filter
│   │   ├── pipeline.rs          #   Pipeline DAG engine (topological sort)
│   │   ├── lifecycle.rs         #   Onboard / Offboard lifecycle
│   │   ├── coldstart.rs         #   Cold-start self-healing
│   │   ├── recipe.rs            #   Blueprint recipe validation
│   │   ├── agent.rs             #   Agent pool & provisioning
│   │   ├── paths.rs             #   Path resolution (Herdr + standalone)
│   │   └── protocol.rs          #   Shared UDS types (Contracts 3.x/4.x)
│   ├── migrations/              #   SQL (001_catalog, 002_blueprint, ...)
│   └── tests/                   #   Integration & E2E tests (171 total)
├── templates/                   # `janus init` scaffolds from here
│   ├── blueprint.toml           #   Default project recipe
│   ├── agents/                  #   Architect, Builder, Tester roles
│   ├── workflows/               #   12 workflow templates
│   └── pipelines/               #   req2spec, spec2software, adr-process
├── configs/                     # Factory defaults
│   ├── agents.toml              #   Global agent pool
│   ├── offboard.toml            #   Offboard configuration
│   └── global_rules.md          #   Shared rules
├── scripts/
│   └── pre-push                 #   Git hook: fmt + clippy + test + PG E2E
├── .github/workflows/ci.yml     #   CI: PG + tmux + Herdr, all 171 tests
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
│   ├── workflows/               #   Workflow definitions
│   │   ├── wf_architect_design.toml
│   │   ├── wf_builder_implement.toml
│   │   └── wf_tester_validate.toml
│   ├── pipelines/               #   DAG pipeline definitions
│   │   ├── req2spec.toml
│   │   └── spec2software.toml
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

### Bootstrap MetaMach

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

### Start a project

```bash
cd my-project
janus init
```

This scaffolds `.janus/` with:
- `blueprint.toml` — edit the `name` and `default_workflow` fields
- `agents/` — architect, builder, tester role templates
- `workflows/` — 12 workflow templates
- `pipelines/` — 3 pipeline DAG templates (`req2spec`, `spec2software`, `adr-process`)

### Onboard a blueprint

```bash
janus onboard --blueprint my-project
```

Validates `.janus/blueprint.toml` (name, workflow, openwiki scope), creates a
dedicated `metamach_blueprint_my_project` database with the absurd schema, and
registers the blueprint in the catalog.

### Dispatch a workflow

```bash
janus dispatch --blueprint my-project                 # uses default_workflow
janus dispatch --blueprint my-project --workflow ci   # override workflow
```

The daemon spawns a detached background task that:
1. Claims the absurd task (pull-mode lease)
2. For each step: creates a tmux session → runs `janush -c "<command>"` (Tool Guard gated)
3. Captures stdout/stderr, records exit code
4. Checkpoints after each step (cold-start resume on daemon restart)
5. Reaches `COMPLETED` or `FAILED`

### Check progress

```bash
janus status                           # all active blueprints
janus status --blueprint my-project    # one blueprint
janus status --json                    # machine-readable
```

### Pipeline DAGs (ADR-021)

```bash
# Validate a pipeline definition
janus pipeline validate .janus/pipelines/spec2software.toml

# Generate a pipeline from natural language (ADR-023, LLM-assisted)
janus pipeline plan --blueprint my-project \
  "Architect designs, Builder implements, Tester validates, loop until green"
```

Pipelines define DAGs with `[nodes]` and dependency `needs` edges. The engine
topologically sorts nodes and executes independent branches in parallel.

### Offboard a blueprint

```bash
janus offboard --blueprint my-project
```

Purges operational data from the per-blueprint database, archives the audit
trail, git-commits `production_report.md`, and marks the blueprint `OFFBOARDED`.

### Herdr integration

```bash
herdr plugin link ./janus           # register MetaMach plugin
# prefix+j opens the Dispatcher TUI overlay
```

---

## Customization Dimensions

| Dimension | Location | Description |
|---|---|---|
| **Agent Pool** | `configs/agents.toml` + `.janus/agents/` | Permissions, provisioning, quota, fallback chains, pre-flight probes |
| **Workflows** | `templates/workflows/` + `.janus/workflows/` | Linear step sequences: agent + command per step |
| **Pipelines** | `templates/pipelines/` + `.janus/pipelines/` | DAG composition: nodes with `needs` dependencies, parallel execution |
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

- **171 tests**: 168 default + 3 Herdr-gated (run on `--ignored` in CI)
- **CI gates**: `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace`
- **E2E pipeline tests**: onboard → dispatch → multi-step workflow completion, Tool Guard interception
- **Herder contract tests**: manifest parse, version check, E2E smoke (PG + tmux + Herdr)
- **Pre-push hook**: `scripts/pre-push` auto-provisions PG and runs E2E tests

---

## License

MIT — see [LICENSE](LICENSE).
