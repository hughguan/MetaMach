//! UDS wire protocol for `janus.sock` (newline-delimited JSON).
//!
//! The Daemon owns the DB; clients send [`Request`]s and receive [`Response`]s.
//! Progress responses conform to Feature-Spec Contract 3.3.

use std::collections::HashMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Client -> Daemon request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Cheap liveness probe.
    Ping,
    /// List `ACTIVE` blueprints for the Dispatch view.
    Blueprints,
    /// Workflow progress snapshot (Contract 3.3). `blueprint` filters by name.
    Progress { blueprint: Option<String> },
    /// `janush` -> Daemon: ask for a verdict on a command (Contract 3.2).
    /// `task_id`/`step_name` are `Option` because M3 has no running workflow
    /// context yet (tmux/Onboard land in M2.4/M4); they carry the SUSPENDED
    /// target when present.
    GuardCheck {
        execution_id: String,
        blueprint_id: Option<String>,
        task_id: Option<Uuid>,
        step_name: Option<String>,
        cwd: Option<String>,
        argv: Vec<String>,
        env_snapshot: HashMap<String, String>,
    },
    /// `janus onboard --blueprint <name>` (Task 4.3).
    Onboard { name: String },
    /// `janus offboard --blueprint <name>` (Task 4.2).
    Offboard { name: String },
    /// Dispatch a blueprint's workflow onto the absurd engine (M4 Task 4.1
    /// Phase 0b, Contract 3.11). `workflow` overrides the blueprint's
    /// `default_workflow`; `None` uses the default. Returns the absurd-minted
    /// `task_id` immediately - the step loop runs detached on the daemon.
    Dispatch {
        blueprint: String,
        workflow: Option<String>,
    },
}

/// Daemon -> client response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong,
    Blueprints {
        blueprints: Vec<BlueprintInfo>,
    },
    Progress {
        active_tasks: Vec<ActiveTask>,
    },
    /// Daemon -> `janush`: verdict (Contract 3.4). `verdict` is
    /// `"ALLOW"` | `"BLOCK"` | `"REWRITE"`; `rewritten_argv` is set on REWRITE.
    /// `cognitive_context` (0.4.0 Contract 4.1) carries a CognitiveProvider
    /// BLOCK reason when one was returned; omitted on the wire when `None`
    /// (0.3.0-compatible).
    GuardVerdict {
        execution_id: String,
        verdict: String,
        reason: Option<String>,
        rewritten_argv: Option<Vec<String>>,
        correlation_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cognitive_context: Option<String>,
    },
    /// Generic success ack (Onboard/Offboard).
    Ok {
        message: String,
    },
    /// Ack of a `Dispatch` request (Contract 3.11): the absurd-minted task id.
    Dispatch {
        task_id: Uuid,
    },
    Error {
        message: String,
    },
}

/// A dispatchable blueprint (Dispatch view + `janus onboard` target).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueprintInfo {
    pub name: String,
    pub default_workflow: String,
    pub remote_host: Option<String>,
    pub status: String,
}

/// Contract 3.3 progress payload (`janus status --json` emits this verbatim).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProgressPayload {
    pub active_tasks: Vec<ActiveTask>,
}

/// A non-terminal (in-flight) task row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTask {
    pub task_id: Uuid,
    /// Blueprint name (resolved from the FK), per Contract 3.3.
    pub blueprint_id: String,
    pub workflow_name: String,
    pub status: String,
    pub started_at: Option<String>,
    pub elapsed_seconds: Option<i64>,
    pub current_step: Option<String>,
    /// tmux physical-session liveness; lands with Task 2.4 (always false in M2).
    pub tmux_alive: bool,
    /// The current step's tmux session id (`metamach_step_meta.session_name`),
    /// for the daemon's `tmux_alive` second-pass liveness check (Contract 3.3).
    /// Wire-invisible: the dashboard doesn't need it, only the daemon does.
    #[serde(skip)]
    pub session_name: Option<String>,
    pub suspended_reason: Option<String>,
    pub steps: Vec<StepStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepStatus {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_tail: Option<String>,
}

// --- 0.4.0 shared HITL types (ARCH-0.4.0 §5.4) ----------------------------
// `WebhookPayload`, `GatewayVerdict`, and the 16 KiB budget primitives live here
// (the leaf module) so `absurd`, `tool_guard`, and `gateway` all share them
// without a `absurd <-> protocol` cycle (`absurd` already imports `protocol`).

/// Physical size budget for Step stdout_tail / result_cache / HITL scene
/// (Feature-Spec §4 fault matrix, UTC-05-01). The authoritative enforcement
/// point is `janus::absurd` (before the DB write); the HITL scene honors the
/// same budget.
pub const SIZE_BUDGET: usize = 16 * 1024;
pub const BUDGET_TAG: &str = "[MetaMach Log Budget Exceeded]";

/// Truncate to the 16 KiB budget, appending the budget-exceeded tag if cut.
pub fn truncate_16k(s: &str) -> String {
    if s.len() <= SIZE_BUDGET {
        return s.to_string();
    }
    let target = SIZE_BUDGET.saturating_sub(BUDGET_TAG.len());
    let mut cut = target;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(SIZE_BUDGET);
    out.push_str(&s[..cut]);
    out.push_str(BUDGET_TAG);
    out
}

/// HITL default verdict window (seconds). One unified deadline (ARCH-0.4.0
/// §5.3): `expires_at` on the outbound card AND the `await_verdict` blocking
/// timeout both derive from this. A late callback gets `410 Gone`; the awaiter
/// gets `Err(Timeout)` -> BLOCK.
pub const HITL_TIMEOUT_SECS: i64 = 1800;

