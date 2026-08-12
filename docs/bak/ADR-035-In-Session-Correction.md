# ADR-035: In-Session Correction Retry Loop

* **Status:** Proposed (0.7.0 Candidate)
* **Date:** 2026-08-12
* **Target Version:** MetaMach 0.7.0
* **Amends:** ADR-019 (Configurable Agents - Provisioning, Quota & Fallback)
* **Depends on:** ADR-034

## 1. Context & Problem Statement
When an agent step fails validation (e.g., envelope validation failure per ADR-034 or gate check failure), throwing away the run and starting over is inefficient. We need a way to retry the step while providing the agent with context about why it failed. 

## 2. Options Considered
* **Persistent Tmux Sessions:** Rejected. Maintaining a persistent session to reuse within a step contradicts our verified execution model (`run_steps` in `src/workflow/mod.rs`), where each step runs in a single-use tmux session (`tmux-janus-task-<task_id>-<idx>`) and terminates.
* **Augmented Cold Retry with Correction Context (Chosen):** Spawns a new tmux session for the retry but injects the error context into the agent's environment, perfectly fitting the existing execution model.

## 3. Decision
We will implement an "Augmented Cold Retry with Correction Context". On validation failure, the system will re-dispatch the same step with the original command, but append the error context as an environment variable (`METAMACH_CORRECTION_CONTEXT`). This reduces wasted exploration tokens by providing targeted error guidance, avoiding blind re-execution.

## 4. Detailed Specification

### 4.1 Augmented Cold Retry Mechanism
Instead of maintaining a persistent session, we leverage the existing execution model defined in `src/workflow/mod.rs` (`run_steps`):
1. The agent runs to completion in its tmux session.
2. If validation fails, a new retry iteration is triggered.
3. The original step is re-dispatched.
4. The error context is injected via the `METAMACH_CORRECTION_CONTEXT` environment variable.
5. A new single-use tmux session is created for this retry.
6. The agent's system prompt references `$METAMACH_CORRECTION_CONTEXT` to understand and correct the previous failure.

### 4.2 Configuration
The retry loop is capped by `max_correction_attempts = 3` (configurable in the workflow DSL).

### 4.3 Integration with `run_steps`
The retry logic wraps the existing step dispatch in `workflow/mod.rs`. It does not require a custom closure API, but rather a retry loop around the standard step execution and checkpointing mechanism:

```rust
// Integration within workflow/mod.rs run_steps
let mut attempts = 0;
while attempts <= max_correction_attempts {
    // Inject METAMACH_CORRECTION_CONTEXT if attempts > 0
    let step_context = build_step_context(&step, attempts, previous_error);
    
    // Dispatches a new tmux session (tmux-janus-task-<task_id>-<idx>)
    let result = execute_step_in_tmux(&step, &step_context).await;
    
    match validate_step_output(result) {
        Ok(_) => break, // Success
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
* **Negative:** Agent prompts must be updated to process the `$METAMACH_CORRECTION_CONTEXT` environment variable.
