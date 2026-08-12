//! Typed Context Envelopes for Absurd PG Checkpoints (ADR-034).
//!
//! Provides compile-time safety, Serde validation, and structured checkpoint state
//! serialization for PostgreSQL task checkpoints managed by [`crate::absurd::adapter::DurableEngine`].
//!
//! Note on Scene & Truncation (ADR-008 & ADR-034):
//! Envelopes govern the persistent Absurd PG task checkpoint state (`absurd.c_<queue>`).
//! They are distinct from the terminal output tail (`metamach_step_meta.stdout_tail`),
//! which continues to be capped at 16 KiB by [`crate::protocol::truncate_16k`] for scene snapshots.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Error type encountered during envelope creation, validation, or parsing.
#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("Validation failed: {0}")]
    Validation(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Trait implemented by all strongly-typed workflow context envelopes.
pub trait TypedEnvelope: Serialize + for<'de> Deserialize<'de> {
    /// Validates the envelope fields before writing to Absurd PG checkpoints.
    fn validate(&self) -> Result<(), EnvelopeError>;

    /// Validates and serializes the envelope into a `serde_json::Value`.
    fn to_checkpoint_value(&self) -> Result<Value, EnvelopeError> {
        self.validate()?;
        serde_json::to_value(self).map_err(EnvelopeError::Serialization)
    }
}

/// Base context envelope containing standard metadata across workflow step executions.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EnvelopeBase {
    pub status: String, // "COMPLETED" | "SUSPENDED" | "FAILED"
    pub summary: String,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub notes_for_next_agent: String,
}

impl TypedEnvelope for EnvelopeBase {
    fn validate(&self) -> Result<(), EnvelopeError> {
        if self.status.is_empty() {
            return Err(EnvelopeError::MissingField("status".to_string()));
        }
        Ok(())
    }
}

/// Domain-specific envelope for build and compilation steps.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct BuildEnvelope {
    #[serde(flatten)]
    pub base: EnvelopeBase,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
}

impl TypedEnvelope for BuildEnvelope {
    fn validate(&self) -> Result<(), EnvelopeError> {
        self.base.validate()?;
        Ok(())
    }
}

/// Domain-specific envelope for unit and integration testing steps.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TestEnvelope {
    #[serde(flatten)]
    pub base: EnvelopeBase,
    pub tests_passed: usize,
    pub tests_failed: usize,
    #[serde(default)]
    pub test_suite_name: String,
}

impl TypedEnvelope for TestEnvelope {
    fn validate(&self) -> Result<(), EnvelopeError> {
        self.base.validate()?;
        Ok(())
    }
}

/// Standard envelope persisted to Absurd PG via `DurableEngine::set_checkpoint()`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CheckpointEnvelope {
    /// Envelope schema version (default 1).
    pub version: u8,
    /// Task UUID associated with this checkpoint.
    pub task_id: Uuid,
    /// Workflow step name.
    pub step_name: String,
    /// Step status: "COMPLETED", "SUSPENDED", "FAILED".
    pub status: String,
    /// Process exit code if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Human-in-the-loop verdict if step was suspended ("APPROVED", "REJECTED", "OVERRIDDEN").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hitl_verdict: Option<String>,
    /// Optional flag for placeholder/no-op manual steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noop: Option<bool>,
    /// Structured or semi-structured state payload.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub state_data: Value,
}

impl CheckpointEnvelope {
    /// Creates a new v1 `CheckpointEnvelope` instance.
    pub fn new(
        task_id: Uuid,
        step_name: impl Into<String>,
        status: impl Into<String>,
        exit_code: Option<i32>,
        hitl_verdict: Option<String>,
        state_data: Value,
    ) -> Self {
        Self {
            version: 1,
            task_id,
            step_name: step_name.into(),
            status: status.into(),
            exit_code,
            hitl_verdict,
            noop: None,
            state_data,
        }
    }

    /// Sets the `noop` flag for manual/placeholder steps.
    pub fn with_noop(mut self, noop: bool) -> Self {
        self.noop = Some(noop);
        self
    }

    /// Parses a JSON value into a `CheckpointEnvelope`.
    /// Supports pre-0.7.0 legacy checkpoints for backward compatibility.
    pub fn parse_checkpoint(val: &Value, default_task_id: Uuid) -> Self {
        if let Ok(env) = serde_json::from_value::<CheckpointEnvelope>(val.clone()) {
            return env;
        }

        // Fallback decoder for legacy pre-0.7.0 checkpoints: {"step": "...", "status": "..."}
        let step_name = val
            .get("step")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = val
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let exit_code = val.get("exit").and_then(|v| v.as_i64()).map(|i| i as i32);
        let hitl_verdict = val
            .get("hitl_verdict")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let noop = val.get("noop").and_then(|v| v.as_bool());

        Self {
            version: 0, // Legacy format
            task_id: default_task_id,
            step_name,
            status,
            exit_code,
            hitl_verdict,
            noop,
            state_data: val.clone(),
        }
    }
}

impl TypedEnvelope for CheckpointEnvelope {
    fn validate(&self) -> Result<(), EnvelopeError> {
        if self.step_name.is_empty() {
            return Err(EnvelopeError::MissingField("step_name".to_string()));
        }
        if self.status.is_empty() {
            return Err(EnvelopeError::MissingField("status".to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_checkpoint_envelope_serialization_roundtrip() {
        let task_id = Uuid::new_v4();
        let env = CheckpointEnvelope::new(
            task_id,
            "architect_design",
            "COMPLETED",
            Some(0),
            None,
            json!({"artifacts": ["design.md"]}),
        );

        let val = env.to_checkpoint_value().unwrap();
        assert_eq!(val["version"], 1);
        assert_eq!(val["step_name"], "architect_design");
        assert_eq!(val["status"], "COMPLETED");
        assert_eq!(val["exit_code"], 0);

        let parsed = CheckpointEnvelope::parse_checkpoint(&val, task_id);
        assert_eq!(parsed, env);
    }

    #[test]
    fn test_legacy_checkpoint_fallback_decoder() {
        let task_id = Uuid::new_v4();
        let legacy_json = json!({
            "step": "builder_implement",
            "status": "SUSPENDED",
            "hitl_verdict": "APPROVED",
            "exit": 1
        });

        let parsed = CheckpointEnvelope::parse_checkpoint(&legacy_json, task_id);
        assert_eq!(parsed.version, 0);
        assert_eq!(parsed.step_name, "builder_implement");
        assert_eq!(parsed.status, "SUSPENDED");
        assert_eq!(parsed.hitl_verdict.as_deref(), Some("APPROVED"));
        assert_eq!(parsed.exit_code, Some(1));
    }

    #[test]
    fn test_build_envelope_validation() {
        let build_env = BuildEnvelope {
            base: EnvelopeBase {
                status: "COMPLETED".to_string(),
                summary: "Compiled release target successfully".to_string(),
                artifacts: vec!["target/release/janus".to_string()],
                notes_for_next_agent: "Proceed with integration tests".to_string(),
            },
            changed_files: vec!["janus/src/lib.rs".to_string()],
            commit_hash: Some("34aa6ab".to_string()),
        };

        assert!(build_env.validate().is_ok());
        let val = build_env.to_checkpoint_value().unwrap();
        assert_eq!(val["status"], "COMPLETED");
        assert_eq!(val["summary"], "Compiled release target successfully");
    }
}
