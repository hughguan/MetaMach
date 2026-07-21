# 🪐 MetaMach 0.4.0 — Architecture Delta Specification

**Gateway & Ecosystem Integrations** | **Status: Implemented**

> **0.4.0 Strategic Delta:** This document defines the **incremental architectural changes** from the 0.3.0 consensus baseline (`docs/ARCH-0.3.0.md`). It introduces the Cognitive Provider SPI, MCP-based symbol indexing, and an out-of-band HITL Gateway with Microsoft Teams Active Cards integration.

> **✅ Governance Note — Contracts 4.x Propagated.** Contracts 4.1, 4.2, and 4.3a–c have been formally propagated to the sibling specs:
> - `docs/Feature-Spec.md` — Contracts 4.1 (Cognitive Provider SPI), 4.2 (MCP Symbol Indexing), 4.3a (Hermes Run API), 4.3b (Teams Active Cards), 4.3c (WebhookPayload + Gateway Trait), plus two new fault matrix rows
> - `docs/Test-Spec.md` — UTC-10-01 through UTC-10-10 (Gateway dispatch, HTTP listener, timeout, HMAC, Teams cards, cognitive validate/timeout, extract_knowledge, payload enrichment, expires_at expiry)
> - `docs/Review-Spec.md` — 5 REV-GW-xx rows (callback ingress, timeout, dispatch, expiry, Teams format) + 3 REV-COG-xx rows (advisory check, timeout pass-through, extract_knowledge supplement)
>
> The cross-referencing fabric is intact for 4.x. H3 is closed.

---

## 🪐 The 0.4.0 Triad

```
       [ MetaMach 0.4.0 Industrial Suite ]
                       │
       ┌───────────────┼───────────────┐
       ▼               ▼               ▼
  【 janush 】    【 janus-daemon 】 【 janus::gateway 】
 (Interception)    (MM-CORE Brain)  (Hermes/Teams HITL)
```

| Component | Role | Key Invariant |
|-----------|------|---------------|
| **`janush`** | Invisible safety shell — synchronous UDS interception before any command reaches Bash | 30s Fail-Closed fuse; never lets through on timeout |
| **`janus-daemon`** | Background control brain — sole owner of state, DB connection pool, and `janus::tmux` PTY engine | Survives frontend crash; cold-start self-healing from Absurd PG checkpoints |
| **`janus::gateway`** | Physical portal — Hermes Run API envelope, Teams Adaptive Cards, Telegram inline keyboards | Payload-complete; non-blocking dispatch; tmux session never frozen |

> **Naming note:** The code module is `janus::gateway` (inside the `janus` crate). The conceptual name `mach-gateway` appears in branding and documentation to emphasize its role as the bridge between the human workshop and the bare-metal machine engine.

## 📋 0.4.0 Delta Matrix

