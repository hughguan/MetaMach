# ADR-033 Review: Dual-Track Execution, Typed Envelopes & Post-Execution Governance

> **Verdict: 🟡 Not Ready — Requires Significant Rework Before Acceptance**
> 
> The proposal addresses real architectural gaps but has format violations, version contradictions, scope overload, and code-level inaccuracies against the 0.6.0 codebase.

---

## Critical Issues

### C1. Language Violation — English is the Sole Spec Source

Per [`AGENTS.md`](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/AGENTS.md):

> *"docs/ (English) is the sole version-controlled spec source. … Sync direction is always from docs/ to docs/CH/, never the reverse."*

ADR-033 is written almost entirely in Chinese. If accepted, it **must** be rewritten in English for `docs/ADR.md`. The Chinese version would go to `docs/CH/` as a translation artifact.

### C2. ADR Format Non-Compliance

Established ADR format ([ADR-031](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/docs/ADR.md#L439-L447), [ADR-032](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/docs/ADR.md#L451-L460)) uses a **`| Field | Value |` table** with fields: `Context`, `Options Considered`, `Decision`, `Rationale`, `Status`.

ADR-033 uses a completely different structure (numbered prose sections: `1. Context`, `2. Decision`, `3. Detailed Design`, `4. Consequences`, `5. Action Items`). This is incompatible with the ADR.md append format.

> [!IMPORTANT]
> Must be reformatted into the standard `| Field | Value |` table before merging into `docs/ADR.md`.

### C3. Version Target Contradiction

| Location | Claims |
|---|---|
| YAML header (L5) | `Target Version: MetaMach 0.7.0` |
| Section 1 body (L21) | *"在 MetaMach **0.6.0** 中对系统架构进行全方位的'双轨扩展'"* |

These contradict each other. Given that 0.6.0 has already shipped (tagged, CI-green, 193 tests), this should target **0.7.0** consistently throughout.

### C4. Scope Overload — Should Be Split into 3–4 ADRs

The proposal bundles **6 distinct architectural decisions** into a single ADR. Each of these has independent design space, trade-offs, and implementation scope:

| Proposed Sub-Decision | Recommended Split |
|---|---|
| Dual-track isolation (`sandbox` / `bare_metal`) | **ADR-033** (standalone, large scope) |
| Typed context envelopes (Serde validation) | **ADR-034** (checkpoint schema governance) |
| Session-ID correction retry loop | **ADR-035** (agent self-healing) |
| Post-execution `writes` path guard | Could fold into ADR-033 or separate ADR-036 |
| Dynamic API credential lifecycle | **ADR-037** (credential provisioning) |
| Herdr TUI `refs/sandbox/*` harvest | Part of ADR-033 sandbox track |

The existing ADR convention is **one decision per ADR** (or tightly coupled decisions). Bundling credential management with DSL schema changes makes review, implementation tracking, and rollback nearly impossible.

---

## Codebase Accuracy Issues

### A1. Wrong API Names in Code Examples

| ADR-033 Reference | Actual Codebase | File |
|---|---|---|
| `AbsurdEngine` | `DurableEngine` (trait) | [absurd/adapter.rs:96](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/absurd/adapter.rs#L96) |
| `save_checkpoint()` | `set_checkpoint()` | [absurd/adapter.rs:102](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/absurd/adapter.rs#L102) |
| `StepError::InvalidEnvelopeSchema` | Does not exist | — |
| `impl WorkflowStep { fn validate_and_checkpoint }` | `WorkflowStep` is a plain data struct, no methods | [recipe.rs:92](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/recipe.rs#L92) |

### A2. DSL Field Name Mismatches

| ADR-033 TOML | Existing Codebase | Struct |
|---|---|---|
| `depends_on = [...]` | `needs = [...]` | [DagNodeDef](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/recipe.rs#L207-L215) |
| `steps = [{ id = "install", ... }]` | Steps use `name`, not `id` | [WorkflowStep](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/recipe.rs#L92-L103) |
| `agent = "builder_claude"` on `[[nodes]]` | `agent` is a field on `WorkflowStep`, not on `DagNodeDef` | [recipe.rs:207](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/recipe.rs#L207) |
| `janush_safety = "suspend_30s"` | No such field exists anywhere | — |

### A3. Action Items Reference Non-Existent Paths

| ADR-033 Path | Reality |
|---|---|
| `src/workflow/parser.rs` | **Does not exist.** Parsing is in [src/recipe.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/recipe.rs) |
| `src/workflow/envelope.rs` | Does not exist (new file — OK for a proposal, but should reference recipe.rs for DSL changes) |
| `src/tool_guard/post_guard.rs` | Does not exist (new file — OK) |

---

## Architectural Concerns

### AR1. Vendor Lock-In: OpenRouter Hard Dependency

The proposal specifies *"OpenRouter API → dynamic $50 Cap provisioning key"* (§3.5). MetaMach's cognitive SPI ([`src/cognitive/`](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/cognitive/)) is deliberately vendor-agnostic. Hard-coding OpenRouter as the credential provider contradicts this design.

**Recommendation:** Abstract credential provisioning behind a `CredentialProvider` trait with OpenRouter as one possible backend.

### AR2. Best-of-N Selection Criteria Unspecified

The proposal introduces `best_of_n = 3` but never defines:
- **Selection function**: How does the system pick the "best" of N parallel sandbox runs? Test pass rate? Token efficiency? Code diff size?
- **Resource accounting**: 3 parallel runs = 3× compute + 3× LLM token cost. How does this interact with `AgentQuota` limits in [agent.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/agent.rs)?
- **Checkpoint semantics**: The current `set_checkpoint` model assumes one linear progression per task. N parallel runs need a tournament/merge checkpoint model.

### AR3. Git Dependency Assumption

`PostExecutionGuard` (§3.4) requires the workspace to be a Git repository (`git diff`, `git checkout` rollback). This is reasonable for most SDLC use cases but should be:
1. Documented as a hard prerequisite in the ADR.
2. Gracefully degraded (skip guard, log warning) for non-Git workspaces rather than panicking.

### AR4. Session-ID Correction vs. Current Architecture

The proposal adds `--session-id` retry to [agent.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/agent.rs), but the current `agent.rs` has **zero session tracking** — it's a provisioning/quota/preflight module. The actual step execution lives in [workflow/mod.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/workflow/mod.rs) (`run_steps`). The correction retry loop should be integrated into `run_steps`, not `agent.rs`.

### AR5. Amends/Extends Claims — Partially Inaccurate

| Claimed Amendment | Verdict |
|---|---|
| ADR-004 (SQLite Fallback) | ❌ **Not amended.** Typed envelopes affect PG checkpoints, not the SQLite fallback ring. |
| ADR-007 (Fail-Closed 30s) | ❌ **Not amended.** Post-execution guard is orthogonal to the 30s UDS timeout. |
| ADR-019 (Agent Provisioning) | ✅ Reasonably extended by dynamic credential provisioning. |
| ADR-031 (Unified DSL) | ✅ Directly extended by `isolation`, `writes`, `best_of_n` fields. |

---

## What's Good

Despite the issues above, the proposal identifies **real gaps** worth solving:

1. **Post-execution writes guard** fills a genuine blind spot — Tool Guard is pre-execution only ([tool_guard/mod.rs](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/tool_guard/mod.rs) `evaluate()` is command-level AST/regex). File-system side-effect auditing is a valuable defense-in-depth layer.

2. **Typed context envelopes** would reduce checkpoint bloat. The current checkpoint payload is a raw `serde_json::Value` ([workflow/mod.rs:512](file:///Volumes/Ext.Home/hughguanEX/Workspace/metamach/janus/src/workflow/mod.rs#L512): `json!({"step": step.name, "status": "COMPLETED", "exit": 0})`). Schema validation before PG persistence is sound engineering.

3. **Dual-track isolation** is the right direction for expanding MetaMach beyond bare-metal-only execution, especially for pure-software SDLC tasks.

4. **In-session correction** over cold restart is a significant token-cost optimization that aligns with the existing Absurd PG checkpoint-and-resume model.

---

## Recommended Actions

| # | Action | Priority |
|---|---|---|
| 1 | **Rewrite in English** for `docs/ADR.md` compliance | 🔴 Blocking |
| 2 | **Reformat** to standard `\| Field \| Value \|` table structure | 🔴 Blocking |
| 3 | **Fix version** — consistently target `0.7.0` | 🔴 Blocking |
| 4 | **Split into 3–4 focused ADRs** (dual-track, envelopes, session-correction, credentials) | 🟡 Strongly Recommended |
| 5 | **Fix code examples** to use actual API names (`DurableEngine`, `set_checkpoint`, `needs`, `name`) | 🟡 Required |
| 6 | **Fix action item paths** (`recipe.rs` not `parser.rs`) | 🟡 Required |
| 7 | **Define Best-of-N selection** criteria, quota interaction, and checkpoint model | 🟡 Required |
| 8 | **Abstract credential provider** — trait instead of OpenRouter hard-code | 🟡 Required |
| 9 | **Correct Amends/Extends** — drop ADR-004 and ADR-007 claims | 🟢 Cleanup |
| 10 | **Add ADR-032 interaction** — Studio visualization of dual-track sandbox state | 🟢 Nice-to-have |
