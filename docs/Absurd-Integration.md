# Absurd Integration — MetaMach 0.4.0

> **Status: Implemented.** `absurd.sql` is vendored at `janus/sql/absurd.sql` (v0.4.0, upstream commit `9b77b35`), tracked in `janus/sql/ABSURD_VERSION`. The Rust adapter layer (§7.3), CI upstream watcher (§7.2), and replay logic (§3) are spec'd but not yet built. This document describes both the completed vendoring and the planned integration architecture.

---

## Integration Topology

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

**Bottom line:** No standalone coordinator process — just `absurd.sql` written into Postgres stored procedures, combined with Pull-mode Durable Execution.

---

## Absurd Repository Anatomy

While MetaMach vendors only `absurd.sql` (the runtime schema engine), the full [absurd repository](https://github.com/earendil-works/absurd) contains additional tooling and SDKs. This table clarifies what each component does and whether MetaMach needs it:

| Component | What it does | Needed by MetaMach? |
|---|---|---|
| **`sql/absurd.sql`** | Core schema engine: stored procedures for `spawn_task`, `claim_task`, `await_event`, checkpoints, queues | ✅ **Vendored** — the only runtime requirement. Compiled into `janus-daemon` via `include_str!`. |
| **`sql/migrations/`** | Version-to-version DB migration scripts (`0.0.3→0.0.4→…→0.4.0`) | ⬜ **Not yet** — needed only when upgrading our vendored v0.4.0 to a future version. Will be vendored to `janus/sql/migrations/` on-demand. |
| **`sdks/go/`** | Go SDK: worker, context, client wrappers around the stored procedures | ❌ **Not needed** — MetaMach writes its own Rust adapter via sqlx (§7.3). Same SQL interface, no Go dependency. |
| **`sdks/python/`** | Python SDK | ❌ Not needed |
| **`sdks/typescript/`** | TypeScript SDK | ❌ Not needed |
| **`absurdctl/`** | CLI tool: inspect queues, tasks, schema versions | 🔧 **Useful, but external** — ops/debug tool, not compiled into the daemon. Launched separately when needed. |
| **`habitat/`** | Web UI: visualize task timelines (SUSPENDED / RUNNING / FAILED) | 🔧 **Optional** — development visualization tool, launched separately. Code already in `spike/absurd/habitat/` for research. |
| **`tests/`** | Upstream Python test suite | ❌ Not needed — MetaMach has its own Rust test suite. |
| **`scripts/`** | Build scripts for `absurdctl` | ❌ Not needed |

### Why We Don't Need the SDKs

The SDKs are language-specific wrappers that call Absurd's PG stored procedures. MetaMach uses Rust + sqlx to call those same stored procedures directly:

```
Absurd Go SDK:          Go code → lib/pq → PG → SELECT absurd.spawn_task(...)
MetaMach Rust adapter:  Rust → sqlx → PG → SELECT absurd.spawn_task(...)
```

Same SQL interface, no Go/Python/TS dependency. This is the `AbsurdPgAdapter` layer described in §7.3 — ~100 lines of sqlx queries, not a full SDK port. Even if Absurd changes its client SDK signatures, as long as the stored procedure API remains compatible, **MetaMach code changes zero lines**.

### What to Vendor on Upgrade

When upgrading from v0.4.0 to a future absurd version, vendoring the migration script is the only new file needed:

```
sql/migrations/0.4.0-0.5.0.sql  →  janus/sql/migrations/0.4.0-0.5.0.sql
```

This would be applied by `janus-daemon`'s `verify_and_migrate()` path (§7.1), which checks the DB schema version against `EXPECTED_ABSURD_VERSION` and runs the appropriate migration.

---

## 1. Physical Base & Schema Bootstrapping

In MetaMach 0.4.0, `janus-daemon` directly ignites the host PG (`~/.metamach/db/`). The Absurd SQL schema is embedded at compile time and injected on first startup:

1. **Automatic Schema Injection:**
   The first time `janus-daemon` initializes a PG instance, it reads the vendored `absurd.sql` via Rust's `include_str!` and executes the migration against the blueprint's logical database:

   ```rust
   // Inside janus-daemon bootstrap
   pub async fn init_absurd_schema(pg_pool: &PgPool) -> anyhow::Result<()> {
       let absurd_sql = include_str!("../sql/absurd.sql");
       sqlx::query(absurd_sql).execute(pg_pool).await?;
       tracing::info!("Absurd durable execution engine initialized");
       Ok(())
   }
   ```

2. **Multi-Blueprint Physical Isolation (One PG, Multi-DB):**
   Each blueprint gets an independent database (`metamach_blueprint_<name>`). Absurd's queues and step checkpoints are fully isolated per database, eliminating cross-blueprint lock contention.

---

## 2. Rust Native Worker Wrapper & Pull-Mode Integration

Absurd uses **Pull mode**, which matches MetaMach's async reactor perfectly. Within `janus-daemon`, a lightweight Rust task worker is spawned per blueprint:

```rust
// Durable step workflow using Absurd's pull semantics
pub async fn execute_physical_deploy_workflow(
    ctx: WorkflowContext,
    params: DeployParams,
) -> Result<()> {
    // Step 1: Compile firmware (Checkpoint 1)
    let build_artifact = ctx
        .step("compile_firmware", || async {
            let output = janus::tmux::exec_command("cargo build --release").await?;
            Ok(output.artifact_path)
        })
        .await?;

    // Step 2: Trigger HITL interception — the Task enters SUSPEND state,
    // consuming zero CPU/memory while awaiting approval.
    janus::gateway::notify_teams_interception(&params).await?;

    let approval = ctx
        .await_event(&format!("hitl.approve:{}", params.run_id))
        .await?;
    if !approval.is_approved {
        return Err(anyhow::anyhow!("Human operator rejected execution"));
    }

    // Step 3: Physical flash (Checkpoint 2)
    ctx.step("flash_esp32", || async {
        let cmd = format!(
            "esptool.py --port {} write_flash 0x0 {}",
            params.port, build_artifact
        );
        janus::tmux::exec_command(&cmd).await
    })
    .await?;

    Ok(())
}
```

---

## 3. Dual-Track Resilience: PG Crashes → SQLite Fallback → Replay

Absurd depends heavily on PG, but MetaMach mandates SQLite degraded-mode survival. The dual-track approach:

```
[Normal Mode]   janus-daemon ──► Absurd SQL (PG) ──► Step Checkpoints Saved
                               │
                         (PG crashes / OOM)
                               │
                               ▼
[Degraded Mode] janus-daemon ──► fallback.db (SQLite) ──► Events buffered as JSON
                               │
                         (PG restored)
                               │
                               ▼
[Replay & Merge] fallback.db ──► Replay to Absurd PG ──► Checkpoints Restored
```

1. **Normal:** `janus-daemon` calls Absurd's PG stored procedures for task claiming and step submission.
2. **Degraded (PG crash):** On DB exception, the daemon switches to `fallback.db` (SQLite). Unpersisted steps are serialized as WAL entries. The control plane runs degraded; `janush` interception remains active.
3. **Recovery:** When PG restarts, the daemon replays buffered SQLite events back into Absurd's state tables, completing the seamless handover.

> This architecture is already partially implemented: `janus/src/absurd/fallback.rs` provides the SQLite ring buffer; `janus/src/absurd/mod.rs` handles the PG→SQLite transition. The replay logic (replaying SQLite events back into PG on recovery) is the remaining work.

---

## 4. HITL Event Bridge: Absurd ↔ janus::gateway (Teams / Hermes)

Absurd supports `await_event()` (event suspension), which pairs naturally with MetaMach 0.4.0's Teams/Hermes HITL gateway:

1. **Interception fires:** When `janush` intercepts a high-risk command, `janus-daemon` registers a suspended wait event in Absurd: `ctx.await_event("hitl.approve:run_4289")`. The task is suspended in PG.
2. **Teams card dispatched:** `janus::gateway` formats and pushes a Teams Adaptive Card.
3. **Human approval:** The Factory Director taps **Approve** in Teams on mobile.
4. **Event triggered:** `janus::gateway` calls Absurd's `emitEvent("hitl.approve:run_4289", { approved: true })`.
5. **Resume:** Absurd wakes the task inside PG with zero conflict. `janus-daemon` drives `janus::tmux` to unfreeze and dispatch the physical command.

---

## 5. Ops Tooling: absurdctl & habitat

Absurd provides a CLI (`absurdctl`) and a visualization UI (`habitat`). These can be mounted into MetaMach's toolchain:

```bash
# Inspect Absurd task state in a blueprint database
absurdctl inspect-queue -d metamach_blueprint_default
```

During heavy blueprint development, the Factory Director can optionally launch `habitat` (web UI) to visualize task timelines — suspended (awaiting Teams approval), running, or failed steps.

---

## 6. Dependency Strategy: Vendoring vs External Fetch

**Proposal: Vendoring (compile-time embed).** Do not add a Git submodule, and do not introduce a runtime external dependency. Copy `absurd.sql` into the MetaMach repository and embed it into the `janus-daemon` binary via `include_str!`.

### Rationale

1. **Zero runtime network dependency:** In offline/air-gapped workshops, `janus-daemon` reads the compiled-in schema from memory and initializes PG in microseconds.
2. **Deterministic builds:** `Cargo.lock` + the repository fully lock the SQL hash. An upstream release can never break a local build.
3. **Single-binary distribution:** No `curl`, no `git clone`, no external filesystem paths needed at runtime.

**Adopted.** See ADR-015, `janus/sql/absurd.sql`, and `janus/sql/ABSURD_VERSION`.

---

## 7. Upstream Maintenance: Triple Safeguard

To absorb Absurd community upgrades without destabilizing MetaMach:

### 7.1 Compile-Time Schema Version Locking

Absurd maintains an internal schema version. `janus-daemon` must verify it on ignition:

```rust
pub const EXPECTED_ABSURD_VERSION: i32 = 4;

pub async fn verify_and_migrate(pool: &PgPool) -> anyhow::Result<()> {
    let current = get_absurd_schema_version(pool).await?;

    if current < EXPECTED_ABSURD_VERSION {
        tracing::info!("Migrating Absurd schema v{current} -> v{EXPECTED_ABSURD_VERSION}");
        apply_embedded_migration(pool, current).await?;
    } else if current > EXPECTED_ABSURD_VERSION {
        anyhow::bail!(
            "DB Absurd schema (v{current}) is newer than binary (v{EXPECTED_ABSURD_VERSION}). \
             Please update MetaMach!"
        );
    }
    Ok(())
}
```

### 7.2 CI Upstream Watcher

A scheduled GitHub Actions workflow (weekly) checks `earendil-works/absurd` for new releases, runs the MetaMach test suite against the updated schema, and auto-opens a Draft PR if all tests pass.

### 7.3 Rust Abstraction Layer — Direct sqlx Adapter

Absurd currently offers TS, Python, and Go SDKs. The Rust SDK is in early stages. MetaMach can own the Rust integration directly:

```
  ┌─ janus-daemon core logic ─┐
              │
              ▼    (MetaMach-defined trait)
  ┌─ trait DurableEngine ─────┐
              │
              ▼    (Adapter: sqlx → PG stored procedures)
  ┌─ AbsurdPgAdapter ─────────┐
  │   SELECT absurd.spawn_task(...)
  │   SELECT absurd.await_event(...)
              │
              ▼
  ┌─ Host PG (absurd.sql) ────┘
```

**Benefit:** Even if Absurd changes its client SDK signatures, as long as the PG stored procedure API remains compatible, **MetaMach code changes zero lines**. The adapter layer decouples the SDK surface from the SQL contract.

---

## 8. Cross-Check: Conflicts & Resolutions

| # | Topic | Absurd-Integration Proposal | Current Codebase / Docs | Conflict? | Resolution |
|---|---|---|---|---|---|
| 1 | **Dependency classification** | Vendoring: `absurd.sql` compiled into binary | `ARCH.md` §5.4 lists absurd as external, fetched by `make bootstrap` | ✅ Resolved | **Adopted vendoring** (ADR-015). absurd.sql vendored at `janus/sql/absurd.sql` (v0.4.0, commit `9b77b35`), tracked in `janus/sql/ABSURD_VERSION`. ARCH.md updated accordingly. |
| 2 | **Gateway naming** | `mach-gateway` | `janus::gateway` (in-code, ARCH.md, ADR.md) | ✅ Resolved | Canonical name is `janus::gateway` throughout this document. `mach-gateway` is a branding alias only. |
| 3 | **Crate path** | `crates/janus-daemon/src/` | `janus/src/` (flat workspace root) | ✅ Resolved | All paths use `janus/`. MetaMach is a single-crate workspace. |
| 4 | **Schema file location** | `src/sql/absurd.sql` | No such file; migrations live in `janus/migrations/` | ✅ Resolved | absurd.sql vendored at `janus/sql/absurd.sql`. Complements (not replaces) `janus/migrations/`. |
| 5 | **CI workflow name** | `upstream-absurd-check.yml` | `.github/workflows/ci.yml` (only CI workflow) | ✅ Resolved | CI watcher will be added as a separate workflow file when built. |
| 6 | **CLI naming** | `metamachctl` | `janus` (unified CLI) | ✅ Resolved | All CLI references use `janus`. |
| 7 | **Make target** | `make status` | No such target in Makefile | ✅ Resolved | Use `janus status`. No `make status` target needed. |
| 8 | **Habitat UI path** | Not specified | `spike/absurd/habitat/` (gitignored, in spike/ not committed) | ✅ No conflict | The habitat UI already exists in spike/ for research. If adopted, it would become part of the toolkit (not committed to this repo — external dependency like OpenWiki). |
| 9 | **`janus::tmux` API** | `janus_tmux::exec_command()` | `janus::tmux` module uses `DurableBackend` trait | ✅ Resolved | Examples use the trait API from `janus::tmux::DurableBackend`. |
| 10 | **SUSPEND state** | Matches existing ARCH.md invariant 4 and §6.1 (SUSPENDED) | ✅ Already aligned |  |
| 11 | **Dual-track resilience** | Matches ADR-004 (Retain SQLite Fallback) and ADR-008 (16KB Budget) | ✅ Already aligned |  |
| 12 | **De-containerization** | Matches ADR-001 (de-containerization) and ADR-002 (~/.metamach/db/) | ✅ Already aligned |  |

---

## Summary Matrix

| Dimension | Approach | Physical Benefit |
|---|---|---|
| **Dependency Model** | **Vendoring** (compile `absurd.sql` into `janus-daemon`) | Offline-capable, zero network dependency, single-binary distribution. Resolved: ADR-015, `janus/sql/absurd.sql`. |
| **Version Control** | **Hardcoded Version Guard** (DB ↔ Binary strict reconciliation) | Prevents runtime crashes from DB/binary version mismatch. |
| **Upstream Sync** | **CI Upstream Bot** (scheduled weekly, runs test suite) | Automated bug-fix absorption; upstream breaking changes flagged early. |
| **Code Decoupling** | **Self-implemented `sqlx` Adapter** | No dependency on third-party Rust SDK; locks the SQL stored-procedure contract. |

**Bottom line:** Integrating Absurd into MetaMach 0.4.0 does not require Temporal, Inngest, or any heavyweight Java/Go service stack. It requires only writing `absurd.sql` into `~/.metamach/db/` host PG — fully aligned with MetaMach's four architectural pillars: **security (Fail-Closed event suspension), stability (PG + SQLite dual-track), decoupling (Pull mode), and reuse (minimal SQL).**
