# ADR-034: Typed Context Envelopes for Absurd PG Checkpoints

* **Status:** Proposed (0.7.0 Candidate)
* **Date:** 2026-08-12
* **Target Version:** MetaMach 0.7.0
* **Amends:** `docs/contracts/absurd.md`

## 1. Context & Problem Statement

Currently, MetaMach stores checkpoint state via `set_checkpoint` as minimal structured JSON (e.g., `{"task_id": task_id, "reason": "hitl_rejected"}`). Separately, raw stdout is captured in `metamach_step_meta.stdout_tail` by design, which is subject to the 16 KiB scene cap defined in ADR-008.

As the complexity of states restored during execution failures or Human-in-the-Loop (HITL) interventions increases, storing ad-hoc unstructured or weakly structured JSON in the checkpoint state object poses maintainability and type-safety risks. We need a formalized, strongly-typed envelope for these checkpoint states to guarantee schema validation on resumption, without interfering with the existing `stdout_tail` mechanism.

## 2. Options Considered

1. **Continue with Ad-hoc JSON**: Simple, requires no engine changes, but lacks type safety and validation, leading to runtime failures upon resumption.
2. **Schema Registries**: Use an external schema registry (e.g., Protobuf, JSON Schema) to validate checkpoint states. Overly complex and introduces new operational dependencies.
3. **Typed Context Envelopes in Rust**: Define Rust structs for checkpoint state with strict serde serialization, directly integrated into the `DurableEngine::set_checkpoint` flow. This leverages the existing PG checkpointing mechanism while providing compile-time safety and runtime validation.

## 3. Decision

We will implement Typed Context Envelopes in Rust (Option 3). This introduces structured envelopes specifically for the checkpoint state object stored in Absurd PG.

Crucially, envelopes are **PG-checkpoint-only**. The logical "step output" that flows through the `protocol.rs` scene (like HITL cards and progress snapshots) will continue to use the existing truncated `stdout_tail` path and remain subject to the 16 KiB cap (ADR-008).

## 4. Detailed Specification

### Checkpoint Envelope Schema

The typed envelope will encapsulate the state passed to `set_checkpoint`:

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckpointEnvelope {
    pub version: u8,
    pub task_id: Uuid,
    pub reason: String,
    pub state_data: Value,
}
```

### Usage with `DurableEngine`

The checkpoint state will be serialized into this envelope before being passed to `set_checkpoint`. The signature for `set_checkpoint` remains aligned with `DurableEngine`:

```rust
use metamach::futures::BoxFut;
use anyhow::Result;
use uuid::Uuid;
use serde_json::Value;

async fn checkpoint_envelope<'a>(
    engine: &'a dyn DurableEngine,
    queue: &'a str,
    task_id: Uuid,
    step: &'a str,
    envelope: &'a CheckpointEnvelope,
    owner_run: Uuid,
) -> Result<()> {
    let state_json = serde_json::to_value(envelope)?;
    
    // Call set_checkpoint, which returns BoxFut<'a, Result<()>>
    engine.set_checkpoint(
        queue,
        task_id,
        step,
        &state_json,
        owner_run
    ).await
}
```

This ensures that all checkpoints written to the database conform to the `CheckpointEnvelope` structure.

## 5. Consequences

* **Positive**: Stronger guarantees on the shape of checkpoint data during recovery.
* **Positive**: Clear separation of concerns between state checkpointing and stdout truncation (ADR-008).
* **Negative**: Requires migrating or handling legacy unstructured checkpoints during the upgrade to 0.7.0.