| Component | 0.3.0 Baseline | 0.4.0 Delta | Verdict |
|---|---|---|---|
| **Knowledge Base** | In-memory context (blueprint-scoped) | **OpenWiki Cognitive Provider** (SPI, opt-in) | 🔌 New |
| **Symbol Indexing** | Not addressed | **codebase-memory-mcp** (MCP transport, opt-in) | 🔌 New |
| **HITL Routing** | Telegram webhook + local TUI (`tool_guard/webhook.rs`) | **HITL Gateway module** (`janus::gateway`) — payload-complete, no DB dependency | ✅ Extended |
| **Alert Protocol** | Custom `WebhookPayload` JSON (Contract 3.4 bundles) | **Hermes Run API schema** (`/v1/runs`) — compatible envelope with enriched fields | 🔄 Aligned |
| **Enterprise Reach** | Telegram Bot API | **Microsoft Teams Active Cards** (Outgoing Webhook adapter with HTTP ingress) | 🌐 New |
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
├── gateway/                  # 🌐 NEW — HITL Gateway (payload-complete)
│   ├── mod.rs                #   Hermes Run envelope, Teams adapter, HTTP listener
│   └── teams.rs              #   Microsoft Teams Outgoing Webhook + Adaptive Cards
├── cognitive/                # 🔌 NEW — Cognitive Provider SPI
│   └── mod.rs                #   CognitiveProvider trait + OpenWiki adapter
├── tool_guard/               # 🛡️ EXTENDED — (existing 0.3.0)
│   ├── mod.rs                #   Rule engine (unchanged)
│   └── webhook.rs            #   ➕ HITL dispatch now delegates to gateway
└── protocol.rs               #   📜 EXTENDED — Contract 4.x additions + shared types
```

### Module Responsibilities

| Module | `pub mod` | Depends On | Purpose |
|---|---|---|---|
| `janus::gateway` | ✅ | `protocol` (for shared `WebhookPayload` and `GatewayVerdict` types) | Out-of-band HITL dispatch: receives enriched HITL trigger from `tool_guard`, formats per destination (TUI, Teams, Telegram), hosts an HTTP listener for Teams callback ingress, receives remote `Approve`/`Reject`, toggles `janus::tmux` session. |
| `janus::cognitive` | ✅ | `protocol` | SPI trait for domain knowledge providers. Queryable by `tool_guard` when a command requires blueprint-specific context validation. |
| `tool_guard::webhook` | Existing | `gateway` (new dep) | Refactored: instead of directly dispatching to Telegram, constructs enriched `WebhookPayload` (now in `protocol.rs`) and calls `gateway::dispatch(payload)`. Existing `TelegramSender` becomes a `gateway` adapter. |

> **Dependency cycle resolved:** `WebhookPayload` and `GatewayVerdict` are moved to `protocol.rs` (the shared-types crate root). Both `tool_guard` and `gateway` depend on `protocol` — no circular dependency. The `tool_guard → gateway` edge is one-way: `tool_guard` calls `gateway::dispatch()` but `gateway` never imports `tool_guard`.

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

    /// On Offboard, produce a condensed knowledge artifact for the blueprint.
    /// The returned string is written to `production_report.md` **in addition
    /// to** (not replacing) the existing LLM smelt output from `lifecycle::offboard()`.
    /// This is a supplement, not a substitute — the LLM smelt path (Feature-Spec §2.5, Offboard LLM Integration Spec)
    /// remains the primary report generator.
    fn extract_knowledge(
        &self,
        blueprint: &str,
    ) -> Result<String, CognitiveError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CognitiveError {
    #[error("provider not reachable: {0}")]
    Unreachable(String),
    #[error("query timeout")]
    Timeout,
}
```

**0.4.0 delta refinements (from the baseline CognitiveProvider draft):**
- `extract_knowledge` signature simplified: dropped the `task_history: &[TaskSummary]` parameter (the provider can read the blueprint DB directly if needed); now takes only `&self` + `blueprint: &str`.
- `CognitiveError::Internal` variant removed — all provider errors are either `Unreachable` (connection/startup failure) or `Timeout` (query exceeded deadline). Internal provider errors are the provider's own responsibility to handle.
- `validate_command` timeout tightened from 5s to 2s (the cognitive check is advisory, not gating; a shorter timeout keeps the Tool Guard verdict path fast).

**Design invariants:**

- **Cannot block the tmux session.** `validate_command` runs in the `tool_guard` verdict path but with a hard timeout (default 2s). If the provider times out, the daemon proceeds with the standard Tool Guard verdict — the cognitive check is advisory, not gating. The tmux session is never frozen waiting for a cognitive provider.
- **Cannot read database state.** The provider receives only the `argv` + `cwd` + `blueprint` name. It has no access to `absurd_steps`, `blueprints` metadata, or any other PG table.
- **Opt-in per blueprint.** Providers are configured in `blueprints/<name>/janus.toml` under a `[cognitive]` section. Blueprints without this section have no cognitive provider.

---

## IV. codebase-memory-mcp (Contract 4.2)

The **codebase-memory-mcp** MCP server provides symbol-level indexing for the blueprint's codebase. Integration is via the MCP transport protocol.

