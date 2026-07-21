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

## Appendix: Decision Status Legend

| Status | Meaning |
|---|---|
| ✅ Implemented | Code exists, tests pass, CI green |
| 🔄 In Progress | Spec committed, implementation underway |
| 📋 Spec'd Only | Contract written, not yet implemented |
| ❌ Rejected | Considered and explicitly rejected |
| 🔌 New | Introduced in current version |
