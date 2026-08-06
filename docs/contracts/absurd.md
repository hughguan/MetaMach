# Absurd Durable Execution Contract & Integration

> **Status:** Implemented.  
> Vendored SQL: `janus/sql/absurd.sql` (v0.4.0, upstream commit `9b77b35`), tracked in `janus/sql/ABSURD_VERSION`.  
> Rust SPI: `janus::absurd::AbsurdPgAdapter` (`janus/src/absurd/adapter.rs`).

---

## 1. Integration Topology & Pull-Mode Model

```
                        ┌──────────────────────────────────────┐
                        │      ~/.metamach/db/ (Host PG)       │
                        │  - Managed by janus-daemon           │
                        │  - absurd.sql schema loaded          │
                        └──────────────────┬───────────────────┘
                                           │
                             ┌─────────────┴─────────────┐
                             │  Absurd Stored Procedures │
                             └─────────────┬─────────────┘
                                           ▲ (Pull / Task Claims)
                                           │
                             ┌─────────────┴─────────────┐
                             │   janus-daemon (Rust)     │
                             │   Absurd Durable Worker   │
                             └─────────────┬─────────────┘
                                           │ (State Machine Step Controls)
                                           ▼
                     ┌──────────────────────────────────────────┐
                     │   janus::tmux (Physical PTY Sandbox)     │
                     │   janush (Fail-Closed 30s Interceptor)   │
                     └──────────────────────────────────────────┘
```

**No standalone coordinator process** — Absurd is a set of PostgreSQL stored procedures (`absurd.<fn>(...)`). MetaMach's `janus-daemon` acts as the pull-mode worker.

---

## 2. Rust `DurableEngine` Trait Surface

MetaMach defines `janus::absurd::DurableEngine` (`adapter.rs`), mapping 1:1 to Absurd stored procedures over `sqlx::PgPool`:

| Method | Absurd Stored Proc | Purpose |
|---|---|---|
| `create_queue(queue)` | `absurd.create_queue(queue)` | Initialize per-workflow queue tables (`t_`, `r_`, `c_`, `e_`, `w_`, `i_`). Idempotent. |
| `spawn_task(queue, task_name, params)` | `absurd.spawn_task(...)` | Enqueue a task. Mints and returns UUIDv7 `task_id`. |
| `claim_task(queue, worker_id)` | `absurd.claim_task(...)` | Pull-lease 1 task (30s lease limit). |
| `extend_claim(queue, run_id, secs)` | `absurd.extend_claim(...)` | Renew worker lease during step execution. |
| `complete_run(queue, run_id, state)` | `absurd.complete_run(...)` | Mark task run `COMPLETED`. |
| `fail_run(queue, run_id, reason)` | `absurd.fail_run(...)` | Mark task run `FAILED` (triggers retry loop up to `max_attempts: 3`). |
| `set_checkpoint(queue, task_id, step, state, owner_run)` | `absurd.set_task_checkpoint_state(...)` | Checkpoint step state for crash recovery. |
| `get_last_checkpoint(queue, task_id)` | `c_<queue>` table query | Retrieve last `COMPLETED` step checkpoint. |
| `non_terminal_tasks(queue)` | `t_<queue>` table query | Query active tasks (`STARTING`/`RUNNING`/`STOPPED`) for cold-start reconciliation. |
| `emit_event(queue, event_name, payload)` | `absurd.emit_event(...)` | Resume signal for HITL suspended tasks. |
| `await_event(queue, task_id, run_id, step, event_name, timeout)` | `absurd.await_event(...)` | Suspend task execution awaiting external HITL webhook response. |

---

## 3. Physical Databases & Multi-Tenant Isolation

- **Catalog Database (`metamach_db`)**: Registers active blueprints, tenant IDs, and default workflows.
- **Blueprint Databases (`metamach_blueprint_<name>`)**: One dedicated database per active blueprint. Each blueprint database contains:
  - Absurd task engine tables (`absurd.*`)
  - MetaMach step metadata overlay (`metamach_step_meta`)
  - Environmental snapshot tracking (`004_env_snapshot.sql`)

---

## 4. Dual-Track Resilience & Recovery

```
[Normal Mode]   janus-daemon ──► Absurd PG (metamach_blueprint_<name>) ──► Checkpoints Saved
                               │
                         (PG Outage / Connection Loss)
                               │
                               ▼
[Degraded Mode] janus-daemon ──► fallback.db (SQLite Ring Buffer) ──► Events Buffered
                               │
                         (PG Restored)
                               │
                               ▼
[Replay & Merge] fallback.db ──► Replay to Absurd PG ──► Checkpoints Restored
```

If PostgreSQL becomes unreachable, `janus-daemon` buffers step transitions into `fallback.db` (SQLite ring buffer). When PostgreSQL recovers, the daemon automatically replays buffered events into `metamach_step_meta` and resumes normal durable execution.
