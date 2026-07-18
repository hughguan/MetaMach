# 🪐 MetaMach 0.3.0 — Architecture Consensus Baseline

> **0.3.0 Positioning:** This document is the **final arbitration and sign-off** of two rounds of design exploration — the 0.2.0 database layer evolution and the control-plane evolution proposals. All adopted, rejected, and deferred proposals are explicitly locked down here as the sole authoritative architecture baseline for subsequent development.

---

## 📋 Decision Overview

| Proposal Source | Proposal Content | Verdict | Rationale |
|---|---|---|---|
| 0.2.0 | De-containerization (No Docker) | ✅ Adopted | Eliminate virtual NIC/container overhead; achieve zero-dependency distribution after `make bootstrap` |
| 0.2.0 | `~/.metamach/db/` global independent path | ✅ Adopted | Decouple from Herdr plugin lifecycle; PG data fully preserved after power-cycle restart |
| 0.2.0 | Retain SQLite Fallback ring buffer | ✅ Force-Retained | Workshop must "run degraded" during PG outage — cannot halt |
| 0.2.0 | `DROP DATABASE` physical shredding | ❌ Rejected | Destroys audit trail; historical interception/sign-off logs must be permanently archived post-Offboard |
| 0.2.0 | One PG, Multi-DB topology | ✅ Adopted | Independent connection pools, zero cross-blueprint lock contention, resource isolation; performance baseline |
| 0.3.0 | Fail-Closed 30s Timeout interception | ✅ Force-Retained | Existing Feature-Spec 2.2 design; SIGSTOP/SIGCONT alternative rejected |
| 0.3.0 | Isolated tmux Server (`-L metamach-tmux`) | ✅ Already Implemented | In ARCH.md §6 |
| 0.3.0 | Dual-Binary deployment | ✅ Already Implemented | `janus-daemon` + `herdr-janus` is the current architecture |
| 0.3.0 | 16KB Flow Budget dual defense line | ✅ Already Implemented | Feature-Spec §4; janush streaming truncation + Daemon authoritative pre-insert truncation |
| 0.3.0 | tmux Internalization | ✅ Adopted | Four physical requirements: survival autonomy, keep-alive resilience, microsecond signal linkage, zero-dependency distribution; ~3,500 LOC migration |

---

## 🛠️ I. Adopted: 0.3.0 Physical Architecture Baseline

### 1.1 De-containerization: Host-Native Sandbox PG

**Abolish** `docker-compose.yml`, **abolish** Docker dependency.

On first startup, `janus-daemon` directly launches the sole local Postgres physical instance on the host, bound to a fixed local port. `make bootstrap` only needs binary compilation and path symlinks, achieving "copy over, plug in, go" minimal distribution.

**Physical persistence path:** `~/.metamach/db/` — independent from the Herdr plugin state area; PG data fully preserved after power-cycle restart. Absurd Checkpoints are never lost; the Durable commitment never bankrupts.

### 1.2 Dual-Track Data Survival: PG Primary + SQLite Degraded Ring Buffer

**Retain Contract 3.8's SQLite Fallback design.**

```
Normal:    janush → UDS → janus-daemon → Absurd PG (primary read/write)
Degraded:  janush → UDS → janus-daemon → fallback.db (SQLite ring buffer)
Recovery:  PG restored → Log Replay → fallback.db events merged into PG → ring buffer truncated
```

> **Design Red Line:** SQLite is not meant to replace PG's state machine capabilities — it exists so the workshop **stays alive** during PG outages. The PG container may crash due to memory OOM, disk exhaustion, or connection pool depletion; without SQLite fallback, the interception proxy `janush` would completely deadlock the current Shell, paralyzing the physical workshop on the spot.

### 1.3 Compliance Audit: DELETE + Global Audit Archive

**Reject DROP DATABASE.**

On Offboard, execute:
1. LLM smelting → `production_report.md` written to knowledge graph
2. `DELETE` clears `result_cache` JSON large fields (reclaims TOAST physical space)
3. Step execution traces, Tool Guard interception logs, and three-party sign-off records are **incrementally archived** to the global `absurd_audit_log` table

> **Design Red Line:** Even after a blueprint is offboarded, its weeks-long history of "when was an interception triggered" and "who signed off at what time" must be permanently archived in the global audit table for legal traceability.

### 1.4 Multi-Blueprint Concurrency Isolation: One PG, Multi-DB Topology

