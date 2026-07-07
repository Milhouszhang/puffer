use crate::{
    automation_spec_hash, AgentEnvNodeRef, AutomationFlowSpec, AutomationRecord,
    AutomationStepSpec, AutomationTriggerSpec, CompiledWorkflowRole,
};
pub use puffer_workflow::{AgentEnvWorkflowDefinition, AgentEnvWorkflowEdge, AgentEnvWorkflowNode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Internal runtime plan produced from one user-facing Automation record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationCompilePlan {
    pub automation_id: String,
    pub revision: u64,
    pub spec_hash: String,
    pub workflows: Vec<CompiledWorkflowDefinition>,
    pub puffer_bindings: Vec<CompiledPufferBindingPlan>,
}

/// One AgentEnv workflow definition artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledWorkflowDefinition {
    pub role: CompiledWorkflowRole,
    pub definition: Value,
    pub definition_hash: String,
}

/// One Puffer-side connector binding artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledPufferBindingPlan {
    pub trigger_id: String,
    pub binding_slug: String,
    pub connection_slug: String,
    pub connector_slug: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AutomationCompileError {
    #[error("invalid automation spec hash: {0}")]
    Hash(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("failed to serialize AgentEnv workflow definition: {0}")]
    Serialize(String),
}

/// Compiles a persisted Automation into internal AgentEnv workflow artifacts
/// plus Puffer-owned connector binding plans.
pub fn compile_automation(
    record: &AutomationRecord,
) -> Result<AutomationCompilePlan, AutomationCompileError> {
    let spec_hash = automation_spec_hash(&record.spec).map_err(AutomationCompileError::Hash)?;
    let mut workflows = Vec::new();
    workflows.push(compile_root_workflow(record)?);
    workflows.extend(compile_loop_body_workflows(&record.spec.flow)?);

    let puffer_bindings = record
        .spec
        .triggers
        .iter()
        .filter_map(|trigger| match trigger {
            AutomationTriggerSpec::PufferConnection {
                id,
                connection_slug,
                connector_slug,
                ..
            } => Some(CompiledPufferBindingPlan {
                trigger_id: id.clone(),
                binding_slug: automation_binding_slug(&record.id, id),
                connection_slug: connection_slug.clone(),
                connector_slug: connector_slug.clone(),
            }),
            AutomationTriggerSpec::AgentEnvNode { .. } | AutomationTriggerSpec::Manual { .. } => {
                None
            }
        })
        .collect();

    Ok(AutomationCompilePlan {
        automation_id: record.id.clone(),
        revision: record.revision,
        spec_hash,
        workflows,
        puffer_bindings,
    })
}

fn compile_root_workflow(
    record: &AutomationRecord,
) -> Result<CompiledWorkflowDefinition, AutomationCompileError> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut previous_ids = Vec::new();

    for trigger in &record.spec.triggers {
        match trigger {
            AutomationTriggerSpec::AgentEnvNode { id, node, .. } => {
                nodes.push(agentenv_node(id, node));
                previous_ids.push(id.clone());
            }
            AutomationTriggerSpec::Manual { id, .. }
            | AutomationTriggerSpec::PufferConnection { id, .. } => {
                nodes.push(puffer_ingress_node(&record.id, id));
                previous_ids.push(id.clone());
            }
        }
    }

    let mut seen_loop = None::<String>;
    for step in &record.spec.flow.steps {
        match step {
            AutomationStepSpec::AgentEnvNode { id, node, .. } => {
                if let Some(loop_id) = seen_loop {
                    return Err(AutomationCompileError::Unsupported(format!(
                        "loop continuation compilation is not implemented yet; step `{id}` follows loop `{loop_id}`"
                    )));
                }
                nodes.push(agentenv_node(id, node));
                for previous_id in &previous_ids {
                    edges.push(AgentEnvWorkflowEdge {
                        source: previous_id.clone(),
                        target: id.clone(),
                        condition_script: None,
                    });
                }
                previous_ids = vec![id.clone()];
            }
            AutomationStepSpec::Loop { id, .. } => {
                if seen_loop.is_some() {
                    return Err(AutomationCompileError::Unsupported(
                        "multiple loop steps in one Automation are not implemented yet".into(),
                    ));
                }
                seen_loop = Some(id.clone());
            }
        }
    }

    compiled_workflow(
        CompiledWorkflowRole::Root,
        AgentEnvWorkflowDefinition { nodes, edges },
    )
}

fn compile_loop_body_workflows(
    flow: &AutomationFlowSpec,
) -> Result<Vec<CompiledWorkflowDefinition>, AutomationCompileError> {
    let mut workflows = Vec::new();
    for step in &flow.steps {
        if let AutomationStepSpec::Loop { id, body, .. } = step {
            workflows.push(compile_loop_body_workflow(id, body)?);
        }
    }
    Ok(workflows)
}