```
┌──────────────┐  MCP (stdio)  ┌──────────────────────┐
│ janus-daemon │◄─────────────►│ codebase-memory-mcp  │
│  cognitive   │               │  (external process)  │
│  provider    │               │  Symbol index & AST  │
└──────────────┘               └──────────────────────┘
```

The daemon spawns the MCP server as a child process on first use and communicates via stdin/stdout JSON-RPC. The server is terminated on blueprint Offboard.

**Scope:** The MCP server indexes only the blueprint's own source tree (`blueprints/<name>/`). It has no access to the daemon's source, other blueprints, or the host filesystem outside the blueprint root.

---

## V. HITL Gateway (Contracts 4.3a–c)

The HITL Gateway is a **payload-complete** out-of-band dispatch module. It receives a fully-enriched `WebhookPayload` from `tool_guard` (containing all fields needed by every adapter), formats it per destination, and hosts an HTTP listener for inbound Teams callbacks. The gateway performs **no database lookups** — all data it needs is in the request payload.

### 5.1 Hermes Run API Envelope (Contract 4.3a)

The gateway exposes a local HTTP listener on `127.0.0.1` for inbound Teams callbacks. External reachability is provided by a tunnel or reverse proxy (see §5.1b).

**Outbound (Gateway → Teams):**
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

**Callback (Teams → Gateway HTTP listener):**
```
POST /v1/runs/{run_id}/actions
{
  "action": "approve" | "reject" | "override",
  "override_command": "<optional rewritten argv>",
  "approved_by": "hughguan@contoso.com",
  "timestamp": "2026-07-17T15:04:05Z"
}
```

**`expires_at` semantics:** The `expires_at` timestamp is set to `now + HITL_TIMEOUT` (default 30 minutes). If the callback arrives after `expires_at`, the gateway returns `410 Gone`. This bounds the window for replay attacks and prevents stale approvals from resurrecting long-abandoned sessions. The timeout is configurable via `JANUS_HITL_TIMEOUT_SECS`.

### 5.1b HTTP Ingress (Teams Callback Path)

The gateway includes a built-in HTTP listener (`tokio::net::TcpListener`) that binds to `127.0.0.1:<port>` (default `8443`, configurable via `JANUS_GATEWAY_LISTEN_PORT`). This listener exposes exactly one endpoint:

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/v1/runs/{run_id}/actions` | Teams callback ingress |

**External reachability:** Microsoft Teams Outgoing Webhooks require an HTTPS URL. The gateway's internal HTTP listener is **not** directly reachable by Teams. A tunnel or reverse proxy must bridge the gap:

- **Recommended (development/CI):** cloudflared tunnel (`cloudflared tunnel --url http://127.0.0.1:8443`)
- **Recommended (production):** nginx/Caddy reverse proxy with Let's Encrypt TLS termination
- **Documented in:** `docs/Deployment-Spec.md` §7 (Gateway Ingress)

The tunnel/proxy is a **deployment prerequisite**, not part of the daemon binary. The daemon itself never handles TLS or exposes a public port — it only binds to loopback.

### 5.1c await_verdict Threading Model

