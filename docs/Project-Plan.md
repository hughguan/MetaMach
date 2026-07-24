# MetaMach 0.1.0 - Project Plan

> Milestone roadmap (M0–M4) of independently check-in-able, physically network-able factory units.
>
> **Governing architecture baseline:** This plan is aligned to the **0.3.0 consensus** (`docs/ARCH-0.3.0.md`): de-containerized host-native Postgres at `~/.metamach/db/`, One-PG-Multi-DB topology, internalized `janus::tmux`, `DELETE` + global `absurd_audit_log` Offboard, retained SQLite fallback, Fail-Closed 30s interception, 16KB dual defense. Earlier 0.1.0/0.2.0 proposals (Docker, external `herdr-tether`, `DROP DATABASE`, `melt`/`VACUUM`-only Offboard) are **superseded**.

This plan decomposes MetaMach 0.1.0's R&D and grid-connection process into **5 core milestone phases (M0–M4)**. Each milestone is sliced by "compilable, independently commit/check-in-able, 100% Immutable-vs-Mutable compliant" physical features or functional modules (Feature Units), with explicit physical verification methods to ensure the Richmond Hill workshop's grid-connection is seamless and steady.

## Milestone Timeline

```
[Milestone 0] ──> [Milestone 1] ──> [Milestone 2] ──> [Milestone 3] ──> [Milestone 4] ──> [Milestone 5]
 Herdr 0.7.3 Validate    Infra & Shell       Daemon Core         Shield Layer        Lifecycle & Self-Heal   Integration & Release
```

> **M0 is a prerequisite gate:** All Popup/plugin tasks from M1 onward depend on the Herdr 0.7.3 plugin SDK being available. M0 must first validate this external contract; otherwise M1 Task 1.2 is blocked.

## Milestone 0: Herdr 0.7.3 Plugin Contract Validation (External SDK Validation) - ✅ VALIDATED 2026-07-15

> **Timebox:** ~3 days (completed)

- **Status:** Complete. Validated against installed Herdr 0.7.3; contract documented in `docs/herdr-v1-contract.md` (version-controlled English source; `docs/CH/` is gitignored). PoC at `spike/herdr-hello-plugin/` (gitignored). **M1 Task 1.2 (Popup) green-lit.** Key corrections: popup placement is `overlay` (not `popup`); manifest has no `width`/`height`; Herdr injects `HERDR_PLUGIN_ROOT/CONFIG_DIR/STATE_DIR` + `HERDR_SOCKET_PATH`.

- **Goal:** Before investing any MetaMach self-developed code, first verify that the Herdr 0.7.3 plugin SDK is genuinely usable, eliminating M1's largest unknown external dependency. M0 produces no MetaMach business code-only "contract validation evidence + minimal PoC plugin + Herdr 0.7.3 API interface memo."

- **Check-in-able directory structure:**
    `docs/herdr-v1-contract.md` (interface memo), `spike/herdr-hello-plugin/` (PoC plugin, gitignored)

### Tasks

#### Task 0.1: Herdr 0.7.3 Installation & Plugin SDK Availability Verification (Check-in Unit 0a)
- **Description:** Install Herdr 0.7.3 and validate the plugin loading chain end-to-end.
- **Implementation:**
    - Install Herdr 0.7.3; `herdr plugin link` can successfully mount a plugin directory.
    - Verify `prefix+j` keybinding can dispatch to a mounted plugin and `herdr-plugin.toml` is parseable.
    - Verify `placement = "overlay"` is the valid Herdr 0.7.3 pane directive (`popup`/`width`/`height` are NOT valid manifest fields - see `docs/herdr-v1-contract.md`).
- **UAT:** Mount a skeleton plugin; pressing `prefix+j` causes Herdr to actually pop up a Popup of the specified dimensions. Record the actual Herdr 0.7.3 API surface (event hooks, UDS conventions, lifecycle callbacks) into `docs/herdr-v1-contract.md`.