fn compile_loop_body_workflow(
    step_id: &str,
    body: &AutomationFlowSpec,
) -> Result<CompiledWorkflowDefinition, AutomationCompileError> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut previous_id = None::<String>;

    for step in &body.steps {
        match step {
            AutomationStepSpec::AgentEnvNode { id, node, .. } => {
                nodes.push(agentenv_node(id, node));
                if let Some(source) = previous_id {
                    edges.push(AgentEnvWorkflowEdge {
                        source,
                        target: id.clone(),
                        condition_script: None,
                    });
                }
                previous_id = Some(id.clone());
            }
            AutomationStepSpec::Loop { id, .. } => {
                return Err(AutomationCompileError::Unsupported(format!(
                    "nested loop compilation is not implemented yet; loop `{id}` is inside loop `{step_id}`"
                )));
            }
        }
    }

    compiled_workflow(
        CompiledWorkflowRole::LoopBody {
            step_id: step_id.to_string(),
        },
        AgentEnvWorkflowDefinition { nodes, edges },
    )
}

fn agentenv_node(id: &str, node: &AgentEnvNodeRef) -> AgentEnvWorkflowNode {
    AgentEnvWorkflowNode {
        id: id.to_string(),
        node_type: node.node_type.clone(),
        name: node.name.clone(),
        config: node.config.clone(),
        trusted: node.trusted,
        position: None,
    }
}

fn puffer_ingress_node(automation_id: &str, trigger_id: &str) -> AgentEnvWorkflowNode {
    let mut config = std::collections::BTreeMap::new();
    config.insert(
        "path".to_string(),
        Value::String(format!("puffer-automation-{automation_id}-{trigger_id}")),
    );
    config.insert(
        "methods".to_string(),
        Value::Array(vec![Value::String("POST".to_string())]),
    );
    config.insert(
        "authentication".to_string(),
        Value::String("none".to_string()),
    );
    AgentEnvWorkflowNode {
        id: trigger_id.to_string(),
        node_type: "webhook".to_string(),
        name: Some("Puffer trigger".to_string()),
        config,
        trusted: Some(false),
        position: None,
    }
}

fn compiled_workflow(
    role: CompiledWorkflowRole,
    definition: AgentEnvWorkflowDefinition,
) -> Result<CompiledWorkflowDefinition, AutomationCompileError> {
    let definition = serde_json::to_value(definition)
        .map_err(|error| AutomationCompileError::Serialize(error.to_string()))?;
    let definition_hash = stable_json_hash(&definition)?;
    Ok(CompiledWorkflowDefinition {
        role,
        definition,
        definition_hash,
    })
}

