# 🪐 MetaMach 0.4.0 — Architecture Delta Specification

**Gateway & Ecosystem Integrations** | **Status: Proposal / Under Review**

> **0.4.0 Strategic Delta:** This document defines the **incremental architectural changes** from the 0.3.0 consensus baseline (`docs/ARCH-0.3.0.md`). It introduces the Cognitive Provider SPI, MCP-based symbol indexing, and an out-of-band Stateless HITL Gateway with Microsoft Teams Active Cards integration.

---

## 📋 0.4.0 Delta Matrix

| Component | 0.3.0 Baseline | 0.4.0 Delta | Verdict |
|---|---|---|---|
| **Knowledge Base** | In-memory context (blueprint-scoped) | **OpenWiki Cognitive Provider** (SPI, opt-in) | 🔌 New |
| **Symbol Indexing** | Not addressed | **codebase-memory-mcp** (MCP transport, opt-in) | 🔌 New |
| **HITL Routing** | Telegram webhook + local TUI (`tool_guard/webhook.rs`) | **Stateless HITL Gateway module** (`janus::gateway`) | ✅ Extended |
| **Alert Protocol** | Custom `WebhookPayload` JSON (Contract 3.4 bundles) | **Hermes Run API schema** (`/v1/runs`) — compatible envelope | 🔄 Aligned |
| **Enterprise Reach** | Telegram Bot API | **Microsoft Teams Active Cards** (Outgoing Webhook adapter) | 🌐 New |
| **Binary Names** | `janush`, `janus-daemon`, `herdr-janus` | **Unchanged from 0.3.0** | ✅ Baseline |
| **Physical Engine** | `janus::tmux` (`tmux -L metamach-tmux`) | **Unchanged from 0.3.0** | ✅ Baseline |

> **Note:** The `janus-sh → janush` and `janus::tether → janus::tmux` renames are **0.3.0 completed work** (commits `2a162ee` / `beed8ef` / `755cf83`). They are 0.4.0's baseline, not its delta.

---

## I. 0.3.0 Baseline (Unchanged — Reference)

For completeness, the binary and module naming baseline 0.4.0 inherits:

| Artifact | Name | Purpose |
|---|---|---|
| Proxy shell | **`janush`** | UDS-synchronous command interception; injected as `SHELL` at absolute path `${HERDR_PLUGIN_ROOT}/bin/janush` by `janus::tmux` |
| Physical engine | **`janus::tmux`** | Native module managing `remain-on-exit` tmux sessions (`tmux -L metamach-tmux`) |
| Session prefix | **`tmux-janus-task-<uuid>`** | Session naming convention per ARCH §4 |

The `export SHELL=$(which janush)` pattern is **incorrect** — Tether/tmux always injects the absolute compiled path, not a PATH lookup. The `which` approach fails when `janush` is not on the user's `PATH`.

---

## II. Module Tree Placement (New)

The 0.4.0 delta adds two modules and extends one:

```
janus/src/
├── gateway/                  # 🌐 NEW — Stateless HITL Gateway
│   ├── mod.rs                #   Hermes Run envelope, Teams adapter, dispatch
│   └── teams.rs              #   Microsoft Teams Outgoing Webhook + Adaptive Cards
├── cognitive/                # 🔌 NEW — Cognitive Provider SPI
│   └── mod.rs                #   CognitiveProvider trait + OpenWiki adapter
├── tool_guard/               # 🛡️ EXTENDED — (existing 0.3.0)
│   ├── mod.rs                #   Rule engine (unchanged)
│   └── webhook.rs            #   ➕ HITL dispatch now delegates to gateway
└── protocol.rs               #   📜 EXTENDED — Contract 4.x additions
```

### Module Responsibilities

| Module | `pub mod` | Depends On | Purpose |
|---|---|---|---|
| `janus::gateway` | ✅ | `tool_guard` (for `WebhookPayload`), `protocol` (for Contract 4.x) | Out-of-band HITL dispatch: receives HITL trigger from `tool_guard`, formats per destination (TUI, Teams, Telegram), receives remote `Approve`/`Reject`, toggles `janus::tmux` session. |
| `janus::cognitive` | ✅ | `protocol` | SPI trait for domain knowledge providers. Queryable by `tool_guard` when a command requires blueprint-specific context validation. |
| `tool_guard::webhook` | Existing | `gateway` (new dep) | Refactored: instead of directly dispatching to Telegram, constructs `WebhookPayload` and calls `gateway::dispatch(payload)`. Existing `TelegramSender` becomes a `gateway` adapter. |

---

## III. Cognitive Provider SPI (Contract 4.1)

The core daemon never loads OpenWiki AST/graph data into its own heap. Instead, it queries an external provider through a narrow trait.

