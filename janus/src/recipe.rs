//! Blueprint recipe + workflow validation (Feature-Spec Contracts 3.6 / 3.7).
//!
//! `janus onboard` reads `.janus/blueprint.toml`, validates it against
//! Contract 3.6, then reads + validates `workflows/<default_workflow>.toml`
//! (Contract 3.7). Validation failure returns a clear error with NO database
//! write (Feature-Spec §2.5 Onboard step 1).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::pipeline::{PipelineConfig, PipelineMeta, PipelineNode};

/// Parsed `.janus/blueprint.toml` (Contract 3.6).
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
    #[serde(default = "default_wf_name")]
    pub default_workflow: String,
    #[serde(default)]
    pub default_pipeline: Option<String>,
}

fn default_wf_name() -> String {
    "dev-flow".into()
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    pub default_pipeline: Option<String>,
    pub remote_host: Option<String>,
    /// SSH login user for the remote host (M4 Phase 2, `[remote] user`); `None`
    /// -> SSH default. Separate from `remote_host` (which is the host only).
    pub remote_user: Option<String>,
    pub openwiki_scope: Vec<String>,
    /// `janus.toml` verbatim -> JSONB `blueprints.config`.
    pub config_text: String,
    pub workflow: Workflow,
}

/// Read + validate `.janus/blueprint.toml` and its bound workflow.
/// `repo_root` is the Immutable ROOT where `.janus/`, `templates/`, and `workflows/` live
/// (`HERDR_PLUGIN_ROOT` in production; CWD when standalone).
/// Validate a blueprint name per Contract 3.6 / Feature-Spec §2.5: 1-60 chars
/// of alphanumeric + underscore (the charset `sanitize_ident` preserves, and a
/// valid PG ident once prefixed with `metamach_blueprint_`). Rejecting here -
/// before any path join - also prevents path traversal (`..`/`/`) on the read
/// paths (`validate`, `load_recipe`).
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.chars().count() > 60
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        bail!("invalid blueprint name {name:?}: must be 1-60 chars, alphanumeric + underscore");
    }
    Ok(())
}

/// Helper function to validate workflow steps (non-empty list, required name and agent).
pub fn validate_workflow_steps(steps: &[WorkflowStep], wf_name: &str) -> Result<()> {
    if steps.is_empty() {
        bail!("workflow {wf_name} has no steps");
    }
    for (i, s) in steps.iter().enumerate() {
        if s.name.trim().is_empty() {
            bail!("workflow {wf_name} step {i}: name is required");
        }
        if s.agent.trim().is_empty() {
            bail!(
                "workflow {wf_name} step {i} ({}): agent is required",
                s.name
            );
        }
    }
    Ok(())
}

/// Read + validate a workflow file (Contract 3.7). Tries `templates/workflows/<name>.toml`
/// first (0.5.0+ template layout), then `workflows/<name>.toml` (legacy).
pub fn load_workflow(name: &str, repo_root: &Path) -> Result<Workflow> {
    let wf_path = ["templates/workflows", ".janus/workflows", "workflows"]
        .iter()
        .map(|d| repo_root.join(d).join(format!("{name}.toml")))
        .find(|p| p.exists())
        .with_context(|| {
            format!("workflow '{name}' not found in templates/workflows/, .janus/workflows/, or workflows/")
        })?;
    let wf_text = std::fs::read_to_string(&wf_path)
        .with_context(|| format!("read workflow {}", wf_path.display()))?;
    let workflow: Workflow =
        toml::from_str(&wf_text).with_context(|| format!("parse {}", wf_path.display()))?;
    validate_workflow_steps(&workflow.steps, name)?;
    if workflow.workflow.name != name {
        bail!(
            "workflow name {:?} != requested workflow {:?}",
            workflow.workflow.name,
            name
        );
    }
    Ok(workflow)
}

/// Unified workflow result representing either a linear step-by-step workflow
/// or a multi-node DAG workflow (ADR-031).
#[derive(Debug, Clone)]
pub enum UnifiedWorkflow {
    /// Linear sequential workflow (80% path)
    Linear(Workflow),
    /// DAG multi-node workflow (20% path) with execution plan config and inline workflow register
    Dag {
        config: PipelineConfig,
        inline_register: HashMap<String, Workflow>,
    },
}