The host runs a single physical Postgres process (exclusive fixed port). Each Blueprint receives an independent logical database via `CREATE DATABASE metamach_blueprint_<name>` on Onboard.

- **Independent connection pools:** Each blueprint has its own PG connection handle, avoiding cross-blueprint lock contention and connection pool fragmentation
- **Resource isolation:** A single blueprint's OOM or long transaction does not roll back other blueprints' production state
- **Offboard cleanup:** Space reclaimed via `DELETE` + `absurd_audit_log` archiving (not DROP DATABASE)

> The current `blueprint_id` partition scheme serves as a transitional implementation; Multi-DB topology is the performance baseline for multi-blueprint concurrent scenarios.

## 🛡️ II. Force-Retained: 0.3.0 Safety Control Red Lines

### 2.1 Fail-Closed Synchronous Interception

**Reject SIGSTOP/SIGCONT; retain Feature-Spec 2.2's Fail-Closed 30s Timeout.**

- `janush` synchronously suspends, initiates UDS reconciliation with the Daemon
- Default 30s timeout threshold
- Timeout or Daemon unreachable → **Fail-Closed**: return error to Agent and refuse execution, absolutely never let through
- PTY session survives via `remain-on-exit`; Factory Director can `attach` at any time for troubleshooting

### 2.2 Isolated tmux Sandbox

`tmux -L metamach-tmux` independent server, never polluting the Factory Director's host-global tmux sessions. Every Session enforces `remain-on-exit on`.

### 2.3 16KB Flow Budget Dual Defense Line

- **First defense line (janush):** In-memory streaming counter, early truncation at 16KB (optimizes UDS transfer)
- **Second defense line (Daemon pre-insert):** Authoritative 16KB hard truncation + inject `[MetaMach Log Budget Exceeded]` tag
- Both lines target the same cap; the DB write is the final gate

### 2.4 tmux Internalization: MM-CORE Physical Execution Engine

**Adopts the `spike/herdr-tether-migration-evaluation.md` assessment conclusion.**

Refine and migrate herdr-tether's core tmux session management engine (~3,500 LOC) into the `janus::tmux` native Rust module, completely eliminating the life-or-death dependency on a 3★ external plugin.

**Four Physical Requirements Driving This Decision:**

1. **🛑 Physical Survival Autonomy:** herdr-tether v0.3.0 has received no updates since release; MetaMach's PTY hijacking and session keep-alive cannot be bound to an unstable third-party plugin.
2. **⚡ Physical Non-Destruction Keep-Alive:** As a frontend TUI popup plugin, closing the Popup/terminal → SIGHUP → tmux session breaks. After internalization, the Daemon runs as a background resident; tmux sessions are never lost due to frontend destruction.
3. **📉 Microsecond-Level Signal Linkage:** External UDS IPC latency ~5-15ms per operation; internalized in-process function calls <1ms. Tool Guard interception can reach the target pane PID directly.
4. **📦 Zero Third-Party Dependency Distribution:** Compiled artifacts are only two native binaries (`janus-daemon` + `herdr-janus`); copy over, plug in, go.

**Migration Scope:** Core engine only (`DurableBackend` trait + `LifecycleService` + cold-start integration). ~16,000 LOC not reused (80% replaced by existing MetaMach implementations). Only one new dependency: `thiserror`.

**Effort:** ~2 weeks, ~3,500 LOC migration + ~2,600 LOC tests.

---

## 🏁 III. 0.3.0 Sign-Off

> **"Under 0.3.0, MetaMach no longer pursues formal perfectionism, but prioritizes physical-world high availability above all: de-containerization makes deployment zero-dependency, Multi-DB eliminates cross-blueprint lock contention, SQLite Fallback keeps the workshop from ever dying, global audit archiving makes every interception traceable, Fail-Closed makes the security boundary non-negotiable, and tmux Internalization makes the physical execution engine autonomously controllable."**

| Sign-Off Dimension | Status |
|---|---|
| Database Layer (0.2.0 proposals) | ✅ 3 adopted, 1 rejected, 0 deferred |
| Control Plane (0.3.0 proposals) | ✅ 4 adopted, 1 rejected, 0 deferred |
| Consistency with current `docs/ARCH.md` | ✅ Fully aligned |
| Consistency with current `docs/Feature-Spec.md` | ✅ Fully aligned |
