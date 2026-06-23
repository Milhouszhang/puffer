use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// AgentEnv workflow graph definition shared by the editor and runtime API.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AgentEnvWorkflowDefinition {
    /// Ordered workflow nodes in the graph.
    pub nodes: Vec<AgentEnvWorkflowNode>,
    /// Directed edges connecting source nodes to target nodes.
    pub edges: Vec<AgentEnvWorkflowEdge>,
}

/// AgentEnv workflow node with opaque business config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentEnvWorkflowNode {
    /// Stable node identifier unique within the workflow.
    pub id: String,
    /// Runtime node type resolved through AgentEnv node definitions.
    #[serde(rename = "type")]
    pub node_type: String,
    /// Optional display name shown in visual editors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Opaque node config validated by AgentEnv node schemas.
    #[serde(default)]
    pub config: BTreeMap<String, Value>,
    /// Whether this node is trusted for runtime execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted: Option<bool>,
    /// Optional visual editor position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<AgentEnvWorkflowPosition>,
}

/// AgentEnv workflow edge connecting one node to another.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentEnvWorkflowEdge {
    /// Source node id.
    pub source: String,
    /// Target node id.
    pub target: String,
    /// Optional JavaScript condition evaluated by AgentEnv.
    #[serde(
        default,
        rename = "conditionScript",
        skip_serializing_if = "Option::is_none"
    )]
    pub condition_script: Option<String>,
}

/// Visual editor coordinates for one workflow node.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct AgentEnvWorkflowPosition {
    /// Horizontal canvas coordinate.
    pub x: f64,
    /// Vertical canvas coordinate.
    pub y: f64,
}

/// Request payload sent to `POST /v1/workflows`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentEnvCreateWorkflowRequest {
    /// Workflow display name.
    pub name: String,
    /// Optional workflow description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Shared AgentEnv-compatible workflow graph.
    pub definition: AgentEnvWorkflowDefinition,
}

/// Request payload sent to `PUT /v1/workflows/:id`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AgentEnvUpdateWorkflowRequest {
    /// Optional workflow display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional workflow description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional shared AgentEnv-compatible workflow graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<AgentEnvWorkflowDefinition>,
    /// Optional workflow status accepted by AgentEnv.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Request payload sent to `POST /v1/workflows/execute-in-memory`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentEnvInMemoryExecuteRequest {
    /// Shared AgentEnv-compatible workflow graph to validate and run.
    pub definition: AgentEnvWorkflowDefinition,
    /// Optional execution input object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<BTreeMap<String, Value>>,
    /// Optional trigger node id for the in-memory run.
    #[serde(
        default,
        rename = "triggerNodeId",
        skip_serializing_if = "Option::is_none"
    )]
    pub trigger_node_id: Option<String>,
}
