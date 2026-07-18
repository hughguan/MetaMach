//! Blueprint recipe + workflow validation (Feature-Spec Contracts 3.6 / 3.7).
//!
//! `janus onboard` reads `blueprints/<name>/janus.toml`, validates it against
//! Contract 3.6, then reads + validates `workflows/<default_workflow>.toml`
//! (Contract 3.7). Validation failure returns a clear error with NO database
//! write (Feature-Spec §2.5 Onboard step 1).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Parsed `blueprints/<name>/janus.toml` (Contract 3.6).
#[derive(Debug, Clone, Deserialize)]
pub struct BlueprintRecipe {
    pub blueprint: BlueprintSection,
    pub remote: Option<RemoteSection>,
    pub openwiki: OpenwikiSection,
    /// 0.4.0 Cognitive Provider config (Contract 4.1/4.2). Opt-in; blueprints
    /// without a `[cognitive]` section get a `NoopProvider` (fail-open).
    #[serde(default)]
    pub cognitive: Option<CognitiveSection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlueprintSection {
    pub name: String,
    pub default_workflow: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteSection {
    pub host: String,
    pub user: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenwikiSection {
    pub scope: Vec<String>,
}

/// 0.4.0 Cognitive Provider config (Contract 4.1/4.2). Opt-in via
/// `[cognitive.codebase_memory]` in `janus.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct CognitiveSection {
    pub codebase_memory: Option<CodebaseMemoryConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodebaseMemoryConfig {
    /// Transport: `"stdio"` only in 0.4.0 (MCP over the child's stdin/stdout).
    pub transport: String,
    /// External `codebase-memory-mcp` binary name or path.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_cognitive_timeout")]
    pub timeout_secs: u64,
}

fn default_cognitive_timeout() -> u64 {
    5
}

impl BlueprintRecipe {
    pub fn remote_host(&self) -> Option<&str> {
        self.remote.as_ref().map(|r| r.host.as_str())
    }
}

/// Parsed `workflows/<name>.toml` (Contract 3.7).
#[derive(Debug, Clone, Deserialize)]
pub struct Workflow {
    pub workflow: WorkflowSection,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowSection {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    pub agent: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub toolset: Option<Vec<String>>,
}

/// A fully validated recipe ready for Onboard registration.
#[derive(Debug, Clone)]
pub struct ValidatedRecipe {
    pub name: String,
    pub default_workflow: String,
    pub remote_host: Option<String>,
    pub openwiki_scope: Vec<String>,
    /// `janus.toml` verbatim -> JSONB `blueprints.config`.
    pub config_text: String,
    pub workflow: Workflow,
}

/// Read + validate `blueprints/<name>/janus.toml` and its bound workflow.
/// `repo_root` is the Immutable ROOT where `blueprints/` and `workflows/` live
/// (`HERDR_PLUGIN_ROOT` in production; CWD when standalone).
pub fn validate(name: &str, repo_root: &Path) -> Result<ValidatedRecipe> {
    if name.is_empty() || name.contains('/') {
        bail!("invalid blueprint name {name:?}");
    }
    let recipe_path: PathBuf = repo_root.join("blueprints").join(name).join("janus.toml");
    let config_text = std::fs::read_to_string(&recipe_path)
        .with_context(|| format!("read blueprint recipe {}", recipe_path.display()))?;
    let recipe: BlueprintRecipe =
        toml::from_str(&config_text).with_context(|| format!("parse {}", recipe_path.display()))?;

    // Contract 3.6 required fields.
    if recipe.blueprint.name.trim().is_empty() {
        bail!(
            "blueprint.name is required (empty in {})",
            recipe_path.display()
        );
    }
    if recipe.blueprint.default_workflow.trim().is_empty() {
        bail!("blueprint.default_workflow is required");
    }
    if recipe.openwiki.scope.is_empty() {
        bail!("openwiki.scope must list at least one index scope");
    }
    if recipe.blueprint.name != name {
        bail!(
            "blueprint.name {:?} != directory name {name:?}",
            recipe.blueprint.name
        );
    }

    // Workflow file must exist + conform (Contract 3.7).
    let wf_path = repo_root
        .join("workflows")
        .join(format!("{}.toml", recipe.blueprint.default_workflow));
    let wf_text = std::fs::read_to_string(&wf_path)
        .with_context(|| format!("read workflow {}", wf_path.display()))?;
    let workflow: Workflow =
        toml::from_str(&wf_text).with_context(|| format!("parse {}", wf_path.display()))?;
    if workflow.steps.is_empty() {
        bail!(
            "workflow {} has no steps",
            recipe.blueprint.default_workflow
        );
    }
    for (i, s) in workflow.steps.iter().enumerate() {
        if s.name.trim().is_empty() {
            bail!(
                "workflow {} step {i}: name is required",
                recipe.blueprint.default_workflow
            );
        }
        if s.agent.trim().is_empty() {
            bail!(
                "workflow {} step {i} ({}): agent is required",
                recipe.blueprint.default_workflow,
                s.name
            );
        }
    }
    if workflow.workflow.name != recipe.blueprint.default_workflow {
        bail!(
            "workflow name {:?} != default_workflow {:?}",
            workflow.workflow.name,
            recipe.blueprint.default_workflow
        );
    }

    let remote_host = recipe.remote_host().map(str::to_string);
    Ok(ValidatedRecipe {
        name: recipe.blueprint.name,
        default_workflow: recipe.blueprint.default_workflow,
        remote_host,
        openwiki_scope: recipe.openwiki.scope,
        config_text,
        workflow,
    })
}

/// Read + parse `blueprints/<name>/janus.toml` into a [`BlueprintRecipe`] (no
/// workflow validation). Used by the 0.4.0 cognitive check + offboard to load
/// the `[cognitive]` config without re-validating the bound workflow on every
/// command. Cheaper than [`validate`] for the per-command advisory path.
pub fn load_recipe(name: &str, repo_root: &Path) -> Result<BlueprintRecipe> {
    let recipe_path = repo_root.join("blueprints").join(name).join("janus.toml");
    let config_text = std::fs::read_to_string(&recipe_path)
        .with_context(|| format!("read blueprint recipe {}", recipe_path.display()))?;
    toml::from_str(&config_text).with_context(|| format!("parse {}", recipe_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_valid(root: &Path) {
        fs::create_dir_all(root.join("blueprints/joyrobots/openwiki")).unwrap();
        fs::write(
            root.join("blueprints/joyrobots/janus.toml"),
            r#"
[blueprint]
name = "joyrobots"
default_workflow = "dev-flow"
[openwiki]
scope = ["spike-prime"]
"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("workflows")).unwrap();
        fs::write(
            root.join("workflows/dev-flow.toml"),
            r#"
[workflow]
name = "dev-flow"
[[steps]]
name = "scout"
agent = "scout"
"#,
        )
        .unwrap();
    }

    #[test]
    fn validates_a_good_recipe() {
        let d = tempdir().unwrap();
        write_valid(d.path());
        let r = validate("joyrobots", d.path()).unwrap();
        assert_eq!(r.name, "joyrobots");
        assert_eq!(r.default_workflow, "dev-flow");
        assert_eq!(r.remote_host, None);
        assert_eq!(r.openwiki_scope, vec!["spike-prime".to_string()]);
        assert_eq!(r.workflow.steps.len(), 1);
    }

    #[test]
    fn fails_when_workflow_missing() {
        let d = tempdir().unwrap();
        write_valid(d.path());
        fs::remove_file(d.path().join("workflows/dev-flow.toml")).unwrap();
        let err = validate("joyrobots", d.path()).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("workflow"), "{err}");
    }

    #[test]
    fn fails_when_scope_empty() {
        let d = tempdir().unwrap();
        write_valid(d.path());
        fs::write(
            d.path().join("blueprints/joyrobots/janus.toml"),
            r#"
[blueprint]
name = "joyrobots"
default_workflow = "dev-flow"
[openwiki]
scope = []
"#,
        )
        .unwrap();
        assert!(validate("joyrobots", d.path()).is_err());
    }

    #[test]
    fn fails_when_name_mismatches_dir() {
        let d = tempdir().unwrap();
        write_valid(d.path());
        fs::write(
            d.path().join("blueprints/joyrobots/janus.toml"),
            r#"
[blueprint]
name = "other"
default_workflow = "dev-flow"
[openwiki]
scope = ["x"]
"#,
        )
        .unwrap();
        let err = validate("joyrobots", d.path()).unwrap_err();
        assert!(err.to_string().contains("other"));
    }

    #[test]
    fn parses_cross_host_recipe() {
        let d = tempdir().unwrap();
        fs::create_dir_all(d.path().join("blueprints/gatemetric/openwiki")).unwrap();
        fs::write(
            d.path().join("blueprints/gatemetric/janus.toml"),
            r#"
[blueprint]
name = "gatemetric"
default_workflow = "firmware-deploy"
[remote]
host = "192.168.1.100"
user = "builder"
[openwiki]
scope = ["mpu6050"]
"#,
        )
        .unwrap();
        fs::create_dir_all(d.path().join("workflows")).unwrap();
        fs::write(
            d.path().join("workflows/firmware-deploy.toml"),
            r#"
[workflow]
name = "firmware-deploy"
[[steps]]
name = "cross-compile"
agent = "deployer"
host = "remote"
"#,
        )
        .unwrap();
        let r = validate("gatemetric", d.path()).unwrap();
        assert_eq!(r.remote_host.as_deref(), Some("192.168.1.100"));
        assert_eq!(r.workflow.steps[0].host.as_deref(), Some("remote"));
    }
}