```rust
/// Contract 4.1 — Cognitive Provider SPI.
///
/// Implementations are opt-in and communicate via local IPC (Unix socket
/// or stdio). The daemon holds at most one active provider per blueprint;
/// providers are lazily started by `janus-daemon` on first query and
/// terminated on blueprint Offboard.
pub trait CognitiveProvider: Send + Sync {
    /// Validate whether a given command is consistent with the blueprint's
    /// domain constraints. Returns `None` when the provider has no opinion
    /// (pass-through); returns `Some(reason)` to recommend a BLOCK verdict
    /// with a human-readable explanation.
    fn validate_command(
        &self,
        blueprint: &str,
        argv: &[String],
        cwd: Option<&str>,
    ) -> Result<Option<String>, CognitiveError>;

    /// On Offboard, produce a consolidated knowledge artifact from the
    /// blueprint's execution history. Called once; output written to
    /// `blueprints/<name>/openwiki/production_report.md`.
    fn extract_knowledge(
        &self,
        blueprint: &str,
        task_history: &[TaskSummary],
    ) -> Result<String, CognitiveError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CognitiveError {
    #[error("provider unreachable: {0}")]
    Unreachable(String),
    #[error("provider timeout")]
    Timeout,
    #[error("provider internal error: {0}")]
    Internal(String),
}
```

**Design invariants:**
- **Fail-open on provider absence.** If no CognitiveProvider is configured for a blueprint, command validation skips with no penalty — `tool_guard`'s existing rule engine remains the sole gate.
- **Timeout-bounded.** Every IPC call to a provider carries a configurable deadline (default 5s). On timeout, the provider's verdict is discarded and the rule engine proceeds.
- **No persistent connection.** Providers are stateless from the daemon's perspective — each query is a self-contained request/response round-trip.

---

## IV. codebase-memory-mcp Integration (Contract 4.2)

AST/Tree-sitter processing is offloaded to a standalone `codebase-memory-mcp` service communicating via Anthropic MCP (JSON-RPC over stdio or localhost HTTP).

```
[janush command trap] → [tool_guard::Engine]
                              │
                              │ (if fault trace detected)
                              ▼
                     [cognitive::query_mcp()]
                              │
                              │ MCP: tools/call "resolve_symbol"
                              ▼
                     [codebase-memory-mcp]  ← external process
                              │
                              │ returns: { symbol, file, definition, callers[] }
                              ▼
                     [gateway::dispatch()] → Teams / TUI card
```

**Design invariants:**
- **Lazy, on-fault only.** MCP is not polled during normal execution. The daemon calls it only when `tool_guard`'s `require_approval` or `blacklist` cause triggers — appending symbol context to the HITL card payload.
- **Process isolation.** `codebase-memory-mcp` runs as a child process spawned by `janus-daemon` on first use. A crash or OOM in the MCP process never touches the daemon's memory space.
- **Transport.** MCP over stdio (default) with localhost HTTP fallback. The transport is configured per-blueprint in `janus.toml`:

```toml
# blueprints/<name>/janus.toml (new optional section)
[cognitive.codebase_memory]
transport = "stdio"          # "stdio" | "http"
command = "codebase-memory-mcp"
args = ["--workspace", "/path/to/repo"]
# For HTTP transport:
# endpoint = "http://127.0.0.1:9191"
timeout_secs = 5
```

---

## V. Stateless HITL Gateway (Contract 4.3)

The Gateway is an out-of-band proxy that decouples notification delivery from core runtime — a Teams webhook timeout or network partition never freezes the local tmux session.

```
[janush Traps Command]
        │
        ▼
[tool_guard::Engine] ── BLOCK verdict ──► [gateway::dispatch()]
        │                                        │
        │ (mark SUSPENDED,                        ├─► TUI (herdr-janus via UDS)
        │  tmux session frozen)                   ├─► TelegramSender (existing)
        │                                        └─► TeamsSender (new)
        ▼
   [await HITL response]  ◄── /v1/runs callback ── [Teams / TUI button]
        │
        ▼
   [gateway::apply_verdict()] → janus::tmux (release/kill session)
```

### 5.1 Hermes Run API Envelope (Contract 4.3a)

The Gateway exposes a local UDS endpoint (`janus-gateway.sock`) that speaks the Hermes Run API schema:

```
POST /v1/runs
{
  "run_id": "<correlation_id>",
  "status": "requires_action",
  "action": {
    "type": "hitl_approval",
    "payload": {
      "blueprint": "gatemetric",
      "task_id": "<uuid>",
      "step": "cross-compile",
      "command": "make CROSS_COMPILE=arm-none-eabi-",
      "cause": "require_approval: cross-compile on remote",
      "stdout_tail": "<16KB truncated>",
      "expires_at": "2026-07-18T00:00:00Z"
    }
  }
}
```