/// Internal TOML deserialization target for DAG mode workflow files.
#[derive(Debug, Clone, Deserialize)]
struct DagWorkflowFile {
    pub workflow: WorkflowSection,
    #[serde(default)]
    pub nodes: Vec<DagNodeDef>,
}

#[derive(Debug, Clone, Deserialize)]
struct DagNodeDef {
    pub id: String,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub workflow: Option<String>,
    #[serde(default)]
    pub steps: Option<Vec<WorkflowStep>>,
}

/// Helper to parse a DAG workflow file into UnifiedWorkflow::Dag.
fn parse_dag_workflow(dag: DagWorkflowFile) -> Result<UnifiedWorkflow> {
    let mut pipeline_nodes = Vec::new();
    let mut inline_register = HashMap::new();

    for node in dag.nodes {
        match (node.workflow, node.steps) {
            (Some(wf_name), None) => {
                if wf_name.trim().is_empty() {
                    bail!("node '{}': workflow name cannot be empty", node.id);
                }
                pipeline_nodes.push(PipelineNode {
                    id: node.id,
                    workflow: wf_name,
                    needs: node.needs,
                });
            }
            (None, Some(steps)) => {
                let synthetic_name = format!("__inline_{}", node.id);
                validate_workflow_steps(&steps, &synthetic_name)?;
                let synthetic_wf = Workflow {
                    workflow: WorkflowSection {
                        name: synthetic_name.clone(),
                        description: Some(format!("Inline steps for DAG node '{}'", node.id)),
                    },
                    steps,
                };
                inline_register.insert(synthetic_name.clone(), synthetic_wf);
                pipeline_nodes.push(PipelineNode {
                    id: node.id,
                    workflow: synthetic_name,
                    needs: node.needs,
                });
            }
            (Some(_), Some(_)) => {
                bail!(
                    "node '{}': cannot specify both 'workflow' and 'steps'",
                    node.id
                );
            }
            (None, None) => {
                bail!(
                    "node '{}': must specify either 'workflow' or 'steps'",
                    node.id
                );
            }
        }
    }

    let config = PipelineConfig {
        pipeline: PipelineMeta {
            name: dag.workflow.name,
            description: dag.workflow.description,
        },
        nodes: pipeline_nodes,
    };
    config.validate()?;

    Ok(UnifiedWorkflow::Dag {
        config,
        inline_register,
    })
}

/// Unified loader for workflows (ADR-031).
///
/// Searches `.janus/workflows/`, `templates/workflows/`, `workflows/` for `<name>.toml`.
///
/// Automatically determines mode:
/// - Single-line linear mode (`[workflow]` + `[[steps]]`) -> `UnifiedWorkflow::Linear`
/// - Unified DAG mode (`[workflow]` + `[[nodes]]`) -> `UnifiedWorkflow::Dag`
pub fn load_unified_workflow(name: &str, repo_root: &Path) -> Result<UnifiedWorkflow> {
    let candidate_paths = [
        repo_root
            .join(".janus/workflows")
            .join(format!("{name}.toml")),
        repo_root
            .join("templates/workflows")
            .join(format!("{name}.toml")),
        repo_root.join("workflows").join(format!("{name}.toml")),
    ];

    let path = candidate_paths
        .iter()
        .find(|p| p.exists())
        .with_context(|| format!("workflow '{name}' not found in workflows directories"))?;

    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read workflow {}", path.display()))?;

    // 1. Try parsing unified DAG format ([workflow] + [[nodes]])
    if let Ok(dag) = toml::from_str::<DagWorkflowFile>(&text)
        && !dag.nodes.is_empty()
    {
        if dag.workflow.name != name {
            bail!(
                "workflow name {:?} != requested name {:?}",
                dag.workflow.name,
                name
            );
        }
        return parse_dag_workflow(dag);
    }

    // 2. Fallback: parse standard linear workflow ([workflow] + [[steps]])
    let wf: Workflow =
        toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    validate_workflow_steps(&wf.steps, name)?;
    if wf.workflow.name != name {
        bail!(
            "workflow name {:?} != requested workflow {:?}",
            wf.workflow.name,
            name
        );
    }

    Ok(UnifiedWorkflow::Linear(wf))
}

pub fn read_blueprint_name(repo_root: &Path) -> Result<String> {
    let recipe_path: PathBuf = repo_root.join(".janus/blueprint.toml");
    let text = std::fs::read_to_string(&recipe_path)?;
    let recipe: BlueprintRecipe = toml::from_str(&text)?;
    Ok(recipe.blueprint.name)
}

