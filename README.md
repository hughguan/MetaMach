# 🛡️ MetaMach 0.1.0

> **An industrial-grade, durable AI Software Factory OS powered by Janus Daemon and distributed physical execution sessions.**

MetaMach 0.1.0 orchestrates specialized AI agents (Claude Code, Codex, Pi) as isolated, ephemeral function nodes inside robust, survivable engineering pipelines—managed straight from your pocket via Telegram or TUI.

---

## 🪐 Core Pillars

- **🧠 Brain-as-a-Daemon (Janus Daemon)**  
  The control plane is a standalone Rust daemon (`janus-daemon`) that owns the entire state machine, database connection pool, and event gateway. The Herdr plugin runs as a lightweight shadow client (`herdr-janus`) responsible only for rendering and interaction—UI crashes never lose engineering context.

- **🔌 Cross-Host Session Durability (Tether Engine — In Migration)**
  All physical agent execution sessions are backed by `remain-on-exit` tmux sessions. The engine is migrating from the external `herdr-tether` v0.3.0 plugin into a native `janus::tether` Rust module (~3,500 LOC port, ~2,600 LOC tests). Once internalized, tmux sessions survive frontend popup destruction (SIGHUP immunity) and operate at microsecond-level IPC latency. See `spike/herdr-tether-migration-evaluation.md` for the full evaluation.

- **🛡️ Durable Workflows & HITL (Self-Healing)**
  Workflow state is transactional and atomic. When an AI hits a blocking failure (compile error, permission denied), the pipeline auto-suspends, preserves the terminal live, and signals a **Human-in-the-Loop** approval via Telegram/Teams. Resume at the exact breakpoint.

- **🧱 De-containerized (No Docker)**
  Postgres runs as a host-native Sandbox process, not a Docker container. `make bootstrap` compiles, symlinks, and launches PG directly — no Docker, no Compose, no container network overhead. Physical persistence at `~/.metamach/db/` survives power-cycle restarts.

- **📊 Dual-Track Data Survival**
  Normal writes go through `janus-sh → UDS → janus-daemon → Absurd PG`. If PG goes down, the SQLite ring buffer (`fallback.db`) keeps the workshop alive. On PG recovery, the ring buffer replays into PG automatically.

---

## 📐 Three-Dimensional Customization

