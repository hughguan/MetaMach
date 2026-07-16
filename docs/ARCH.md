# MetaMach 1.0 — System Architecture

> A silicon-grade industrial production machine powered by Janus Daemon and distributed durable execution sessions.

## 1. Philosophical Pillars

In the era of distributed AI co-development, traditional AI programming or agent scheduling is largely "stateless single-shot invocation." Under long-running, heavy-load, multi-station, cross-physical-host R&D scenarios, systems are highly vulnerable to process crashes from network jitter, API circuit-breakers, or context loss—fragmenting the development flow.

**MetaMach 1.0** completely overturns this fragile topology. It adopts a **"daemon as the brain, shadow plugin as the shell"** architecture of high cohesion and loose coupling, decomposing the system into **Agent Pool** (production factors), **Workflows** (pipeline SOPs), and **Blueprints** (product recipes):

- **Brain-as-a-Daemon (Janus Daemon) — Central Nervous System:** Core control flow and state transitions are entirely owned by the always-running background daemon **`janus-daemon`**, which holds an exclusive database connection pool and event listener gateway. The Herdr-side plugin is merely a lightweight shadow client (`herdr-janus`) dedicated to terminal rendering and interaction.

- **Cross-Host Session Durability (Tether Engine) — Body and Bones Intact:** Combined with **Tether Engine (Tmux/SSH)** to erase network boundaries. Underlying physical process sites are locked down by native `remain-on-exit` tmux sessions—even if the physical network or SSH connection drops, the session never dies. Re-attach and restore the scene at any moment.

- **Durable Workflows & HITL — Resilient Closed Loop:** Workflow state does not degrade with single-execution outcomes. When the AI encounters an insurmountable obstacle (e.g., compile errors, privilege violations), the pipeline auto-suspends at the breakpoint, preserves the terminal scene, and introduces **Human-in-the-Loop** intervention. After the fix, one-click Resume for seamless handoff.

## 2. Feature Specifications

### 2.1 Central Agent Administrator: Janus Daemon

**Janus Daemon** is the cognitive brain of the entire ecosystem, running independently as the system's sole data read/write and scheduling hub:

- **Absurd Transaction Reconciliation:** Before every Step begins, a transition state (e.g., `STARTING` / `STOPPING`) must be declared in **Absurd Postgres**. Upon success, the `result_cache` JSON payload is atomically committed, guaranteeing idempotent self-healing after any disaster restart.

- **Agent Security Sandbox (janus-sh):** Abandoning the fantasy of asynchronous interception within the Herdr process, Janus points the underlying `SHELL` to a custom proxy shell `janus-sh`. Any command an Agent attempts to execute in tmux must pass through a synchronous UDS socket reconciliation with Janus Daemon, undergoing **Event-Driven Tool Guard** in-memory review before safe execution.

### 2.2 Three-Dimensional Customization

#### A. Agent Pool & Stack

All AI resources and security permissions are registered and managed globally by the factory principal:

- **Credential Sandboxing:** All API keys and SSH keys are uniformly mounted and decrypted in `/dev/shm` (RAM disk), never contaminating the code repository.
- **Role Qualification:** Agents for different roles (e.g., `Scout` for code scanning, `Coder` for patch generation, `Deployer` for cross-device flashing) receive custom model selection and Toolset restrictions (Permission Level).

#### B. Workflows

Declarative configuration files define high-cohesion, high-reusability assembly lines:

- **Multi-Station Chaining:** Declare which Agent type each Step executes (e.g., `run_agent(scout)` → `run_agent(coder)` → `run_test`).
- **Cross-Host Deployment:** Support declaring physical machine environments. A pipeline can dispatch an Agent locally on Step 1 to modify code, then in Step 2 automatically tunnel the instruction via Tether over OpenSSH to a remote compilation server for heavy builds.

#### C. Blueprints

Product lines reside under `blueprints/`, maintaining absolute physical cleanliness:

