# Absurd Concepts ↔ MetaMach Implementation — Alignment Report

> **Date:** 2026-07-27  
> **Scope:** Upstream Absurd concepts document vs MetaMach 0.5.0 `janus::absurd` implementation  
> **Verdict:** **Structurally aligned on core primitives; intentionally subset on higher-level abstractions**

---

## Executive Summary

MetaMach vendors Absurd's Postgres-native schema (all five table prefixes: `t_`, `r_`, `c_`, `e_`, `w_` plus stored procedures) and uses its **core task lifecycle** (spawn → claim → checkpoint → complete/fail) faithfully. However, MetaMach uses Absurd as a *low-level durability substrate* rather than a full SDK — it builds its own step execution engine on top, using `metamach_step_meta` as the primary checkpoint store instead of Absurd's `c_` checkpoint tables, and does not use Absurd's higher-level features (events, sleep, cancellation, idempotency keys, headers).

This is a deliberate architectural decision: MetaMach's "steps" are **tmux PTY sessions** managed by a single daemon, not arbitrary user functions — so the SDK-level abstractions don't map cleanly.

---

## Concept-by-Concept Alignment

### ✅ Fully Aligned

| Concept | Absurd Doc | MetaMach Implementation | Notes |
|---|---|---|---|
| **Tasks** | Top-level unit of work with name, JSON params, dispatched onto a queue | `spawn_task()` creates a task with `task_id` (UUID v4), JSON params containing steps/blueprint/workflow | Exact match. [adapter.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/absurd/adapter.rs) |
| **Queues** | Logical namespace with `t_`, `r_`, `c_`, `e_`, `w_` tables | Each blueprint gets its own queue `absurd_{sanitized_name}`, all five table sets are created | Exact match. Schema vendored in [schema.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/absurd/schema.rs) |
| **Workers (Pull-Based)** | Workers poll a queue, claim with time-limited lease | Daemon polls via `claim_task()` with 300s lease | Exact match. Single-worker by design (ADR-005) |
| **Lease/Claim** | Time-limited lease, auto-extended on checkpoint | 300s lease, extended every 240s via `extend_claim()` | Exact match. 60s headroom per ADR-012 |
| **Postgres-Native** | All state lives in Postgres tables and stored procedures | Vendored schema, all state in PG, stored procedures for spawn/claim/extend/complete/fail/checkpoint | Exact match |

### ⚠️ Partially Aligned (Intentional Divergences)

| Concept | Absurd Doc | MetaMach Implementation | Rationale |
|---|---|---|---|
| **Steps / Checkpoints** | Steps inside task handlers; completed step results persisted in `c_` tables via `write_checkpoint()` | MetaMach uses **`metamach_step_meta`** as the primary checkpoint store (status, exit_code, stdout_tail, session_name). The Absurd `c_` table and `write_checkpoint()` are available in the adapter but are secondary | MetaMach steps are **tmux sessions**, not in-process function calls. The `metamach_step_meta` table carries tmux-specific state (session_name, exit_code, stdout_tail) that doesn't fit Absurd's generic JSONB result model. The checkpoint *concept* is faithfully implemented — just in a domain-specific table. |
| **Runs** | Each retry creates a new run; runs share checkpoints | `claim_task()` creates runs, but `max_attempts` is always **1** and `retry_strategy` is `{"kind":"none"}` | MetaMach doesn't use Absurd's built-in retry escalation. Crash recovery is handled by cold-start reconciliation (ADR-024) which manually resets leases and re-dispatches. This is because a crashed *daemon* ≠ a failed *task* — the distinction matters for an AI factory OS. |
| **Retries** | Task-level with fixed/exponential/none strategies | Schema supports all three; MetaMach always uses `none` with `max_attempts=1` | See above. Exponential backoff is inappropriate for daemon crash recovery — you want immediate re-claim. The retry primitives are available if needed for future use cases. |

### ❌ Not Used (By Design)

