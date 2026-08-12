# MetaMach Architecture Decision Records

> **Purpose:** This file captures the key architectural decisions across MetaMach's evolution from 0.1.0 through 0.6.0. Each ADR documents a decision: context, options considered, final choice, and rationale. This is the permanent record — once converged here, the delta files (`ARCH-0.2.0.md`, `ARCH-0.3.0.md`, `ARCH-0.4.0.md`) are archived to `docs/CH/` for backup.

---

## ADR-001: De-containerization — Host-Native Sandbox PG

| Field | Value |
|---|---|
| **Context** | 0.1.0 relied on Docker Compose to spin up a Postgres container. This introduced Docker as a hard dependency and added virtual NIC, container network bridge, and resident memory overhead. |
| **Options Considered** | (1) Keep Docker Compose, (2) Switch to host-native `initdb`/`pg_ctl`, (3) Use SQLite exclusively. |
| **Decision** | **Adopted** — Abolish `docker-compose.yml`; use host-native PG managed by `Makefile`. |
| **Rationale** | Eliminates Docker dependency; reduces deployment to "install PG, run `make bootstrap`"; removes ~100MB Docker overhead. Zero-dependency distribution. |
| **Status** | ✅ Implemented in 0.3.0 (commits `313daa8`, `b9d85a6`) |

---

## ADR-002: `~/.metamach/db/` Global Independent Path

| Field | Value |
|---|---|
| **Context** | PG data paths proposed in 0.1.0 were tied to Herdr plugin state dir — PG data could be accidentally erased on Herdr upgrade/uninstall. |
| **Options Considered** | (1) Herdr state dir (`~/.local/state/herdr/plugins/`), (2) RAM disk (`/dev/shm`), (3) Independent global path `~/.metamach/db/`. |
| **Decision** | **Adopted** — PG cluster lives at `~/.metamach/db/`, decoupled from Herdr lifecycle. |
| **Rationale** | Survives plugin upgrades, power-cycle restarts, and `make clean`. Herdr state dir stores only runtime artifacts (socket, PID, fallback DB). |
| **Status** | ✅ Implemented in 0.3.0 |

---

## ADR-003: One PG, Multi-DB Topology

