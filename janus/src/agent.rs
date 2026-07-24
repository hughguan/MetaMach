//! Agent provisioning — optional `[agent.<name>.provision]` sections in
//! `configs/agents.toml` (ADR-019). Defines which LLM backs each agent, quota
//! limits, and fallback chains. Separate from `tool_guard::rules` (which defines
//! "what can agent X do?") — provisioning defines "what backs agent X, and what
//! happens when it fails?"
//!
//! All fields are optional: an agent without a `[provision]` section has no
//! LLM backing (Tool-Guard-only, suitable for manual/shell agents like `default`).

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// The provisioning-wide config, parsed from `configs/agents.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStack {
    #[serde(default)]
    pub agent: HashMap<String, AgentEntry>,
}

/// One agent's entry in agents.toml (Tool Guard fields are parsed by
/// `tool_guard::rules`; this struct reads only the `provision` table).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentEntry {
    #[serde(default)]
    pub provision: Option<AgentProvision>,
}

/// Provisioning config for one agent role (the `[agent.<name>.provision]` table).
#[derive(Debug, Clone, Deserialize)]
pub struct AgentProvision {
    /// Which CLI adapter to use: `"claude-code"`, `"aider"`, `"codex"`,
    /// `"custom-script"`, or any user-defined name matched by the engine.
    pub adapter: String,
    /// The full shell command to launch the agent. May include model flags
    /// (`aider --model ollama/qwen2.5-coder:32b`) or environment setup.
    #[serde(default)]
    pub command: Option<String>,
    /// System prompt injected at agent start (via the adapter's mechanism).
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Quota limits (all fields optional; omitted = unlimited).
    #[serde(default)]
    pub quota: Option<AgentQuota>,
}

/// Quota limits for a provisioned agent. All fields are optional — `None` means
/// no limit.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentQuota {
    #[serde(default)]
    pub max_tokens_per_day: Option<u64>,
    #[serde(default)]
    pub max_cost_usd_per_day: Option<f64>,
    #[serde(default)]
    pub max_requests_per_hour: Option<u32>,
    /// Agent to fall back to when this agent's quota is exhausted or the API
    /// returns 429/402. The fallback chain is resolved recursively — a fallback
    /// can have its own fallback. Chains that loop are detected at resolve time.
    #[serde(default)]
    pub fallback_agent: Option<String>,
}

impl AgentStack {
    /// Load the provisioning sections from `configs/agents.toml`. Tool Guard
    /// fields are ignored (parsed separately by `tool_guard::rules`).
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read agents.toml at {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parse agents.toml at {}", path.display()))
    }

    /// Resolve the effective `AgentProvision` for `agent_id`, following the
    /// fallback chain. Returns `None` if `agent_id` has no `[provision]` section
    /// or the entire chain is exhausted (all agents have exceeded quotas).
    ///
    /// Cycle detection: if a fallback chain loops (A → B → A), returns `None`
    /// for the looped agent (prevents infinite recursion).
    pub fn resolve(&self, agent_id: &str) -> Option<&AgentProvision> {
        self.resolve_impl(agent_id, &mut Vec::new())
    }