The `await_verdict` method blocks the **gateway's dedicated verdict thread**, not the tmux control thread. The threading architecture:

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  tool_guard      │────►│  gateway          │     │  janus::tmux    │
│  (control-loop)  │     │  ┌──────────────┐ │     │  (session mgr)  │
│                  │     │  │ verdict thread│ │     │                 │
│  dispatch() ─────┼────►│  │ await_verdict │ │     │  tmux session   │
│  (non-blocking)  │     │  │ (blocks here) │ │     │  (runs freely)  │
│  return          │     │  └──────────────┘ │     │                 │
│  continue loop   │     │  HTTP listener    │     │  never frozen    │
└─────────────────┘     └──────────────────┘     └─────────────────┘
```

1. `tool_guard` calls `gateway::dispatch(payload)` — non-blocking; the gateway spawns a verdict thread and returns immediately.
2. The tmux session continues running uninterrupted — the Agent's process is not paused.
3. The verdict thread blocks on `await_verdict(correlation_id, timeout)`.
4. When the callback arrives (or timeout expires), the verdict thread signals `tool_guard` via a oneshot channel.
5. `tool_guard` applies the verdict (Approve/Reject/Override) to the next `janush` command check.

**Invariant: the tmux session is never frozen waiting for a HITL verdict.** The control loop continues processing other requests; only the specific step's verdict is gated.

### 5.2 Microsoft Teams Active Cards (Contract 4.3b)

The `TeamsSender` adapter translates the enriched `WebhookPayload` into an Adaptive Card.

| Card Field | Source |
|---|---|
| Title | `"HITL: {cause}"` |
| Body | `command` + `stdout_tail` (budget-truncated) |
| Action: Approve | `POST /v1/runs/{run_id}/actions` with `action: "approve"` |
| Action: Reject | `POST /v1/runs/{run_id}/actions` with `action: "reject"` |
| Action: Override | `POST /v1/runs/{run_id}/actions` with `action: "override"` + `override_command` |

**Security model:**

- **Authentication.** The Teams Outgoing Webhook uses HMAC payload signing with a pre-shared secret (`JANUS_TEAMS_HMAC_SECRET`, provisioned in the platform-appropriate secret directory: `/dev/shm` on Linux, `$TMPDIR` on macOS per the existing `make ram-disk` workaround in Deployment-Spec §1). The Gateway validates the HMAC on every inbound callback; unsigned or mismatched payloads are discarded.
- **Replay protection.** Each `run_id` accepts exactly one callback. Duplicate approvals for the same `run_id` return `409 Conflict`.
- **Scope isolation.** The callback payload carries the `run_id` only — it cannot reference other blueprints, tasks, or internal paths. The Gateway resolves the `run_id` to its owning blueprint and `tmux` session ID via an in-memory pending-verdict map (populated by `dispatch()`, indexed by `run_id`), structurally preventing cross-blueprint interference.
- **No reverse probe.** The Teams endpoint can only POST `action` verdicts to the `/actions` callback URI. There is no GET endpoint, no status query, and no mechanism to read environment variables or database state.

### 5.3 Gateway Trait (Contract 4.3c)

```rust
/// Contract 4.3c — HITL Gateway dispatch trait.
pub trait HitlGateway: Send + Sync {
    /// Dispatch a HITL card to all configured channels. Returns
    /// `Ok(())` on success; the `correlation_id` is already in
    /// `payload.correlation_id` (the gateway never mints it).
    /// Non-blocking: spawns a verdict thread and returns immediately.
    fn dispatch(&self, payload: &WebhookPayload) -> Result<(), GatewayError>;

    /// Block until a verdict arrives for the given correlation_id, or
    /// until the timeout expires (fail-closed: timeout = BLOCK).
    /// Called from the gateway's dedicated verdict thread, never from
    /// the tmux control thread.
    ///
    /// `timeout` is `Duration::from_secs(JANUS_HITL_TIMEOUT_SECS)` —
    /// the same deadline as `expires_at`. A late callback gets `410 Gone`
    /// from the HTTP listener; the awaiter gets `Err(Timeout)` → BLOCK.
    fn await_verdict(
        &self,
        correlation_id: &str,
        timeout: Duration,
    ) -> Result<GatewayVerdict, GatewayError>;
}

