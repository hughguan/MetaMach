# ADR-034: Typed Context Envelopes for Absurd PG Checkpoints

* **Status:** Proposed (0.7.0 Candidate)
* **Date:** 2026-08-12
* **Target Version:** MetaMach 0.7.0
* **Author:** MetaMach Architecture Group
* **Amends/Extends:** ADR-021 (Absurd Durable Tasks), ADR-031 (Unified Workflow DSL)

---

## 1. Context & Problem Statement

In MetaMach 0.6.0, step execution state is stored in Absurd PostgreSQL via `DurableEngine::set_checkpoint()`. Currently, step payloads are persisted as unconstrained `serde_json::Value` objects (e.g. `json!({"step": step.name, "status": "COMPLETED", "exit": 0})`).

When AI agents execute multi-step workflows, raw stdout output, loose strings, and unvalidated JSON objects can bleed into Absurd PG checkpoint state. This causes two main problems:
1. **Context Pollution**: Subsequent steps receive unvalidated prompt text, increasing token consumption and risk of hallucination.
2. **Storage Overhead**: Unstructured output text bloats the Absurd catalog tables (`absurd.c_<queue>`).

---

## 2. Decision

We decide to introduce **Typed Context Envelopes (`TypedEnvelope`)** for checkpoint persistence in MetaMach 0.7.0.

### Key Resolution Points:

1. **Serde Schema Requirement**: All cross-step data outputs must implement the `TypedEnvelope` trait and deserialize cleanly into structured Rust Serde types before checkpointing.
2. **Standard Base Envelope (`EnvelopeBase`)**: Every envelope includes standard status metadata, summary, produced artifacts list, and next-agent advisory notes.
3. **Validation Gateway**: The workflow engine (`janus::workflow`) runs envelope deserialization prior to calling `DurableEngine::set_checkpoint()`. Invalid output triggers a schema error before database write.

---

## 3. Detailed Specification

### 3.1 Rust Envelope Definitions (`janus/src/workflow/envelope.rs`)

```rust
use serde::{Deserialize, Serialize};

pub trait TypedEnvelope: Serialize + for<'de> Deserialize<'de> {
    fn validate(&self) -> Result<(), EnvelopeError>;
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EnvelopeBase {
    pub status: String, // "completed" | "failed" | "suspended"
    pub summary: String,
    pub artifacts: Vec<String>,
    pub notes_for_next_agent: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BuildEnvelope {
    #[serde(flatten)]
    pub base: EnvelopeBase,
    pub changed_files: Vec<String>,
    pub commit_hash: Option<String>,
}

impl TypedEnvelope for BuildEnvelope {
    fn validate(&self) -> Result<(), EnvelopeError> {
        if self.base.status.is_empty() {
            return Err(EnvelopeError::MissingField("status".into()));
        }
        Ok(())
    }
}
```

### 3.2 Workflow Checkpoint Validation (`janus/src/workflow/mod.rs`)

```rust
pub fn checkpoint_envelope<T: TypedEnvelope>(
    engine: &dyn DurableEngine,
    queue: &str,
    task_id: &str,
    step_name: &str,
    envelope: &T,
    run_id: i64,
) -> Result<(), WorkflowError> {
    envelope.validate()?;
    let payload = serde_json::to_value(envelope)?;
    engine.set_checkpoint(queue, task_id, step_name, &payload, run_id)?;
    Ok(())
}
```

---

## 4. Consequences

### Positive:
- **Clean State Boundaries**: Eliminates unstructured prompt leakage into PostgreSQL checkpoints.
- **Token Reduction**: Downstream agents consume compact, validated context envelopes rather than raw terminal stdout.
- **Strict Contracts**: Clear data schema expectations between Architect, Builder, and Tester roles.

### Negative / Cost:
- **Schema Overhead**: Agents must format output to match expected JSON envelope structure.
