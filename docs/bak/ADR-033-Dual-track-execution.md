# ADR-033: Dual-Track Execution Isolation & Post-Execution Writes Guard

* **Status:** 🔄 Phase 2a Implemented (Worktree + Isolated Tmux) / Phase 2b Spec'd Only (Unprivileged OS User) (0.7.0)
* **Date:** 2026-08-12
* **Target Version:** MetaMach 0.7.0
* **Amends:** ADR-001 (De-containerization), ADR-007 (Fail-Closed 30s Timeout Interception), ADR-019 (Configurable Agents - Provisioning, Quota & Fallback)
* **Dependencies:** ADR-036 (Harvest Pipeline) depends on Phase 1 of this ADR.

## 1. Context & Problem Statement
Currently, MetaMach executes all steps on the host bare-metal node. We need a way to run untrusted or experimental agent steps in isolation, without risking the main workspace state, and verify their outputs before merging.

## 2. Options Considered
* **Docker/Podman Containers:** Rejected because ADR-001 explicitly removed Docker as a hard dependency to maintain the "No Docker required" host-native competitive moat.
* **Persistent VMs:** Too heavy and slow for step-level isolation.
* **Host-Native Isolation (Chosen):** Extend ADR-001's philosophy by using host-native tools: a separate tmux server, an unprivileged OS user, and a separate Git worktree per sandbox run.

## 3. Decision
We will implement host-native isolation (Dual-Track Execution) for steps marked as sandboxed. This aligns with and extends our de-containerization strategy (ADR-001). We will also implement a Post-Execution Writes Guard to verify changes before they are applied. Auto-rollback of unauthorized changes will not be destructive; instead, unauthorized changes will be snapshotted to a recovery ref, suspending the step and escalating for HITL review.

Single-instance sandbox isolation (`isolation = "sandbox"`) will ship in Phase 1. Multi-instance parallel `best_of_n` exploration will be deferred to Phase 2 after a dedicated pre-provisioned user-pool (`janus-sandbox-01..N`) management mechanism lands, preventing daemon security issues related to creating OS users on the fly.

## 4. Detailed Specification

### 4.1 Host-Native Isolation
Sandboxing is achieved via:
- A separate tmux server: `tmux -L metamach-sandbox-<id>`
- Execution under a pre-provisioned unprivileged OS user (`janus-sandbox-worker`).
- A dedicated Git worktree per sandbox run to avoid colliding with main workspace modifications.

### 4.2 Recipe DSL Additions
We extend the private `DagNodeDef` struct in `src/recipe.rs:207-215` with new optional fields (`isolation`, `best_of_n`, `writes`) while preserving existing fields like `workflow`:

```rust
#[derive(Debug, Clone, Deserialize)]
struct DagNodeDef {
    pub id: String,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub workflow: Option<String>,
    #[serde(default)]
    pub steps: Option<Vec<WorkflowStep>>,
    #[serde(default)]
    pub isolation: Option<String>, // e.g., "sandbox"
    #[serde(default)]
    pub best_of_n: Option<u8>,     // Deferred to Phase 2 user-pool implementation
    #[serde(default)]
    pub writes: Option<Vec<String>>, // Allowed write paths
}
```

Example `.janus/workflows/demo.toml` including explicit `agent` fields on inline steps:

```toml
[[nodes]]
id = "build_web_ui"
isolation = "sandbox"
writes = ["apps/web/dist/", "apps/web/package.json"]
steps = [
  { name = "install", agent = "builder_claude", command = "bun install" },
  { name = "test", agent = "builder_claude", command = "bun test" }
]
```

### 4.3 Post-Execution Writes Guard & Parallel DAG Execution
To prevent parallel DAG nodes from producing interleaved diffs that cannot be attributed per-node, the `PostExecutionGuard` is restricted to linear mode steps only, **unless** each DAG sandbox node operates in its own isolated Git worktree.

If an agent makes unauthorized changes (outside the allowed `writes` list), the guard triggers. To align with the HITL philosophy (escalate, not destroy), the system will:
1. Snapshot unauthorized changes to a recovery ref (`refs/metamach/rollback/<step_id>`).
2. Suspend the step.
3. Escalate via the HITL gateway for human review.

## 5. Consequences
* **Positive:** Safely execute experimental steps without risking main workspace corruption.
* **Positive:** Preserves the "No Docker required" moat by utilizing host-native isolation techniques.
* **Positive:** Non-destructive escalations maintain HITL principles.
* **Negative:** Increased complexity in managing Git worktrees, tmux servers, and pre-provisioned user pools.
