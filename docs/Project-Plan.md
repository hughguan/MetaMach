# MetaMach 1.0 — Project Plan

> Milestone roadmap (M0–M4) of independently check-in-able, physically network-able factory units.

This plan decomposes MetaMach 1.0's R&D and grid-connection process into **5 core milestone phases (M0–M4)**. Each milestone is sliced by "compilable, independently commit/check-in-able, 100% Immutable-vs-Mutable compliant" physical features or functional modules (Feature Units), with explicit physical verification methods to ensure the Richmond Hill workshop's grid-connection is seamless and steady.

## Milestone Timeline

```
[Milestone 0] ──> [Milestone 1] ──> [Milestone 2] ──> [Milestone 3] ──> [Milestone 4]
 Herdr 0.7.3 Validate    Infra & Shell       Daemon Core         Shield Layer        Lifecycle & Self-Heal
```

> **M0 is a prerequisite gate:** All Popup/plugin tasks from M1 onward depend on the Herdr 0.7.3 plugin SDK being available. M0 must first validate this external contract; otherwise M1 Task 1.2 is blocked.

## Milestone 0: Herdr 0.7.3 Plugin Contract Validation (External SDK Validation) - ✅ VALIDATED 2026-07-15

- **Status:** Complete. Validated against installed Herdr 0.7.3; contract documented in `docs/herdr-v1-contract.md` (version-controlled English source; `docs/CH/` is gitignored). PoC at `spike/herdr-hello-plugin/` (gitignored). **M1 Task 1.2 (Popup) green-lit.** Key corrections: popup placement is `overlay` (not `popup`); manifest has no `width`/`height`; Herdr injects `HERDR_PLUGIN_ROOT/CONFIG_DIR/STATE_DIR` + `HERDR_SOCKET_PATH`.

- **Goal:** Before investing any MetaMach self-developed code, first verify that the Herdr 0.7.3 plugin SDK is genuinely usable, eliminating M1's largest unknown external dependency. M0 produces no MetaMach business code—only "contract validation evidence + minimal PoC plugin + Herdr 0.7.3 API interface memo."

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
- **UAT:** PoC plugin pops up on `prefix+j`, keyboard focus does not escape, `Esc` closes, UDS communication round-trip succeeds. If any item fails, M1 does not start—align contract with Herdr 0.7.3 upstream first.

## Milestone 1: Infrastructure Grid-Connection & Shadow Shell (Immutable & Base)

- **Goal:** Establish Immutable/Mutable directory separation, start Absurd Postgres container, get the lightweight shadow client Popup rendering.

- **Check-in-able directory structure:**
    `janus/herdr-plugin.toml`, `janus/src/bin/herdr_janus.rs`, `docker-compose.yml`, `Makefile`

### Tasks

#### Task 1.1: Absurd Postgres Container & Migrations Initialization (Check-in Unit 1)
- **Description:** Write and commit `docker-compose.yml` and `janus/migrations/`.
- **Implementation:**
    - Create `metamach_db` in Postgres; write `001_init_absurd.sql` initializing table structure (`blueprints`, `absurd_tasks`, `absurd_steps`).
    - Configure the container to auto-mount and execute migration scripts on startup.
- **UAT:** Run `docker compose up -d`; inside the container execute `\dt` to see all initialized database physical tables.

#### Task 1.2: Shadow Shell Popup & TUI Rendering (Check-in Unit 2)
- **Description:** Write `herdr-plugin.toml` and implement `herdr_janus.rs` shadow client.
- **Implementation:**
    - Declare `[[panes]] id = "dispatcher" placement = "overlay" command = ["herdr-janus"]` in `herdr-plugin.toml` (no `width`/`height` - Herdr manages overlay sizing; see `docs/herdr-v1-contract.md`).
    - Use `ratatui` in `herdr_janus.rs` to render a static "production dispatch dashboard" interactive interface. Focus auto-locked; exit on `Esc`.
- **UAT:** Execute `herdr plugin link` to mount the plugin; press `prefix+j` inside Herdr; an 80%-width floating Popup smoothly pops up at screen center.

## Milestone 2: Twin-Process UDS Communication & Scheduling Brain (Daemon Core)

