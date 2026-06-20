use anyhow::{Context, Result};
use puffer_config::{load_config, ConfigPaths};
use puffer_workflow::WorkflowRuntimeRecord;
use serde_json::Value;

const WORKFLOW_ID_KEYS: &[&str] = &[
    "workflowId",
    "workflow_id",
    "workflowSlug",
    "workflow_slug",
    "id",
];

/// Creates one workflow in the configured runtime.
pub(crate) fn handle_workflow_create(paths: &ConfigPaths, params: &Value) -> Result<Value> {
    let client = workflow_runtime_client(paths)?;
    let request = record_param_or_root(params, "workflow", "workflow_create")?;
    Ok(serde_json::to_value(client.create_workflow(&request)?)?)
}

/// Deploys one workflow in the configured runtime.
pub(crate) fn handle_workflow_deploy(paths: &ConfigPaths, params: &Value) -> Result<Value> {
    let client = workflow_runtime_client(paths)?;
    let workflow_id = required_string(params, WORKFLOW_ID_KEYS, "workflow id")?;
    Ok(serde_json::to_value(client.deploy_workflow(&workflow_id)?)?)
}

/// Executes one workflow in the configured runtime.
pub(crate) fn handle_workflow_execute(paths: &ConfigPaths, params: &Value) -> Result<Value> {
    let client = workflow_runtime_client(paths)?;
    let workflow_id = required_string(params, WORKFLOW_ID_KEYS, "workflow id")?;
    let request = workflow_execute_request(params)?;
    Ok(serde_json::to_value(
        client.execute_workflow(&workflow_id, &request)?,
    )?)
}

/// Lists executions for one workflow from the configured runtime.
pub(crate) fn handle_workflow_list_executions(
    paths: &ConfigPaths,
    params: &Value,
) -> Result<Value> {
    let client = workflow_runtime_client(paths)?;
    let workflow_id = required_string(params, WORKFLOW_ID_KEYS, "workflow id")?;
    Ok(serde_json::to_value(client.list_executions(&workflow_id)?)?)
}

/// Fetches one workflow execution from the configured runtime.
pub(crate) fn handle_workflow_get_execution(paths: &ConfigPaths, params: &Value) -> Result<Value> {
    let client = workflow_runtime_client(paths)?;
    let workflow_id = required_string(params, WORKFLOW_ID_KEYS, "workflow id")?;
    let execution_id = required_string(
        params,
        &["executionId", "execution_id", "runId", "run_id"],
        "execution id",
    )?;
    Ok(serde_json::to_value(
        client.get_execution(&workflow_id, &execution_id)?,
    )?)
}

fn workflow_runtime_client(paths: &ConfigPaths) -> Result<puffer_workflow::WorkflowRuntimeClient> {
    let config = load_config(paths).context("load workflow backend config")?;
    crate::daemon_workflow_runtime::workflow_runtime_client(paths, &config)
        .context("create workflow runtime client")
}

fn workflow_execute_request(params: &Value) -> Result<WorkflowRuntimeRecord> {
    if params.get("request").is_some() {
        return record_param_or_root(params, "request", "workflow_execute request");
    }
    if params.get("execution").is_some() {
        return record_param_or_root(params, "execution", "workflow_execute request");
    }

    let mut object = serde_json::Map::new();
    if let Some(input) = params.get("input") {
        object.insert("input".to_string(), input.clone());
    }
    if let Some(trigger_node_id) = params
        .get("triggerNodeId")
        .or_else(|| params.get("trigger_node_id"))
    {
        object.insert("triggerNodeId".to_string(), trigger_node_id.clone());
    }
    WorkflowRuntimeRecord::try_from(Value::Object(object))
        .context("workflow_execute request must be a JSON object")
}

fn record_param_or_root(params: &Value, key: &str, label: &str) -> Result<WorkflowRuntimeRecord> {
    let value = params.get(key).unwrap_or(params).clone();
    WorkflowRuntimeRecord::try_from(value).with_context(|| format!("{label} must be a JSON object"))
}

fn required_string(params: &Value, keys: &[&str], label: &str) -> Result<String> {
    for key in keys {
        if let Some(value) = params
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(value.to_string());
        }
    }
    anyhow::bail!("missing {label}")
}