fn stable_json_hash(value: &Value) -> Result<String, AutomationCompileError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| AutomationCompileError::Serialize(error.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn automation_binding_slug(automation_id: &str, trigger_id: &str) -> String {
    format!("automation-{automation_id}-{trigger_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AutomationFlowSpec, AutomationLoopInput, AutomationLoopSpec, AutomationLoopStopSpec,
        AutomationReviewSpec, AutomationSource, AutomationSpec, AutomationStatus,
        AUTOMATION_SPEC_VERSION,
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    fn node(node_type: &str) -> AgentEnvNodeRef {
        AgentEnvNodeRef {
            node_type: node_type.to_string(),
            name: Some(node_type.to_string()),
            trusted: Some(true),
            config: BTreeMap::new(),
        }
    }

    fn record(flow: AutomationFlowSpec, triggers: Vec<AutomationTriggerSpec>) -> AutomationRecord {
        AutomationRecord {
            id: "reply-helper".into(),
            status: AutomationStatus::Enabled,
            revision: 7,
            spec: AutomationSpec {
                spec_version: AUTOMATION_SPEC_VERSION,
                name: "Reply helper".into(),
                description: None,
                source: AutomationSource::Blank,
                instructions: "Draft a reply.".into(),
                run_location: Default::default(),
                triggers,
                flow,
                review: AutomationReviewSpec::default(),
            },
            runtime: Default::default(),
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    fn manual_trigger() -> AutomationTriggerSpec {
        AutomationTriggerSpec::Manual {
            id: "manual".into(),
            summary: None,
        }
    }

    #[test]
    fn linear_automation_compiles_one_root_workflow() {
        let record = record(
            AutomationFlowSpec {
                steps: vec![
                    AutomationStepSpec::AgentEnvNode {
                        id: "draft".into(),
                        node: node("llm"),
                        summary: None,
                    },
                    AutomationStepSpec::AgentEnvNode {
                        id: "format".into(),
                        node: node("transform"),
                        summary: None,
                    },
                ],
            },
            vec![manual_trigger()],
        );

        let plan = compile_automation(&record).unwrap();

        assert_eq!(plan.workflows.len(), 1);
        assert_eq!(plan.workflows[0].role, CompiledWorkflowRole::Root);
        assert_eq!(
            plan.workflows[0].definition["nodes"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            plan.workflows[0].definition["edges"],
            json!([
                {"source": "manual", "target": "draft"},
                {"source": "draft", "target": "format"}
            ])
        );
    }

    #[test]
    fn step_edges_are_linear_without_self_loop() {
        let record = record(
            AutomationFlowSpec {
                steps: vec![
                    AutomationStepSpec::AgentEnvNode {
                        id: "a".into(),
                        node: node("a"),
                        summary: None,
                    },
                    AutomationStepSpec::AgentEnvNode {
                        id: "b".into(),
                        node: node("b"),
                        summary: None,
                    },
                ],
            },
            vec![manual_trigger()],
        );

        let plan = compile_automation(&record).unwrap();
        let edges = plan.workflows[0].definition["edges"].as_array().unwrap();

        assert!(edges.iter().all(|edge| edge["source"] != edge["target"]));
    }

    #[test]
    fn puffer_connection_trigger_generates_binding_plan() {
        let record = record(
            AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "draft".into(),
                    node: node("llm"),
                    summary: None,
                }],
            },
            vec![AutomationTriggerSpec::PufferConnection {
                id: "incoming".into(),
                connection_slug: "telegram-user".into(),
                connector_slug: Some("telegram-login".into()),
                filter: None,
                ignore_filters: Vec::new(),
                contact_ids: Vec::new(),
                summary: None,
            }],
        );

        let plan = compile_automation(&record).unwrap();

        assert_eq!(plan.puffer_bindings.len(), 1);
        assert_eq!(
            plan.puffer_bindings[0].binding_slug,
            "automation-reply-helper-incoming"
        );
        assert_eq!(plan.puffer_bindings[0].connection_slug, "telegram-user");
        assert_eq!(plan.workflows[0].definition["nodes"][0]["id"], "incoming");
        assert_eq!(plan.workflows[0].definition["nodes"][0]["type"], "webhook");
        assert_eq!(
            plan.workflows[0].definition["edges"],
            json!([
                {"source": "incoming", "target": "draft"}
            ])
        );
    }

    #[test]
    fn agentenv_trigger_becomes_root_start_node() {
        let record = record(
            AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "draft".into(),
                    node: node("llm"),
                    summary: None,
                }],
            },
            vec![AutomationTriggerSpec::AgentEnvNode {
                id: "webhook".into(),
                node: node("webhook"),
                summary: None,
            }],
        );

        let plan = compile_automation(&record).unwrap();

        assert_eq!(
            plan.workflows[0].definition["nodes"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            plan.workflows[0].definition["edges"],
            json!([
                {"source": "webhook", "target": "draft"}
            ])
        );
    }

    #[test]
    fn loop_compiles_root_and_loop_body_without_backedge() {
        let record = record(
            AutomationFlowSpec {
                steps: vec![
                    AutomationStepSpec::AgentEnvNode {
                        id: "prepare".into(),
                        node: node("prepare"),
                        summary: None,
                    },
                    AutomationStepSpec::Loop {
                        id: "retry".into(),
                        loop_spec: AutomationLoopSpec::Repeat {
                            input: AutomationLoopInput::Trigger,
                            stop_when: AutomationLoopStopSpec::OutputEquals {
                                path: "$.done".into(),
                                value: json!(true),
                            },
                            max_iterations: 3,
                        },
                        body: AutomationFlowSpec {
                            steps: vec![AutomationStepSpec::AgentEnvNode {
                                id: "attempt".into(),
                                node: node("attempt"),
                                summary: None,
                            }],
                        },
                        summary: None,
                    },
                ],
            },
            vec![manual_trigger()],
        );

        let plan = compile_automation(&record).unwrap();

        assert_eq!(plan.workflows.len(), 2);
        assert_eq!(
            plan.workflows[1].role,
            CompiledWorkflowRole::LoopBody {
                step_id: "retry".into()
            }
        );
        for workflow in &plan.workflows {
            for edge in workflow.definition["edges"].as_array().unwrap() {
                assert_ne!(edge["source"], edge["target"]);
            }
        }
    }

    #[test]
    fn definition_hash_is_stable() {
        let record = record(
            AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "draft".into(),
                    node: node("llm"),
                    summary: None,
                }],
            },
            vec![manual_trigger()],
        );

        let first = compile_automation(&record).unwrap();
        let second = compile_automation(&record).unwrap();

        assert_eq!(
            first.workflows[0].definition_hash,
            second.workflows[0].definition_hash
        );
    }

    #[test]
    fn unsupported_loop_continuation_returns_clear_error() {
        let record = record(
            AutomationFlowSpec {
                steps: vec![
                    AutomationStepSpec::Loop {
                        id: "retry".into(),
                        loop_spec: AutomationLoopSpec::Repeat {
                            input: AutomationLoopInput::Trigger,
                            stop_when: AutomationLoopStopSpec::OutputEquals {
                                path: "$.done".into(),
                                value: json!(true),
                            },
                            max_iterations: 3,
                        },
                        body: AutomationFlowSpec {
                            steps: vec![AutomationStepSpec::AgentEnvNode {
                                id: "attempt".into(),
                                node: node("attempt"),
                                summary: None,
                            }],
                        },
                        summary: None,
                    },
                    AutomationStepSpec::AgentEnvNode {
                        id: "after".into(),
                        node: node("after"),
                        summary: None,
                    },
                ],
            },
            vec![manual_trigger()],
        );

        let error = compile_automation(&record).unwrap_err();

        assert!(error
            .to_string()
            .contains("loop continuation compilation is not implemented yet"));
    }
}