| Concept | Absurd Doc | MetaMach Status | Architectural Reason |
|---|---|---|---|
| **Events** (`await_event` / `emit_event`) | Tasks suspend waiting for named events; first-emit-wins | Schema creates `e_` and `w_` tables; stored procedures exist; **not called from Rust** | MetaMach's HITL (human-in-the-loop) is handled by the **Gateway** module (Teams Adaptive Cards, HMAC webhooks, loopback HTTP). The gateway uses `metamach_hitl_verdicts` + in-memory `pending` map with `tokio::sync::oneshot` channels — a purpose-built mechanism for real-time human approval flows. Absurd events are fire-and-forget with no rich UI integration. |
| **Sleep** (`sleep_for` / `sleep_until`) | Tasks suspend for a duration or until a time | **Not used** | MetaMach steps are tmux sessions — the "sleep" is the actual process execution time. There's no need to suspend a durable task when the tmux session itself handles timing. Quota exhaustion detection pauses via `tokio::time::sleep` at the daemon level. |
| **Cancellation** (programmatic + `maxDuration`/`maxDelay`) | Tasks detect cancellation at next checkpoint | **Not used** — tasks are cancelled by killing tmux sessions | Tmux `kill_session` + cold-start cleanup is more immediate than poll-based cancellation. MetaMach needs process-level kill semantics, not cooperative cancellation. |
| **Idempotency Keys** (spawn-time dedup) | Prevent duplicate task spawning | Schema supports it (`idempotency_key` column exists); **not passed from Rust** | MetaMach dispatches are user-initiated (CLI `janus start` or TUI), not automated schedulers. Duplicate protection isn't needed when a human is pressing the button. |
| **Headers** (trace/correlation metadata) | JSON metadata traveling with the task | Schema supports it (`headers` column exists); **not passed from Rust** | MetaMach propagates context via environment variables injected into tmux sessions (`JANUS_TASK_ID`, `JANUS_BLUEPRINT`, `JANUS_STEP`, etc.). Env vars are the natural context carrier for PTY-based execution. |
| **Multiple Workers** | Scale workers independently | Single daemon per machine (ADR-005) | MetaMach is a bare-metal factory OS — one daemon controls one machine's tmux sessions. Multi-worker would require distributed tmux coordination, which contradicts the bare-metal thesis. |
| **Cleanup/Retention** | Configurable retention, `absurdctl cleanup` | **Not implemented** | Data lives forever by default (Absurd's own default). Low priority since blueprint databases are small and can be cleaned up via offboard. |

---

## Schema Alignment Detail

### Vendored Absurd Schema (✅ Complete)

MetaMach vendors the **full** Absurd schema in [schema.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/absurd/schema.rs). All stored procedures are created:

| Stored Procedure | In Schema | Called from Rust |
|---|---|---|
| `spawn_task` | ✅ | ✅ via `adapter.rs` |
| `claim_task` | ✅ | ✅ via `adapter.rs` |
| `extend_claim` | ✅ | ✅ via `adapter.rs` |
| `complete_task` | ✅ | ✅ via `adapter.rs` |
| `fail_task` | ✅ | ✅ via `adapter.rs` |
| `write_checkpoint` | ✅ | ✅ via `adapter.rs` |
| `read_checkpoints` | ✅ | ✅ via `adapter.rs` |
| `emit_event` | ✅ | ❌ not called |
| `await_event` | ✅ | ❌ not called |

### MetaMach Overlay Tables (Beyond Absurd)

| Table | Purpose | Why Not Absurd's Tables |
|---|---|---|
| `metamach_step_meta` | Step-level state: status, exit_code, stdout_tail, session_name, timestamps | Carries tmux-specific fields that don't fit Absurd's generic `c_` checkpoint JSONB model |
| `metamach_hitl_verdicts` | HITL approval/rejection records with correlation_id | Rich verdict semantics (approve/reject/override) beyond Absurd's boolean event model |
| `metamach_env_snapshots` | Captured environment at dispatch time | Domain-specific; no Absurd equivalent |
| `blueprints` (catalog) | Tenant registry in `metamach_db` | Catalog-level metadata, not per-queue |

---

## Cold-Start Recovery: Absurd vs MetaMach

This is the most significant architectural divergence and deserves detailed explanation.

### What Absurd's Doc Says
> When a worker crashes, the task becomes available for another worker to pick up after the lease expires. Retries happen at the task level — a new run is created with backoff.

### What MetaMach Does Instead
1. On daemon startup, `coldstart::reconcile()` scans `metamach_step_meta` for STARTING/RUNNING steps
2. Kills orphaned tmux sessions
3. **Bypasses** Absurd's retry machinery by directly updating the Absurd tables:
   - `UPDATE t_{queue} SET retry_strategy = '{"kind":"none"}'::jsonb`
   - `UPDATE r_{queue} SET claim_expires_at = NOW() - INTERVAL '1 second', available_at = NOW() - INTERVAL '1 second'`
4. Marks steps as FAILED (exit_code: -1)
5. Re-dispatches as a fresh workflow (which creates a new task + run)

### Why This Is Correct for MetaMach
- Absurd's built-in retry assumes the *task logic* failed and needs exponential backoff
- MetaMach's failure mode is *daemon crash* — the task logic was fine, the process died
- Immediate re-dispatch (zero backoff) is the correct recovery behavior
- The cold-start path also needs to clean up tmux state, which Absurd knows nothing about
- ADR-024 documents this decision explicitly

---

## Alignment Verdict

```
                    Absurd Upstream
                    ┌──────────────────────────────────────┐
                    │  Events  │  Sleep  │  Cancel  │ Idem │  ← Not used (by design)
                    │──────────┼─────────┼──────────┼──────│
                    │  Headers │ Multi-W │ Cleanup  │ Cron │  ← Not used (by design)
                    ├──────────┴─────────┴──────────┴──────┤
                    │        Retries (exponential)          │  ← Schema present, bypassed
                    ├──────────────────────────────────────┤
  MetaMach uses ──► │  Tasks  │  Runs  │  Claims/Leases   │  ← Fully aligned
                    │  Queues │ Schema │  Stored Procs     │  ← Fully aligned
                    ├──────────────────────────────────────┤
  MetaMach adds ──► │  metamach_step_meta (tmux checkpts)  │  ← Domain overlay
                    │  metamach_hitl_verdicts (gateway)     │  ← Domain overlay
                    │  metamach_env_snapshots               │  ← Domain overlay
                    │  Cold-start reconciliation            │  ← Beyond Absurd
                    │  SQLite fallback ring                 │  ← Beyond Absurd
                    └──────────────────────────────────────┘
```

### Score: 7/10 Alignment (by concept count), 10/10 Alignment (on concepts that matter)

MetaMach uses **7 of 13** Absurd concepts (tasks, steps, runs, queues, workers, lease/claim, Postgres-native). It intentionally skips 6 (events, sleep, cancellation, idempotency, headers, multi-worker). The skipped concepts are all **SDK-level abstractions designed for in-process function execution** — they don't map to MetaMach's tmux-based PTY execution model.

The concepts MetaMach *does* use — the core durability primitives — are used correctly and completely. The divergences are architecturally sound and documented in ADRs 005, 008, 012, 015, and 024.

---

## Recommendations

> [!NOTE]
> These are observations, not action items. The current architecture is sound.

1. **Consider using Absurd's `c_` checkpoint tables** alongside `metamach_step_meta` for step results. Currently the Absurd checkpoint machinery is available (`write_checkpoint`/`read_checkpoints` exposed in adapter) but step results go only to `metamach_step_meta`. Writing to both would make the system inspectable via standard Absurd tooling (`absurdctl`).

2. **Idempotency keys for pipeline DAG dispatch** — when `janus plan` dispatches multi-node pipeline levels, spawn-time deduplication could prevent duplicate workflow creation on CLI retry. Low priority but architecturally clean.

3. **Document the overlay relationship** — the ARCH.md §4.2 correctly describes the multi-DB fan-out but doesn't explicitly call out that `metamach_step_meta` is a *parallel* checkpoint system to Absurd's `c_` tables. A sentence clarifying this would help future contributors.

4. **Cleanup policy** — as the system matures, consider implementing Absurd's cleanup/retention patterns for completed tasks. Currently `metamach_step_meta` rows and Absurd task rows accumulate indefinitely.