/// The HITL verdict window in seconds, from `JANUS_HITL_TIMEOUT_SECS` (must be
/// positive; falls back to [`HITL_TIMEOUT_SECS`] = 30 min). **Single source of
/// truth** shared by [`WebhookPayload::build`] (`expires_at`) and the gateway's
/// `await_verdict` timeout - so the two can never drift (e.g. a malformed
/// `JANUS_HITL_TIMEOUT_SECS=0` no longer makes the card instantly expire while
/// the awaiter blocks for 30 min).
pub fn hitl_timeout_secs() -> i64 {
    std::env::var("JANUS_HITL_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(HITL_TIMEOUT_SECS)
}

/// Abstract HITL card (Feature-Spec §2.4; ARCH-0.4.0 §5.4 enriched). The Daemon
/// constructs this; the gateway + each adapter translate it into a native format.
/// `scene` is the 0.3.0 field (kept as a legacy alias); `stdout_tail` is the 0.4.0
/// canonical Hermes-named field - the two are always equal at construction.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    pub task_id: Option<Uuid>,
    pub execution_id: String,
    pub correlation_id: String, // == Hermes run_id
    pub cause: String,
    pub command: String,
    pub reason: String,
    pub scene: String,      // 16KB-truncated; legacy alias for stdout_tail
    pub resume_key: String, // "metamach-resume:{correlation_id}"
    // 0.4.0 enrichment for the Hermes Run API envelope / Teams adapter.
    pub blueprint: String,   // owning blueprint name
    pub step: String,        // current step name
    pub stdout_tail: String, // canonical (Hermes naming); == scene
    pub expires_at: String,  // ISO 8601; now + JANUS_HITL_TIMEOUT_SECS
}

impl WebhookPayload {
    /// Build the card. `scene`/`stdout_tail` are 16KB-truncated from
    /// `{command}\n{reason}`; `expires_at` is `now + JANUS_HITL_TIMEOUT_SECS`
    /// (env override; default `HITL_TIMEOUT_SECS` = 30 min).
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        task_id: Option<Uuid>,
        execution_id: &str,
        correlation_id: &str,
        cause: &str,
        command: &str,
        reason: &str,
        blueprint: &str,
        step: &str,
    ) -> Self {
        let scene_src = format!("{command}\n{reason}");
        // The scene is the only unbounded field; hard-cap at the 16 KiB budget.
        let scene = if scene_src.len() > SIZE_BUDGET {
            truncate_16k(&scene_src)
        } else {
            scene_src
        };
        let secs = hitl_timeout_secs();
        let expires_at = (Utc::now() + chrono::Duration::seconds(secs)).to_rfc3339();
        Self {
            task_id,
            execution_id: execution_id.to_string(),
            correlation_id: correlation_id.to_string(),
            cause: cause.to_string(),
            command: command.to_string(),
            reason: reason.to_string(),
            scene: scene.clone(),
            resume_key: format!("metamach-resume:{correlation_id}"),
            blueprint: blueprint.to_string(),
            step: step.to_string(),
            stdout_tail: scene,
            expires_at,
        }
    }
}

/// HITL gateway verdict (ARCH-0.4.0 §5.3). Distinct from `tool_guard::Verdict`
/// (command interception `ALLOW | BLOCK | REWRITE`): this is the human approval
/// result returned from a remote Teams/TUI callback.
#[derive(Debug, Clone)]
pub enum GatewayVerdict {
    Approve,
    Reject,
    Override { rewritten_argv: Vec<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_16k_keeps_small_strings() {
        assert_eq!(truncate_16k("hello"), "hello");
    }

    #[test]
    fn truncate_16k_caps_and_tags_oversized() {
        let s = "x".repeat(SIZE_BUDGET * 2);
        let out = truncate_16k(&s);
        assert!(out.len() <= SIZE_BUDGET, "len {} > budget", out.len());
        assert!(out.ends_with(BUDGET_TAG));
    }

    #[test]
    fn truncate_16k_respects_char_boundary() {
        // é = 2 bytes; the cut must not split a codepoint.
        let s = "é".repeat(SIZE_BUDGET);
        let out = truncate_16k(&s);
        assert!(out.len() <= SIZE_BUDGET);
        assert!(out.chars().all(|c| c == 'é') || out.ends_with(BUDGET_TAG));
    }

    #[test]
    fn payload_scene_capped_to_16kib() {
        let big = "x".repeat(SIZE_BUDGET * 4);
        let p = WebhookPayload::build(
            Some(Uuid::nil()),
            "exec",
            "corr",
            "blacklist",
            &big,
            "r",
            "gatemetric",
            "cross-compile",
        );
        assert!(
            p.scene.len() <= SIZE_BUDGET,
            "scene {} > budget",
            p.scene.len()
        );
        assert!(p.scene.ends_with(BUDGET_TAG));
        assert_eq!(p.stdout_tail, p.scene); // alias invariant (UTC-10-09)
    }

    #[test]
    fn payload_enrichment_fields_populated() {
        let p = WebhookPayload::build(
            None,
            "exec-1",
            "corr-9",
            "require_approval",
            "make flash",
            "r",
            "gatemetric",
            "flash",
        );
        assert_eq!(p.resume_key, "metamach-resume:corr-9");
        assert_eq!(p.correlation_id, "corr-9");
        assert_eq!(p.blueprint, "gatemetric");
        assert_eq!(p.step, "flash");
        assert!(!p.expires_at.is_empty());
        assert_eq!(p.stdout_tail, p.scene); // UTC-10-09
    }
}