- **Custom Recipe (janus.toml):** Binds a default workflow, declares OpenWiki federated knowledge graph index scope, and configures remote SSH target IP.
- **On/Offboarding:**

    - _Onboard:_ The Factory Director executes `janus onboard --blueprint <name>`. The Daemon takes over with the standard onboarding process:
        1. **Recipe Validation:** Read and validate `blueprints/<name>/janus.toml`, confirm `workflows/<default_workflow>.toml` exists;
        2. **Pre-Ignition Checks:** Probe Absurd Postgres reachability, tmux readiness; for cross-host blueprints, best-effort probe remote SSH target (unreachable = warn only);
        3. **Tenant Registration:** Using `blueprint_id` as the partition key, `INSERT … ON CONFLICT DO UPDATE` one row of `ACTIVE` blueprint metadata (**idempotent**; can reactivate previously `OFFBOARDED` blueprints);
        4. **Workflow Binding:** Persist the default SOP workflow binding;
        5. **Knowledge Graph Loading & Experience Inheritance:** Index `blueprints/<name>/openwiki/`; if a prior `production_report.md` exists, prioritize indexing it and inject key avoidance patterns as `## Previous Incidents` few-shot examples into the next-generation Agent's System Prompt;
        6. **Onboarding Ready:** Status set to `ACTIVE`; the product line immediately appears in the Popup dispatch menu.

    - _Offboard:_ Extract all Step execution traces and Tool Guard interception records from the project's development period. Auto-smelt them via the configured LLM into a latest **"Quality Inspection Report & Production Whitepaper (production_report.md)"** in the local OpenWiki, while simultaneously wiping expired JSON cache large fields from the database for lossless anti-bloat compaction. This whitepaper is recycled as immune antibodies on the next Onboard, closing the "Onboard → Produce → Offboard → Re-Onboard" evolutionary loop.

## 3. System Architecture Topology

MetaMach 1.0 implements an industrial-grade isolation scheme of "independent brain monitoring, shadow client passthrough, physical session attachment, data logical multi-tenancy":

- **Control Plane:**
    - **`janus-daemon` (resident process):** Responsible for core logic scheduling, maintaining a persistent connection to Absurd Postgres, listening for external Teams/TG async messages. Also exposes the `progress` query primitive: aggregating real-time status from `absurd_tasks` JOIN `absurd_steps` plus Tether physical session liveness signals, serving as the sole authoritative data source for the workflow progress dashboard.
    - **`herdr-janus` (shadow plugin):** Passive execution. Declared in `herdr-plugin.toml` as a `[[panes]]` entrypoint with `placement = "overlay"` (validated Herdr 0.7.3 directive; see `docs/herdr-v1-contract.md`), dedicated to launching session-modal interaction popups, sending commands to the Daemon via UDS socket. The Popup has two built-in views: **Dispatch** and **Progress**, switchable by the Factory Director with one key. The Progress view polls the Daemon's `progress` primitive at a fixed cadence (1–2s) to render the workflow progress dashboard.

- **Physical Execution Plane:**
    - **`herdr-tether` (physical engine):** A standalone CLI binary, leveraging tmux's `remain-on-exit` feature to manage cross-host physical Sessions (format: `tether-janus-task-<uuid>`).

- **Persistence Plane:**
    - **Absurd Postgres (Absurd DB):** Single-database, multi-tenant design. Uses `blueprint_id` as the physical partition key, centrally managing Step states.
    - **OpenWiki (shared RAG skill):** Packaged as a standard Agent Skill. When an Agent encounters a code blind spot, it initiates a precise RAG retrieval via the `openwiki_query` tool; Janus intercepts and preferentially looks up in a Postgres-level cache (Git-SHA deduplication), returning precise AST code snippets with zero latency.