pub fn validate(name: &str, repo_root: &Path) -> Result<ValidatedRecipe> {
    // Name check runs BEFORE any DB write or file read.
    validate_name(name)?;
    let recipe_path: PathBuf = repo_root.join(".janus/blueprint.toml");
    let config_text = std::fs::read_to_string(&recipe_path).with_context(|| {
        format!(
            "read blueprint recipe {} (run 'janus init' first?)",
            recipe_path.display()
        )
    })?;
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
            "blueprint.name {:?} does not match requested name {name:?}",
            recipe.blueprint.name
        );
    }

    // Workflow file must exist + conform (Contract 3.7).
    let workflow = load_workflow(&recipe.blueprint.default_workflow, repo_root)?;

    let remote_host = recipe.remote_host().map(str::to_string);
    let remote_user = recipe.remote.as_ref().and_then(|r| r.user.clone());
    Ok(ValidatedRecipe {
        name: recipe.blueprint.name,
        default_workflow: recipe.blueprint.default_workflow,
        default_pipeline: recipe.blueprint.default_pipeline,
        remote_host,
        remote_user,
        openwiki_scope: recipe.openwiki.scope,
        config_text,
        workflow,
    })
}

/// Read + parse `.janus/blueprint.toml` into a [`BlueprintRecipe`] (no
/// workflow validation). Used by the 0.4.0 cognitive check + offboard to load
/// the `[cognitive]` config without re-validating the bound workflow on every
/// command. Cheaper than [`validate`] for the per-command advisory path.
pub fn load_recipe(name: &str, repo_root: &Path) -> Result<BlueprintRecipe> {
    // Validate the name (same rule as `validate`) so a malformed `blueprint`
    // from a GuardCheck can't path-traverse via `..`/`/`. Callers treat the
    // error as warn-and-pass-through (cognitive supplement skipped).
    validate_name(name)?;
    let recipe_path = repo_root.join(".janus/blueprint.toml");
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
        fs::create_dir_all(root.join(".janus")).unwrap();
        fs::write(
            root.join(".janus/blueprint.toml"),
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
            d.path().join(".janus/blueprint.toml"),
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
            d.path().join(".janus/blueprint.toml"),
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
        fs::create_dir_all(d.path().join(".janus")).unwrap();
        fs::write(
            d.path().join(".janus/blueprint.toml"),
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

    #[test]
    fn rejects_invalid_blueprint_names() {
        // UTC-05-04b / Feature-Spec §2.5: names must be 1-60 chars, alphanumeric
        // + underscore. Validation runs before any file/DB access, so no recipe
        // file is needed for the rejected cases.
        let d = tempdir().unwrap();
        let bad = ["", "has space", "has-dash", "has/slash", "has.dot"];
        for name in bad {
            let err = validate(name, d.path()).unwrap_err();
            assert!(
                err.to_string().contains("invalid blueprint name"),
                "{name:?} should be rejected: {err}"
            );
        }
        // Over 60 chars is rejected.
        let too_long = "a".repeat(61);
        let err = validate(&too_long, d.path()).unwrap_err();
        assert!(
            err.to_string().contains("invalid blueprint name"),
            "61-char name should be rejected: {err}"
        );
        // A 60-char name passes the name check (it only fails later on the
        // missing recipe file, with a non-validation error).
        let max = "a".repeat(60);
        let err = validate(&max, d.path()).unwrap_err();
        assert!(
            !err.to_string().contains("invalid blueprint name"),
            "60-char name should pass the name check: {err}"
        );
    }

    #[test]
    fn load_recipe_rejects_invalid_names() {
        // load_recipe validates the name (same rule as validate) so a malformed
        // `blueprint` from a GuardCheck can't path-traverse via `..`/`/`. The
        // name check runs before any file read, so no recipe file is needed.
        let d = tempdir().unwrap();
        for name in ["..", "../../etc/passwd", "has space", "a/b", "has.dot"] {
            let err = load_recipe(name, d.path()).unwrap_err();
            assert!(
                err.to_string().contains("invalid blueprint name"),
                "{name:?} should be rejected: {err}"
            );
        }
        // A valid name + recipe loads fine.
        write_valid(d.path());
        let r = load_recipe("joyrobots", d.path()).unwrap();
        assert_eq!(r.blueprint.name, "joyrobots");
    }

    #[test]
    fn test_load_unified_workflow_linear() {
        let d = tempdir().unwrap();
        let wf_dir = d.path().join(".janus/workflows");
        fs::create_dir_all(&wf_dir).unwrap();
        fs::write(
            wf_dir.join("build.toml"),
            "[workflow]\nname = \"build\"\n[[steps]]\nname = \"compile\"\nagent = \"builder\"\ncommand = \"cargo build\"\n",
        )
        .unwrap();

        let unified = load_unified_workflow("build", d.path()).unwrap();
        match unified {
            UnifiedWorkflow::Linear(wf) => {
                assert_eq!(wf.workflow.name, "build");
                assert_eq!(wf.steps.len(), 1);
                assert_eq!(wf.steps[0].name, "compile");
            }
            UnifiedWorkflow::Dag { .. } => panic!("expected Linear workflow"),
        }
    }

    #[test]
    fn test_load_unified_workflow_dag_with_inline_and_refs() {
        let d = tempdir().unwrap();
        let wf_dir = d.path().join(".janus/workflows");
        fs::create_dir_all(&wf_dir).unwrap();

        fs::write(
            wf_dir.join("compile_step.toml"),
            "[workflow]\nname = \"compile_step\"\n[[steps]]\nname = \"cc\"\nagent = \"builder\"\ncommand = \"cargo build\"\n",
        )
        .unwrap();

        fs::write(
            wf_dir.join("pipeline_dag.toml"),
            r#"
[workflow]
name = "pipeline_dag"
description = "DAG workflow with inline and external node"

[[nodes]]
id = "node1"
workflow = "compile_step"

[[nodes]]
id = "node2"
needs = ["node1"]
steps = [
    { name = "test", agent = "tester", command = "cargo test" }
]
"#,
        )
        .unwrap();

        let unified = load_unified_workflow("pipeline_dag", d.path()).unwrap();
        match unified {
            UnifiedWorkflow::Dag {
                config,
                inline_register,
            } => {
                assert_eq!(config.pipeline.name, "pipeline_dag");
                assert_eq!(config.nodes.len(), 2);
                assert_eq!(config.nodes[0].id, "node1");
                assert_eq!(config.nodes[0].workflow, "compile_step");
                assert_eq!(config.nodes[1].id, "node2");
                assert_eq!(config.nodes[1].workflow, "__inline_node2");

                let inline_wf = inline_register
                    .get("__inline_node2")
                    .expect("inline wf registered");
                assert_eq!(inline_wf.steps.len(), 1);
                assert_eq!(inline_wf.steps[0].name, "test");
            }
            UnifiedWorkflow::Linear(_) => panic!("expected Dag workflow"),
        }
    }

    #[test]
    fn test_load_unified_workflow_legacy_pipeline_rejected() {
        let d = tempdir().unwrap();
        let pipe_dir = d.path().join(".janus/pipelines");
        fs::create_dir_all(&pipe_dir).unwrap();

        fs::write(
            pipe_dir.join("legacy_pipe.toml"),
            r#"
[pipeline]
name = "legacy_pipe"
description = "Legacy pipeline format"

[[nodes]]
id = "n1"
workflow = "wf1"
"#,
        )
        .unwrap();

        // In Phase 3 (v0.8.0), legacy .janus/pipelines/ paths and [pipeline] headers are no longer searched or parsed.
        assert!(load_unified_workflow("legacy_pipe", d.path()).is_err());
    }

    #[test]
    fn test_load_unified_workflow_node_mutex_validation() {
        let d = tempdir().unwrap();
        let wf_dir = d.path().join(".janus/workflows");
        fs::create_dir_all(&wf_dir).unwrap();

        // Specified both workflow and steps
        fs::write(
            wf_dir.join("both.toml"),
            r#"
[workflow]
name = "both"

[[nodes]]
id = "invalid_node"
workflow = "wf1"
steps = [
    { name = "s1", agent = "builder" }
]
"#,
        )
        .unwrap();

        assert!(load_unified_workflow("both", d.path()).is_err());

        // Specified neither workflow nor steps
        fs::write(
            wf_dir.join("neither.toml"),
            r#"
[workflow]
name = "neither"

[[nodes]]
id = "invalid_node"
"#,
        )
        .unwrap();

        assert!(load_unified_workflow("neither", d.path()).is_err());
    }
}
