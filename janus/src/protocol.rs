//! UDS wire protocol for `janus.sock` (newline-delimited JSON).
//!
//! The Daemon owns the DB; clients send [`Request`]s and receive [`Response`]s.
//! Progress responses conform to Feature-Spec Contract 3.3.

use std::collections::HashMap;

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
    /// context yet (Tether/Onboard land in M2.4/M4); they carry the SUSPENDED
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
    GuardVerdict {
        execution_id: String,
        verdict: String,
        reason: Option<String>,
        rewritten_argv: Option<Vec<String>>,
        correlation_id: String,
    },
    /// Generic success ack (Onboard/Offboard).
    Ok {
        message: String,
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
    /// Tether physical-session liveness; lands with Task 2.4 (always false in M2).
    pub tether_alive: bool,
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
