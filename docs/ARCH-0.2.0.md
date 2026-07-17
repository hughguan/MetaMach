# MetaMach 0.2.0 — Database Layer Architecture Evolution

> **📋 Design Evolution Record:** This document captures the database deployment architecture exploration from 0.1.0 (initial design) to 0.2.0. Some proposals herein (Drop SQLite, DROP DATABASE) were later **rejected** in the 0.3.0 consensus baseline. See `docs/ARCH-0.3.0.md` for final decisions.

After a rigorous industrial-grade single-machine audit, we pulled the system back from over-engineered distributed fantasies and established a minimalist security perimeter centered on **`Absurd Postgres` state machine authority**.

The version numbers have been precisely adjusted to the development phase: **`0.1.0` (initial design)** and **`0.2.0` (final practical)**. Below is the summary of major decisions and details regarding MetaMach's database deployment and usage changes.

---

## 💾 Database Deployment Changes & Decision Ledger (0.1.0 vs. 0.2.0)

| Dimension | MetaMach 0.1.0 Initial Design | MetaMach 0.2.0 Final Practical | Core Change Details | Physical Rationale |
| --- | --- | --- | --- | --- |
| **Container Dependency** | Relies on Docker-Compose to spin up Postgres container | **Complete De-containerization (No Docker)** — Host-native Sandbox local physical ignition | Abolish `docker-compose.yml`; `janus-daemon` directly manages the sole Postgres physical instance on the host. | 1. **Eliminate system redundancy:** Remove virtual NICs, container network bridges, and Docker resident memory overhead. 2. **Zero-dependency distribution:** Achieve "copy over, plug in, go" minimal distribution experience after `make bootstrap`. |
| **Data Persistence Medium** | Planned SQLite as PG crash Fallback backup | ~~Complete SQLite Abandonment (Drop SQLite)~~ — **❌ Later rejected in 0.3.0** (SQLite fallback retained per Contract 3.8) | Abandoned the fantasy of "simulating a high-dimensional state machine on SQLite with business code"; the control plane is 100% locked to **Postgres + Absurd**. | 1. **Absurd strong dependency:** Absurd relies on PG-exclusive stored procedures, `ON CONFLICT` locks, and event waiting (`awaitEvent`) for high-dimensional Durable Execution. 2. **Physical deadlock prevention:** SQLite triggers busy-wait locks under multi-Agent concurrent writes; PG has native high-concurrency lock throughput. |
| **Physical Topology** | Each Blueprint owns a dedicated physical PG instance and port | **Single Physical Instance + Logical Multi-Database** `(One PG, Multi-DB)` | The host runs only one physical Postgres process (exclusive fixed port); each Blueprint maps to an independent `Logical Database`. | 1. **Defend against memory explosion:** Avoid hundreds of MB memory accumulation from starting independent PG processes per blueprint when idle. 2. **Prevent connection pool fragmentation:** Consolidate into one physical connection handle, drastically reducing CPU context-switch overhead. |
| **Lifecycle & Cleanup** | Clear large JSON caches via stored procedures on Offboard | ~~Direct `DROP DATABASE` on Offboard~~ — **❌ Later rejected in 0.3.0** (see §1.3 of ARCH-0.3.0.md) | Execute `DROP DATABASE metamach_blueprint_<name> WITH (FORCE);` on offboarding. | 1. **Filesystem-level cleanup:** `DROP DATABASE` physically deletes all disk files for that logical database, directly reclaiming space without slow `VACUUM FULL`. 2. **Security isolation:** Ensure offboarded blueprint process noise is physically burned after reading. |
| **Data Storage Path** | Planned to use volatile `pg_tmp` (RAM disk) | **Host-native local SSD physical persistence** — `~/.metamach/db/` | Abandon pure volatile RAM disk and Herdr state area paths; maintain the physical cluster in an independent global directory under the user's home directory. | 1. **Seismic resilience requirement:** Software development and testing cycles typically span weeks. If stored in volatile RAM, Absurd Checkpoints would be physically lost on restart, bankrupting the Durable commitment. 2. **Escape plugin lifecycle hijacking:** Avoid the risk of accidental data erasure during Herdr uninstall or upgrade. |

---

## 🛠️ Deep Technical Analysis of 0.2.0 Major Changes

### 1. Why Firmly Drop SQLite in 0.2.0, Keep Postgres + Absurd? **⚠️ Note: This proposal was later rejected in 0.3.0; SQLite fallback is retained per Contract 3.8.**

We briefly considered switching the entire data layer to lightweight embedded SQLite. However, **Absurd's core soul is precisely that it pushes the extremely complex logic of distributed systems — "Exactly-Once" state recovery, atomic Checkpoint writes, and race-free event caching (first-emit wins) — entirely down into Postgres Stored Functions**.

If we dropped PG in 0.2.0, it would mean we must reinvent the distributed transaction scheduling wheel inside `janus-daemon` with Rust business code, causing system complexity to rapidly spiral out of control. Therefore, **PG strong dependency is a non-negotiable physical red line for MetaMach 0.2.0**.

### 2. The Elegant Closed Loop of "Single Physical Instance + Logical Multi-Database"

To free the Factory Director from Docker awareness while preventing multiple concurrent blueprints from starving the host's CPU and memory, we designed the **One PG, Multi-DB** topology:

* **Onboard Phase:** `janus-daemon` first launches the sole local physical Postgres Cluster process (bound to a fixed local port). Then, via `CREATE DATABASE metamach_blueprint_<name>`, it instantly allocates an independent logical multi-tenant space for the new product line and performs `absurdctl` initialization.

* **Offboard Phase:** When the task line is sealed, after generating the `production_report.md` knowledge artifact, directly execute `DROP DATABASE metamach_blueprint_<name> WITH (FORCE)`. Postgres will instantly shred all physical files under that logical database at the filesystem level, completing the cleanest, most thorough "lossless anti-bloat physical slimming." **⚠️ Note: DROP DATABASE was later rejected in 0.3.0; the adopted approach is DELETE + absurd_audit_log archiving.**

### 3. Home Directory SSD Physical Persistence Design (`~/.metamach/db/`)

Native `pg_tmp` lives and dies with the connection — its pure volatility is fatal in automotive bus or embedded development cycles spanning weeks. In 0.2.0, we consolidated the data path to `~/.metamach/db/`.

Even in the event of unexpected workshop power loss or system hibernation, after restart `janus-daemon` can still smoothly bring up the local PG, relying on Absurd's Checkpoints to seamlessly resume from the last `COMPLETED` step. At the same time, this completely escapes the Herdr plugin lifecycle's constraints on data security.

---

## 🏁 Summary

Through the 0.2.0 reshaping, MetaMach's data layer has fully matured:

> **"During the weeks of development tug-of-war, use `~/.metamach/db/`'s Sandbox PG to lock down state and process logs (100% Durable); at the final moment of Offboard, generate `production_report.md` for knowledge inheritance, then DELETE + archive to `absurd_audit_log` for lossless slimming (100% auditable)."**
> 
> **⚠️ Note:** The original 0.2.0 conclusion referenced "consensus Amend into Git history" and "DROP DATABASE" — both were later rejected in the 0.3.0 consensus. See `docs/ARCH-0.3.0.md` for the final design.