> **CLI & Binary Architecture (unified entrypoint + dedicated binaries):** The system uses a unified `janus` CLI as the single entrypoint for the Factory Director and management surface, with subcommands in two categories: (1) **lifecycle/query subcommands**—`janus onboard`, `janus offboard`, `janus status`—all lightweight clients communicating with the resident `janus-daemon` via UDS (**Daemon must be running**; `janus daemon` explicitly launches it); (2) underlying dedicated binaries—`janus-daemon` (resident brain), `herdr-janus` (shadow client, loaded by Herdr), `janus-sh` (proxy shell, injected as `SHELL` by Tether), `herdr-tether` (physical execution engine, external dependency). Thus `janus offboard` is equivalent to "client sends offboard command to Daemon via UDS," not a standalone direct DB connection. All `herdr-tether` invocations use the `herdr-tether <subcommand>` form (e.g., `herdr-tether open`, `herdr-tether attach`); bare `tether <subcommand>` is no longer used.

## 4. Component Interactivity

Taking `gatemetric` (BMX attitude evaluation system) deployed across local and remote compilation servers via the **standard dev-flow** assembly line as an example, demonstrating the precision interlock between components.

### Execution Sequence Flow

```
sequenceDiagram
    autonumber
    actor Human as Factory Director
    participant Client as herdr-janus
    participant Daemon as janus-daemon
    participant Absurd as Absurd Postgres
    participant Guard as Tool Guard (janus-sh)
    participant Tether as herdr-tether
    participant OW as OpenWiki
    participant Teams as MS Teams / TUI

    Human->>Client: Press prefix+j to wake Dispatcher popup
    Client->>Daemon: Request ACTIVE blueprints via UDS
    Daemon-->>Client: Return gatemetric attributes & available workflows
    Human->>Client: Select gatemetric & dispatch dev-flow
    Client->>Daemon: Send command, close Popup

    Note over Daemon, Absurd: [Phase 1: Scouting & Memory Load]
    Daemon->>Absurd: TX check: SELECT status FROM blueprints WHERE id = 'gatemetric'
    Absurd-->>Daemon: Return ACTIVE status
    Daemon->>OW: Wake OpenWiki RAG: load prior production_report.md avoidance patterns
    OW-->>Daemon: Inject Coder Agent System Prompt (avoid I2C pin conflicts)

    Note over Daemon, Tether: [Phase 2: Local Coder Station]
    Daemon->>Tether: Create local tmux session: "tether-janus-step1-uuid"
    Tether->>Tether: Force remain-on-exit
    Daemon->>Tether: Run Coder Agent, inject filter algorithm patch
    Tether-->>Daemon: Write complete, output Git Diff

    Note over Daemon, Tether: [Phase 3: Cross-Host Compilation (Remote SSH)]
    Daemon->>Absurd: Write Step 1 Checkpoint: result_cache = Git Diff JSON (hash dedup)
    Daemon->>Tether: Wake remote compile server session via OpenSSH BatchMode
    Daemon->>Tether: Read Diff JSON from Absurd, inject into remote shell, run "make cross-compile"
    Tether->>Tether: Remote cross-compile fails, pin config missing (Exit Code != 0)
    Tether-->>Daemon: Capture error scene (auto-capped at 16KB budget)

    Note over Guard, Teams: [Phase 4: HITL Safety Fuse & Manual Takeover]
    Daemon->>Absurd: Lock state to SUSPENDED (non-destructive, remote tmux session never killed)
    Daemon->>Teams: Send alert card: [Compile failed! Pin 21 conflict. Click Resume or TUI debug]
    Human->>Tether: Via Herdr TUI, attach into the still-alive remote tmux scene, manually fix C++ header
    Human->>Teams: Click [Resume Workflow] on mobile

    Note over Daemon, Absurd: [Phase 5: Offboard Smelting & Experience Evolution]
    Teams-->>Daemon: Receive resume command
    Daemon->>Tether: Drive Tether to re-run "make cross-compile" remotely
    Tether-->>Daemon: Compile passes, QA success!
    Daemon->>Daemon: janus offboard --blueprint gatemetric
    Daemon->>Absurd: Execute melt_blueprint_data(), wipe large JSONs, DB footprint instantly shrinks
    Daemon->>OW: Write pin conflict fix into blueprints/gatemetric/openwiki/production_report.md
    Daemon-->>Human: Evolution archived. Next Agent onboards with this immune antibody!
```