| Dimension | Description |
|-----------|-------------|
| **Agent Pool & Stack** | Global registration of AI resources—API keys and SSH credentials decrypted in `/dev/shm` (RAM disk, never leaked to disk). Fine-grained role-based permission levels (Scout / Coder / Deployer). |
| **Workflows** | Declarative `.toml` pipelines (`workflows/*.toml`). Chain multiple agent stations across local and remote SSH hosts. |
| **Blueprints** | Physical project containers under `blueprints/<name>/`. Each binds a custom `janus.toml` recipe, a multi-tenant database partition, OpenWiki knowledge scope, and optional remote compilation targets. On offboarding, auto-compacts the database and generates a `production_report.md`. |

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
└── Makefile                  # Factory master switch (no Docker)
```

---

## 📚 Documentation

Full design specifications live directly under `docs/` (English) - this is the **sole version-controlled spec source**. Chinese translations and the `*-Review.md` audit deep-dives live under `docs/CH/`, which is gitignored (local-only, non-authoritative); when the two disagree, `docs/` wins.

| Doc | Scope |
|-----|-------|
| `ARCH.md` | System architecture, topology, component interactivity |
| `ARCH-0.2.0.md` | Database layer evolution exploration (0.1.0 → 0.2.0 proposals) |
| `ARCH-0.3.0.md` | **Architecture consensus baseline** — final arbitration of all proposals |
| `PRD.md` | Product requirements & Factory Director user journey |
| `Feature-Spec.md` | Engineering feature specs, data contracts (Contracts 3.1–3.8), fault matrix |
| `Project-Plan.md` | Milestone roadmap (M0–M4) & check-in units |
| `Review-Spec.md` | Audit/review standards & sign-off criteria |
| `Test-Spec.md` | Test cases (UTC-01..07) & QA strategy |
| `Deployment-Spec.md` | Physical deployment, bootstrap, directory mapping |
| `ARCH-review.md` | Cross-document architectural audit & action items |

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
2. Compiles `janus-daemon`, `herdr-janus`, and `janus-sh` in release mode
3. Launches host-native Postgres and runs all migrations

After bootstrap, press `prefix+j` inside Herdr to open the Dispatcher console and dispatch a workflow.

### Shutdown
```bash
make db-down   # Gracefully stop the Postgres instance
make clean     # Clean build artifacts and unmount RAM disk
```

---

## 🏛️ Architecture 0.3.0 Consensus

The 0.3.0 baseline (`docs/ARCH-0.3.0.md`) is the final arbitration of two rounds of design exploration. Key decisions:

| Proposal | Verdict | Rationale |
|---|---|---|
| De-containerization (No Docker) | ✅ Adopted | Eliminate virtual NIC/container overhead; zero-dependency distribution |
| `~/.metamach/db/` independent path | ✅ Adopted | Decouple from Herdr plugin lifecycle; survive power-cycle restart |
| Retain SQLite Fallback ring buffer | ✅ Force-Retained | Workshop must "run degraded" during PG outage |
| `DROP DATABASE` physical shredding | ❌ Rejected | Destroys audit trail; DELETE + `absurd_audit_log` archiving instead |
| One PG, Multi-DB topology | ✅ Adopted | Independent connection pools, zero cross-blueprint lock contention |
| Tether Internalization | ✅ Adopted | 4 physical requirements: survival autonomy, durability, IPC latency, single-binary |
| Fail-Closed 30s Timeout | ✅ Force-Retained | SIGSTOP/SIGCONT rejected; existing Feature-Spec 2.2 design |

See `docs/ARCH-0.3.0.md` for full details and `spike/herdr-tether-migration-evaluation.md` for the tether internalization plan.

## 🧪 Current Test Coverage

**39/39 tests passing** (36 lib + 3 bin) — all CI gates green (`cargo fmt`, `clippy -D warnings`, `cargo test --workspace`).

| Priority | Coverage | Status |
|---|---|---|
| 🔴 Blocker (4/4) | Cold-start, 30s timeout, UDS proxy, melt blueprint | ✅ All pass |
| 🟠 Critical (12/13) | Absurd CRUD, lifecycle, Tool Guard rules, webhook | ✅ 12 pass, 1 partial (UDS fuzz) |
| 🟡 Major (12/15) | Recipe, workflow, multi-blueprint, Telegram | ✅ 12 pass, 3 partial/non-impl |

**Known gaps** (tracked in `docs/Test-Spec.md`):
- `UTC-02-04`: UDS fuzz/rate-limit tests not yet written
- `UTC-04-02`: Telegram callback polling not wired
- `UTC-04-03`: Teams adapter not implemented (LoggingSender fallback only)
- `UTC-03-02/05`: Cross-host & concurrent workflow — blocked until tether internalization
- `UTC-07-xx`: No benchmark harness exists

## 🚀 CI/CD Status

- **No CI workflow yet** — `.github/workflows/` does not exist. This is P0.
- The 0.3.0 architecture directly affects CI design: CI runner needs host-native PG (not Docker), tmux 3.3+, and `thiserror` dependency.
- See the ARCH 0.3.0 review output for a complete CI workflow specification.

## 🛡️ Resilience Invariants

- **Remain-on-Exit**: Every Tether-powered session is 100% crash-proof. AI process segfaults? Syntax errors? The tmux terminal stays alive, preserving full context.
- **16KB Budget**: Step checkpoints and stdout capture are strictly capped at 16KB. Database `Janus GC` prunes expired entries every 24 hours—no unbounded bloat.
- **janus-sh Proxy Shell**: Agent commands are intercepted via a UDS sync protocol before reaching bash. High-risk operations are physically suspended until HITL approval via Teams/Telegram.
- **Stateless Cold Start**: Postgres is the sole source of truth. After a full power loss, `janus-daemon` reconnects, identifies the last completed checkpoint, and resumes execution at the breakpoint—no `tmux-resurrect` reliance.
- **SQLite Fallback**: If PG crashes, the SQLite ring buffer (`fallback.db`) keeps the workshop alive. On PG recovery, events are replayed into PG and the ring buffer is truncated.
- **Global Audit Archive**: Offboarded blueprint data is not dropped (`DROP DATABASE` rejected); instead, `result_cache` is DELETEd, and step execution traces are archived in `absurd_audit_log` for legal traceability.