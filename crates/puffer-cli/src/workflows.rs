use crate::cli_args::{WorkflowCommand, WorkflowRunsCommand};
use anyhow::{Context, Result};
use puffer_config::{load_config, ConfigPaths};
use puffer_workflow::{
    WorkflowRuntimeClient, WorkflowRuntimeClientConfig, WorkflowRuntimeCreateWorkflowRequest,
    WorkflowRuntimeExecuteRequest, WorkflowRuntimeExecution, WorkflowRuntimeInMemoryExecuteRequest,
    WorkflowRuntimeRecord, WorkflowRuntimeWorkflow,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::workflow_runtime_helpers::{
    record_string, record_string_for_keys, workflow_execute_summary,
};

const AGENTENV_API_URL_ENV: &str = "AGENTENV_API_URL";
const AGENTENV_UI_URL_ENV: &str = "AGENTENV_UI_URL";
const AGENTENV_WORKSPACE_ID_ENV: &str = "AGENTENV_WORKSPACE_ID";
const AGENTENV_API_KEY_ENV: &str = "AGENTENV_API_KEY";
const AGENTENV_SMOKE_WORKFLOW_ID_ENV: &str = "AGENTENV_SMOKE_WORKFLOW_ID";
const AGENTENV_SMOKE_WORKFLOW_NAME_ENV: &str = "AGENTENV_SMOKE_WORKFLOW_NAME";
const AGENTENV_SMOKE_WORKFLOW_JSON_ENV: &str = "AGENTENV_SMOKE_WORKFLOW_JSON";
const AGENTENV_SMOKE_EXECUTE_JSON_ENV: &str = "AGENTENV_SMOKE_EXECUTE_JSON";
const AGENTENV_SMOKE_IN_MEMORY_JSON_ENV: &str = "AGENTENV_SMOKE_IN_MEMORY_JSON";
const DEFAULT_SMOKE_WORKFLOW_NAME: &str = "puffer-runtime-smoke";

/// Executes a `puffer workflow ...` CLI command against the configured runtime.
pub(crate) fn run_workflow_command(command: WorkflowCommand, paths: &ConfigPaths) -> Result<()> {
    match command {
        WorkflowCommand::Ls { json } => {
            let workflows = workflow_runtime_client(paths)?.list_workflows()?;
            if json {
                print_json(&workflows)
            } else {
                print_workflow_table(&workflows);
                Ok(())
            }
        }
        WorkflowCommand::Runs { command } => run_runs_command(command, paths),
        WorkflowCommand::Run {
            workflow_slug,
            trigger_json,
            json,
        } => run_once(paths, &workflow_slug, trigger_json.as_deref(), json),
        WorkflowCommand::SmokeTest { workflow_id, json } => {
            run_smoke_test(paths, workflow_id.as_deref(), json)
        }
    }
}

fn run_once(
    paths: &ConfigPaths,
    workflow_slug: &str,
    trigger_json: Option<&str>,
    json: bool,
) -> Result<()> {
    let input = match trigger_json {
        Some(raw) => serde_json::from_str(raw).context("parse --trigger-json")?,
        None => serde_json::json!({}),
    };
    let request = workflow_execute_request(input)?;
    let response = workflow_runtime_client(paths)?.execute_workflow(workflow_slug, &request)?;
    if json {
        print_json(&response)
    } else {
        println!("{}", workflow_execute_summary(workflow_slug, &response));
        Ok(())
    }
}

fn run_runs_command(command: WorkflowRunsCommand, paths: &ConfigPaths) -> Result<()> {
    match command {
        WorkflowRunsCommand::Ls {
            workflow_slug,
            json,
        } => {
            let executions = workflow_runtime_client(paths)?.list_executions(&workflow_slug)?;
            if json {
                print_json(&executions)
            } else {
                print_execution_table(&executions);
                Ok(())
            }
        }
    }
}

fn workflow_runtime_client(paths: &ConfigPaths) -> Result<puffer_workflow::WorkflowRuntimeClient> {
    let config = load_config(paths).context("load workflow backend config")?;
    crate::daemon_workflow_runtime::workflow_runtime_client(paths, &config)
}

#[derive(Debug)]
struct SmokeRuntime {
    client: WorkflowRuntimeClient,
    config: WorkflowSmokeConfigDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowSmokeConfigDto {
    source: String,
    api_url: String,
    ui_url: Option<String>,
    workspace_id: String,
    api_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowSmokeRequestDto {
    method: String,
    url: String,
    headers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowSmokeStepDto {
    name: String,
    method: String,
    url: String,
    state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    item_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowSmokeReportDto {
    success: bool,
    config: WorkflowSmokeConfigDto,
    requests: Vec<WorkflowSmokeRequestDto>,
    steps: Vec<WorkflowSmokeStepDto>,
    workflow_id: String,
    execution_id: String,
}

fn run_smoke_test(paths: &ConfigPaths, workflow_id_arg: Option<&str>, json: bool) -> Result<()> {
    let runtime = smoke_runtime(paths)?;
    let requests = smoke_requests(&runtime.config);
    let workflow_name = env_trimmed(AGENTENV_SMOKE_WORKFLOW_NAME_ENV)
        .unwrap_or_else(|| DEFAULT_SMOKE_WORKFLOW_NAME.to_string());
    let mut steps = Vec::new();

    let api_key_context = runtime
        .client
        .api_key_context()
        .context("test API key context")?;
    steps.push(passed_step(
        "api_key_context",
        "GET",
        &request_url(&runtime.config.api_url, "/v1/auth/api-key-context"),
        None,
        record_id(&api_key_context),
        None,
    ));

    let node_definitions = runtime
        .client
        .list_node_definitions()
        .context("list workflow node definitions")?;
    steps.push(passed_step(
        "list_node_definitions",
        "GET",
        &request_url(&runtime.config.api_url, "/v1/workflows/node-definitions"),
        Some(node_definitions.len()),
        None,
        None,
    ));

    let workflows = runtime.client.list_workflows().context("list workflows")?;
    steps.push(passed_step(
        "list_workflows",
        "GET",
        &request_url(&runtime.config.api_url, "/v1/workflows"),
        Some(workflows.len()),
        None,
        None,
    ));

    let requested_workflow_id = workflow_id_arg
        .and_then(non_empty)
        .map(ToString::to_string)
        .or_else(|| env_trimmed(AGENTENV_SMOKE_WORKFLOW_ID_ENV));
    let existing_workflow = requested_workflow_id
        .as_deref()
        .and_then(|id| find_workflow_by_id(&workflows, id))
        .or_else(|| find_workflow_by_name(&workflows, &workflow_name));

    let (workflow_id, workflow_record) = if let Some(workflow) = existing_workflow {
        let id = record_id(&workflow).context("existing workflow is missing an id")?;
        steps.push(passed_step(
            "create_workflow",
            "POST",
            &request_url(&runtime.config.api_url, "/v1/workflows"),
            None,
            Some(id.clone()),
            Some("skipped; existing smoke workflow found".to_string()),
        ));
        (id, Some(workflow))
    } else if let Some(id) = requested_workflow_id {
        steps.push(passed_step(
            "create_workflow",
            "POST",
            &request_url(&runtime.config.api_url, "/v1/workflows"),
            None,
            Some(id.clone()),
            Some("skipped; workflow id supplied".to_string()),
        ));
        (id, None)
    } else {
        let create_request = smoke_create_workflow_request(&workflow_name)?;
        let created = runtime
            .client
            .create_workflow(&create_request)
            .context(
                "create minimal smoke workflow; set AGENTENV_SMOKE_WORKFLOW_JSON if the runtime needs a different create DTO",
            )?;
        let id = record_id(&created).context("created workflow response is missing an id")?;
        steps.push(passed_step(
            "create_workflow",
            "POST",
            &request_url(&runtime.config.api_url, "/v1/workflows"),
            None,
            Some(id.clone()),
            None,
        ));
        (id, Some(created))
    };

    if workflow_record
        .as_ref()
        .is_some_and(workflow_record_is_deployed)
    {
        steps.push(passed_step(
            "deploy_workflow",
            "POST",
            &request_url(
                &runtime.config.api_url,
                &format!("/v1/workflows/{workflow_id}/deploy"),
            ),
            None,
            Some(workflow_id.clone()),
            Some("skipped; workflow already appears deployed".to_string()),
        ));
    } else {
        let deployed = runtime
            .client
            .deploy_workflow(&workflow_id)
            .context("deploy smoke workflow")?;
        steps.push(passed_step(
            "deploy_workflow",
            "POST",
            &request_url(
                &runtime.config.api_url,
                &format!("/v1/workflows/{workflow_id}/deploy"),
            ),
            None,
            record_id(&deployed).or_else(|| Some(workflow_id.clone())),
            None,
        ));
    }

    let execute_request = smoke_execute_request()?;
    let execute_response = runtime
        .client
        .execute_workflow(&workflow_id, &execute_request)
        .context("execute smoke workflow")?;
    let mut execution_id = execution_id_from_record(&execute_response);
    steps.push(passed_step(
        "execute_workflow",
        "POST",
        &request_url(
            &runtime.config.api_url,
            &format!("/v1/workflows/{workflow_id}/execute"),
        ),
        None,
        execution_id.clone(),
        None,
    ));

    let executions = runtime
        .client
        .list_executions(&workflow_id)
        .context("list workflow executions")?;
    if execution_id.is_none() {
        execution_id = executions.first().and_then(execution_id_from_record);
    }
    steps.push(passed_step(
        "list_executions",
        "GET",
        &request_url(
            &runtime.config.api_url,
            &format!("/v1/workflows/{workflow_id}/executions"),
        ),
        Some(executions.len()),
        None,
        None,
    ));

    let execution_id =
        execution_id.context("execution response and execution list are missing an id")?;
    let execution = runtime
        .client
        .get_execution(&workflow_id, &execution_id)
        .context("fetch workflow execution")?;
    steps.push(passed_step(
        "get_execution",
        "GET",
        &request_url(
            &runtime.config.api_url,
            &format!("/v1/workflows/{workflow_id}/executions/{execution_id}"),
        ),
        None,
        execution_id_from_record(&execution).or_else(|| Some(execution_id.clone())),
        None,
    ));

    if let Some(in_memory_request) = smoke_in_memory_request()? {
        let in_memory = runtime
            .client
            .execute_in_memory(&in_memory_request)
            .context("execute in-memory workflow")?;
        steps.push(passed_step(
            "execute_in_memory",
            "POST",
            &request_url(&runtime.config.api_url, "/v1/workflows/execute-in-memory"),
            None,
            execution_id_from_record(&in_memory),
            Some(format!("enabled by {AGENTENV_SMOKE_IN_MEMORY_JSON_ENV}")),
        ));
    }

    let report = WorkflowSmokeReportDto {
        success: true,
        config: runtime.config,
        requests,
        steps,
        workflow_id,
        execution_id,
    };
    if json {
        print_json(&report)
    } else {
        print_smoke_report(&report);
        Ok(())
    }
}

fn smoke_runtime(paths: &ConfigPaths) -> Result<SmokeRuntime> {
    let env_values = [
        env_trimmed(AGENTENV_API_URL_ENV),
        env_trimmed(AGENTENV_WORKSPACE_ID_ENV),
        env_trimmed(AGENTENV_API_KEY_ENV),
    ];
    if env_values.iter().any(Option::is_some) {
        let api_url = env_values[0]
            .clone()
            .with_context(|| format!("missing {AGENTENV_API_URL_ENV}"))?;
        let workspace_id = env_values[1]
            .clone()
            .with_context(|| format!("missing {AGENTENV_WORKSPACE_ID_ENV}"))?;
        let api_key = env_values[2]
            .clone()
            .with_context(|| format!("missing {AGENTENV_API_KEY_ENV}"))?;
        let client = WorkflowRuntimeClient::new(WorkflowRuntimeClientConfig::new(
            api_url.clone(),
            api_key.clone(),
            workspace_id.clone(),
        ))
        .context("create workflow runtime client from AGENTENV_* env")?;
        return Ok(SmokeRuntime {
            client,
            config: WorkflowSmokeConfigDto {
                source: "env".to_string(),
                api_url: api_root_for_display(&api_url),
                ui_url: env_trimmed(AGENTENV_UI_URL_ENV),
                workspace_id,
                api_key: redact_secret(&api_key),
            },
        });
    }

    let config = load_config(paths).context("load workflow backend config")?;
    let mut backend = config.workflow_backend.clone();
    backend.normalize();
    let api_key = if backend.api_token_secret_id.is_empty() {
        "secret:<missing>".to_string()
    } else {
        format!("secret:{}", backend.api_token_secret_id)
    };
    let client = crate::daemon_workflow_runtime::workflow_runtime_client(paths, &config)
        .context("create workflow runtime client from saved config")?;
    Ok(SmokeRuntime {
        client,
        config: WorkflowSmokeConfigDto {
            source: "config.workflow_backend".to_string(),
            api_url: backend.api_base_url,
            ui_url: non_empty(&backend.frontend_url).map(ToString::to_string),
            workspace_id: backend.workspace_id,
            api_key,
        },
    })
}

fn smoke_requests(config: &WorkflowSmokeConfigDto) -> Vec<WorkflowSmokeRequestDto> {
    vec![
        smoke_request(config, "GET", "/v1/auth/api-key-context", false),
        smoke_request(config, "GET", "/v1/workflows/node-definitions", false),
        smoke_request(config, "GET", "/v1/workflows", true),
        smoke_request(config, "POST", "/v1/workflows", true),
        smoke_request(config, "POST", "/v1/workflows/:id/deploy", true),
        smoke_request(config, "POST", "/v1/workflows/:id/execute", true),
        smoke_request(config, "GET", "/v1/workflows/:id/executions", true),
        smoke_request(
            config,
            "GET",
            "/v1/workflows/:id/executions/:executionId",
            true,
        ),
        smoke_request(config, "POST", "/v1/workflows/execute-in-memory", true),
    ]
}

fn smoke_request(
    config: &WorkflowSmokeConfigDto,
    method: &str,
    path: &str,
    include_workspace: bool,
) -> WorkflowSmokeRequestDto {
    let mut headers = vec![format!("X-API-Key: {}", config.api_key)];
    if include_workspace {
        headers.push(format!("X-Workspace-ID: {}", config.workspace_id));
    }
    WorkflowSmokeRequestDto {
        method: method.to_string(),
        url: request_url(&config.api_url, path),
        headers,
    }
}

fn passed_step(
    name: &str,
    method: &str,
    url: &str,
    item_count: Option<usize>,
    record_id: Option<String>,
    message: Option<String>,
) -> WorkflowSmokeStepDto {
    WorkflowSmokeStepDto {
        name: name.to_string(),
        method: method.to_string(),
        url: url.to_string(),
        state: "passed".to_string(),
        item_count,
        record_id,
        message,
    }
}

fn smoke_create_workflow_request(name: &str) -> Result<WorkflowRuntimeCreateWorkflowRequest> {
    if let Some(raw) = env_trimmed(AGENTENV_SMOKE_WORKFLOW_JSON_ENV) {
        return runtime_record_from_json(&raw, AGENTENV_SMOKE_WORKFLOW_JSON_ENV);
    }
    runtime_record_from_value(json!({
        "name": name,
        "definition": {
            "nodes": [
                {
                    "id": "smoke-webhook",
                    "type": "webhook",
                    "name": "Smoke webhook",
                    "config": {
                        "path": "puffer-runtime-smoke",
                        "methods": ["POST"],
                        "authentication": "none"
                    },
                    "trusted": false,
                    "position": {
                        "x": 0,
                        "y": 0
                    }
                }
            ],
            "edges": []
        }
    }))
}

fn smoke_execute_request() -> Result<WorkflowRuntimeExecuteRequest> {
    if let Some(raw) = env_trimmed(AGENTENV_SMOKE_EXECUTE_JSON_ENV) {
        return runtime_record_from_json(&raw, AGENTENV_SMOKE_EXECUTE_JSON_ENV);
    }
    runtime_record_from_value(json!({
        "input": {
            "source": "puffer-runtime-smoke",
            "timestamp_ms": unix_timestamp_ms()
        }
    }))
}

fn smoke_in_memory_request() -> Result<Option<WorkflowRuntimeInMemoryExecuteRequest>> {
    env_trimmed(AGENTENV_SMOKE_IN_MEMORY_JSON_ENV)
        .map(|raw| runtime_record_from_json(&raw, AGENTENV_SMOKE_IN_MEMORY_JSON_ENV))
        .transpose()
}

fn runtime_record_from_json<T>(raw: &str, env_name: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(raw).with_context(|| format!("parse {env_name} as JSON object"))
}

fn runtime_record_from_value<T>(value: Value) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(value).context("build smoke workflow runtime JSON object")
}

fn workflow_execute_request(input: Value) -> Result<WorkflowRuntimeRecord> {
    if !input.is_object() {
        anyhow::bail!("workflow execute input must be a JSON object");
    }
    let mut fields = BTreeMap::new();
    fields.insert("input".to_string(), input);
    Ok(WorkflowRuntimeRecord::new(fields))
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_workflow_table(workflows: &[WorkflowRuntimeWorkflow]) {
    println!("ID                   NAME                           STATUS");
    for workflow in workflows {
        println!(
            "{:<20} {:<30} {}",
            record_string(workflow, &["id", "workflow_id", "slug"]),
            record_string(workflow, &["name", "display_name", "title"]),
            record_string(workflow, &["status", "state"])
        );
    }
}

fn print_execution_table(executions: &[WorkflowRuntimeExecution]) {
    println!("ID                   STATUS          STARTED");
    for execution in executions {
        println!(
            "{:<20} {:<15} {}",
            record_string(execution, &["execution_id", "id", "run_id"]),
            record_string(execution, &["status", "state"]),
            record_string(execution, &["started_at", "started_at_ms", "created_at"])
        );
    }
}

fn print_smoke_report(report: &WorkflowSmokeReportDto) {
    println!("Workflow runtime smoke test passed");
    println!("API URL: {}", report.config.api_url);
    if let Some(ui_url) = &report.config.ui_url {
        println!("UI URL: {ui_url}");
    }
    println!("Workspace: {}", report.config.workspace_id);
    println!("Workflow: {}", report.workflow_id);
    println!("Execution: {}", report.execution_id);
    for step in &report.steps {
        let count = step
            .item_count
            .map(|value| format!(" ({value} items)"))
            .unwrap_or_default();
        let id = step
            .record_id
            .as_ref()
            .map(|value| format!(" id={value}"))
            .unwrap_or_default();
        let message = step
            .message
            .as_ref()
            .map(|value| format!(" - {value}"))
            .unwrap_or_default();
        println!(
            "{:<22} {:<4} {}{}{}{}",
            step.name, step.method, step.url, count, id, message
        );
    }
}

fn record_id(record: &WorkflowRuntimeRecord) -> Option<String> {
    record_string_for_keys(record, &["id", "workflow_id", "workflowId", "slug"])
}

fn execution_id_from_record(record: &WorkflowRuntimeRecord) -> Option<String> {
    record_string_for_keys(
        record,
        &["execution_id", "executionId", "id", "run_id", "runId"],
    )
}

fn find_workflow_by_id(
    workflows: &[WorkflowRuntimeWorkflow],
    workflow_id: &str,
) -> Option<WorkflowRuntimeWorkflow> {
    workflows
        .iter()
        .find(|workflow| {
            record_string_for_keys(workflow, &["id", "workflow_id", "workflowId", "slug"])
                .as_deref()
                == Some(workflow_id)
        })
        .cloned()
}

fn find_workflow_by_name(
    workflows: &[WorkflowRuntimeWorkflow],
    workflow_name: &str,
) -> Option<WorkflowRuntimeWorkflow> {
    workflows
        .iter()
        .find(|workflow| {
            record_string_for_keys(
                workflow,
                &[
                    "id",
                    "workflow_id",
                    "workflowId",
                    "slug",
                    "name",
                    "display_name",
                    "title",
                ],
            )
            .as_deref()
                == Some(workflow_name)
        })
        .cloned()
}

fn workflow_record_is_deployed(record: &WorkflowRuntimeRecord) -> bool {
    record_string_for_keys(
        record,
        &[
            "deployment_status",
            "deploymentStatus",
            "status",
            "state",
            "lifecycle_state",
            "lifecycleState",
        ],
    )
    .is_some_and(|value| matches!(value.as_str(), "deployed" | "active" | "enabled" | "ready"))
}

fn request_url(api_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        api_root_for_display(api_url),
        path.trim_start_matches('/')
    )
}

fn api_root_for_display(api_url: &str) -> String {
    let trimmed = api_url.trim().trim_end_matches('/');
    trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_string()
}

fn env_trimmed(name: &str) -> Option<String> {
    env::var(name).ok().and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn redact_secret(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= 8 {
        return "<redacted>".to_string();
    }
    let prefix = trimmed.chars().take(4).collect::<String>();
    let suffix = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or_default()
}