### Progress Query Sequence

Progress dashboard interaction is extremely lightweight, decoupled from the heavy assembly sequence above, triggered by the Factory Director within the Popup:

```
sequenceDiagram
    autonumber
    actor Human as Factory Director
    participant Client as herdr-janus
    participant Daemon as janus-daemon
    participant Absurd as Absurd Postgres

    Human->>Client: prefix+j wake Popup, switch to "Progress" view
    loop Every 1–2s polling
        Client->>Daemon: UDS sends progress query
        Daemon->>Absurd: SELECT absurd_tasks JOIN absurd_steps WHERE status IN (non-terminal)
        Absurd-->>Daemon: Return in-flight tasks & their step states
        Daemon-->>Client: Return progress payload (step status / current step / elapsed / stdout summary)
    end
    Client-->>Human: Render progress dashboard; SUSPENDED steps highlighted with Resume entry
```

> This query is a **read-only bypass**: it does not occupy the workflow execution transaction channel, does not interfere with running steps, and only reads authoritative state from Absurd Postgres. In non-TUI environments, the `janus status` CLI uses the same `progress` primitive, outputting plain-text/JSON snapshots.

## 5. GitHub Monorepo Directory Structure

To fully comply with Herdr 0.7.3's **"Immutable ROOT vs. Mutable State"** physical isolation boundary, the entire `metamach` repository uses the following organizational topology:

```
metamach/ (Single monorepo — silicon factory headquarters)
├── .github/
│   └── workflows/
│       └── build-janus.yml       # CI: cross-platform janus binary compilation
│
├── Makefile                      # Factory master switch
├── docker-compose.yml            # One-click Absurd Postgres container
├── README.md                     # Factory operations manual & safety whitepaper
├── .gitignore                    # Strictly filter local temp sandboxes, PG data dirs, local state
│
│   # ====================================================================
│   # 1. JANUS CORE (supreme control brain & shadow client)
│   # ====================================================================
├── janus/
│   ├── Cargo.toml                # Rust workspace config
│   ├── herdr-plugin.toml         # Herdr 0.7.3 plugin contract (popup declaration & event hooks)
│   ├── migrations/               # Postgres init migration scripts
│   │   └── 001_init_absurd.sql
│   └── src/
│       ├── bin/
│       │   ├── janus_daemon.rs   # Resident background daemon
│       │   ├── herdr_janus.rs    # Ultra-lightweight Herdr shadow client
│       │   └── janus_sh.rs       # Proxy shell
│       │
│       ├── tool_guard/           # janus-sh in-memory interception & allowlist filtering
│       ├── absurd/               # Exclusive sqlx Postgres connection pool, reconciliation & GC
│       └── tui/                  # 80%-width Popup keyboard UI (Ratatui)
│
│   # ====================================================================
│   # 2. CONFIG & EXTERNAL DEPENDENCIES
│   # ====================================================================
├── configs/                      # Global static config
│   ├── agents.toml               # Agent Pool registration & permission allowlist
│   ├── tmux.conf                 # Tether tmux init config (remain-on-exit)
│   └── global_rules.md           # Factory-wide developer rules (Agent onboarding safety lines)
│
├── openwiki/                     # External: langchain-ai/openwiki — RAG knowledge federation engine
│   └── configs/                  # OpenWiki engine config (binary built from external repo)
│
├── workflows/                    # Unified pipeline SOPs
│   ├── dev-flow.toml             # Standard R&D pipeline
│   ├── debug-flow.toml           # Diagnostic & debugging pipeline
│   └── firmware-deploy.toml      # Physical cross-compile & flash pipeline
│
│   # ====================================================================
│   # 3. BLUEPRINTS (product lines / target development projects)
│   # ====================================================================
├── blueprints/
│   │
│   ├── joyrobots/                # JoyRobots (modular education robot platform)
│   │   ├── janus.toml            # Custom recipe (bound to dev-flow)
│   │   ├── src/                  # Pure project source
│   │   └── openwiki/             # Local knowledge graph (Spike Prime API)
│   │
│   └── gatemetric/               # GateMetric (BMX attitude evaluation system)
│       ├── janus.toml            # Custom recipe (bound to firmware-deploy, SSH compile target)
│       ├── firmware/             # ESP32 filter C++/Arduino source
│       ├── 3d-enclosure/         # Bambu Lab X1C sensor enclosure CAD/STL
│       └── openwiki/             # Local knowledge graph (MPU6050 timing & production_report immunity)
│
│   # ====================================================================
│   # 4. PROVISIONING (maintenance & sandbox mounting)
│   # ====================================================================
└── provisioning/
    ├── bootstrap.sh              # Zero-dependency one-click deploy (symlinks config dirs, starts PG)
    └── init-user-db.sh           # Postgres role, permission & metamach_db init script

# ═══════════════════════════════════════════════════════════════════════
# EXTERNAL DEPENDENCIES (separate repos, fetched/built by make bootstrap)
# ═══════════════════════════════════════════════════════════════════════
# herdr-tether → https://github.com/moneycaringcoder/herdr-tether
#    Physical execution engine: tmux session management, remain-on-exit, cross-host SSH
# absurd       → https://github.com/earendil-works/absurd
#    Absurd Postgres engine: transaction reconciliation, connection pool, melt_blueprint_data
# openwiki     → https://github.com/langchain-ai/openwiki
#    Federated knowledge RAG engine: shared skill retrieval, production_report indexing
```