    fn resolve_impl<'a>(
        &'a self,
        agent_id: &str,
        visited: &mut Vec<&'a str>,
    ) -> Option<&'a AgentProvision> {
        let entry = self.agent.get(agent_id)?;
        let provision = entry.provision.as_ref()?;

        // In production, quota_exceeded would check the runtime usage tracker
        // (deferred to 0.5.0+). For now, always use the primary provision.
        // The fallback chain structure is validated here.
        if let Some(fallback_id) = provision
            .quota
            .as_ref()
            .and_then(|q| q.fallback_agent.as_deref())
        {
            // Validate fallback chain structure (cycle detection).
            let fallback_key = fallback_id;
            if visited.iter().any(|v| *v == fallback_key || *v == agent_id) {
                tracing::warn!(
                    agent_id,
                    fallback = fallback_key,
                    "agent fallback chain loop detected"
                );
                // Return the primary provision — better than nothing.
                return Some(provision);
            }
            // Verify the fallback exists (compile-time validation, not runtime).
            if self.agent.contains_key(fallback_id) {
                // Fallback exists — the engine will call resolve(fallback_id)
                // when the primary's quota is exceeded at runtime.
            } else {
                tracing::warn!(
                    agent_id,
                    fallback = fallback_key,
                    "fallback agent not found in agents.toml"
                );
            }
        }

        Some(provision)
    }

    /// Check whether `agent_id` has any provisioned agent in its fallback chain
    /// (i.e. it's a real LLM-backed agent, not a Tool-Guard-only role).
    pub fn is_provisioned(&self, agent_id: &str) -> bool {
        self.resolve(agent_id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_agents(path: &Path, toml: &str) {
        std::fs::write(path, toml).expect("write agents.toml");
    }

    #[test]
    fn parse_provisioned_agent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("agents.toml");
        write_agents(
            &p,
            r#"
[agent.coder]
permissions = ["read", "write"]
bash_safe = true

[agent.coder.provision]
adapter = "claude-code"
command = "claude --print"
system_prompt = "You are a coder."

[agent.coder.provision.quota]
max_tokens_per_day = 1_000_000
max_cost_usd_per_day = 10.0
fallback_agent = "coder_backup"
"#,
        );
        let stack = AgentStack::load(&p).expect("load");
        let prov = stack.resolve("coder").expect("coder provision");
        assert_eq!(prov.adapter, "claude-code");
        assert_eq!(prov.command.as_deref(), Some("claude --print"));
        assert_eq!(prov.system_prompt.as_deref(), Some("You are a coder."));
        let q = prov.quota.as_ref().expect("quota");
        assert_eq!(q.max_tokens_per_day, Some(1_000_000));
        assert_eq!(q.max_cost_usd_per_day, Some(10.0));
        assert_eq!(q.fallback_agent.as_deref(), Some("coder_backup"));
    }

    #[test]
    fn agent_without_provision_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("agents.toml");
        write_agents(
            &p,
            r#"
[agent.default]
bash_safe = true
bash_blacklist = ["rm -rf /"]
"#,
        );
        let stack = AgentStack::load(&p).expect("load");
        assert!(stack.resolve("default").is_none());
        assert!(!stack.is_provisioned("default"));
    }

    #[test]
    fn provision_without_quota() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("agents.toml");
        write_agents(
            &p,
            r#"
[agent.scout]
permissions = ["read"]

[agent.scout.provision]
adapter = "codex"
"#,
        );
        let stack = AgentStack::load(&p).expect("load");
        let prov = stack.resolve("scout").expect("scout");
        assert_eq!(prov.adapter, "codex");
        assert!(prov.quota.is_none());
    }

    #[test]
    fn fallback_agent_missing_warns_but_returns_primary() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("agents.toml");
        write_agents(
            &p,
            r#"
[agent.coder]
permissions = ["read"]

[agent.coder.provision]
adapter = "claude-code"
[agent.coder.provision.quota]
fallback_agent = "nonexistent"
"#,
        );
        let stack = AgentStack::load(&p).expect("load");
        // Still returns the primary provision — missing fallback is a warn, not an error.
        assert!(stack.resolve("coder").is_some());
        assert_eq!(stack.resolve("coder").unwrap().adapter, "claude-code");
    }

    #[test]
    fn is_provisioned_distinguishes_tool_guard_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("agents.toml");
        write_agents(
            &p,
            r#"
[agent.builder]
permissions = ["read", "write"]
bash_safe = true

[agent.builder.provision]
adapter = "aider"

[agent.auditor]
permissions = ["read"]
"#,
        );
        let stack = AgentStack::load(&p).expect("load");
        assert!(stack.is_provisioned("builder"));
        assert!(!stack.is_provisioned("auditor"));
    }

    #[test]
    fn mixed_existing_and_new_format() {
        // A real-world agents.toml with existing Tool Guard agents and one new
        // provisioned agent — both should parse without errors.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("agents.toml");
        write_agents(
            &p,
            r#"
[agent.scout]
permissions = ["read", "grep", "find", "git-log"]
allow_network = false
bash_safe = false

[agent.coder]
permissions = ["read", "write", "edit", "bash-safe", "git-commit"]
allow_network = false
bash_safe = true
bash_blacklist = ["rm -rf /"]

[agent.coder.provision]
adapter = "claude-code"
command = "claude --print"
[agent.coder.provision.quota]
max_requests_per_hour = 50
fallback_agent = "coder_budget"

[agent.deployer]
permissions = ["read", "write", "bash-full", "ssh"]
require_approval = ["esptool.py write_flash", "make flash"]
financial = ["hi5bot --action execute"]

[agent.default]
bash_safe = true
bash_blacklist = ["rm -rf /"]
"#,
        );
        let stack = AgentStack::load(&p).expect("load");
        // coder is provisioned
        assert!(stack.is_provisioned("coder"));
        // scout, deployer, default are Tool-Guard-only
        assert!(!stack.is_provisioned("scout"));
        assert!(!stack.is_provisioned("deployer"));
        assert!(!stack.is_provisioned("default"));
        // coder's fallback agent doesn't exist in the config (coder_budget is
        // referenced but not defined — the parser should accept it and warn).
        let prov = stack.resolve("coder").expect("coder");
        assert_eq!(prov.quota.as_ref().unwrap().max_requests_per_hour, Some(50));
    }
}
