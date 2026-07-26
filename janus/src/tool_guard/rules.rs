//! Parse `configs/agents.toml` (Feature-Spec Contract 3.5) into rule structs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// All configured agent profiles, keyed by role name (`[agent.<name>]`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentRules {
    #[serde(default)]
    pub agent: HashMap<String, AgentProfile>,
}

/// One agent role's qualification + Tool Guard rules (Contract 3.5).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentProfile {
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub allow_network: bool,
    #[serde(default)]
    pub bash_safe: bool,
    #[serde(default)]
    pub bash_blacklist: Vec<String>,
    #[serde(default)]
    pub require_approval: Vec<String>,
    /// M3 extension: financial-class commands rewritten to dry-run (Contract 3.4).
    #[serde(default)]
    pub financial: Vec<String>,
}

impl AgentRules {
    /// Load and parse `agents.toml` from `path`.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read agents.toml at {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parse agents.toml at {}", path.display()))
    }

    /// Load from multiple paths in priority order (first wins for duplicate keys).
    /// Paths that don't exist are skipped silently. At least one must succeed.
    pub fn load_merged(paths: &[PathBuf]) -> Result<Self> {
        let mut merged = AgentRules::default();
        let mut loaded = false;
        for path in paths.iter().rev() {
            // Reverse: lower priority loads first, higher priority overwrites.
            if path.exists() {
                let rules = Self::load(path)?;
                merged.agent.extend(rules.agent);
                loaded = true;
            }
        }
        if !loaded {
            anyhow::bail!("no agents.toml found in {paths:?}");
        }
        Ok(merged)
    }
}
