//! Pipeline DAG engine (ADR-021, 0.4.9). A Pipeline composes Workflows into
//! a directed acyclic graph: nodes declare their dependencies via `needs`, the
//! engine topologically sorts them, and nodes at the same level can run in parallel.
//!
//! Format: `pipelines/<name>.toml`

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Parsed `pipelines/<name>.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipelineConfig {
    pub pipeline: PipelineMeta,
    #[serde(default)]
    pub nodes: Vec<PipelineNode>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipelineMeta {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipelineNode {
    pub id: String,
    pub workflow: String,
    #[serde(default)]
    pub needs: Vec<String>,
}

/// Topologically sorted execution plan. Each level contains nodes that can
/// run in parallel.
#[derive(Debug)]
pub struct ExecutionPlan {
    pub levels: Vec<Vec<PipelineNode>>,
}

impl PipelineConfig {
    /// Load and validate a pipeline from `pipelines/<name>.toml`.
    pub fn load(name: &str, repo_root: &Path) -> Result<Self> {
        let path = repo_root.join("pipelines").join(format!("{name}.toml"));
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read pipeline {}", path.display()))?;
        let config: Self =
            toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        config.validate()?;
        if config.pipeline.name != name {
            bail!(
                "pipeline name mismatch: file declares '{}', expected '{name}'",
                config.pipeline.name
            );
        }
        Ok(config)
    }

    /// Validate node uniqueness and dependency references.
    fn validate(&self) -> Result<()> {
        if self.nodes.is_empty() {
            bail!("pipeline '{}' has no nodes", self.pipeline.name);
        }
        let mut seen = HashSet::new();
        for node in &self.nodes {
            if !seen.insert(&node.id) {
                bail!("duplicate node id '{}'", node.id);
            }
        }
        for node in &self.nodes {
            for dep in &node.needs {
                if !seen.contains(dep) {
                    bail!("node '{}' references unknown dependency '{}'", node.id, dep);
                }
            }
        }
        Ok(())
    }

    /// Topologically sort into an [`ExecutionPlan`] with parallel levels.
    /// Detects cycles and returns an error with the unresolved nodes.
    pub fn plan(&self) -> Result<ExecutionPlan> {
        let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut in_degree: HashMap<&str, usize> = HashMap::new();

        for node in &self.nodes {
            in_degree.entry(&node.id).or_insert(0);
            for dep in &node.needs {
                dependents.entry(dep).or_default().push(&node.id);
                *in_degree.entry(&node.id).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<&str> = self
            .nodes
            .iter()
            .filter(|n| *in_degree.get(n.id.as_str()).unwrap_or(&0) == 0)
            .map(|n| n.id.as_str())
            .collect();

        let mut levels: Vec<Vec<&str>> = Vec::new();
        let mut sorted = Vec::new();

        while !queue.is_empty() {
            let level: Vec<&str> = queue.drain(..).collect();
            let mut next_queue = VecDeque::new();
            for &id in &level {
                sorted.push(id);
                if let Some(deps) = dependents.get(id) {
                    for &dep_id in deps {
                        let entry = in_degree.get_mut(dep_id).unwrap();
                        *entry -= 1;
                        if *entry == 0 {
                            next_queue.push_back(dep_id);
                        }
                    }
                }
            }
            levels.push(level);
            queue = next_queue;
        }

        if sorted.len() != self.nodes.len() {
            let sorted_set: HashSet<_> = sorted.into_iter().collect();
            let unvisited: Vec<_> = self
                .nodes
                .iter()
                .filter(|n| !sorted_set.contains(n.id.as_str()))
                .map(|n| n.id.as_str())
                .collect();
            bail!(
                "cycle detected in pipeline '{}': nodes {:?} could not be resolved",
                self.pipeline.name,
                unvisited
            );
        }

        let node_map: HashMap<&str, &PipelineNode> =
            self.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

        Ok(ExecutionPlan {
            levels: levels
                .into_iter()
                .map(|level| level.into_iter().map(|id| node_map[id].clone()).collect())
                .collect(),
        })
    }
}

impl ExecutionPlan {
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pipeline() -> PipelineConfig {
        PipelineConfig {
            pipeline: PipelineMeta {
                name: "full-release".into(),
                description: None,
            },
            nodes: vec![
                PipelineNode {
                    id: "compile".into(),
                    workflow: "wf_cargo_build".into(),
                    needs: vec![],
                },
                PipelineNode {
                    id: "audit".into(),
                    workflow: "wf_cross_audit".into(),
                    needs: vec!["compile".into()],
                },
                PipelineNode {
                    id: "flash".into(),
                    workflow: "wf_esptool_flash".into(),
                    needs: vec!["audit".into()],
                },
            ],
        }
    }

    #[test]
    fn topo_sort_linear_chain() {
        let config = sample_pipeline();
        let plan = config.plan().expect("plan");
        assert_eq!(plan.levels.len(), 3);
        assert_eq!(plan.levels[0][0].id, "compile");
        assert_eq!(plan.levels[1][0].id, "audit");
        assert_eq!(plan.levels[2][0].id, "flash");
    }

    #[test]
    fn topo_sort_diamond() {
        let config = PipelineConfig {
            pipeline: PipelineMeta {
                name: "diamond".into(),
                description: None,
            },
            nodes: vec![
                PipelineNode {
                    id: "a".into(),
                    workflow: "wf_a".into(),
                    needs: vec![],
                },
                PipelineNode {
                    id: "b".into(),
                    workflow: "wf_b".into(),
                    needs: vec!["a".into()],
                },
                PipelineNode {
                    id: "c".into(),
                    workflow: "wf_c".into(),
                    needs: vec!["a".into()],
                },
                PipelineNode {
                    id: "d".into(),
                    workflow: "wf_d".into(),
                    needs: vec!["b".into(), "c".into()],
                },
            ],
        };
        let plan = config.plan().expect("plan");
        assert_eq!(plan.levels.len(), 3);
        assert_eq!(plan.levels[0].len(), 1);
        assert_eq!(plan.levels[1].len(), 2);
        assert_eq!(plan.levels[2].len(), 1);
    }

    #[test]
    fn topo_sort_independent_nodes() {
        let config = PipelineConfig {
            pipeline: PipelineMeta {
                name: "parallel".into(),
                description: None,
            },
            nodes: vec![
                PipelineNode {
                    id: "a".into(),
                    workflow: "wf_a".into(),
                    needs: vec![],
                },
                PipelineNode {
                    id: "b".into(),
                    workflow: "wf_b".into(),
                    needs: vec![],
                },
                PipelineNode {
                    id: "c".into(),
                    workflow: "wf_c".into(),
                    needs: vec![],
                },
            ],
        };
        let plan = config.plan().expect("plan");
        assert_eq!(plan.levels.len(), 1);
        assert_eq!(plan.levels[0].len(), 3);
    }

    #[test]
    fn cycle_detection() {
        let config = PipelineConfig {
            pipeline: PipelineMeta {
                name: "cycle".into(),
                description: None,
            },
            nodes: vec![
                PipelineNode {
                    id: "a".into(),
                    workflow: "wf_a".into(),
                    needs: vec!["b".into()],
                },
                PipelineNode {
                    id: "b".into(),
                    workflow: "wf_b".into(),
                    needs: vec!["a".into()],
                },
            ],
        };
        let err = config.plan().unwrap_err();
        assert!(
            err.to_string().contains("cycle"),
            "expected cycle error, got: {err}"
        );
    }

    #[test]
    fn unknown_dependency_rejected() {
        let config = PipelineConfig {
            pipeline: PipelineMeta {
                name: "bad".into(),
                description: None,
            },
            nodes: vec![PipelineNode {
                id: "a".into(),
                workflow: "wf_a".into(),
                needs: vec!["nonexistent".into()],
            }],
        };
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("unknown dependency"),
            "expected unknown dep error, got: {err}"
        );
    }

    #[test]
    fn duplicate_node_id_rejected() {
        let config = PipelineConfig {
            pipeline: PipelineMeta {
                name: "dup".into(),
                description: None,
            },
            nodes: vec![
                PipelineNode {
                    id: "a".into(),
                    workflow: "wf_a".into(),
                    needs: vec![],
                },
                PipelineNode {
                    id: "a".into(),
                    workflow: "wf_b".into(),
                    needs: vec![],
                },
            ],
        };
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("duplicate node id"),
            "expected duplicate error, got: {err}"
        );
    }
}
