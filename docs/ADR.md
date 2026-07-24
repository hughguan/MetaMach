# MetaMach Architecture Decision Records

> **Purpose:** This file captures the key architectural decisions across MetaMach's evolution from 0.1.0 through 0.4.0. Each ADR documents a decision: context, options considered, final choice, and rationale. This is the permanent record — once converged here, the delta files (`ARCH-0.2.0.md`, `ARCH-0.3.0.md`, `ARCH-0.4.0.md`) are archived to `docs/CH/` for backup.

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
| **Rationale** | The M0 spike (`docs/herdr-v1-contract.md`) validated Herdr 0.7.3's actual behavior: `placement = "overlay"` (not `popup`), no `width`/`height` manifest fields, `id = "metamach.janus"` (not com.metamach.janus), `min_herdr_version = "0.7.3"`. The two-pane design was over-engineered — one pane with internal view switching is simpler. The `herdr plugin pane close` approach is unnecessary; Herdr closes the overlay when the process exits. |
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
| **Context** | Phase 2 (`M4-4.1-design.md` §2.1) proposed a separate `SshTmuxBackend` type for cross-host SSH tmux sessions — a new struct, new file (`tmux/ssh.rs`), new `DurableBackend` impl duplicating ~100 lines of identical tmux command construction. The only difference between local and remote tmux is an `ssh <host>` prefix on the CLI command. |
| **Options Considered** | (1) Separate `SshTmuxBackend` type (M4-4.1-design.md §2.1), (2) Same `TmuxBackend` with optional `ssh <host>` prefix, (3) Multi-daemon topology (remote daemon + remote PG per host). |
| **Decision** | **Adopted** — Option (2): `TmuxBackend` gains a `with_ssh(host)` constructor. The `ssh <host>` prefix is prepended to all tmux CLI calls (`new-session`, `display-message`, `capture-pane`, etc.). All `DurableBackend` methods remain identical. Remote janush ↔ daemon connectivity uses SSH `-R` reverse tunnel to map the local `janus.sock` to `/tmp/mm-<host>.sock` on the remote host — zero remote configuration. |
| **Rationale** | All `DurableBackend` operations are tmux CLI calls; `ssh <host> tmux ...` is syntactically identical to `tmux ...`. A single backend with optional SSH prefix is ~20 lines vs ~100 lines of duplicated code. The reverse tunnel keeps Tool Guard local (same agents.toml, same GuardCheck, same verdicts). Remote host needs only tmux + janush (two binaries, scp once) — no daemon, no PG, no agents.toml, no gateway. |
| **Status** | 📋 Spec'd Only — Phase 2 implementation. ADR locks the decision before code lands. |

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
| **Status** | 📋 Spec'd Only — 0.4.7 implementation pending. |

---

## Appendix: Decision Status Legend

| Status | Meaning |
|---|---|
| ✅ Implemented | Code exists, tests pass, CI green |
| 🔄 In Progress | Spec committed, implementation underway |
| 📋 Spec'd Only | Contract written, not yet implemented |
| ❌ Rejected | Considered and explicitly rejected |
| 🔌 New | Introduced in current version |