> **External Dependencies & Mutable Configuration:**
> - `herdr-tether`, `absurd`, and `openwiki` are independent external repositories, not compiled within this monorepo. `make bootstrap` handles fetching/building/linking these dependencies.
> - Runtime mutable configuration (e.g., `agents.toml`) must be symlinked into **`${HERDR_PLUGIN_CONFIG_DIR}`** (i.e., `~/.config/herdr/plugins/config/metamach.janus`). All transaction logs, cached SQLite, and temporary socket files must reside under **`${HERDR_PLUGIN_STATE_DIR}`** (i.e., `~/.local/state/herdr/plugins/metamach.janus`). This ensures that updating plugin source via GitHub **never accidentally wipes any local financial or development state data**.

## 6. Resilience Invariants

1. **Physical Non-Destruction — tmux Remain-on-Exit:** Every Tether-managed physical Session is injected with `remain-on-exit on`. When an AI process exits due to a segfault or syntax error, the terminal scene is 100% preserved. The physical process is never killed, preventing development context from vanishing into thin air.

2. **Capacity Anti-Bloat — 16KB Budget & SQL Pruning:** Absurd Postgres never expands without bounds. All Step Checkpoint large JSON caches and terminal stdout captures exceeding 16KB are forcibly truncated. A daily `Janus GC` transaction auto-cleans all Blueprint cache fields for tasks completed more than 3 days ago.

3. **Zero-Privilege-Escalation — Physical janus-sh Interception:** No reliance on AI self-restraint. All high-risk commands must pass through `janus-sh` for synchronous reconciliation with Janus Daemon over a UDS pipe before reaching the underlying Bash. Without Teams/TUI approval detected, commands are forcibly rewritten in memory or rejected entirely.

4. **Stateless Cold Start — Absolute Rejection of tmux-resurrect:** The sole source of truth for state is Postgres. After a server room restart, the system directly reads the last Completed Step's JSON cache from the database, assigns a brand-new Tether Session UUID, and instantly picks up seamlessly at the physical breakpoint.