#### Task 0.2: Minimal Popup PoC Plugin (Check-in Unit 0b)
- **Description:** Implement a "Hello World" Popup plugin with the Herdr 0.7.3 SDK, validating all interaction primitives MetaMach will subsequently need.
- **Implementation:**
    - PoC plugin renders a `ratatui` Popup, captures keyboard focus, safely pops the stack on `Esc`.
    - Verify the Popup can communicate with a background process via UDS (proving the M2 `herdr-janus` ↔ `janus-daemon` pathway).
- **UAT:** PoC plugin pops up on `prefix+j`, keyboard focus does not escape, `Esc` closes, UDS communication round-trip succeeds. If any item fails, M1 does not start-align contract with Herdr 0.7.3 upstream first.

## Milestone 1: Infrastructure Grid-Connection & Shadow Shell (Immutable & Base)

> **Timebox:** ~2 weeks

- **Goal:** Establish Immutable/Mutable directory separation, stand up the **host-native** Absurd Postgres cluster + global catalog DB (no Docker), and get the lightweight shadow client Popup rendering.

- **Check-in-able directory structure:**
    `janus/herdr-plugin.toml`, `janus/src/bin/herdr_janus.rs`, `Makefile` (native PG targets), `janus/migrations/` (catalog + blueprint split)

### Tasks