- **Goal:** Implement the resident background daemon `janus-daemon`, establish UDS socket highway between the twin processes, achieve lazy-start self-healing and singleton lock.

- **Check-in-able directory structure:**
    `janus/src/bin/janus_daemon.rs`, `janus/src/absurd/`

### Tasks

#### Task 2.1: `janus-daemon` Resident Background Service & UDS Handshake (Check-in Unit 3)
- **Description:** Implement `janus_daemon.rs` and manage the `janus.sock` physical socket.
- **Implementation:**
    - Daemon binds UDS listener at `~/.local/state/.../janus.sock` on startup.
    - **Singleton File Lock (PID Lock):** Write current process PID to `~/.local/state/.../janus.pid`. If a second launch detects that file and the PID inside corresponds to a still-alive `janus-daemon` process, safely exit to prevent duplicate UDS binding (stale PID detection: if the PID is not alive, overwrite and start).
    - When UDS receives a request, the Daemon queries Absurd Postgres's `blueprints` table and returns all `status = 'ACTIVE'` real blueprint lists to the client (`herdr-janus`)—no longer using Mock data (M1 already has the table; data can come from migration seeds or M4's `janus onboard`).

#### Task 2.2: Shadow Client UDS Reconciliation & "Lazy-Start" (Check-in Unit 4)
- **Description:** Refactor `herdr_janus.rs` to connect to the Daemon for data exchange with seamless self-healing startup.
- **Implementation:**
    - When the Factory Director presses `prefix+j` to wake the client, the shadow client first probes `janus.sock`.
    - If probe fails, the shadow client silently launches `janus-daemon` in the background using `std::process::Command::spawn()` with detach (`setsid` to detach from controlling terminal, standard streams redirected to `/dev/null`, see Feature-Spec §2.1); once ready, establish UDS connection and dynamically fetch product data to render the dashboard.
- **UAT:** Manually kill `janus-daemon`; directly press `prefix+j` inside Herdr. Popup should appear without delay and a new `janus.pid` should auto-generate in the background.

#### Task 2.3: Workflow Progress Query & Dashboard Rendering (Check-in Unit 4b)
- **Description:** Implement the read-only `progress` query primitive on the Daemon side; add "Progress" view to the Popup and `janus status` CLI on the `herdr-janus` side.
- **Implementation:**
    - Daemon exposes `progress` UDS query: uses a read-only transaction to aggregate `absurd_tasks JOIN absurd_steps` (filtering non-terminal), overlaid with Tether `tmux has-session` liveness signals; returns the Contract 3.3-defined payload. This query uses an independent read-only channel, never contending with workflow write transactions.
    - `herdr-janus` Popup adds Progress view; `Tab` toggles between "Dispatch / Progress" views; Progress view polls `progress` at 1–2s cadence and renders using `ratatui` tables grouped by blueprint. `SUSPENDED` steps highlighted with `[A]ttach` / `[R]esume` entries.
    - Implement `janus status [--blueprint <name>] [--json]` CLI reusing the same `progress` primitive, outputting plain-text/JSON snapshots.
- **UAT:** After dispatching a multi-step workflow, switch to Progress view; step states should progress with real execution within 2s (`PENDING -> RUNNING -> COMPLETED`); artificially trigger `SUSPENDED`—that row highlights within 1s. In an SSH environment, `janus status` should output an in-flight task snapshot consistent with the dashboard.

> Note: This task lands the query and rendering skeleton in M2; the real Task/Step data it reads becomes progressively richer through M3 (janus-sh step execution) and M4 (cross-host workflows). M2 can use migration seed tasks to validate rendering.