**Callback (Teams/TUI → Gateway):**
```
POST /v1/runs/{run_id}/actions
{
  "action": "approve" | "reject" | "override",
  "override_command": "<optional rewritten argv>",
  "approved_by": "hughguan@contoso.com",
  "timestamp": "2026-07-17T15:04:05Z"
}
```

### 5.2 Microsoft Teams Active Cards (Contract 4.3b)

The `TeamsSender` adapter translates the `WebhookPayload` into an Adaptive Card.

| Card Field | Source |
|---|---|
| Title | `"HITL: {cause}"` |
| Body | `command` + `stdout_tail` (budget-truncated) |
| Action: Approve | `POST /v1/runs/{run_id}/actions` with `action: "approve"` |
| Action: Reject | `POST /v1/runs/{run_id}/actions` with `action: "reject"` |
| Action: Override | `POST /v1/runs/{run_id}/actions` with `action: "override"` + `override_command` |

**Security model:**

- **Authentication.** The Teams Outgoing Webhook uses HMAC payload signing with a pre-shared secret (`JANUS_TEAMS_HMAC_SECRET`, provisioned in `/dev/shm`). The Gateway validates the HMAC on every inbound callback; unsigned or mismatched payloads are discarded.
- **Replay protection.** Each `run_id` accepts exactly one callback. Duplicate approvals for the same `run_id` return `409 Conflict`.
- **Scope isolation.** The callback payload carries the `run_id` only — it cannot reference other blueprints, tasks, or internal paths. The Gateway resolves the `run_id` to its owning blueprint and `tmux` session ID internally, structurally preventing cross-blueprint interference.
- **No reverse probe.** The Teams endpoint can only POST `action` verdicts to the `/actions` callback URI. There is no GET endpoint, no status query, and no mechanism to read environment variables or database state.

### 5.3 Gateway Trait (Contract 4.3c)

```rust
/// Contract 4.3c — HITL Gateway dispatch trait.
pub trait HitlGateway: Send + Sync {
    /// Dispatch a HITL card to all configured channels. Returns the
    /// correlation_id used to match the inbound callback.
    fn dispatch(&self, payload: &WebhookPayload) -> Result<String, GatewayError>;

    /// Block until a verdict arrives for the given correlation_id, or
    /// until the timeout expires (fail-closed: timeout = BLOCK).
    fn await_verdict(
        &self,
        correlation_id: &str,
        timeout: Duration,
    ) -> Result<Verdict, GatewayError>;
}

#[derive(Debug, Clone)]
pub enum Verdict {
    Approve,
    Reject,
    Override { rewritten_argv: Vec<String> },
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("channel unavailable: {0}")]
    ChannelError(String),
    #[error("verdict timeout")]
    Timeout,
    #[error("invalid callback signature")]
    AuthFailed,
}
```

---

## VI. Integration with Existing 0.3.0 Infrastructure

| 0.3.0 Component | 0.4.0 Change |
|---|---|
| `tool_guard::webhook::WebhookPayload` | **Unchanged.** Still the abstract HITL card. `webhook.rs` now calls `gateway::dispatch(payload)` instead of directly instantiating `TelegramSender`. |
| `tool_guard::webhook::TelegramSender` | **Moved** to `gateway/` as a `HitlGateway` adapter. |
| `tool_guard::webhook::LoggingSender` | **Reclassified** as the gateway's `null` channel (always fires; guarantees audit trail even when Teams/Telegram are down). |
| `absurd::SIZE_BUDGET` (16KB) | **Unchanged.** HITL cards honor the same budget. `stdout_tail` in the Hermes envelope is truncated by `truncate_16k()`. |
| `protocol::Response::GuardVerdict` | **Extended** — adds optional `cognitive_context` field (populated by CognitiveProvider when available). |
| `lifecycle::offboard()` | **Extended** — calls `CognitiveProvider::extract_knowledge()` if a provider is configured for the blueprint. |

---

## 🏁 0.4.0 Sign-Off

| Dimension | Status |
|---|---|
| Binary names | ✅ 0.3.0 baseline — no change |
| Physical execution engine | ✅ 0.3.0 baseline — `janus::tmux` only |
| Cognitive Provider SPI | 🔌 New — Contract 4.1 |
| codebase-memory-mcp integration | 🔌 New — Contract 4.2 |
| HITL Gateway | 🌐 New — Contract 4.3a/b/c |
| Teams Active Cards | 🌐 New — Contract 4.3b |
| Hermes Run API alignment | 🔄 Compatible — Contract 4.3a |
| Module tree placement | ✅ Defined in §II |
| Integration with 0.3.0 infra | ✅ Defined in §VI |

> **"0.4.0 keeps the 0.3.0 core lean — no AST parsing in the daemon heap, no notification I/O on the control-loop thread. The Cognitive SPI and HITL Gateway are external extensions that mount only when needed, insulating the physical tmux session from upstream service failures."**