#[derive(Debug, Clone)]
pub enum GatewayVerdict {
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

> **Name collision note:** `GatewayVerdict` is distinct from `tool_guard::Verdict` (the existing `ALLOW | BLOCK | REWRITE` enum). The gateway's verdict is about HITL approval (Approve/Reject/Override); the tool guard's verdict is about command interception. They are separate types in separate modules with no overlap.

### 5.4 Enriched WebhookPayload (moved to protocol.rs)

The existing `WebhookPayload` is extended with the fields needed by the Hermes envelope and moved to `protocol.rs` to break the `tool_guard ↔ gateway` dependency cycle. Both modules depend on `protocol`, not on each other.

```rust
/// Shared HITL card type (in `protocol.rs`).
/// Enriched with Hermes Run API fields for Teams adapter compatibility.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    // Existing fields (unchanged from 0.3.0)
    pub task_id: Option<Uuid>,
    pub execution_id: String,
    pub correlation_id: String,        // == Hermes run_id
    pub cause: String,
    pub command: String,
    pub reason: String,
    pub scene: String,                 // 16KB-truncated; legacy alias for stdout_tail
    pub resume_key: String,            // "metamach-resume:{correlation_id}"

    // 0.4.0 enrichment for Hermes / Teams adapter
    pub blueprint: String,             // owning blueprint name
    pub step: String,                  // current step name
    pub stdout_tail: String,           // 16KB-truncated; canonical field (Hermes naming);
                                       // always equal to `scene` at construction
    pub expires_at: String,            // ISO 8601; now + HITL_TIMEOUT
}
```

> **`correlation_id` semantics:** `correlation_id` is the single source of truth for matching outbound cards to inbound callbacks. It is generated once by `tool_guard` per HITL event and passed through the entire chain (`tool_guard → gateway::dispatch → Hermes run_id → Teams card → callback → gateway::await_verdict`). It is never re-assigned or duplicated. The `resume_key` field is a derived convenience (`"metamach-resume:{correlation_id}"`) for the Telegram inline keyboard.

---

## VI. Integration with Existing 0.3.0 Infrastructure

| 0.3.0 Component | 0.4.0 Change |
|---|---|
| `tool_guard::webhook::WebhookPayload` | **Moved** to `protocol.rs` and **enriched** with `blueprint`, `step`, `stdout_tail`, `expires_at`. `webhook.rs` now calls `gateway::dispatch(payload)` instead of directly instantiating `TelegramSender`. |
| `tool_guard::webhook::TelegramSender` | **Moved** to `gateway/` as a `HitlGateway` adapter. |
| `tool_guard::webhook::LoggingSender` | **Reclassified** as the gateway's `null` channel (always fires; guarantees audit trail even when Teams/Telegram are down). |
| `absurd::SIZE_BUDGET` (16KB) | **Unchanged.** HITL cards honor the same budget. `stdout_tail` in the Hermes envelope is truncated by `truncate_16k()`. |
| `protocol::Response::GuardVerdict` | **Extended** — adds optional `cognitive_context` field (populated by CognitiveProvider when available). |
| `lifecycle::offboard()` | **Extended** — calls `CognitiveProvider::extract_knowledge()` if a provider is configured for the blueprint. The output is **appended** to the LLM smelt `production_report.md` (supplement, not replacement). |

---

## 🏁 0.4.0 Sign-Off

| Dimension | Status |
|---|---|
| Binary names | ✅ 0.3.0 baseline — no change |
| Physical execution engine | ✅ 0.3.0 baseline — `janus::tmux` only |
| Cognitive Provider SPI | 🔌 New — Contract 4.1 |
| codebase-memory-mcp integration | 🔌 New — Contract 4.2 |
| HITL Gateway (payload-complete) | 🌐 New — Contract 4.3a/b/c |
| HTTP ingress for Teams callbacks | 🌐 New — §5.1b (loopback listener + tunnel prerequisite) |
| Teams Active Cards | 🌐 New — Contract 4.3b |
| Hermes Run API alignment | 🔄 Compatible — Contract 4.3a |
| WebhookPayload enrichment | 🔄 Extended — blueprint, step, stdout_tail, expires_at |
| Module tree placement | ✅ Defined in §II |
| Dependency cycle (tool_guard ↔ gateway) | ✅ Resolved — shared types in `protocol.rs` |
| Integration with 0.3.0 infra | ✅ Defined in §VI |
| Contracts 4.x governance propagation | ✅ Propagated to Feature-Spec (Contracts 4.1–4.3c), Test-Spec (UTC-10-xx), Review-Spec (REV-GW/COG-xx) |

> **"0.4.0 keeps the 0.3.0 core lean — no AST parsing in the daemon heap, no notification I/O on the control-loop thread. The Cognitive SPI and HITL Gateway are external extensions that mount only when needed, insulating the physical tmux session from upstream service failures. The gateway's verdict thread blocks independently; the tmux session is never frozen."**