#### Task 1.1: Host-Native Absurd Postgres & Multi-DB Migrations (Check-in Unit 1)
- **Description:** Stand up the de-containerized Postgres cluster and the split migration set (global catalog DB vs per-blueprint DB), per 0.3.0 §1.1/§1.4. No `docker-compose.yml`.
- **Implementation:**
    - **Single owner:** `janus-daemon` owns the PG lifecycle on first startup - `initdb -D ~/.metamach/db/`, `pg_ctl start`, role + random password persisted to `~/.metamach/db/.pgpass` (chmod 0600), and migration execution. The Makefile `db-up` target is a thin shim (initdb + `pg_ctl start`, or `janus-daemon --init-only`); it must NOT duplicate role/DB/migration logic. Use `--auth-local=scram-sha-256` (not `trust`); surface, never swallow, bootstrap errors.
    - **Global catalog DB:** create one catalog database (e.g., `metamach`) holding the `blueprints` registry and the **global `absurd_audit_log`** table. Migration `001_catalog.sql` is applied at bootstrap.
    - **Per-blueprint DB schema (F1):** the durable-execution engine is [Absurd](https://github.com/earendil-works/absurd) v0.4.0 - `absurdctl init` applies `absurd.sql` (absurd owns the task/checkpoint/event tables + functions; MetaMach does **not** redefine them). Migration `002_blueprint.sql` adds only the thin `metamach_step_meta` overlay (`target_sha VARCHAR(64)`, `exit_code`, `stdout_tail`, `started_at`) keyed by absurd's UUID `task_id`; `status` + `result_cache` live in absurd's checkpoint state JSONB. Applied against `metamach_blueprint_<name>` on `janus onboard` (M4 Task 4.3). Cross-DB FKs to `blueprints(id)` are **dropped**; `blueprint_name` is a routing key, not a FK. (Drops the prior `003_target_sha.sql` - `target_sha` is now a column in the overlay.)
    - **`fallback_events` (Contract 3.8):** add `blueprint_name` + `target_sha` columns so Log Replay can route events to the correct per-blueprint DB and preserve SHA-lock state across PG outages.
- **UAT:** `make bootstrap` brings up native PG at `~/.metamach/db/` (no Docker); `psql -h ~/.metamach/db -d metamach -c '\dt'` shows `blueprints` + `absurd_audit_log`; `~/.metamach/db/.pgpass` is mode 0600.

#### Task 1.2: Shadow Shell Popup & TUI Rendering (Check-in Unit 2)
- **Description:** Write `herdr-plugin.toml` and implement `herdr_janus.rs` shadow client.
- **Implementation:**
    - Declare `[[panes]] id = "dispatcher" placement = "overlay" command = ["herdr-janus"]` in `herdr-plugin.toml` (no `width`/`height` - Herdr manages overlay sizing; see `docs/herdr-v1-contract.md`).
    - Use `ratatui` in `herdr_janus.rs` to render a static "production dispatch dashboard" interactive interface. Focus auto-locked; exit on `Esc`.
- **UAT:** Execute `herdr plugin link` to mount the plugin; press `prefix+j` inside Herdr; a floating overlay Popup appears at screen center (sized by Herdr's overlay defaults, not the manifest).

#### Task 1.3: Scaffold Config Files (Check-in Unit 3)
- **Description:** Create the three required config files in `configs/` that the daemon, tmux, and agents depend on.
- **Implementation:**
    - `configs/agents.toml`: Agent Pool registration with role permissions, bash blacklists, and network allowlists per Contract 3.6. Include default `scout`, `coder`, and `deployer` roles.
    - `configs/tmux.conf`: tmux init config with `remain-on-exit on` per ARCH §6.1, bound to socket `metamach-tmux`.
    - `configs/global_rules.md`: Factory-wide developer rules loaded into Agent System Prompts on onboarding.
- **UAT:** `configs/agents.toml` is valid TOML and parseable by the daemon; `configs/tmux.conf` sets `remain-on-exit on`; `configs/global_rules.md` is non-empty Markdown.

#### Task 1.4: CI/CD Pipeline (Check-in Unit 4)
- **Description:** Create `.github/workflows/ci.yml` with native PG service, tmux, and full test suite.
- **Implementation:**
    - CI runs on `ubuntu-24.04` with `postgres:16` service container (health check: `pg_isready`).
    - Steps: `apt-get install -y tmux postgresql-client`, `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo build --release --locked`, `cargo test --workspace`.
    - PG-gated integration tests (UTC-05-02/04/04b, UTC-03-01/03/05) run with `-- --ignored` as a **blocking** gate (they pass on CI in parallel). SSH-credential-gated tests, when added, get a separate `continue-on-error: true` step so an absent SSH key doesn't break the gate.
    - Cache cargo registry and target directory for faster builds.
- **UAT:** Push to `main` triggers CI; all gates pass (fmt, clippy, test, PG-gated tests).

## Milestone 2: Twin-Process UDS Communication & Scheduling Brain (Daemon Core)

> **Timebox:** ~3 weeks

- **Goal:** Implement the resident background daemon `janus-daemon`, establish UDS socket highway between the twin processes, achieve lazy-start self-healing and singleton lock.

- **Check-in-able directory structure:**
    `janus/src/bin/janus_daemon.rs`, `janus/src/absurd/`

### Tasks

#### Task 2.1: `janus-daemon` Resident Background Service & UDS Handshake (Check-in Unit 3)
- **Description:** Implement `janus_daemon.rs` and manage the `janus.sock` physical socket.
- **Implementation:**
    - Daemon binds UDS listener at `~/.local/state/.../janus.sock` on startup.
    - **Singleton File Lock (PID Lock):** Write current process PID to `~/.local/state/.../janus.pid`. If a second launch detects that file and the PID inside corresponds to a still-alive `janus-daemon` process, safely exit to prevent duplicate UDS binding (stale PID detection: if the PID is not alive, overwrite and start).
    - When UDS receives a request, the Daemon queries Absurd Postgres's `blueprints` table and returns all `status = 'ACTIVE'` real blueprint lists to the client (`herdr-janus`)-no longer using Mock data (M1 already has the table; data can come from migration seeds or M4's `janus onboard`).

#### Task 2.2: Shadow Client UDS Reconciliation & "Lazy-Start" (Check-in Unit 4)
- **Description:** Refactor `herdr_janus.rs` to connect to the Daemon for data exchange with seamless self-healing startup.
- **Implementation:**
    - When the Factory Director presses `prefix+j` to wake the client, the shadow client first probes `janus.sock`.
    - If probe fails, the shadow client silently launches `janus-daemon` in the background using `std::process::Command::spawn()` with detach (`setsid` to detach from controlling terminal, standard streams redirected to `/dev/null`, see Feature-Spec §2.1); once ready, establish UDS connection and dynamically fetch product data to render the dashboard.
- **UAT:** Manually kill `janus-daemon`; directly press `prefix+j` inside Herdr. Popup should appear without delay and a new `janus.pid` should auto-generate in the background.

#### Task 2.3: Workflow Progress Query & Dashboard Rendering (Check-in Unit 4b)
- **Description:** Implement the read-only `progress` query primitive on the Daemon side; add "Progress" view to the Popup and `janus status` CLI on the `herdr-janus` side.
- **Implementation:**
    - Daemon exposes `progress` UDS query: uses a read-only transaction to aggregate `absurd_tasks JOIN absurd_steps` (filtering non-terminal), overlaid with tmux `tmux has-session` liveness signals; returns the Contract 3.3-defined payload. This query uses an independent read-only channel, never contending with workflow write transactions.
    - `herdr-janus` Popup adds Progress view; `Tab` toggles between "Dispatch / Progress" views; Progress view polls `progress` at 1–2s cadence and renders using `ratatui` tables grouped by blueprint. `SUSPENDED` steps highlighted with `[A]ttach` / `[R]esume` entries.
    - Implement `janus status [--blueprint <name>] [--json]` CLI reusing the same `progress` primitive, outputting plain-text/JSON snapshots.
- **UAT:** After dispatching a multi-step workflow, switch to Progress view; step states should progress with real execution within 2s (`PENDING -> RUNNING -> COMPLETED`); artificially trigger `SUSPENDED`-that row highlights within 1s. In an SSH environment, `janus status` should output an in-flight task snapshot consistent with the dashboard.

> Note: This task lands the query and rendering skeleton in M2; the real Task/Step data it reads becomes progressively richer through M3 (janush step execution) and M4 (cross-host workflows). M2 can use migration seed tasks to validate rendering. **Multi-DB fan-out:** because `absurd_tasks`/`absurd_steps` live in per-blueprint DBs, the `progress` query must iterate each `metamach_blueprint_<name>` and union in Rust (a single `JOIN` cannot span databases). The Daemon sets `RUNNING` after returning the `ALLOW` verdict (Contract 3.4) so the dashboard can render the `STARTING -> RUNNING` transition.

#### Task 2.4: tmux Internalization - `janus::tmux` Native Module (Check-in Unit 4c)
- **Description:** Per 0.3.0 §2.4, migrate herdr-tether's core tmux session engine (~3,500 LOC: `DurableBackend` trait + `LifecycleService` + cold-start integration) into the native `janus::tmux` Rust module inside `janus-daemon`. The external `herdr-tether` binary is **deprecated and no longer fetched**. New dependency: `thiserror`. Effort ~2 weeks (+~2,600 LOC tests).
- **Implementation:**
    - Port session create/attach/kill against an isolated tmux server `tmux -L metamach-tmux`, with **per-session** `remain-on-exit on` (no `-g`; never pollutes the director's personal tmux).
    - Expose the `janus tmux open|attach|list` subcommand surface (define the subcommand table in ARCH §3 / Feature-Spec and cross-reference from Deployment-Spec §6.2).
    - In-process signal linkage to Tool Guard (<1ms) replaces the prior ~5-15ms external UDS IPC path.
- **UAT:** `janus tmux open --command "sleep 100"` launches a session in `tmux -L metamach-tmux`; force-close the foreground view; `tmux -L metamach-tmux list-sessions` still shows `tmux-janus-*` alive; `janus tmux attach` restores the scene in milliseconds. No `herdr-tether` binary exists in the build.
- **Note:** Landing `janus::tmux` in M2 resolves the prior M3 dependency inversion - `janush` (M3) can now be tested inside real tmux panes.

#### Task 2.5: OpenWiki External Dependency Fetch & RAG Query Verification (Check-in Unit 4d)
- **Description:** Integrate the external dependency OpenWiki (https://github.com/langchain-ai/openwiki) into the build flow, and connect the Daemon -> OpenWiki RAG query chain-pre-positioning for M4 Offboard write-back and Agent onboarding retrieval.
- **Implementation:**
    - `make bootstrap` adds `openwiki` target: fetch/build OpenWiki engine; configure index scopes for `blueprints/<name>/openwiki/` and global `configs/global_rules.md`.
    - Daemon implements `openwiki_query` bypass: when an Agent encounters a code blind spot and initiates RAG retrieval, Daemon preferentially hits the Absurd Postgres-level cache (Git-SHA dedup); on miss, queries the OpenWiki engine.
    - Verify index scope isolation: different blueprints' local knowledge graphs do not cross-contaminate.
- **UAT:** After indexing a blueprint's `openwiki/`, `openwiki_query` returns precise AST snippets; cross-blueprint query results do not leak. After Offboard writes back `production_report.md`, re-indexing can retrieve it (closing the loop with M4 Tasks 4.2/4.3).

## Milestone 3: Physical Sandbox, Proxy Shell & Security Guard (Shield Layer)

> **Timebox:** ~2 weeks

- **Goal:** Push the security gate from outside the Herdr process down into the physical boundary of tmux, implementing `janush` synchronous interception and Tool Guard allowlist filtering.

- **Check-in-able directory structure:**
    `janus/src/tool_guard/`, `janus/target/release/janush` (independent compilation target)

### Tasks

#### Task 3.1: Compile Proxy Shell `janush` (Check-in Unit 5)
- **Description:** Implement a lightweight system command synchronous proxy in Rust.
- **Implementation:**
    - `janush` itself is a minimal CLI program. When awakened, it does not execute the command; instead, it throws the current `argv` array to `janus-daemon` via UDS.
    - `janush` remains in a blocked state until it receives an `ALLOW` or `REWRITE` verdict from `janus-daemon`, then delivers to the real host `/bin/sh` for execution.

#### Task 3.2: Tool Guard In-Memory Rule Engine & Teams Approval Suspension (Check-in Unit 6)
- **Description:** Implement the security guard decision matrix and non-destructive suspension in the Daemon.
- **Implementation:**
    - When the Daemon receives a command thrown by `janush` (e.g., unauthorized network download, high-risk physical deletion, live financial order execution), it checks against `configs/agents.toml` qualification restrictions.
    - **Non-Destructive Suspension:** If the command is an unauthorized high-risk instruction, mark the state as `SUSPENDED`. The Daemon does not kill the underlying PTY, prevents `janush` from dispatching downward, and simultaneously sends a card with a `[Resume]` button via Telegram (primary) / Teams (secondary) webhook.
    - **Fail-Closed 30s Timeout (0.3.0 §2.1):** `janush` synchronously blocks on UDS reconciliation with a 30s default threshold. If the Daemon is unreachable or the round-trip exceeds 30s, `janush` returns an error to the Agent and **refuses execution** (never lets the command through); the PTY survives via `remain-on-exit` for director troubleshooting. SIGSTOP/SIGCONT alternatives are rejected.
- **UAT:** In an Agent pane, first create a sentinel: `mkdir -p /tmp/metamach-test-guard-$(uuidgen) && echo s > /tmp/metamach-test-guard-$(uuidgen)/sentinel`, then force-execute a blacklisted `rm -rf /tmp/metamach-test-guard-$(uuidgen)`; the terminal should instantly synchronously suspend (Remain-on-Exit), the sentinel survives, and the phone receives the approval card.

## Milestone 4: Cross-Host Durability, Cold Self-Heal & Offboard Archive (Advanced & Prune)

> **Timebox:** ~4 weeks
> **Status (2026-07-21):** Partial. Task 4.2 (Offboard) + Task 4.3 (Onboard) are implemented and tested. Task 4.1 is **fully implemented** (Phase 0a absurd adapter, Phase 0b workflow engine, Phase 1 cold-start re-exec/retry, HITL resume loop, Phase 2 cross-host SSH transport via TmuxFactory + reverse tunnel). Task 4.4 (`target_sha` enforcement) is **implemented** — the engine detects HEAD advancement mid-step, marks the step SUSPENDED with CONCURRENCY_RACE_ALERT, and the retry loop re-runs against the new HEAD.

- **Goal:** Grid-connect `janus::tmux` cross-host, implement cold-start zero-state self-heal + SQLite Log Replay (abandon tmux-resurrect), and Offboard trace purge + audit archive.

- **Check-in-able directory structure:**
    `workflows/`, `blueprints/` (product recipes), `janus/src/bin/janus_daemon.rs` (supplement Onboard / Offboard / audit-archive / `target_sha` submodules), `janus/src/tmux/` (internalized `janus::tmux`)

### Tasks

#### Task 4.1: Cross-Host `janus::tmux` Driver, Cold-Start Self-Heal & SQLite Log Replay (Check-in Unit 7) - ✅ DONE (Phase 0a absurd adapter, Phase 0b workflow engine + Dispatch, Phase 1 cold-start re-exec/retry, HITL resume loop, Phase 2 cross-host SSH transport via TmuxFactory + reverse tunnel; only Task 4.4 target_sha enforcement remains deferred)
- **Description:** Drive cross-host SOP sessions through the internalized `janus::tmux`, implement cold-start zero-state self-heal (no tmux-resurrect), and the degraded-mode SQLite fallback + Log Replay per 0.3.0 §1.2/§2.4.
- **Implementation:**
    - When a Workflow Step declares a remote compilation server, the Daemon drives the internal `janus::tmux` module (not an external binary) to inject the payload env via SSH into a `remain-on-exit` remote session.
    - **Cold-Start Reconciliation:** on Daemon start, scan each `metamach_blueprint_<name>` DB for non-terminal tasks; read the last `COMPLETED` `result_cache`, assign a fresh tmux Session UUID, and resume at the breakpoint.
    - **SQLite Log Replay:** while PG is unreachable, step transitions write to `${HERDR_PLUGIN_STATE_DIR}/fallback.db` (ring buffer, tagged with `blueprint_name`); on PG recovery, the Daemon replays and merges events into the correct per-blueprint DB with zero loss.
- **UAT:** During a heavy compile, stop native PG (`pg_ctl stop -D ~/.metamach/db/`, NOT `docker compose stop`) and kill the Daemon; dispatch a step during the outage (writes to `fallback.db`); restart PG + Daemon; the Daemon reconstructs from the last `COMPLETED` checkpoint within 0.5s and replays `fallback.db` with no state loss.

#### Task 4.2: Offboard Trace Purge & Audit Archive (Check-in Units 8a/8b/8c)
- **Description:** Implement `janus offboard --blueprint <name>` per 0.3.0 §1.3: LLM-smelt `production_report.md`, then `DELETE` `result_cache` and **archive** traces/interception/sign-off logs to the global `absurd_audit_log` (NOT `DROP DATABASE`, NOT a `melt`/`VACUUM`-only step). Split into three check-in units per Project-Plan-Review §2.3.
- **Implementation:**
    - **8a - Audit Archive + DELETE (F2):** Daemon-orchestrated multi-DB sequence (a single stored proc cannot span DBs): from `metamach_blueprint_<name>`, archive Step traces + Tool Guard interception logs + three-party sign-off records into the global `absurd_audit_log` (catalog DB, `task_id UUID`), then purge operational data via absurd's generic `cleanup_tasks` (absurd provides **no** `melt_blueprint_data` / `offboard_blueprint_data` proc - that was an inaccurate ARCH-0.2.0 assumption). The per-blueprint database is **retained** (not dropped). MetaMach's `offboard_blueprint_data` is a Daemon function, not an absurd proc.
    - **8b - LLM Smelt:** per `configs/offboard.toml` (Contract TBD: endpoint, `api_key_env`, model, `max_input_tokens`), summarize the archived trace into high-density Markdown -> `blueprints/<name>/openwiki/production_report.md`. 120s timeout; on failure write `production_report.raw.json` fallback. Async; Offboard returns immediately.
    - **8c - Git Commit:** additively commit `production_report.md` to the blueprint Git repo (no `--amend`, no history rewrite per ARCH §2.2C design decision); push with configured credentials.
- **UAT:** For a product with heavy logs, `janus offboard --blueprint gatemetric` => (a) `absurd_audit_log` gains the archived rows; (b) `SELECT datname FROM pg_database WHERE datname='metamach_blueprint_gatemetric'` still returns a row (DB not dropped); (c) `production_report.md` is additively committed; (d) per-blueprint `result_cache` JSONs are gone.

#### Task 4.3: Blueprint Onboard & Multi-DB Tenant Registration (Check-in Unit 8d)
- **Description:** Implement `janus onboard --blueprint <name>` per 0.3.0 §1.4 (Multi-DB baseline), symmetric with Offboard.
- **Implementation:**
    - Read/validate `blueprints/<name>/janus.toml` + `workflows/<default_workflow>.toml` existence; validate blueprint name (charset + length <= 44 bytes, to fit `metamach_blueprint_<name>` within the Postgres 63-byte identifier limit).
    - Pre-ignition checks: catalog DB reachable, `janus::tmux`/tmux ready; best-effort SSH probe for cross-host blueprints (unreachable = `WARN` only).
    - **Multi-DB tenant registration (F1):** `CREATE DATABASE metamach_blueprint_<name>` (catch SQLSTATE 42P04 => idempotent success; standard PG has no `CREATE DATABASE IF NOT EXISTS`), then `absurdctl init` (installs the absurd engine into the DB) + run `002_blueprint.sql` (the `metamach_step_meta` overlay), and `INSERT ... ON CONFLICT (name) DO UPDATE` a `status='ACTIVE'` row in the **global catalog** `blueprints` table; also `absurd.create_queue('<name>.<workflow>')` per bound workflow. Because `CREATE DATABASE` cannot run inside a transaction, wrap post-create steps in compensation: on any failure after the DB is created, `DROP DATABASE` to avoid a half-activated state.
    - **Knowledge inheritance:** index `blueprints/<name>/openwiki/`; if a prior `production_report.md` exists, inject key patterns as `## Previous Incidents` few-shot into the Agent System Prompt.
    - Broadcast `blueprint_registered` UDS event; Popup dispatch menu refreshes.
- **UAT:** On a clean workshop, `janus onboard --blueprint joyrobots` => `SELECT datname FROM pg_database WHERE datname='metamach_blueprint_joyrobots'` returns a row; the catalog `blueprints` table has one `ACTIVE` row; the Popup menu shows it; repeated Onboard is idempotent (42P04 caught); re-Onboard of an Offboarded blueprint recycles `production_report.md`.

#### Task 4.4: `target_sha` Optimistic Locking Enforcement (Check-in Unit 8e) - ✅ DONE (HEAD advancement detected mid-step in run_steps; stale result discarded, step marked SUSPENDED with CONCURRENCY_RACE_ALERT; retry loop re-runs against new HEAD on absurd's next claim)
- **Description:** Enforce Git-SHA optimistic locking on remote step reports per ARCH §6.5 / Feature-Spec, closing the race where a slow remote report overwrites locally-evolved code. (Schema/migration is preparatory from M1 Task 1.1; this task lands enforcement.)
- **Implementation:**
    - Define the Daemon -> `janus::tmux` **dispatch payload** and the **remote step-report payload** contracts (each carries `dispatch_sha`/`target_sha`, `execution_id`, `exit_code`, `stdout_tail`) - currently absent from Contracts 3.2/3.4.
    - On dispatch, pin `HEAD` into `metamach_step_meta.target_sha` (all-zeros sentinel = non-git blueprint, skip the lock); echo `dispatch_sha` in the report.
    - On report return, compare `report.dispatch_sha == current HEAD`; mismatch => discard the stale report, mark the step `SUSPENDED`, emit `CONCURRENCY_RACE_ALERT` via the HITL channel, and auto-reschedule by writing a **new** `absurd_tasks` row against the new `HEAD` (the `UPDATE ... WHERE target_sha = $4` guard zeroes out for stale pre-reschedule reports).
- **UAT:** Dispatch a step with `target_sha=X`; mutate the underlying git ref to `Y`; submit a report with stale `dispatch_sha=X`; the Daemon rejects it (SHA-mismatch), marks `SUSPENDED`, fires `CONCURRENCY_RACE_ALERT`, and a fresh dispatch against `Y` succeeds.
- **Note:** ARCH §6.5 and Feature-Spec cite "Task 2.4" for this enforcement; that cross-reference is stale (Task 2.4 is now tmux Internalization) and should point here (Task 4.4).

## Milestone 5: Integration Testing & Release Gate

> **Timebox:** ~2 weeks

- **Goal:** Run full integration test suite across all modules, verify cross-module contracts, and prepare for release.

- **Check-in-able directory structure:**
    `tests/` (integration tests per crate), `docs/CH/` (audit artifacts)

### Tasks

#### Task 5.1: Integration Test Suite (Check-in Unit 9)
- **Description:** Implement the integration test suite covering all test cases from `docs/Test-Spec.md`.
- **Implementation:**
    - Test Suites 2.1–2.9: UTC-01-xx through UTC-09-xx, covering daemon, sandbox, tmux, HITL, lifecycle, dashboard, benchmarks, degraded mode, and tmux module.
    - SSH-gated tests (UTC-09-04) use `#[ignore = "requires SSH credentials"]`.
    - Benchmark harness (UTC-07-xx) uses `criterion`; deferred to P2, not a release gate.
- **UAT:** `cargo test --workspace` passes all non-ignored tests; `cargo test --workspace -- --ignored` passes all PG-gated tests (blocking in CI); SSH-gated tests, when added, skip gracefully via a separate `continue-on-error` step.

#### Task 5.2: Cross-Module Contract Verification (Check-in Unit 10)
- **Description:** Verify all UDS contracts (3.2–3.5), schema contracts (3.1/3.1b), and lifecycle contracts (Onboard/Offboard) hold across module boundaries.
- **Implementation:**
    - janush ↔ janus-daemon: Contract 3.2/3.4 payload round-trip with all verdict types.
    - janus::tmux ↔ janus-daemon: Contract 3.5 dispatch/report payload with target_sha locking.
    - herdr-janus ↔ janus-daemon: Contract 3.3 progress query payload.
    - Multi-DB: Onboard creates per-blueprint DB, Offboard archives + DELETEs, no cross-contamination.
- **UAT:** All contract-level integration tests pass; no cross-module deserialization errors.

#### Task 5.3: Release Preparation (Check-in Unit 11)
- **Description:** Finalize version, update CHANGELOG, tag release, and verify all check-in gates.
- **Implementation:**
    - Bump version in `janus/Cargo.toml` and `herdr-plugin.toml` to `0.3.0`.
    - Generate `CHANGELOG.md` from conventional commits.
    - GPG-sign the release tag: `git tag -s v0.3.0 -m "MetaMach 0.3.0 — de-containerized, Multi-DB, internalized tmux"`.
    - Final check-in gate sweep: all 5 hygiene gates pass.
- **UAT:** `git tag -v v0.3.0` verifies the GPG signature; `cargo build --release --locked` produces clean binaries; `make bootstrap` completes end-to-end.

## Check-in Gates

To keep the repository history clean, every Check-in commit must pass the following CI/CD verification:

1. `cargo fmt --all -- --check` (100% format alignment)
2. `cargo clippy --all-targets -- -D warnings` (100% static safety detection, zero warnings)
3. `cargo test --workspace` (100% local fallback DB & transaction unit tests passing)
4. **Regression:** all UAT validations from prior milestones still pass (no M4 regression breaks M1-M3).
5. **0.3.0 baseline hygiene:** the commit introduces no `docker-compose.yml`, no `docker compose` invocations, no `herdr-tether` external binary fetch, and no `melt_blueprint_data`/`DROP DATABASE` Offboard path.
6. Inspect committed files; strictly prohibit accidentally committing any plaintext keys, `.env` files, or local `janus.sock`/`~/.metamach/db/` to Git.
