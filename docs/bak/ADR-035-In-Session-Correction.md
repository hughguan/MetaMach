# ADR-035: Augmented Cold Retry with Correction Context for Step Self-Healing

* **Status:** Proposed (0.7.0 Candidate)
* **Date:** 2026-08-12
* **Target Version:** MetaMach 0.7.0
* **Amends:** ADR-019 (Configurable Agents - Provisioning, Quota & Fallback)
* **Depends on:** ADR-034 (Typed Context Envelopes)

## 1. Context & Problem Statement
When an agent step fails validation (e.g., envelope validation failure per ADR-034 or gate check failure), throwing away the run and starting over without error context is inefficient. We need a way to retry the step while providing the agent with context about why it failed. 

## 2. Options Considered
* **Persistent Tmux Sessions:** Rejected. Maintaining a persistent session to reuse within a step contradicts our verified execution model (`run_steps` in `src/workflow/mod.rs`), where each step runs in a single-use tmux session (`tmux-janus-task-<task_id>-<idx>`) and terminates.
* **Augmented Cold Retry with Correction Context (Chosen):** Spawns a new single-use tmux session for the retry but injects the error context into the agent's environment, perfectly fitting the existing execution model.

## 3. Decision
We will implement an "Augmented Cold Retry with Correction Context". On validation failure, the system will re-dispatch the same step with the original command, but append the error context as an environment variable (`METAMACH_CORRECTION_CONTEXT`). Agent role prompt templates (`templates/agents/*.md`) will reference `$METAMACH_CORRECTION_CONTEXT` to process error guidance. If an agent ignores the variable, execution gracefully degrades to a standard cold retry.

The retry limit is configured via `max_correction_attempts = 3` in the DSL on `[[steps]]` (or `[workflow]` as default fallback).

## 4. Detailed Specification

### 4.1 Augmented Cold Retry Mechanism
Instead of maintaining a persistent session, we leverage the existing execution model defined in `src/workflow/mod.rs` (`run_steps`):
1. The agent runs to completion in its single-use tmux session.
2. If envelope validation or gate checks fail, a new retry iteration is triggered.
3. The original step is re-dispatched.
4. The error context is injected via the `METAMACH_CORRECTION_CONTEXT` environment variable.
5. A new single-use tmux session is created for this retry.
6. The agent's system prompt references `$METAMACH_CORRECTION_CONTEXT` to understand and correct the previous failure.

### 4.2 Integration with `run_steps` (Illustrative Pseudocode)

The following pseudocode illustrates how the retry logic wraps the existing step dispatch in `workflow/mod.rs` without requiring persistent session hacks:

```rust
// Illustrative integration logic within workflow/mod.rs run_steps
let max_correction_attempts = step.max_correction_attempts.unwrap_or(3);
let mut attempts = 0;
let mut previous_error: Option<String> = None;

while attempts <= max_correction_attempts {
    // Inject METAMACH_CORRECTION_CONTEXT if attempts > 0
    let env_context = previous_error.as_ref().map(|err| {
        format!("Attempt {}/{}: Previous step execution failed validation: {}", attempts, max_correction_attempts, err)
    });
    
    // Dispatches a new single-use tmux session (tmux-janus-task-<task_id>-<idx>)
    let result = dispatch_step_tmux(&step, env_context.as_deref()).await;
    
    match validate_step_output(&result) {
        Ok(envelope) => {
            checkpoint_envelope(&engine, &queue, task_id, &step.name, &envelope, owner_run).await?;
            break;
        }
        Err(e) => {
            attempts += 1;
            previous_error = Some(e.to_string());
        }
    }
}
```

## 5. Consequences
* **Positive:** Reduces wasted exploration tokens by providing targeted error guidance, avoiding blind re-execution.
* **Positive:** Perfectly matches the existing tmux-poll async execution loop and checkpointing model without requiring persistent state hacks.
* **Negative:** Agent prompts in `templates/agents/` must be updated to process the `$METAMACH_CORRECTION_CONTEXT` environment variable.