| Field | Value |
|---|---|
| **Context** | Blueprints need database isolation to prevent cross-blueprint lock contention and connection pool fragmentation. |
| **Options Considered** | (1) One PG per blueprint (per-process), (2) Single-DB with `blueprint_id` partition key, (3) One PG instance, one logical DB per blueprint (`CREATE DATABASE`). |
| **Decision** | **Adopted** — Single physical PG, per-blueprint logical databases (`metamach_blueprint_<name>`). |
| **Rationale** | Independent connection pools per blueprint; no cross-blueprint lock contention; resource isolation (OOM in one blueprint doesn't others); avoids hundreds of MB memory from per-process PG instances. |
| **Status** | ✅ Implemented in 0.3.0 (migrations `001_catalog.sql`, `002_blueprint.sql`) |

---

## ADR-004: Retain SQLite Fallback Ring Buffer

| Field | Value |
|---|---|
| **Context** | 0.2.0 initially proposed dropping SQLite entirely in favor of PG-only. PG may crash due to OOM, disk exhaustion, or connection pool depletion. |
| **Options Considered** | (1) Drop SQLite completely, (2) Retain SQLite as degraded-mode ring buffer. |
| **Decision** | **Force-Retained** — SQLite fallback (Contract 3.8) keeps the workshop alive during PG outages. |
| **Rationale** | Without SQLite fallback, the interception proxy `janush` would deadlock the current Shell during PG outage, paralyzing the physical workshop on the spot. SQLite is not a PG replacement — it's a survival layer. |
| **Status** | ✅ Implemented in 0.3.0 (`janus/src/absurd/fallback.rs`) |

---

## ADR-005: DELETE + Audit Archive (Reject DROP DATABASE)

| Field | Value |
|---|---|
| **Context** | 0.2.0 proposed `DROP DATABASE metamach_blueprint_<name>` on Offboard for physical shredding. This destroys audit trail. |
| **Options Considered** | (1) `DROP DATABASE` (physical shred), (2) `DELETE` + `absurd_audit_log` archive. |
| **Decision** | **Rejected** DROP DATABASE. **Adopted** DELETE + incremental archive to `absurd_audit_log`. |
| **Rationale** | Even after a blueprint is offboarded, its weeks-long history of intercept triggers, sign-off timestamps, and step traces must be permanently archived for legal traceability. DROP DATABASE destroys this. DELETE reclaims TOAST space; audit log preserves non-repudiation trail. |
| **Status** | ✅ Implemented in 0.3.0 (`janus/src/lifecycle.rs`) |

---

## ADR-006: tmux Internalization

| Field | Value |
|---|---|
| **Context** | The `herdr-tether` external plugin (AGPL-3.0, 3★ on crates.io) managed tmux sessions. It had received zero updates since release, hadn't been forked, and used a JSON-file StateStore incompatible with MetaMach's Absurd PG architecture. Three external dependencies made every compile a supply-chain risk. |
| **Drivers** | (1) 🛑 Physical survival autonomy — can't depend on unmaintained plugin, (2) ⚡ Keep-alive — external plugin dies with frontend SIGHUP, (3) 📉 IPC latency — external UDS (5-15ms) vs in-process (<1ms), (4) 📦 Single-binary distribution. |
| **Decision** | **Adopted** — Internalize ~3,500 LOC into `janus::tmux` native module. |
| **Rationale** | Eliminates external dependency risk; in-process calls eliminate UDS latency; daemon-owned sessions survive frontend destruction; single binary distribution. ~16,000 LOC not ported (80% replaced by existing MetaMach implementations). |
| **Status** | ✅ Implemented in 0.3.0 (Phase 1: `janus::tmux` module, commits `2a162ee`/`beed8ef`) |

---

## ADR-007: Fail-Closed 30s Timeout Interception

| Field | Value |
|---|---|
| **Context** | If the daemon is unreachable, `janush` must not let commands through. SIGSTOP/SIGCONT was proposed as an alternative. |
| **Options Considered** | (1) SIGSTOP/SIGCONT (pause process), (2) Fail-closed sync timeout. |
| **Decision** | **Force-Retained** — existing Feature-Spec 2.2 design. 30s timeout = BLOCK. |
| **Rationale** | SIGSTOP/SIGCONT cannot be intercepted from outside the process group without root. Fail-closed is the only non-negotiable security boundary. Timeout ensures the terminal doesn't hang indefinitely. |
| **Status** | ✅ Verified in 0.3.0 (`tests/uds_contract.rs` UTC-02-06) |

---

## ADR-008: 16KB Flow Budget Dual Defense

| Field | Value |
|---|---|
| **Context** | Step stdout can grow unbounded, causing DB bloat and OOM. |
| **Options Considered** | (1) Unbounded capture, (2) Single 16KB cap in DB layer. |
| **Decision** | **Force-Retained** — dual defense: (a) `janush` in-memory streaming truncation, (b) Daemon authoritative pre-insert truncation + `[MetaMach Log Budget Exceeded]` tag. |
| **Rationale** | First line optimizes UDS transfer; second line is the final gate. Both target the same 16KB cap. |
| **Status** | ✅ Implemented in 0.3.0 (`janus/src/protocol.rs`, `janus/src/absurd/fallback.rs`) |

---

## ADR-009: Isolated tmux Server (`-L metamach-tmux`)

| Field | Value |
|---|---|
| **Context** | Without isolation, `janus::tmux` sessions pollute the host-global tmux server. |
| **Decision** | **Already Implemented** — dedicated tmux server `tmux -L metamach-tmux`. |
| **Rationale** | Never interferes with the Factory Director's personal tmux sessions. Sessions survive terminal close (no SIGHUP). Socket isolation prevents cross-blueprint session leaks. |
| **Status** | ✅ Implemented in 0.3.0 (`configs/tmux.conf`) |

---

## ADR-010: Cognitive Provider SPI (Contract 4.1)

| Field | Value |
|---|---|
| **Context** | 0.3.0 had no mechanism to inject blueprint-specific domain knowledge into the Tool Guard verdict path. OpenAI/RAG data either lived in-memory (heap bloat) or was not available at all. |
| **Options Considered** | (1) In-process AST parsing (heap bloat, OOM risk), (2) SQL-based (hot DB path, not a query engine), (3) SPI with external provider. |
| **Decision** | **Adopted** — Narrow `CognitiveProvider` trait with `validate_command` (advisory, 2s timeout) and `extract_knowledge` (offboard supplement). Opt-in per blueprint. |
| **Rationale** | Keeps daemon heap clean; providers are lazily started and terminated on Offboard. Advisory-only — timeout = pass-through (no false BLOCKs). |
| **Status** | ✅ Implemented in 0.4.0 (`janus/src/cognitive/`, `d1a62b9`) |

---

## ADR-011: codebase-memory-mcp (Contract 4.2)

| Field | Value |
|---|---|
| **Context** | AST/Tree-sitter symbol graph generation is CPU-intensive and OOM-prone. Doing it in-process would bloat the daemon. |
| **Options Considered** | (1) In-process Tree-sitter, (2) External MCP server, (3) No symbol indexing. |
| **Decision** | **Adopted** — Offload to external `codebase-memory-mcp` server via MCP stdio transport. |
| **Rationale** | Process isolation — a crash/OOM in the MCP process never touches the daemon. Lazy on-fault only (not polled during normal execution). Blueprint-scoped (no cross-blueprint symbol leaks). |
| **Status** | ✅ Implemented in 0.4.0 (`janus/src/cognitive/`, `d1a62b9`) |

---

## ADR-012: HITL Gateway (Contracts 4.3a–c)

| Field | Value |
|---|---|
| **Context** | 0.3.0's HITL was limited to Telegram inline keyboards and local TUI prompts. Network drops or external lag could freeze the local terminal. Authentication was minimal. |
| **Options Considered** | (1) Keep in-process Telegram sender only, (2) Gateway module with Hermes Run API envelope, (3) External proxy service. |
| **Decision** | **Adopted** — `janus::gateway` module with payload-complete dispatch, non-blocking verdict thread, loopback HTTP listener. |
| **Rationale** | Payload-complete: all data in the request, no DB lookups needed. Non-blocking: tmux session is never frozen. HTTP loopback listener enables Teams/Telegram callbacks without external proxies. HMAC-SHA256 authentication. |
| **Status** | ✅ Implemented in 0.4.0 (`janus/src/gateway/`, `a87f2c1`) |

---

## ADR-013: Teams Active Cards (Contract 4.3b)

| Field | Value |
|---|---|
| **Context** | Enterprise users need Microsoft Teams integration for HITL approvals. Telegram is consumer-grade — missing Adaptive Cards, corporate compliance, audit trails. |
| **Options Considered** | (1) Teams-only (drop Telegram), (2) Maintain both adapters, (3) Abstract adapter trait. |
| **Decision** | **Adopted** — Teams as secondary adapter alongside Telegram. `HitlGateway` trait with `LoggingSender` (always fires), `TelegramSender` (existing), `TeamsSender` (new). |
| **Rationale** | Both adapters share the same Hermes Run API envelope. Teams provides Adaptive Cards with Approve/Reject/Override buttons for enterprise compliance. Telegram remains for consumer/quick-setup use. |
| **Status** | ✅ Implemented in 0.4.0 (`janus/src/gateway/teams.rs`) |

---

## ADR-014: WebhookPayload Relocation to `protocol.rs`

| Field | Value |
|---|---|
| **Context** | `WebhookPayload` lived in `tool_guard::webhook`. Both `gateway` and `tool_guard` need it — creating a circular dependency (`tool_guard → gateway → ... → tool_guard`). |
| **Options Considered** | (1) Duplicate the type, (2) Move to `absurd` (creates `absurd ↔ protocol` cycle), (3) Move to `protocol.rs` (the leaf module). |
| **Decision** | **Adopted** — Move `WebhookPayload`, `GatewayVerdict`, `SIZE_BUDGET`, `truncate_16k`, `BUDGET_TAG` to `protocol.rs`. |
| **Rationale** | `protocol.rs` imports nothing from the crate — it's a leaf module. Both `tool_guard` and `gateway` depend on `protocol` with no cycle. Enriched with `blueprint`, `step`, `stdout_tail`, `expires_at` for Hermes envelope compatibility. |
| **Status** | ✅ Implemented in 0.4.0 (Phase 0, `f288069`) |

---

## ADR-015: Vendoring absurd.sql (Absurd Schema Engine)

| Field | Value |
|---|---|
| **Context** | The `absurd.sql` schema engine was previously classified as an external dependency: fetched by `make bootstrap` and maintained separately. This introduced a runtime dependency — if the absurd repo were unreachable or if an upstream change broke compatibility, `janus-daemon` could not bootstrap new blueprint databases. |
| **Options Considered** | (1) Keep absurd as an external dependency (status quo), (2) Vendor absurd.sql into the monorepo and compile it into the binary via `include_str!`. |
| **Decision** | **Adopted** — Vendor `absurd.sql` at `janus/sql/absurd.sql` (v0.4.0, upstream commit `9b77b35`). The version is tracked in `janus/sql/ABSURD_VERSION`. Every upstream tag update is captured as a commit to this file (version marker in the header). |
| **Rationale** | Zero runtime network dependency — the daemon reads the schema from its own binary. Deterministic builds — the SQL hash is locked by the repo. Single-binary distribution. Downstream updates are opt-in: a scheduled CI watcher checks for new upstream releases and opens a Draft PR with the updated `absurd.sql` + version bump. |
| **Status** | ✅ Implemented in 0.4.0 (`fe6572e`+) |

---

## ADR-016: Herdr Plugin Architecture (herdr-janus Shadow Client)

| Field | Value |
|---|---|
| **Context** | The Herdr 0.7.3 plugin model provides pane entrypoints (`[[panes]]`), injected environment variables (`HERDR_PLUGIN_ROOT`, `HERDR_PLUGIN_CONFIG_DIR`, `HERDR_PLUGIN_STATE_DIR`, `HERDR_SOCKET_PATH`), and placement directives (`overlay | split | tab | zoomed`). `herdr-janus` was always intended as a lightweight shadow client, but the original Chinese design doc (`docs/bak/herdr-plugin.md`) proposed an over-engineered approach with two panes, invalid placement directives, and CLI modes that don't match the actual implementation. |
| **Options Considered** | (1) Two-pane design (interception-popup + dashboard) with CLI `--mode` flags, (2) Single-pane design with internal Tab-toggle (Dispatch ↔ Progress), M0-validated against Herdr 0.7.3. |
| **Decision** | **Adopted** — Single `dispatcher` pane with `placement = "overlay"`, internal `Tab` toggle between Dispatch (ACTIVE blueprints) and Progress (in-flight tasks). Keybinding is configured in `~/.config/herdr/config.toml` (not the plugin manifest). The plugin process runs a ratatui TUI; Herdr closes the overlay automatically on process exit — no explicit `herdr plugin pane close` call needed. |
| **Rationale** | The M0 spike (`docs/contracts/herdr.md`) validated Herdr 0.7.3's actual behavior: `placement = "overlay"` (not `popup`), no `width`/`height` manifest fields, `id = "metamach.janus"` (not com.metamach.janus), `min_herdr_version = "0.7.3"`. The two-pane design was over-engineered — one pane with internal view switching is simpler. The `herdr plugin pane close` approach is unnecessary; Herdr closes the overlay when the process exits. |
| **Status** | ✅ Implemented in 0.3.0+ (M2). `janus/herdr-plugin.toml` + `janus/src/bin/herdr_janus.rs`. |

### Manifest (Corrected)

The actual `janus/herdr-plugin.toml`:

```toml
id = "metamach.janus"
name = "MetaMach Janus"
version = "0.4.1"
min_herdr_version = "0.7.3"

[[panes]]
id = "dispatcher"
title = "MetaMach Dispatcher"
placement = "overlay"
command = ["herdr-janus"]
```

### Communication Flow (Corrected)

```
┌─ Herdr Terminal Emulator ──────────────────────────────────────┐
│  Factory Director presses prefix+j                              │
│  → Herdr opens overlay pane, spawns herdr-janus process         │
│  → herdr-janus reads HERDR_PLUGIN_STATE_DIR/janus.sock          │
│  → Connects via UDS to janus-daemon                             │
│                                                                  │
│  ┌─ herdr-janus (overlay pane, ratatui TUI) ─────────────────┐  │
│  │  Tab                  → toggle Dispatch ↔ Progress         │  │
│  │  Dispatch view        → select blueprint, dispatch         │  │
│  │  Progress view        → 1-2s poll, render task status      │  │
│  │  Esc / q              → exit process, Herdr closes overlay │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
         │ UDS
         ▼
┌─ janus-daemon (MM-CORE background process) ────────────────────┐
│  - Serves Blueprints, Progress, Ping, GuardCheck               │
│  - State owned by Absurd PG                                    │
└─────────────────────────────────────────────────────────────────┘
```

### Injected Environment Variables (Validated M0)

| Variable | Purpose | Used by |
|---|---|---|
| `HERDR_PLUGIN_ROOT` | Immutable plugin checkout (blueprints/, workflows/, bin/) | `janus-daemon` for `repo_root` |
| `HERDR_PLUGIN_CONFIG_DIR` | Mutable config (`agents.toml`) | `janus-daemon` for `agents_toml_path` |
| `HERDR_PLUGIN_STATE_DIR` | Mutable state (`janus.sock`, `janus.pid`, `fallback.db`) | All binaries for `paths::state_dir` |
| `HERDR_SOCKET_PATH` | Herdr's own control socket | Not currently used by MetaMach |

### Dev Workflow

```bash
herdr plugin link ~/metamach/janus          # register from local manifest dir
herdr plugin list                           # verify (enabled, source, warnings)
herdr plugin pane open --plugin metamach.janus --entrypoint dispatcher  # manual test
```

### Cross-Check: Corrections from Chinese Design Doc

| # | Original Proposal | Corrected | Rationale |
|---|---|---|---|
| 1 | `placement = "popup"` | `placement = "overlay"` | M0-validated: Herdr 0.7.3 enum is `overlay \| split \| tab \| zoomed`, not `popup`. |
| 2 | `width = "80%"`, `height = "60%"` | Removed | Not valid Herdr 0.7.3 manifest fields. Sizing is managed by Herdr. |
| 3 | `min_herdr_version = "0.7.0"` | `"0.7.3"` | Validated against actual installed version. |
| 4 | `id = "com.metamach.janus"` | `"metamach.janus"` | Matching existing manifest and tenant key in paths. |
| 5 | Two `[[panes]]` (popup + dashboard) | One `[[panes]]` (dispatcher) | Internal Tab toggle handles view switching. |
| 6 | `[[keys.command]]` in manifest | Configured in Herdr's `config.toml` | Keybindings are host-level, not plugin-level. |
| 7 | `[[actions]]` with `herdr plugin pane open` | Not needed | Pane opens via `herdr plugin pane open --entrypoint dispatcher`. |
| 8 | `~/.metamach/janus.sock` | `HERDR_PLUGIN_STATE_DIR/janus.sock` | Uses `paths::sock_path()` resolution. |
| 9 | `HERDR_BIN_PATH` env var | Not a documented Herdr var | Not in the M0-validated env var set. |
| 10 | `--mode popup` CLI flag | Internal View enum, Tab toggle | `herdr-janus` has no CLI modes; always renders ratatui TUI. |
| 11 | `herdr plugin pane close` call from plugin | Process exits → Herdr closes overlay | Plugin should not call Herdr CLI; exit is sufficient. |

### Inherited Design Principles (from herdr-tether analysis)

| Principle | herdr-tether Limitation | MetaMach 0.4.0 Solution |
|---|---|---|
| **16KB Flow Budget** | Fail on over-budget | Dual-defense: janush streaming + daemon pre-insert truncation with `[Log Budget Exceeded]` tag |
| **tmux Session Isolation** | Could attach to external sessions | Strict `tmux -L metamach-tmux` isolation; never touches host-global tmux |
| **Non-Destructive View Close** | Closing view = session at risk | `remain-on-exit on`; SIGHUP immunity via `janus::tmux` daemon-owned sessions |
| **Fail-Closed on Unknown** | Unknown = assume safe | 30s fail-closed timeout; never lets through on uncertainty |
| **Idempotent Recovery** | State files, no atomicity | Absurd PG checkpoints; cold-start reads last COMPLETED step |
| **File Mode 0600** | Atomic writes | UDS socket, fallback.db, PG data dir all enforce 0600 permissions |
| **SSH BatchMode** | Could not parse SSH Include | Host-native SSH binary; inherits all system SSH config resolution |
| **Not a Sandbox** | Tether was not a sandbox | janush is a gatekeeper — once approved, commands execute bare-metal (no virtualization) |

---

## ADR-017: Remote Workload Model — SSH as tmux Transport Prefix

| Field | Value |
|---|---|
| **Context** | Phase 2 (`docs/bak/M4-4.1-design.md` §2.1) proposed a separate `SshTmuxBackend` type for cross-host SSH tmux sessions — a new struct, new file (`tmux/ssh.rs`), new `DurableBackend` impl duplicating ~100 lines of identical tmux command construction. The only difference between local and remote tmux is an `ssh <host>` prefix on the CLI command. |
| **Options Considered** | (1) Separate `SshTmuxBackend` type (`docs/bak/M4-4.1-design.md` §2.1), (2) Same `TmuxBackend` with optional `ssh <host>` prefix, (3) Multi-daemon topology (remote daemon + remote PG per host). |
| **Decision** | **Adopted** — Option (2): `TmuxBackend` gains a `with_ssh(host)` constructor. The `ssh <host>` prefix is prepended to all tmux CLI calls (`new-session`, `display-message`, `capture-pane`, etc.). All `DurableBackend` methods remain identical. Remote janush ↔ daemon connectivity uses SSH `-R` reverse tunnel to map the local `janus.sock` to `/tmp/mm-<host>.sock` on the remote host — zero remote configuration. |
| **Rationale** | All `DurableBackend` operations are tmux CLI calls; `ssh <host> tmux ...` is syntactically identical to `tmux ...`. A single backend with optional SSH prefix is ~20 lines vs ~100 lines of duplicated code. The reverse tunnel keeps Tool Guard local (same agents.toml, same GuardCheck, same verdicts). Remote host needs only tmux + janush (two binaries, scp once) — no daemon, no PG, no agents.toml, no gateway. |
| **Status** | ✅ Implemented in 0.4.5 (`6ac8b9e`). |

---

## ADR-018: Stream Filter — ANSI Strip + Progress Bar Collapse (0.4.6)

| Field | Value |
|---|---|
| **Context** | The `truncate_16k` budget caps step output at 16KB, but ANSI escape codes, progress bars (`[=====>  ] 45%`), and repetitive lines (`ACK` × 50) consume the budget with noise. HITL cards and Progress logs show unreadable terminal escape sequences instead of clean text. |
| **Options Considered** | (1) Do nothing — 16KB truncation is sufficient, (2) Add a Stream Filter layer before `truncate_16k` that strips ANSI, collapses progress bars, and deduplicates repeating lines, (3) Full PTY state-machine parser (overkill). |
| **Decision** | **Adopted** — Option (2): `janus/src/workflow/filter.rs` provides `clean_pty_output(raw) -> String` as a pure function. Inserted into the existing `capture_pane -> truncate_16k` pipeline in `run_steps`. Three stages: ANSI strip, progress bar collapse, duplicate line dedup. |
| **Rationale** | ~100 lines of pure functions, 0 new dependencies, unit-testable (input: ANSI string, output: clean text). Transforms 16KB of escape-code noise into 2KB of structured output. Does not change any API, protocol, or database schema. |
| **Status** | ✅ Implemented in 0.4.6 (`6591699`). |

---

## ADR-019: Configurable Agents — Provisioning, Quota & Fallback (0.4.7)

| Field | Value |
|---|---|
| **Context** | The existing `configs/agents.toml` defines Tool Guard permissions ("what can agent X do?"). It has no concept of which LLM backs each agent, quota limits, or fallback chains when the primary agent is exhausted. As the workflow engine dispatches real agents, it needs to know which LLM provider to use and what to do when quotas are exceeded. |
| **Options Considered** | (1) Keep agents.toml Tool-Guard-only, add provisioning elsewhere, (2) Extend agents.toml with an optional `[agent.X.provision]` section (co-located with the agent it provisions), (3) Separate provisioning config file. |
| **Decision** | **Adopted** — Option (2): extend `agents.toml` with optional `[agent.X.provision]` sections. Each agent can declare an `adapter` (claude-code, codex, aider), a `command`, a `system_prompt`, a `quota` block (`max_tokens_per_day`, `max_cost_usd_per_day`, `max_requests_per_hour`), and a `fallback_agent` for automatic degradation. No new file, 100% backward compatible — existing Tool Guard entries need no changes. |
| **Rationale** | Co-locating provisioning with permissions keeps the agent definition in one place. The `AgentStack` parser (`janus/src/agent.rs`, ~150 lines) resolves fallback chains recursively. Runtime quota tracking is deferred to 0.5.0+ (needs the engine); the config format and parser ship first so the engine has a defined provisioning model to consume. |
| **Status** | ✅ Implemented in 0.4.7 (`2addfce`). |

---

## ADR-020: Observer Panel — TUI HITL + Enhanced Progress (0.4.8)

| Field | Value |
|---|---|
| **Context** | `herdr-janus` currently has Dispatch (blueprint selection) and Progress (task status) views, but cannot approve/reject HITL suspensions — the Director must use Teams or Telegram. The Progress view shows step status only, without live log tails or SUSPENDED countdown timers. |
| **Options Considered** | (1) Keep herdr-janus as-is (HITL = Teams/Telegram only), (2) Enhance the Progress view with HITL gate interaction + live log display, (3) Build a separate observer binary. |
| **Decision** | **Adopted** — Option (2): enhance the existing `herdr-janus` binary. Add `y`/`n` keybindings for HITL approval/rejection (sends `GateAction` UDS request via the existing `janus::gateway` callback path), `Enter` to expand a step's 16KB `stdout_tail`, and red-highlight + countdown for SUSPENDED steps. No new binary, no new dependencies — ~100 lines in `herdr-janus`. |
| **Rationale** | The gateway callback path already handles HITL verdicts; this adds a TUI entrypoint. The Observer is a UX enhancement on existing infrastructure — same UDS, same Progress data, same gateway. Zero daemon changes beyond one new UDS request type (`GateAction`). |
| **Status** | ✅ Implemented in 0.4.8 (`97665f4`). |

---

## ADR-022: Time-Driven Suspension — Quota Exhaustion & Scheduled Sleep (0.5.0)

| Field | Value |
|---|---|
| **Context** | The 0.4.x engine's only pause mechanism is event-driven HITL (`await_event`). When an Agent's quota is exhausted (Coding Plan returns 429 or quota exceeded), the engine retries up to `max_attempts: 3` — if all fail, the task goes terminal `FAILED` and the Director must manually re-dispatch. For quota resets at UTC midnight (e.g., Coding Plan daily limits), a time-driven sleep is the correct response: suspend the task until the quota window renews rather than failing or retrying blindly. |
| **Options Considered** | (1) Keep `max_attempts: 3` retry only (status quo — treats quota exhaustion as transient failure), (2) Add time-driven sleep via absurd's `sleep()` stored procedure: detect quota exhaustion in the engine → call `engine.sleep(seconds)` → absurd auto-wakes the task → re-claim → resume, (3) Handle quota exhaustion entirely in the Agent Adapter layer (outside the engine). |
| **Decision** | **Adopted** — Option (2): add `sleep` to the `DurableEngine` trait and integrate it into the engine's retry loop. On step exit ≠ 0, the engine inspects `stdout_tail` for quota-exhaustion patterns (429, quota exceeded, rate limit) and the Configurable Agent's quota config (ADR-019). If quota exhaustion is detected, the engine calls `engine.sleep(queue, task_id, run_id, seconds)` instead of `fail_run` — absurd suspends the task for the specified duration, then auto-wakes it. The engine re-claims and resumes from the same step. |
| **Rationale** | absurd already supports `sleep()` — this is a one-method addition to the `DurableEngine` trait. The pattern detection in `stdout_tail` is heuristic but low-risk: if the detection is wrong, the engine falls back to `fail_run`. The sleep duration is configurable via `JANUS_QUOTA_SLEEP_SECONDS` (default: until next UTC hour boundary, or a fixed 300s). Time-driven sleep fills the gap between "retry immediately" and "fail permanently" — it's the correct response for quota-bound resources. |
| **Status** | ✅ Implemented in 0.5.0 (`3bb4aad`). |

---

## ADR-023: Agent Planner — LLM-Assisted Pipeline Generation (0.5.0)

| Field | Value |
|---|---|
| **Context** | Writing workflow DAG definitions by hand requires knowing Workflow names, node IDs, and dependency edges. For non-programmer Factory Directors, this is a significant adoption barrier. |
| **Options Considered** | (1) Manual TOML editing only (status quo), (2) LLM-assisted CLI tool that generates Workflow DAG TOML from natural language, (3) Web-based drag-and-drop editor (Canvas Studio, 0.6.0). |
| **Decision** | **Adopted** — Option (2): `janus plan` CLI subcommand. The Planner reads the Workflow library (`templates/workflows/*.toml` + `.janus/workflows/*.toml`), sends a catalog + user prompt to the LLM (using existing Coding Plan provider), receives generated Workflow DAG TOML, runs validation, and writes to `.janus/workflows/`. Three-phase interactive workflow: Draft → Revise → Commit. |
| **Rationale** | CLI-only, zero daemon changes, zero new API keys (reuses Coding Plan provider). LLM is advisory — validation is the final gate. Fallback: hand-write workflows always works. Natural-language generation removes the biggest UX barrier for non-programmer users. |
| **Status** | ✅ Implemented in 0.5.0. |

---

## ADR-024: Environmental Snapshot Injection (0.5.0)

| Field | Value |
|---|---|
| **Context** | On replay/crash-recovery, a re-executed step may encounter different physical conditions than its original run (different system time, different USB devices plugged in, different network state). Absurd's checkpoint determinism assumes identical conditions — a gap between pure-digital replay and physical-world state. |
| **Options Considered** | (1) Do nothing — steps are expected to be self-determining, (2) Capture an environmental snapshot (`JANUS_ENV_TIMESTAMP`, `JANUS_ENV_TTY_DEVICES`) at step dispatch time and store it in `metamach_step_meta` so the agent can compare on replay, (3) Full environment dump (overkill). |
| **Decision** | **Adopted** — Option (2): `step_command()` injects `JANUS_ENV_TIMESTAMP` (UTC ISO 8601) and `JANUS_ENV_TTY_DEVICES` (comma-separated serial ports found under `/dev/tty*`). These are stored in a new `env_snapshot` JSONB column on `metamach_step_meta` (004 migration). The agent can read them on replay to detect changed conditions. |
| **Rationale** | ~30 lines in `step_command()` + 1 new DB column. The snapshot is a diagnostic aid, not enforcement — the agent decides how to use it. Low implementation cost, high value for debugging replay failures in hardware pipelines. |
| **Status** | 📋 Spec'd Only — 0.5.0 implementation. |

---

## ADR-025: Dual-Path Log Pipeline — Raw Disk Cache + PG Metadata (0.5.0)

| Field | Value |
|---|---|
| **Context** | The current log pipeline captures PTY output → `clean_pty_output` → `truncate_16k` → `metamach_step_meta.stdout_tail` (PG). The full raw output (potentially megabytes of ESP-IDF compilation logs) is discarded after 16KB truncation — the Director cannot access the full build log when debugging a failure. |
| **Options Considered** | (1) Store full output in PG (WAL bloat), (2) Store 16KB in PG + full raw log on disk at `/tmp/metamach/logs/{task_id}_{step}.raw`, (3) Keep only 16KB truncation (status quo). |
| **Decision** | **Adopted** — Option (2): dual-path pipeline. `capture_pane` output is written in full to `/tmp/metamach/logs/{task_id}_{step_name}.raw` (disk, pruned by Janus GC at 7-day retention). The truncated 16KB continues to be stored in `metamach_step_meta.stdout_tail` for dashboard/Progress display. |
| **Rationale** | ~40 lines in `run_steps`: `std::fs::write(log_path, raw)` after `capture_pane`. Zero PG schema changes. The `/tmp/metamach/` path is local, disposable (RAM disk on some hosts), and auto-pruned. Full logs available for debugging without WAL pressure. |
| **Status** | 📋 Spec'd Only — 0.5.0 implementation. |

---

## ADR-026: Hardware Pre-flight Probe Hooks (0.5.0)

| Field | Value |
|---|---|
| **Context** | `janush` intercepts high-risk commands (e.g., `esptool.py write_flash`) and either blocks (blacklist) or suspends (require_approval). But once the Director approves, janush blindly allows re-execution — there is no check for whether the hardware operation was already completed. On crash replay, a previously-successful flash operation would re-flash the chip, wasting write cycles and risking corruption. |
| **Options Considered** | (1) Leave as-is — Director approval is the gate (status quo), (2) Add optional pre-flight probe hooks in janush that check hardware state before allowing physical commands, (3) Move all hardware probing to the agent level. |
| **Decision** | **Adopted** — Option (2): janush gains a `[agent.X.preflight]` section in `agents.toml` mapping commands to probe scripts. Before allowing a command, janush runs the probe, inspects exit code + stdout, and can `BypassExecution` (probe confirms already done) or `RequireApproval` (probe fails, escalate to HITL). The probe is a simple shell command: `esptool.py verify_flash --port {port} {bin}`. |
| **Rationale** | ~50 lines in janush + config. Leverages janush's existing pre-execution path. Probes are optional per-agent — no overhead for agents without physical commands. The probe outcome is logged in `metamach_step_meta.hitl_verdict` as `BYPASSED` for audit trail. |
| **Status** | 📋 Spec'd Only — 0.5.0 implementation. |

---

## ADR-027: Extensible Physical Safety Gateway — Probe SPI + Policy DSL (0.6.0)

| Field | Value |
|---|---|
| **Context** | ADR-026 (`[agent.X.preflight]` shell hooks) covers the immediate ESP32 flash idempotency use case, but as MetaMach expands to more hardware targets (STM32, 3D printers, PLCs, robotics), a shell-command-based config becomes limiting. Different targets need different probe logic, environmental safety interlocks (temperature, cover sensors), and policy composition. |
| **Options Considered** | (1) Keep shell-command preflight hooks only (no further abstraction), (2) Evolve janush into an extensible physical safety gateway with Rust-level probe/interlock traits + a policy TOML, (3) External hardware safety daemon (separate process). |
| **Decision** | **Pending Review** — Option (2): `janush` gains a 4-stage pipeline (Parse → Probe → Policy → Execute) with three extension points: `PhysicalProbe` trait (hardware state querying, e.g., ESP32 flash hash verification), `SafetyInterlock` trait (environmental condition checking, e.g., ambient temperature < 65C), and a `janush-policy.toml` that maps command patterns to probes/interlocks and actions (ALLOW/BLOCK/BYPASS/SUSPEND_30S/REJECT). |
| **Rationale** | The trait-based SPI decouples janush from specific hardware knowledge — probes are loaded declaratively via config. The policy TOML lets the Factory Director add hardware safety rules without touching Rust. Three operation modes: (A) physical idempotency bypass (probe confirms already done → skip), (B) HITL suspension (high-risk → 30s human approval), (C) hard circuit break (interlock tripped → immediate reject). This is a natural evolution of ADR-026's shell hooks into a typed, testable, composable plugin architecture. |
| **Status** | 📋 Pending Review — 0.6.0 candidate. Depends on ADR-026 (shell hooks) being battle-tested in 0.5.0 before generalizing. |

---

## ADR-028: E2E Pipeline Test Suite — CI Mock + Manual LLM Validation (0.4.9.4)

| Field | Value |
|---|---|
| **Context** | Test-Spec Suite 2.11 defines three end-to-end multi-agent pipelines (`req2spec`, `spec2software`, `adr-process`) that validate the full DevSecOps lifecycle across ARCHITECT, BUILDER, and TESTER agents. These require real LLM agents, API keys, PG, and tmux — they cannot run in CI. But the pipeline mechanics (DAG engine, parallel execution, Tool Guard, checkpoint/recovery, git commit) need CI coverage. |
| **Options Considered** | (1) E2E tests only (manual, no CI coverage of pipeline mechanics), (2) Mock-agent CI tests + manual LLM validation (dual-path), (3) No E2E tests — rely on unit + integration tests. |
| **Decision** | **Adopted** — Option (2): dual-path approach. **CI path:** `tests/e2e_pipeline.rs` with mock agents (deterministic shell scripts: `echo APPROVED`, `echo 'spec content' > docs/...`) that exercise the full DAG engine, Tool Guard interception, checkpoint/recovery, cold-start resume, and git commit — zero LLM dependency. **Manual path:** pre-release validation following the Suite 2.11 procedures with real LLM agents, run from a macOS/Linux host with PG + tmux + API keys. **Blueprint:** `.janus/` with `blueprint.toml`, `agents.toml` with architect/builder/tester roles, `.janus/workflows/req2spec.toml`, `.janus/workflows/spec2software.toml`, `.janus/workflows/adr-process.toml`. |
| **Rationale** | Mock-agent tests give CI confidence that the pipeline mechanics work end-to-end (DAG → Dispatch → Progress → COMPLETED → git commit) without the cost, flakiness, and API key dependency of real LLMs. Manual validation with real agents catches integration issues that mocks can't (LLM prompt quality, real output format, actual API behavior). The `software-dev` blueprint serves as both the CI test fixture and the manual validation target. |
| **Status** | 📋 Spec'd Only — 0.4.9.4 implementation (0.5.0 prep). |

## ADR-029: Project-Based Templates — `.janus/` as Sole Config Directory (0.5.0)

| Field | Value |
|---|---|
| **Context** | Before 0.5.0, per-project configuration was scattered: blueprint recipe at `blueprints/<name>/janus.toml`, openwiki content at `blueprints/<name>/openwiki/`, workflow definitions at `workflows/` or `templates/workflows/`, pipeline definitions at `pipelines/` or `templates/pipelines/`, and agent overrides at `.janus/agents/`. This required the daemon to search multiple directory roots and made `janus init` scatter files into inconsistent locations. The `blueprints/` prefix was also redundant: the blueprint *name* is in the TOML, not the directory path. |
| **Options Considered** | (1) Keep `blueprints/<name>/janus.toml` + scattered workflow/pipeline dirs, (2) Consolidate everything under `.janus/` — single source of truth per project, (3) Put everything in project root with individual dotfiles (no `.janus/` directory). |
| **Decision** | **Adopted** — Option (2): all per-project MetaMach configuration lives under `.janus/`. The blueprint recipe moves from `blueprints/<name>/janus.toml` to `.janus/blueprint.toml`. `janus init` copies templates into `.janus/agents/`, `.janus/workflows/`, `.janus/pipelines/`, and creates `.janus/blueprint.toml`. The daemon searches `.janus/workflows/` first (before `templates/workflows/` and `workflows/`). `load_previous_incidents` reads from `.janus/openwiki/production_report.md`. Backward-compatible: `load_workflow` still checks `templates/workflows/` and `workflows/` as fallbacks. |
| **Rationale** | Single `.janus/` root eliminates directory ambiguity. The daemon only needs one path: `repo_root.join(".janus")`. The `blueprints/` convention was a misfeature — the blueprint name is a TOML field, not a directory name, so path-traversal via `..`/`/` was already mitigated by `validate_name()`. Configuration that ships with MetaMach (`templates/`) remains separate from project-local overrides (`.janus/`). |
| **Status** | ✅ Implemented — `janus init` scaffolds `.janus/` from `templates/`. `recipe::validate()` and `recipe::load_recipe()` read `.janus/blueprint.toml`. `load_workflow` checks `.janus/workflows/` in search path. All 168 tests pass, CI green. |

---

## ADR-030: Rejection of `just`/`justfile` Migration in Favor of `Makefile` (0.5.0)

| Field | Value |
|---|---|
| **Context** | Proposed migrating MetaMach's task runner from GNU `Makefile` to `just`/`justfile` to improve command syntax, positional parameter handling, and error reporting. |
| **Options Considered** | (1) Replace `Makefile` entirely with `justfile`, (2) Retain `Makefile` as primary zero-dependency entrypoint and optionally add a `justfile` proxy adapter, (3) Maintain `Makefile` exclusively (status quo). |
| **Decision** | **Rejected** Option (1). **Adopted** Option (3) (with optional Option 2 proxy adapter allowed for local dev). `Makefile` remains the authoritative task runner. |
| **Rationale** | `make` is ubiquitous on Linux and macOS, preserving MetaMach's "Zero-Dependency Out-of-the-Box" bootstrap contract (PRD §Zero-Dependency; SPEC.md Part 4; ADR-001/015). Requiring `just` adds an unneeded prerequisite for first-time onboarding. `Makefile` is already fully implemented (189 lines, 16 targets), 100% test-backed, and deeply integrated into `AGENTS.md`, `CLAUDE.md`, `README.md`, and `scripts/pre-push`. |
| **Status** | ❌ Rejected — `Makefile` retained as sole primary entrypoint. |

---

## ADR-031: Unification of Workflow and Pipeline DSLs (0.5.1)

| Field | Value |
|---|---|
| **Context** | MetaMach 0.5.0 forcibly separated Workflow (sequential `[[steps]]` with Absurd PG checkpoints) and Pipeline (multi-node DAG with Kahn topological scheduling). For 80% of routine linear tasks, users were forced to understand both abstractions or write single-node pipeline boilerplate. AI agents frequently confused `.janus/pipelines/` vs `.janus/workflows/` boundaries. |
| **Options Considered** | (1) Keep separate Workflow and Pipeline DSLs (status quo), (2) Merge DSL at configuration layer into unified `Workflow` with Linear and DAG modes, while preserving distinct physical execution engines. |
| **Decision** | **Adopted** — Abolish independent Pipeline DSL files; unify all workflow declarations under `.janus/workflows/` as `Workflow`. Support **Linear Mode** (sequential steps, direct `handle_dispatch → spawn_workflow` execution, skipping Kahn sort) and **DAG Mode** (multi-node dependency graph with `needs`, Kahn level-parallel execution via `handle_dispatch_pipeline`). Support **Hybrid Node Composition** (nodes can reference standalone workflows via `workflow = "<name>"` or inline `steps = [...]` via an in-process register). Maintain ADR-021 durability boundary (ephemeral DAG orchestration, durable per-node Absurd PG tasks). |
| **Rationale** | Reduces cognitive load for developers and AI agents to a single `Workflow` concept. Linear mode (80% case) bypasses Kahn DAG sorting entirely for zero performance overhead. Fully backward-compatible: `load_unified_workflow` uses a try-parse deserializer supporting `[workflow]` header format and legacy `[pipeline]` bridging. SemVer migration across 0.5.1 (warnings & dual-track) -> 0.5.2 (default scaffolding) -> 0.5.3 (hard removal). Amends ADR-021 and ADR-029. |
| **Status** | ✅ Implemented — 0.5.1 (Amends ADR-021 & ADR-029). |

---

## ADR-032: MetaMach Studio — Visual Workflow DAG Editor & Web Observer (0.6.0 Candidate)

| Field | Value |
|---|---|
| **Context** | MetaMach 0.5.0/0.5.1 unified workflows and pipelines under `.janus/workflows/` (ADR-031). Complex multi-node DAG workflows and human-in-the-loop (HITL) safety gate approvals benefit from visual graph editing, real-time node state visualization, and physical PTY terminal streaming. |
| **Options Considered** | (1) Embed Web UI inside `janus-daemon`, (2) Standalone `janus-studio` sidecar binary over UDS (`janus.sock`), (3) CLI/TUI-only. |
| **Decision** | **Adopted as Candidate ADR (0.6.0)** — Option (2): Standalone `janus-studio` sidecar binary exposing REST + WebSocket on `127.0.0.1:8443`. Keep `janus-daemon` zero-web-dependency. Full details in `docs/ADR-032-canvas-studio.md`. |
| **Rationale** | Preserves core daemon zero-web-dependency isolation. Provides visual DAG authoring, step state visualization, and web HITL approval without daemon memory overhead. |
| **Status** | ✅ Implemented — 0.6.0 (Amends ADR-020, ADR-029, ADR-031). |

---

## ADR-033: Dual-Track Execution Isolation & Post-Execution Writes Guard (0.7.0 Candidate)

| Field | Value |
|---|---|
| **Context** | MetaMach 0.6.0 executes all workflow steps on the host bare-metal node via `janush` UDS gatekeeping and `janus::tmux`. Pure-software SDLC tasks (React builds, Python tests) do not require hardware locks but cannot leverage parallel exploration. Tool Guard (ADR-007) intercepts commands pre-execution but cannot audit file-system side effects post-execution. |
| **Options Considered** | (1) Keep bare-metal host execution for all tasks (status quo). (2) Introduce Docker/podman container sandboxes — rejected: contradicts ADR-001 De-containerization and the "No Docker required" competitive moat. (3) Host-native isolation via separate tmux servers (`tmux -L metamach-sandbox-<id>`), unprivileged OS users, and per-run Git worktrees, with post-execution `writes` path boundary checks. |
| **Decision** | **Adopted as Candidate ADR (0.7.0)** — Option (3): Extend unified DSL with `isolation` (`"sandbox"` / `"bare_metal"`, default `"bare_metal"`) and `writes` path whitelist on `DagNodeDef`. Sandbox track uses host-native isolation (separate tmux server, unprivileged user, Git worktree) — no container engine dependency. `PostExecutionGuard` checks Git workspace diffs post-step; unauthorized file writes are snapshotted to `refs/metamach/rollback/<step_id>`, the step is suspended, and HITL escalation is triggered (no destructive auto-rollback). Guard restricted to linear-mode steps unless DAG nodes use per-node worktrees. Preserves existing `DagNodeDef.workflow` field for ADR-031 Hybrid Node Composition. Full details in `docs/bak/ADR-033-Dual-Track-Execution.md`. |
| **Rationale** | Extends ADR-001 de-containerization philosophy with host-native sandboxing. Post-execution guard provides defense-in-depth complementing pre-execution Tool Guard. HITL-escalation-on-violation aligns with ADR-026/027 governance philosophy. Default `"bare_metal"` ensures 100% backward compatibility. ADR-036 Harvest Pipeline depends on this ADR. |
| **Status** | 🔄 Phase 1 Implemented (Writes Guard & DSL) / Phase 2 Spec'd Only (Sandboxed Tmux Track) — 0.7.0 (Amends ADR-001, ADR-007, ADR-031). |

---

## ADR-034: Typed Context Envelopes for Absurd PG Checkpoints (0.7.0 Candidate)

| Field | Value |
|---|---|
| **Context** | Step checkpoint state in 0.6.0 is stored via `DurableEngine::set_checkpoint()` as ad-hoc `serde_json::Value` objects (e.g., `json!({"step": name, "status": "COMPLETED"})`). Raw stdout is captured separately in `metamach_step_meta.stdout_tail` (16 KiB cap, ADR-008). As checkpoint complexity grows (HITL verdicts, multi-field error contexts), ad-hoc JSON risks runtime failures on state resumption. |
| **Options Considered** | (1) Continue with ad-hoc JSON checkpoint values (status quo). (2) External schema registries (Protobuf, JSON Schema) — rejected: introduces operational dependencies. (3) Rust Serde-typed `CheckpointEnvelope` structs validated at checkpoint write time. |
| **Decision** | **Adopted as Candidate ADR (0.7.0)** — Option (3): Define `CheckpointEnvelope` structs in `janus/src/workflow/envelope.rs` with Serde validation before `DurableEngine::set_checkpoint(queue, task_id: Uuid, step, state: &Value, owner_run: Uuid)`. Envelopes are PG-checkpoint-only; scene snapshots (HITL cards, progress) continue using truncated `stdout_tail` (ADR-008). Full details in `docs/bak/ADR-034-Typed-Context-Envelopes.md`. |
| **Rationale** | Stronger type guarantees on checkpoint data during cold-start recovery and HITL resumption. Clear separation between durable checkpoint state (Absurd PG, unbounded) and scene rendering (`stdout_tail`, 16 KiB cap). References `docs/contracts/absurd.md` for persistence contract. |
| **Status** | ✅ Implemented — 0.7.0 (Amends `docs/contracts/absurd.md`). |

---

## ADR-035: Augmented Cold Retry with Correction Context for Step Self-Healing (0.7.0 Candidate)

| Field | Value |
|---|---|
| **Context** | When a step fails envelope validation (ADR-034) or gate checks, the current model discards the run entirely and re-dispatches from scratch with no error guidance. MetaMach's execution model (`run_steps` in `workflow/mod.rs`) uses single-use tmux sessions per step — the agent runs to completion, exits, stdout is captured, and the session is cleaned up. There is no persistent interactive session to "append prompts to." |
| **Options Considered** | (1) Cold restart with no error context (status quo). (2) Persistent tmux session with `send-keys` prompt injection — rejected: contradicts the single-use tmux-per-step execution model; `janush` is a proxy shell, not an agent runtime. (3) Augmented cold retry: re-dispatch the same step in a new tmux session with error context injected via `METAMACH_CORRECTION_CONTEXT` environment variable. |
| **Decision** | **Adopted as Candidate ADR (0.7.0)** — Option (3): On validation failure, re-dispatch the step with the original command plus `METAMACH_CORRECTION_CONTEXT` env var containing the specific error message. Agent system prompts reference this variable for targeted correction. Retry capped at `max_correction_attempts = 3` (configurable in workflow DSL). Fits the existing `run_steps` tmux-poll execution loop without architectural changes. Full details in `docs/bak/ADR-035-In-Session-Correction.md`. |
| **Rationale** | Reduces wasted exploration tokens by providing targeted error guidance instead of blind re-execution. Fits the existing tmux-per-step, agent-exits execution model perfectly. No persistent session or agent runtime changes required. Depends on ADR-034 for envelope validation triggers. |
| **Status** | ✅ Implemented — 0.7.0 (Depends on ADR-034; Amends ADR-019). |

---

## ADR-036: Pluggable Credential Provisioning & Herdr Harvest Pipeline (0.7.0 Candidate)

| Field | Value |
|---|---|
| **Context** | Agents require dynamic, scoped credentials for external API access. Long-lived host credentials injected into sandbox environments risk key leaks and unbudgeted overuse. Sandbox outputs (ADR-033) need a secure extraction path to avoid polluting host Git branches. |
| **Options Considered** | (1) Pass host environment keys directly to sandboxes (status quo — insecure). (2) Hardcoded credential managers (AWS STS, Vault) — rejected: too rigid for diverse deployments. (3) Pluggable `CredentialProvider` SPI with ephemeral keys and lifecycle management, plus Herdr TUI harvest pipeline for sandbox output review. |
| **Decision** | **Adopted as Candidate ADR (0.7.0)** — Option (3), split into two sequenced phases. **Phase 1 (Credential SPI, independent):** Implement `CredentialProvider` trait in `janus/src/credential.rs` (top-level module, not cognitive/) using `BoxFut` pattern (matching `DurableEngine` conventions). Auto-revoke on task completion; cold-start sweep in `coldstart.rs` revokes orphaned keys from crashed tasks. **Phase 2 (Harvest Pipeline, depends on ADR-033):** Collect sandbox diffs as `refs/sandbox/*` with Herdr TUI diff preview (`[H]`) and merge control (`[M]`). Full details in `docs/bak/ADR-036-Credential-Provisioning-And-Harvest.md`. |
| **Rationale** | Pluggable SPI avoids vendor lock-in. Phase split allows Credential SPI to ship independently. Cold-start key sweep prevents orphaned credential leaks. Amends ADR-010 (Cognitive Provider SPI) and ADR-019 (Agent Provisioning) for credential concern separation; Phase 2 depends on ADR-033. |
| **Status** | 🔄 Phase 1 & Phase 2 Harvest Engine Implemented / TUI Keybindings Spec'd Only — 0.7.0 (Amends ADR-010, ADR-019; Phase 2 depends on ADR-033). |

---

## Appendix: Decision Status Legend

| Status | Meaning |
|---|---|
| ✅ Implemented | Code exists, tests pass, CI green |
| 🔄 In Progress | Spec committed, implementation underway |
| 📋 Spec'd Only | Contract written, not yet implemented |
| ❌ Rejected | Considered and explicitly rejected |
| 🔌 New | Introduced in current version |