#### Task 2.4: Tether Engine External Dependency Fetch & Local Session Verification (Check-in Unit 4c)
- **Description:** Integrate the external dependency `herdr-tether` (https://github.com/moneycaringcoder/herdr-tether) into the `make bootstrap` fetch/build flow, and validate that local tmux session primitives are available—pre-positioning for M3 (janus-sh testing inside Tether panes) and M4 (cross-host).
- **Implementation:**
    - Add `tether` target in `make bootstrap`: fetch `herdr-tether` source via git submodule or cargo git dependency and `cargo build --release`; install the artifact to `${HERDR_PLUGIN_ROOT}/bin/herdr-tether`.
    - Verify `herdr-tether open --command "sleep 100"` creates a persistent tmux session inside an independent tmux server (`tmux -L metamach-tether`) with per-session `remain-on-exit on` (no `-g` global flag; does not pollute the director's personal tmux).
    - Verify `herdr-tether attach` can re-attach in milliseconds with scene preserved.
- **UAT:** After `make bootstrap`, `herdr-tether open --command "sleep 100"` launches a session; force-close the foreground view; `tmux list-sessions` still shows `tether-janus-*` session alive; `herdr-tether attach` restores the scene in milliseconds.

#### Task 2.5: OpenWiki External Dependency Fetch & RAG Query Verification (Check-in Unit 4d)
- **Description:** Integrate the external dependency OpenWiki (https://github.com/langchain-ai/openwiki) into the build flow, and connect the Daemon → OpenWiki RAG query chain—pre-positioning for M4 Offboard write-back and Agent onboarding retrieval.
- **Implementation:**
    - `make bootstrap` adds `openwiki` target: fetch/build OpenWiki engine; configure index scopes for `blueprints/<name>/openwiki/` and global `configs/global_rules.md`.
    - Daemon implements `openwiki_query` bypass: when an Agent encounters a code blind spot and initiates RAG retrieval, Daemon preferentially hits the Absurd Postgres-level cache (Git-SHA dedup); on miss, queries the OpenWiki engine.
    - Verify index scope isolation: different blueprints' local knowledge graphs do not cross-contaminate.
- **UAT:** After indexing a blueprint's `openwiki/`, `openwiki_query` returns precise AST snippets; cross-blueprint query results do not leak. After Offboard writes back `production_report.md`, re-indexing can retrieve it (closing the loop with M4 Tasks 4.2/4.3).

## Milestone 3: Physical Sandbox, Proxy Shell & Security Guard (Shield Layer)

- **Goal:** Push the security gate from outside the Herdr process down into the physical boundary of tmux, implementing `janus-sh` synchronous interception and Tool Guard allowlist filtering.

- **Check-in-able directory structure:**
    `janus/src/tool_guard/`, `janus/target/release/janus-sh` (independent compilation target)

### Tasks

#### Task 3.1: Compile Proxy Shell `janus-sh` (Check-in Unit 5)
- **Description:** Implement a lightweight system command synchronous proxy in Rust.
- **Implementation:**
    - `janus-sh` itself is a minimal CLI program. When awakened, it does not execute the command; instead, it throws the current `argv` array to `janus-daemon` via UDS.
    - `janus-sh` remains in a blocked state until it receives an `ALLOW` or `REWRITE` verdict from `janus-daemon`, then delivers to the real host `/bin/sh` for execution.

#### Task 3.2: Tool Guard In-Memory Rule Engine & Teams Approval Suspension (Check-in Unit 6)
- **Description:** Implement the security guard decision matrix and non-destructive suspension in the Daemon.
- **Implementation:**
    - When the Daemon receives a command thrown by `janus-sh` (e.g., unauthorized network download, high-risk physical deletion, live financial order execution), it checks against `configs/agents.toml` qualification restrictions.
    - **Non-Destructive Suspension:** If the command is an unauthorized high-risk instruction, mark the state as `SUSPENDED`. The Daemon does not kill the underlying PTY, prevents `janus-sh` from dispatching downward, and simultaneously sends a card with a `[Resume]` button via Telegram (primary) / Teams (secondary) webhook.
- **UAT:** In an Agent pane, first create a sentinel: `mkdir -p /tmp/metamach-test-guard-$(uuidgen) && echo s > /tmp/metamach-test-guard-$(uuidgen)/sentinel`, then force-execute a blacklisted `rm -rf /tmp/metamach-test-guard-$(uuidgen)`; the terminal should instantly synchronously suspend (Remain-on-Exit), the sentinel survives, and the phone receives the approval card.

## Milestone 4: Cross-Host Durability, Cold Self-Heal & Offboard Smelting (Advanced & Prune)

- **Goal:** Grid-connect Tether cross-host tmux, implement cold-start zero-state self-healing (abandon tmux-resurrect), and Offboard degradation smelting.

- **Check-in-able directory structure:**
    `workflows/`, `blueprints/` (product recipes), `janus/src/bin/janus_daemon.rs` (supplement GC / Onboard / Offboard submodules)

### Tasks

#### Task 4.1: Cross-Host Tether tmux Driver & Cold-Start Self-Healing (Check-in Unit 7)
- **Description:** Implement cross-host SOP session driving and power-loss restart self-healing.
- **Implementation:**
    - When a Workflow executes to the next Step, if a remote compilation server is declared, the Daemon automatically calls local `herdr-tether` to inject the Payload environment variables into the remote via SSH.
    - **Cold-Start Reconciliation:** After Daemon starts, it scans Absurd Postgres. If there are unfinished tasks (`RUNNING`), it directly reads the last `COMPLETED` `result_cache` JSON. It assigns a new Tether Session UUID and re-runs the task in the background, seamlessly picking up at the breakpoint.
- **UAT:** During a heavy compilation, artificially `docker compose stop` Postgres and kill the Daemon. After restart, the Daemon should reconstruct the scene from the database Step cache within 0.5s and seamlessly resume.

#### Task 4.2: Offboard Degradation Smelter (Melt DB Cache) (Check-in Unit 8)
- **Description:** Implement the `janus offboard` command.
- **Implementation:**
    - On Offboard, auto-scan the database, package that Blueprint's historical Step errors and Tool Guard interception logs.
    - Call the configured LLM to summarize them into high-density Markdown, writing to `./blueprints/<name>/openwiki/production_report.md`.
    - **PG Auto-Degradation (Pruning):** Call stored procedure `melt_blueprint_data` to completely physically wipe the corresponding Steps' `result_cache` large JSON from the primary database, preserving only metadata statistics for audit, achieving database anti-bloat compaction.
- **UAT:** For a product that has accumulated significant compilation logs, execute `janus offboard --blueprint gatemetric`; locally successfully commit and push a `production_report.md` to the Git remote; the PG database physical volume undergoes a cliff-like contraction.

#### Task 4.3: Blueprint Onboard & Tenant Registration (Check-in Unit 8b)
- **Description:** Implement the `janus onboard --blueprint <name>` command, completing the Onboard-side lifecycle closed loop symmetric with Offboard.
- **Implementation:**
    - Read and validate `blueprints/<name>/janus.toml` (required fields + `workflows/<default_workflow>.toml` existence); clear error on validation failure with no database write.
    - Execute pre-ignition self-checks: Absurd Postgres reachable, tmux ready; for cross-host blueprints, best-effort SSH connectivity probe against `[remote].host` (unreachable only `WARN`).
    - **Idempotent Tenant Registration:** `INSERT … ON CONFLICT (name) DO UPDATE` write `blueprints` row (`status='ACTIVE'`, `config`, `openwiki_scope`, `remote_host`, `onboarded_at`). Re-onboarding an already `OFFBOARDED` blueprint reactivates it.
    - **Knowledge Graph Loading & Experience Inheritance:** Index `blueprints/<name>/openwiki/`; if prior `production_report.md` exists, parse its structured blocks and inject as `## Previous Incidents` few-shot into that blueprint's Agent System Prompt template.
    - After onboarding is ready, broadcast `blueprint_registered` event via UDS; Popup dispatch menu instantly refreshes.
- **UAT:** On a clean workshop with zero product lines, execute `janus onboard --blueprint joyrobots`; the `blueprints` table gains one `ACTIVE` row and the Popup menu instantly shows the product; repeated execution has no side effect (idempotent). For an already Offboarded blueprint, re-Onboard and verify its `production_report.md` is recycled into the next-generation Agent's System Prompt.

## Check-in Gates

To keep MetaMach 1.0's repository history clean, every Check-in commit must pass the following CI/CD verification:

1. `cargo fmt --all -- --check` (100% format alignment)
2. `cargo clippy --all-targets -- -D warnings` (100% static safety detection, zero warnings)
3. `cargo test --workspace` (100% local fallback DB & transaction unit tests passing)
4. Inspect committed files; strictly prohibit accidentally committing any plaintext keys, `.env` files, or local `janus.sock` to Git.
