# ADR-035: In-Session Correction Retry Loop for Step Self-Healing

* **Status:** Proposed (0.7.0 Candidate)
* **Date:** 2026-08-12
* **Target Version:** MetaMach 0.7.0
* **Author:** MetaMach Architecture Group
* **Amends/Extends:** ADR-019 (Pipeline DAG Engine), ADR-034 (Typed Context Envelopes)

---

## 1. Context & Problem Statement

When an AI agent produces invalid JSON output or fails a test gate in MetaMach 0.6.0, recovery traditionally requires a process cold restart. Cold restarting launches a new agent session, re-parsing instructions and re-reading project state.

This approach has two major drawbacks:
1. **High Token Overhead**: Re-establishing full conversation context for minor JSON formatting syntax errors consumes significant tokens.
2. **Execution Latency**: Cold restarts introduce seconds of process initialization latency for trivial syntax fixes.

---

## 2. Decision

We decide to implement an **In-Session Correction Retry Loop (`--session-id` Self-Healing)** within `janus::workflow::run_steps` in MetaMach 0.7.0.

### Key Resolution Points:

1. **Session Reuse**: When a step output fails envelope validation (ADR-034) or post-execution gate checks, the existing interactive session (`--session-id`) is preserved.
2. **Micro-Correction Prompt**: The workflow runner appends a targeted error prompt (e.g. *"Schema Violation: Field 'summary' missing. Please re-output valid envelope."*) directly into the active session context.
3. **Attempt Cap**: In-session retries are capped at `max_attempts = 3`. If all 3 attempts fail, the step aborts and triggers normal Absurd PG failure handling.

---

## 3. Detailed Specification

### 3.1 Self-Healing State Loop (`janus/src/workflow/mod.rs`)

```rust
pub async fn run_step_with_correction<F>(
    session_id: &str,
    step: &WorkflowStep,
    max_attempts: usize,
    mut execute_fn: F,
) -> Result<TypedEnvelopePayload, StepError>
where
    F: FnMut(&str, Option<&str>) -> futures::future::BoxFuture<'static, Result<String, StepError>>,
{
    let mut attempt = 1;
    let mut correction_prompt: Option<String> = None;

    while attempt <= max_attempts {
        let raw_output = execute_fn(session_id, correction_prompt.as_deref()).await?;
        
        match parse_and_validate_envelope(&raw_output) {
            Ok(envelope) => return Ok(envelope),
            Err(err) if attempt < max_attempts => {
                attempt += 1;
                correction_prompt = Some(format!(
                    "Correction Attempt {}/{}: Output failed schema validation ({err}). Re-output valid JSON envelope.",
                    attempt, max_attempts
                ));
            }
            Err(err) => return Err(StepError::AttemptsExhausted(err.to_string())),
        }
    }

    Err(StepError::AttemptsExhausted("Max retries exceeded".into()))
}
```

---

## 4. Consequences

### Positive:
- **Cost Reduction**: Reusing active sessions for syntax fixes reduces token consumption by 70%+ compared to cold restarts.
- **Faster Self-Healing**: Eliminates process spawn and context re-read latency.

### Negative / Cost:
- **Session Memory**: Active sessions must be kept resident during the retry window.
