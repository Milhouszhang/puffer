//! Internal Automation compile/deploy/run helpers.

use anyhow::{bail, Context, Result};
use puffer_automation::{
    compile_automation, AutomationLoopInput, AutomationLoopSpec, AutomationLoopStopSpec,
    AutomationRecord, AutomationRunLocation, AutomationRuntimeState, AutomationRuntimeStatus,
    AutomationStatus, AutomationStepSpec, AutomationStore, CompiledAgentEnvWorkflow,
    CompiledPufferBinding, CompiledWorkflowDefinition, CompiledWorkflowRole,
};
use puffer_config::{load_config, ConfigPaths, WorkflowBackendMode};
use puffer_subscriptions::{
    ActionSpec, WorkflowActionOutput, WorkflowBindingSpec, WorkflowBindingStatus,
};
use puffer_workflow::{
    AgentEnvWorkflowDefinition, WorkflowRuntimeClient, WorkflowRuntimeCreateWorkflowRequest,
    WorkflowRuntimeError, WorkflowRuntimeErrorKind, WorkflowRuntimeInMemoryExecuteRequest,
    WorkflowRuntimeRecord, WorkflowRuntimeUpdateWorkflowRequest, WorkflowRuntimeWorkflow,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::daemon::DaemonState;
use crate::workflow_runtime_helpers::workflow_execute_summary;

const WORKFLOW_ID_KEYS: &[&str] = &[
    "id",
    "workflowId",
    "workflow_id",
    "workflowSlug",
    "workflow_slug",
];
const AUTOMATION_ID_KEYS: &[&str] = &["id", "automation_id", "automationId"];

#[derive(Debug)]
struct AutomationRunOutput {
    compiled: bool,
    record: AutomationRecord,
    result: Value,
    summary: String,
}

#[derive(Debug)]
struct AutomationExecutionOutput {
    result: Value,
    summary: String,
}

pub(crate) fn handle_automation_compile_deploy(
    state: &DaemonState,
    params: &Value,
) -> Result<Value> {
    let automation_id = required_automation_id(params)?;
    let expected_revision = optional_expected_revision(params)?;
    user_facing_automation_result(compile_and_deploy_automation(
        state,
        &automation_id,
        expected_revision,
    ))
}

pub(crate) fn handle_automation_sync_preview(state: &DaemonState, params: &Value) -> Result<Value> {
    let automation_id = required_automation_id(params)?;
    let expected_revision = optional_expected_revision(params)?;
    user_facing_automation_result(sync_preview_automation(
        state,
        &automation_id,
        expected_revision,
    ))
}

pub(crate) fn handle_automation_run_preview(state: &DaemonState, params: &Value) -> Result<Value> {
    let automation_id = required_automation_id(params)?;
    let input = params.get("input").cloned().unwrap_or_else(|| json!({}));
    run_automation(state, &automation_id, input)
}

pub(crate) fn handle_automation_run_history(state: &DaemonState, params: &Value) -> Result<Value> {
    let automation_id = required_automation_id(params)?;
    let runs = load_run_history(&automation_run_history_path(state.config_paths()))?
        .runs
        .into_iter()
        .filter(|run| run.automation_id == automation_id)
        .collect::<Vec<_>>();
    Ok(json!({ "automation_id": automation_id, "runs": runs }))
}

fn compile_and_deploy_automation(
    state: &DaemonState,
    automation_id: &str,
    expected_revision: Option<u64>,
) -> Result<Value> {
    let record = compile_and_deploy_with_store(
        state.config_paths(),
        state.automation_store(),
        automation_id,
        expected_revision,
    )?;
    Ok(json!({
        "id": record.id,
        "revision": record.revision,
        "runtime": runtime_summary(&record.runtime),
    }))
}

fn sync_preview_automation(
    state: &DaemonState,
    automation_id: &str,
    expected_revision: Option<u64>,
) -> Result<Value> {
    let record = sync_preview_with_store(
        state.config_paths(),
        state.automation_store(),
        automation_id,
        expected_revision,
    )?;
    Ok(json!({
        "id": record.id,
        "revision": record.revision,
        "runtime": runtime_summary(&record.runtime),
    }))
}

fn run_automation(state: &DaemonState, automation_id: &str, input: Value) -> Result<Value> {
    let started_at_ms = puffer_subscriptions::now_ms();
    let result = run_automation_preview_with_store(
        state.config_paths(),
        state.automation_store(),
        automation_id,
        input,
    );
    let ended_at_ms = puffer_subscriptions::now_ms();
    match result {
        Ok(output) => {
            let response = automation_preview_response(&output);
            append_run_history(
                &automation_run_history_path(state.config_paths()),
                AutomationRunHistoryRecord {
                    id: format!("preview-{automation_id}-{started_at_ms}"),
                    automation_id: automation_id.to_string(),
                    title: "Test run".to_string(),
                    status: "completed".to_string(),
                    started_at_ms,
                    duration_ms: (ended_at_ms - started_at_ms).max(0),
                    summary: output.summary.clone(),
                    source_event: Some("manual_preview".to_string()),
                    compiled: output.compiled,
                    runtime_status: output.record.runtime.status,
                    result: Some(output.result.clone()),
                    error: None,
                    approval: Some(AutomationRunApprovalRecord {
                        required: output.record.spec.review.human_approval_required,
                        status: "not_required_for_preview".to_string(),
                    }),
                },
            )?;
            Ok(response)
        }
        Err(error) => {
            let detail = format!("{error:#}");
            tracing::warn!(error = %detail, automation_id, "automation preview failed");
            let message = public_automation_error_message(&error);
            let runtime_status = state
                .automation_store()
                .get(automation_id)
                .ok()
                .map(|record| record.runtime.status)
                .unwrap_or_default();
            append_run_history(
                &automation_run_history_path(state.config_paths()),
                AutomationRunHistoryRecord {
                    id: format!("preview-{automation_id}-{started_at_ms}"),
                    automation_id: automation_id.to_string(),
                    title: "Test run".to_string(),
                    status: "error".to_string(),
                    started_at_ms,
                    duration_ms: (ended_at_ms - started_at_ms).max(0),
                    summary: message.clone(),
                    source_event: Some("manual_preview".to_string()),
                    compiled: false,
                    runtime_status,
                    result: None,
                    error: Some(message.clone()),
                    approval: Some(AutomationRunApprovalRecord {
                        required: true,
                        status: "not_created".to_string(),
                    }),
                },
            )?;
            Err(anyhow::anyhow!(message))
        }
    }
}

fn user_facing_automation_result<T>(result: Result<T>) -> Result<T> {
    result.map_err(|error| {
        let detail = format!("{error:#}");
        tracing::warn!(error = %detail, "automation runtime operation failed");
        anyhow::anyhow!(public_automation_error_message(&error))
    })
}

fn public_automation_error_message(error: &anyhow::Error) -> String {
    let detail = format!("{error:#}");
    let lower = detail.to_ascii_lowercase();
    if lower.contains("docker_missing") || lower.contains("docker") && lower.contains("not found") {
        return "Docker is required to run local automations. Install or start Docker, then try again."
            .to_string();
    }
    if lower.contains("image_missing") {
        return "The local automation runtime image is not installed. Install the AgentEnv local runtime image, then try again."
            .to_string();
    }
    if lower.contains("incompatible_runtime")
        || lower.contains("node definitions")
        || lower.contains("rejected local credentials")
        || lower.contains("migrate failed")
        || lower.contains("seed failed")
    {
        return "The local automation runtime is not compatible with this Puffer build. Update or rebuild the local runtime image, then try again."
            .to_string();
    }
    if lower.contains("token is not configured") || lower.contains("credentials") {
        return "Automation runtime credentials are not ready. Local runs are configured by Puffer when the local runtime starts; cloud runs need a connected AgentEnv Cloud account."
            .to_string();
    }
    if lower.contains("error sending request")
        || lower.contains("connection refused")
        || lower.contains("timed out")
        || lower.contains("http://")
        || lower.contains("https://")
    {
        return "Automation runtime is unreachable. Start the selected runtime, then try again."
            .to_string();
    }
    if lower.contains("workflow artifact")
        || lower.contains("workflow runtime")
        || lower.contains("/v1/")
    {
        return "Automation runtime could not prepare this automation. Check the selected run location and try again."
            .to_string();
    }
    detail
}

fn automation_preview_response(output: &AutomationRunOutput) -> Value {
    json!({
        "id": output.record.id,
        "status": "completed",
        "summary": output.summary,
        "result": output.result,
        "compiled": output.compiled,
        "runtime": runtime_summary(&output.record.runtime),
    })
}

fn automation_run_history_path(paths: &ConfigPaths) -> PathBuf {
    paths.user_config_dir.join("automation_runs.json")
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AutomationRunHistoryFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    runs: Vec<AutomationRunHistoryRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AutomationRunHistoryRecord {
    id: String,
    automation_id: String,
    title: String,
    status: String,
    started_at_ms: i128,
    duration_ms: i128,
    summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_event: Option<String>,
    compiled: bool,
    runtime_status: AutomationRuntimeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    approval: Option<AutomationRunApprovalRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AutomationRunApprovalRecord {
    required: bool,
    status: String,
}

fn load_run_history(path: &Path) -> Result<AutomationRunHistoryFile> {
    if !path.exists() {
        return Ok(AutomationRunHistoryFile::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read automation run history `{}`", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(AutomationRunHistoryFile::default());
    }
    serde_json::from_str(&raw)
        .with_context(|| format!("parse automation run history `{}`", path.display()))
}

fn append_run_history(path: &Path, record: AutomationRunHistoryRecord) -> Result<()> {
    let mut history = load_run_history(path)?;
    history.version = 1;
    history.runs.insert(0, record);
    if history.runs.len() > 500 {
        history.runs.truncate(500);
    }
    write_run_history(path, &history)
}

fn write_run_history(path: &Path, history: &AutomationRunHistoryFile) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("create automation run history dir `{}`", dir.display()))?;
    }
    let tmp = path.with_extension("tmp");
    let body = serde_json::to_vec_pretty(history).context("serialize automation run history")?;
    std::fs::write(&tmp, body)
        .with_context(|| format!("write automation run history temp `{}`", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("replace automation run history `{}`", path.display()))?;
    Ok(())
}

pub(crate) fn run_automation_with_store(
    paths: &ConfigPaths,
    store: &AutomationStore,
    automation_id: &str,
    input: Value,
) -> Result<WorkflowActionOutput> {
    let mut record = store.get(automation_id)?;
    ensure_live_automation_can_run(&record)?;
    if runtime_needs_deploy(&record)? {
        record = compile_and_deploy_with_store(paths, store, automation_id, Some(record.revision))?;
    }
    ensure_live_automation_can_run(&record)?;
    let execution = execute_automation(paths, &record, input)?;
    let output = AutomationRunOutput {
        compiled: !record.runtime.agentenv_workflows.is_empty(),
        record,
        result: execution.result,
        summary: execution.summary,
    };
    Ok(WorkflowActionOutput::new(output.summary))
}

fn run_automation_preview_with_store(
    paths: &ConfigPaths,
    store: &AutomationStore,
    automation_id: &str,
    input: Value,
) -> Result<AutomationRunOutput> {
    let record = store.get(automation_id)?;
    ensure_preview_automation_can_run(&record)?;
    let execution = execute_automation_preview(paths, &record, input)?;
    Ok(AutomationRunOutput {
        compiled: !record.runtime.agentenv_workflows.is_empty(),
        record,
        result: execution.result,
        summary: execution.summary,
    })
}

fn execute_automation_preview(
    paths: &ConfigPaths,
    record: &AutomationRecord,
    input: Value,
) -> Result<AutomationExecutionOutput> {
    let config = load_config(paths).context("load workflow backend config")?;
    let client = crate::daemon_workflow_runtime::workflow_runtime_client_for_mode(
        paths,
        &config,
        workflow_backend_mode_for_run_location(record.spec.run_location),
    )
    .context("create workflow runtime client")?;
    let plan = compile_automation(record).context("compile automation preview")?;
    let root = compiled_workflow_for_role(&plan.workflows, &CompiledWorkflowRole::Root)
        .context("compiled Automation has no root workflow definition")?;
    let root_definition = compiled_agentenv_definition(root)?;
    let root_trigger_id = root_trigger_node_id(record);
    let root_output = match execute_in_memory_value(
        &client,
        root_definition,
        json!({ "trigger": input }),
        root_trigger_id.as_deref(),
    ) {
        Ok(value) => value,
        Err(error) if preview_can_fallback_to_deployed_execution(&error, record) => {
            return execute_automation(paths, record, input)
                .context("fall back to deployed workflow preview execution");
        }
        Err(error) => return Err(error),
    };

    let Some((loop_step_id, loop_spec)) = first_loop_step(record) else {
        let summary = format!(
            "automation `{}` completed: {}",
            record.id,
            summarize_value(&root_output)
        );
        return Ok(AutomationExecutionOutput {
            result: root_output,
            summary,
        });
    };

    let loop_role = CompiledWorkflowRole::LoopBody {
        step_id: loop_step_id.clone(),
    };
    let loop_workflow =
        compiled_workflow_for_role(&plan.workflows, &loop_role).with_context(|| {
            format!("compiled Automation has no loop body workflow for `{loop_step_id}`")
        })?;
    let loop_definition = compiled_agentenv_definition(loop_workflow)?;
    let loop_output =
        execute_loop_in_memory(&client, loop_definition, loop_spec, &input, &root_output)
            .with_context(|| format!("run loop `{loop_step_id}`"))?;

    let summary = format!(
        "automation `{}` completed loop `{}`: {}",
        record.id,
        loop_step_id,
        summarize_value(&loop_output)
    );
    Ok(AutomationExecutionOutput {
        result: loop_output,
        summary,
    })
}

fn preview_can_fallback_to_deployed_execution(
    error: &anyhow::Error,
    record: &AutomationRecord,
) -> bool {
    if record.runtime.status != AutomationRuntimeStatus::Deployed {
        return false;
    }
    error.chain().any(|cause| {
        cause
            .downcast_ref::<WorkflowRuntimeError>()
            .is_some_and(|runtime_error| {
                matches!(
                    runtime_error.kind,
                    WorkflowRuntimeErrorKind::RuntimeUnreachable
                        | WorkflowRuntimeErrorKind::WorkspaceInaccessible
                        | WorkflowRuntimeErrorKind::IncompatibleRuntime
                )
            })
    })
}

fn compile_and_deploy_with_store(
    paths: &ConfigPaths,
    store: &AutomationStore,
    automation_id: &str,
    expected_revision: Option<u64>,
) -> Result<AutomationRecord> {
    compile_with_store(paths, store, automation_id, expected_revision, true)
}

fn sync_preview_with_store(
    paths: &ConfigPaths,
    store: &AutomationStore,
    automation_id: &str,
    expected_revision: Option<u64>,
) -> Result<AutomationRecord> {
    let record = store.get(automation_id)?;
    if let Some(expected_revision) = expected_revision {
        if record.revision != expected_revision {
            bail!(
                "automation `{automation_id}` revision conflict: expected {expected_revision}, found {}",
                record.revision
            );
        }
    }
    if record.runtime.status == AutomationRuntimeStatus::Deployed && !runtime_needs_deploy(&record)?
    {
        return Ok(record);
    }
    compile_with_store(paths, store, automation_id, expected_revision, false)
}

fn compile_with_store(
    paths: &ConfigPaths,
    store: &AutomationStore,
    automation_id: &str,
    expected_revision: Option<u64>,
    deploy_live: bool,
) -> Result<AutomationRecord> {
    let record = store.get(automation_id)?;
    if let Some(expected_revision) = expected_revision {
        if record.revision != expected_revision {
            bail!(
                "automation `{automation_id}` revision conflict: expected {expected_revision}, found {}",
                record.revision
            );
        }
    }
    match compile_inner(paths, store, &record, deploy_live) {
        Ok(record) => Ok(record),
        Err(error) => {
            let message = format!("{error:#}");
            if let Err(store_error) =
                store.replace_runtime_error(&record.id, record.revision, message)
            {
                return Err(error).with_context(|| {
                    format!("failed to record Automation runtime error: {store_error:#}")
                });
            }
            Err(error)
        }
    }
}

fn compile_inner(
    paths: &ConfigPaths,
    store: &AutomationStore,
    record: &AutomationRecord,
    deploy_live: bool,
) -> Result<AutomationRecord> {
    let plan = compile_automation(&record).context("compile automation")?;
    let config = load_config(paths).context("load workflow backend config")?;
    let client = crate::daemon_workflow_runtime::workflow_runtime_client_for_mode(
        paths,
        &config,
        workflow_backend_mode_for_run_location(record.spec.run_location),
    )
    .context("create workflow runtime client")?;

    let mut compiled_workflows = Vec::new();
    for workflow in &plan.workflows {
        let workflow_id = deploy_workflow_definition(
            &client,
            &record,
            workflow,
            existing_workflow_id(&record, &workflow.role),
            deploy_live,
        )?;
        compiled_workflows.push(CompiledAgentEnvWorkflow {
            role: workflow.role.clone(),
            workflow_id: Some(workflow_id),
            definition_hash: Some(workflow.definition_hash.clone()),
            deployed: deploy_live,
        });
    }

    if deploy_live {
        deploy_puffer_bindings(&record)?;
    }

    let runtime = AutomationRuntimeState {
        spec_hash: Some(plan.spec_hash),
        compiled_revision: Some(record.revision),
        status: if deploy_live {
            AutomationRuntimeStatus::Deployed
        } else {
            AutomationRuntimeStatus::DraftSynced
        },
        agentenv_workflows: compiled_workflows,
        puffer_bindings: if deploy_live {
            plan.puffer_bindings
                .into_iter()
                .map(|binding| CompiledPufferBinding {
                    trigger_id: binding.trigger_id,
                    binding_slug: binding.binding_slug,
                })
                .collect()
        } else {
            Vec::new()
        },
        last_error: None,
    };
    store
        .replace_runtime(&record.id, record.revision, runtime)
        .context("save automation runtime state")
}

fn deploy_workflow_definition(
    client: &WorkflowRuntimeClient,
    record: &AutomationRecord,
    workflow: &CompiledWorkflowDefinition,
    existing_id: Option<String>,
    deploy: bool,
) -> Result<String> {
    let definition: AgentEnvWorkflowDefinition =
        serde_json::from_value(workflow.definition.clone())
            .context("compiled workflow definition must match AgentEnv schema")?;
    let name = workflow_display_name(record, &workflow.role);
    let description = Some(format!(
        "Internal workflow artifact for Puffer Automation `{}` revision {}.",
        record.id, record.revision
    ));

    let artifact = if let Some(workflow_id) = existing_id {
        let request = WorkflowRuntimeUpdateWorkflowRequest {
            name: Some(name),
            description,
            definition: Some(definition),
            status: None,
        };
        client
            .update_workflow(&workflow_id, &request)
            .with_context(|| format!("update workflow `{workflow_id}`"))?
    } else {
        let request = WorkflowRuntimeCreateWorkflowRequest {
            name,
            description,
            definition,
        };
        client
            .create_workflow(&request)
            .context("create workflow artifact")?
    };

    let workflow_id = runtime_workflow_id(&artifact)?;
    if deploy {
        client
            .deploy_workflow(&workflow_id)
            .with_context(|| format!("deploy workflow `{workflow_id}`"))?;
    }
    Ok(workflow_id)
}

pub(crate) fn sync_automation_bindings_after_save(
    previous: Option<&AutomationRecord>,
    record: &AutomationRecord,
) -> Result<()> {
    let Ok(manager) = puffer_core::subscription_manager() else {
        return Ok(());
    };

    if let Some(previous) = previous {
        if previous.revision != record.revision
            && matches!(record.runtime.status, AutomationRuntimeStatus::Stale)
        {
            for slug in generated_binding_slugs(previous) {
                let _ = manager.store().delete(&slug);
            }
            manager.refresh_connection_consumers()?;
            return Ok(());
        }
        let current_slugs = generated_binding_slugs(record);
        for slug in generated_binding_slugs(previous) {
            if !current_slugs.iter().any(|current| current == &slug) {
                let _ = manager.store().delete(&slug);
            }
        }
    }

    for slug in generated_binding_slugs(record) {
        if manager.store().get(&slug).is_none() {
            continue;
        }
        let status = match record.status {
            AutomationStatus::Enabled => WorkflowBindingStatus::Enabled,
            AutomationStatus::Paused | AutomationStatus::Archived => WorkflowBindingStatus::Paused,
        };
        manager.store().set_status(&slug, status)?;
    }
    manager.refresh_connection_consumers()?;
    Ok(())
}

pub(crate) fn remove_automation_bindings(record: &AutomationRecord) -> Result<()> {
    let Ok(manager) = puffer_core::subscription_manager() else {
        return Ok(());
    };
    for slug in generated_binding_slugs(record) {
        let _ = manager.store().delete(&slug);
    }
    manager.refresh_connection_consumers()?;
    Ok(())
}

fn generated_binding_slugs(record: &AutomationRecord) -> Vec<String> {
    let mut slugs = Vec::new();
    for binding in &record.runtime.puffer_bindings {
        slugs.push(binding.binding_slug.clone());
    }
    for trigger in &record.spec.triggers {
        if let puffer_automation::AutomationTriggerSpec::PufferConnection { id, .. } = trigger {
            slugs.push(format!("automation-{}-{id}", record.id));
        }
    }
    slugs.sort();
    slugs.dedup();
    slugs
}

fn deploy_puffer_bindings(record: &AutomationRecord) -> Result<()> {
    if !record.spec.triggers.iter().any(|trigger| {
        matches!(
            trigger,
            puffer_automation::AutomationTriggerSpec::PufferConnection { .. }
        )
    }) {
        return Ok(());
    }
    let manager = puffer_core::subscription_manager()
        .context("subscription manager is required to deploy Automation bindings")?;
    for trigger in &record.spec.triggers {
        let puffer_automation::AutomationTriggerSpec::PufferConnection {
            id,
            connection_slug,
            connector_slug,
            filter,
            ignore_filters,
            contact_ids,
            ..
        } = trigger
        else {
            continue;
        };
        let binding = WorkflowBindingSpec {
            slug: format!("automation-{}-{id}", record.id),
            description: format!("Run Automation {} from trigger {id}", record.id),
            connection_slug: connection_slug.clone(),
            connector_slug: connector_slug.clone(),
            status: match record.status {
                AutomationStatus::Enabled => WorkflowBindingStatus::Enabled,
                AutomationStatus::Paused | AutomationStatus::Archived => {
                    WorkflowBindingStatus::Paused
                }
            },
            filter: filter.clone(),
            ignore_filters: ignore_filters.clone(),
            contact_ids: contact_ids.clone(),
            classify_prompt: None,
            classify_model: None,
            action: ActionSpec::RunAutomation {
                automation_id: record.id.clone(),
            },
            created_at_ms: puffer_subscriptions::now_ms(),
        };
        manager.store().upsert(binding)?;
    }
    manager.refresh_connection_consumers()?;
    Ok(())
}

fn execute_automation(
    paths: &ConfigPaths,
    record: &AutomationRecord,
    input: Value,
) -> Result<AutomationExecutionOutput> {
    let config = load_config(paths).context("load workflow backend config")?;
    let client = crate::daemon_workflow_runtime::workflow_runtime_client_for_mode(
        paths,
        &config,
        workflow_backend_mode_for_run_location(record.spec.run_location),
    )
    .context("create workflow runtime client")?;

    let root_id = workflow_id_for_role(record, &CompiledWorkflowRole::Root)
        .context("compiled Automation has no root workflow id")?;
    let root_trigger_id = root_trigger_node_id(record);
    let root_output = execute_workflow_value(
        &client,
        &root_id,
        json!({ "trigger": input }),
        root_trigger_id.as_deref(),
    )?;

    let Some((loop_step_id, loop_spec)) = first_loop_step(record) else {
        let summary = format!(
            "automation `{}` completed: {}",
            record.id,
            summarize_value(&root_output)
        );
        return Ok(AutomationExecutionOutput {
            result: root_output,
            summary,
        });
    };

    let loop_role = CompiledWorkflowRole::LoopBody {
        step_id: loop_step_id.clone(),
    };
    let loop_workflow_id = workflow_id_for_role(record, &loop_role).with_context(|| {
        format!("compiled Automation has no loop body workflow for `{loop_step_id}`")
    })?;
    let loop_output = execute_loop(&client, &loop_workflow_id, loop_spec, &input, &root_output)
        .with_context(|| format!("run loop `{loop_step_id}`"))?;

    let summary = format!(
        "automation `{}` completed loop `{}`: {}",
        record.id,
        loop_step_id,
        summarize_value(&loop_output)
    );
    Ok(AutomationExecutionOutput {
        result: loop_output,
        summary,
    })
}

fn workflow_backend_mode_for_run_location(
    run_location: AutomationRunLocation,
) -> WorkflowBackendMode {
    match run_location {
        AutomationRunLocation::Local => WorkflowBackendMode::Local,
        AutomationRunLocation::AgentEnvCloud => WorkflowBackendMode::AgentEnvCloud,
    }
}

fn execute_loop(
    client: &WorkflowRuntimeClient,
    workflow_id: &str,
    loop_spec: &AutomationLoopSpec,
    trigger: &Value,
    root_output: &Value,
) -> Result<Value> {
    match loop_spec {
        AutomationLoopSpec::ForEach {
            input,
            item_alias,
            max_iterations,
        } => {
            let collection = resolve_loop_input(input, trigger, root_output, &Value::Null)?;
            let items = collection
                .as_array()
                .context("foreach loop input must resolve to a JSON array")?;
            let limit = max_iterations
                .map(|value| value as usize)
                .unwrap_or(items.len())
                .min(items.len());
            let mut previous_output = Value::Null;
            for (index, item) in items.iter().take(limit).enumerate() {
                previous_output = execute_workflow_value(
                    client,
                    workflow_id,
                    json!({
                        "trigger": trigger,
                        "root_output": root_output,
                        "previous_output": previous_output,
                        "item": item,
                        item_alias: item,
                        "iteration": index,
                    }),
                    None,
                )?;
            }
            Ok(previous_output)
        }
        AutomationLoopSpec::Repeat {
            input,
            stop_when,
            max_iterations,
        } => {
            let mut previous_output = Value::Null;
            for index in 0..*max_iterations {
                let loop_input = resolve_loop_input(input, trigger, root_output, &previous_output)?;
                previous_output = execute_workflow_value(
                    client,
                    workflow_id,
                    json!({
                        "trigger": trigger,
                        "root_output": root_output,
                        "previous_output": previous_output,
                        "loop_input": loop_input,
                        "iteration": index,
                    }),
                    None,
                )?;
                if stop_condition_met(stop_when, &previous_output)? {
                    break;
                }
            }
            Ok(previous_output)
        }
    }
}

fn execute_loop_in_memory(
    client: &WorkflowRuntimeClient,
    definition: AgentEnvWorkflowDefinition,
    loop_spec: &AutomationLoopSpec,
    trigger: &Value,
    root_output: &Value,
) -> Result<Value> {
    match loop_spec {
        AutomationLoopSpec::ForEach {
            input,
            item_alias,
            max_iterations,
        } => {
            let collection = resolve_loop_input(input, trigger, root_output, &Value::Null)?;
            let items = collection
                .as_array()
                .context("foreach loop input must resolve to a JSON array")?;
            let limit = max_iterations
                .map(|value| value as usize)
                .unwrap_or(items.len())
                .min(items.len());
            let mut previous_output = Value::Null;
            for (index, item) in items.iter().take(limit).enumerate() {
                previous_output = execute_in_memory_value(
                    client,
                    definition.clone(),
                    json!({
                        "trigger": trigger,
                        "root_output": root_output,
                        "previous_output": previous_output,
                        "item": item,
                        item_alias: item,
                        "iteration": index,
                    }),
                    None,
                )?;
            }
            Ok(previous_output)
        }
        AutomationLoopSpec::Repeat {
            input,
            stop_when,
            max_iterations,
        } => {
            let mut previous_output = Value::Null;
            for index in 0..*max_iterations {
                let loop_input = resolve_loop_input(input, trigger, root_output, &previous_output)?;
                previous_output = execute_in_memory_value(
                    client,
                    definition.clone(),
                    json!({
                        "trigger": trigger,
                        "root_output": root_output,
                        "previous_output": previous_output,
                        "loop_input": loop_input,
                        "iteration": index,
                    }),
                    None,
                )?;
                if stop_condition_met(stop_when, &previous_output)? {
                    break;
                }
            }
            Ok(previous_output)
        }
    }
}

fn execute_workflow_value(
    client: &WorkflowRuntimeClient,
    workflow_id: &str,
    input: Value,
    trigger_node_id: Option<&str>,
) -> Result<Value> {
    let mut fields = BTreeMap::new();
    fields.insert("input".to_string(), input);
    if let Some(trigger_node_id) = trigger_node_id {
        fields.insert("triggerNodeId".to_string(), json!(trigger_node_id));
    }
    let request = WorkflowRuntimeRecord::new(fields);
    let response = client
        .execute_workflow(workflow_id, &request)
        .with_context(|| format!("execute workflow `{workflow_id}`"))?;
    Ok(serde_json::to_value(response)?)
}

fn execute_in_memory_value(
    client: &WorkflowRuntimeClient,
    definition: AgentEnvWorkflowDefinition,
    input: Value,
    trigger_node_id: Option<&str>,
) -> Result<Value> {
    let request = WorkflowRuntimeInMemoryExecuteRequest {
        definition,
        input: Some(input_fields(input)?),
        trigger_node_id: trigger_node_id.map(ToString::to_string),
    };
    let response = client
        .execute_in_memory(&request)
        .context("execute in-memory workflow")?;
    Ok(serde_json::to_value(response)?)
}

fn input_fields(input: Value) -> Result<BTreeMap<String, Value>> {
    match input {
        Value::Object(map) => Ok(map.into_iter().collect()),
        other => {
            let mut fields = BTreeMap::new();
            fields.insert("value".to_string(), other);
            Ok(fields)
        }
    }
}

fn compiled_workflow_for_role<'a>(
    workflows: &'a [CompiledWorkflowDefinition],
    role: &CompiledWorkflowRole,
) -> Option<&'a CompiledWorkflowDefinition> {
    workflows.iter().find(|workflow| &workflow.role == role)
}

fn compiled_agentenv_definition(
    workflow: &CompiledWorkflowDefinition,
) -> Result<AgentEnvWorkflowDefinition> {
    serde_json::from_value(workflow.definition.clone())
        .context("compiled workflow definition must match AgentEnv schema")
}

fn root_trigger_node_id(record: &AutomationRecord) -> Option<String> {
    record
        .spec
        .triggers
        .iter()
        .find_map(|trigger| match trigger {
            puffer_automation::AutomationTriggerSpec::AgentEnvNode { id, .. }
            | puffer_automation::AutomationTriggerSpec::Manual { id, .. }
            | puffer_automation::AutomationTriggerSpec::PufferConnection { id, .. } => {
                Some(id.clone())
            }
        })
}

fn runtime_needs_preview_sync(record: &AutomationRecord) -> Result<bool> {
    let current_hash = puffer_automation::automation_spec_hash(&record.spec)
        .map_err(|error| anyhow::anyhow!("hash automation spec: {error}"))?;
    Ok(!matches!(
        record.runtime.status,
        AutomationRuntimeStatus::DraftSynced | AutomationRuntimeStatus::Deployed
    ) || record.runtime.compiled_revision != Some(record.revision)
        || record.runtime.spec_hash.as_deref() != Some(current_hash.as_str())
        || record.runtime.agentenv_workflows.is_empty())
}

fn ensure_preview_automation_can_run(record: &AutomationRecord) -> Result<()> {
    if runtime_needs_preview_sync(record)? {
        bail!(
            "automation `{}` runtime is not deployed for revision {}; deploy before running a test preview",
            record.id,
            record.revision
        );
    }
    if automation_has_approval_gated_side_effect(record) {
        bail!(
            "automation `{}` contains connector actions that require human approval; test preview does not execute outward actions",
            record.id
        );
    }
    Ok(())
}

fn runtime_needs_deploy(record: &AutomationRecord) -> Result<bool> {
    let current_hash = puffer_automation::automation_spec_hash(&record.spec)
        .map_err(|error| anyhow::anyhow!("hash automation spec: {error}"))?;
    Ok(record.runtime.status != AutomationRuntimeStatus::Deployed
        || record.runtime.compiled_revision != Some(record.revision)
        || record.runtime.spec_hash.as_deref() != Some(current_hash.as_str())
        || record.runtime.agentenv_workflows.is_empty())
}

fn ensure_live_automation_can_run(record: &AutomationRecord) -> Result<()> {
    if record.status != AutomationStatus::Enabled {
        bail!(
            "automation `{}` is {:?}; only enabled automations can run from connector events",
            record.id,
            record.status
        );
    }
    if automation_has_approval_gated_side_effect(record) {
        bail!(
            "automation `{}` contains connector actions that require human approval; live connector-triggered execution is not enabled for outward actions",
            record.id
        );
    }
    Ok(())
}

fn automation_has_approval_gated_side_effect(record: &AutomationRecord) -> bool {
    flow_has_approval_gated_side_effect(&record.spec.flow)
}

fn flow_has_approval_gated_side_effect(flow: &puffer_automation::AutomationFlowSpec) -> bool {
    flow.steps.iter().any(|step| match step {
        AutomationStepSpec::AgentEnvNode { node, .. } => {
            node.node_type == "puffer_connector_action"
                && (node.config_bool("draft_only")
                    || node.config_bool("human_approval_required")
                    || node.config_bool("external_side_effect"))
        }
        AutomationStepSpec::Loop { body, .. } => flow_has_approval_gated_side_effect(body),
    })
}

trait AgentEnvNodeRefConfigExt {
    fn config_bool(&self, key: &str) -> bool;
}

impl AgentEnvNodeRefConfigExt for puffer_automation::AgentEnvNodeRef {
    fn config_bool(&self, key: &str) -> bool {
        self.config
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
}

fn first_loop_step(record: &AutomationRecord) -> Option<(String, &AutomationLoopSpec)> {
    record.spec.flow.steps.iter().find_map(|step| match step {
        AutomationStepSpec::Loop { id, loop_spec, .. } => Some((id.clone(), loop_spec)),
        AutomationStepSpec::AgentEnvNode { .. } => None,
    })
}

fn resolve_loop_input(
    input: &AutomationLoopInput,
    trigger: &Value,
    root_output: &Value,
    previous_output: &Value,
) -> Result<Value> {
    match input {
        AutomationLoopInput::Trigger => Ok(trigger.clone()),
        AutomationLoopInput::Static { value } => Ok(value.clone()),
        AutomationLoopInput::StepOutput { path, .. } => {
            let value = if previous_output.is_null() {
                root_output
            } else {
                previous_output
            };
            Ok(path
                .as_deref()
                .and_then(|path| json_path_value(value, path))
                .cloned()
                .unwrap_or_else(|| value.clone()))
        }
    }
}

fn stop_condition_met(stop_when: &AutomationLoopStopSpec, output: &Value) -> Result<bool> {
    match stop_when {
        AutomationLoopStopSpec::OutputEquals { path, value } => {
            Ok(json_path_value(output, path).is_some_and(|actual| actual == value))
        }
        AutomationLoopStopSpec::OutputMatches { path, pattern } => {
            let regex = Regex::new(pattern).context("compile loop stop regex")?;
            Ok(json_path_value(output, path)
                .and_then(Value::as_str)
                .is_some_and(|text| regex.is_match(text)))
        }
        AutomationLoopStopSpec::AgentEnvNode { .. } => {
            bail!("loop stop_when AgentEnvNode is not implemented in the Puffer Automation runner")
        }
    }
}

fn json_path_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let trimmed = path.trim();
    let trimmed = trimmed.strip_prefix('$').unwrap_or(trimmed);
    let trimmed = trimmed.strip_prefix('.').unwrap_or(trimmed);
    if trimmed.is_empty() {
        return Some(value);
    }
    let mut current = value;
    for part in trimmed.split('.') {
        current = match current {
            Value::Object(map) => map.get(part)?,
            Value::Array(items) => items.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn workflow_id_for_role(record: &AutomationRecord, role: &CompiledWorkflowRole) -> Option<String> {
    record
        .runtime
        .agentenv_workflows
        .iter()
        .find(|workflow| workflow.role == *role)
        .and_then(|workflow| workflow.workflow_id.clone())
}

fn existing_workflow_id(record: &AutomationRecord, role: &CompiledWorkflowRole) -> Option<String> {
    workflow_id_for_role(record, role)
}

fn runtime_workflow_id(record: &WorkflowRuntimeWorkflow) -> Result<String> {
    for key in WORKFLOW_ID_KEYS {
        if let Some(value) = record
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(value.to_string());
        }
    }
    bail!("workflow runtime response did not include a workflow id")
}

fn workflow_display_name(record: &AutomationRecord, role: &CompiledWorkflowRole) -> String {
    match role {
        CompiledWorkflowRole::Root => format!("Automation {} root", record.id),
        CompiledWorkflowRole::LoopBody { step_id } => {
            format!("Automation {} loop {step_id}", record.id)
        }
        CompiledWorkflowRole::Continuation { step_id } => {
            format!("Automation {} continuation {step_id}", record.id)
        }
        CompiledWorkflowRole::Helper { step_id } => {
            format!("Automation {} helper {step_id}", record.id)
        }
    }
}

fn summarize_value(value: &Value) -> String {
    let text = workflow_execute_summary(
        "automation",
        &WorkflowRuntimeRecord::try_from(value.clone()).unwrap_or_else(|_| {
            let mut fields = BTreeMap::new();
            fields.insert("output".into(), value.clone());
            WorkflowRuntimeRecord::new(fields)
        }),
    );
    if text.chars().count() > 240 {
        format!("{}...", text.chars().take(240).collect::<String>())
    } else {
        text
    }
}

fn runtime_summary(runtime: &AutomationRuntimeState) -> Value {
    json!({
        "status": runtime.status,
        "spec_hash": runtime.spec_hash.clone(),
        "compiled_revision": runtime.compiled_revision,
        "agentenv_workflow_count": runtime.agentenv_workflows.len(),
        "puffer_binding_count": runtime.puffer_bindings.len(),
        "last_error": runtime.last_error.clone(),
    })
}

fn required_automation_id(params: &Value) -> Result<String> {
    for key in AUTOMATION_ID_KEYS {
        if let Some(value) = params
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(value.to_string());
        }
    }
    bail!("missing automation id")
}

fn optional_expected_revision(params: &Value) -> Result<Option<u64>> {
    match (
        params.get("expected_revision"),
        params.get("expectedRevision"),
    ) {
        (Some(_), Some(_)) => {
            bail!("accepts only one of expected_revision or expectedRevision")
        }
        (Some(value), None) | (None, Some(value)) => value
            .as_u64()
            .context("expected_revision must be an unsigned integer")
            .map(Some),
        (None, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon_workflow_backend_settings::save_workflow_backend_settings;
    use crate::daemon_workflow_backend_settings::test_support::{
        lock_secret_store, ScopedSecretStoreKey,
    };
    use crate::desktop_api_types::SaveWorkflowBackendSettingsParams;
    use puffer_automation::{
        automation_spec_hash, AgentEnvNodeRef, AutomationFlowSpec, AutomationLoopInput,
        AutomationReviewSpec, AutomationRunLocation, AutomationSource, AutomationSpec,
        AutomationTriggerSpec, AUTOMATION_SPEC_VERSION,
    };
    use puffer_config::{ensure_workspace_dirs, PufferConfig, WorkflowBackendMode};
    use puffer_core::{install_subscription_manager, subscription_manager};
    use puffer_subscriptions::{SubscriptionManager, SubscriptionManagerBuilder};
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    fn temp_paths(temp: &TempDir) -> ConfigPaths {
        let root = temp.path();
        ConfigPaths {
            workspace_root: root.join("workspace"),
            workspace_config_dir: root.join("workspace").join(".puffer"),
            user_config_dir: root.join("home").join(".puffer"),
            builtin_resources_dir: root.join("resources"),
        }
    }

    fn node(node_type: &str) -> AgentEnvNodeRef {
        AgentEnvNodeRef {
            node_type: node_type.to_string(),
            name: Some(node_type.to_string()),
            trusted: Some(true),
            config: BTreeMap::new(),
        }
    }

    fn manual_trigger() -> AutomationTriggerSpec {
        AutomationTriggerSpec::Manual {
            id: "manual".into(),
            summary: None,
        }
    }

    fn agentenv_trigger(node_type: &str) -> AutomationTriggerSpec {
        AutomationTriggerSpec::AgentEnvNode {
            id: "incoming".into(),
            node: node(node_type),
            summary: None,
        }
    }

    fn linear_spec(instructions: &str) -> AutomationSpec {
        AutomationSpec {
            spec_version: AUTOMATION_SPEC_VERSION,
            name: "Reply helper".into(),
            description: None,
            source: AutomationSource::Blank,
            instructions: instructions.into(),
            run_location: AutomationRunLocation::AgentEnvCloud,
            triggers: vec![manual_trigger()],
            flow: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "draft".into(),
                    node: node("llm"),
                    summary: None,
                }],
            },
            review: AutomationReviewSpec::default(),
        }
    }

    fn connector_trigger_spec(instructions: &str, connection_slug: &str) -> AutomationSpec {
        AutomationSpec {
            triggers: vec![AutomationTriggerSpec::PufferConnection {
                id: "incoming".into(),
                connection_slug: connection_slug.into(),
                connector_slug: Some("telegram-login".into()),
                filter: None,
                ignore_filters: Vec::new(),
                contact_ids: Vec::new(),
                summary: None,
            }],
            ..linear_spec(instructions)
        }
    }

    fn connector_action_spec(external_side_effect: bool) -> AutomationSpec {
        let mut config = BTreeMap::new();
        config.insert("external_side_effect".into(), json!(external_side_effect));
        config.insert("draft_only".into(), json!(external_side_effect));
        AutomationSpec {
            flow: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "send".into(),
                    node: AgentEnvNodeRef {
                        node_type: "puffer_connector_action".into(),
                        name: Some("Send message".into()),
                        trusted: Some(true),
                        config,
                    },
                    summary: None,
                }],
            },
            ..linear_spec("Send a connector action.")
        }
    }

    fn loop_continuation_spec() -> AutomationSpec {
        AutomationSpec {
            flow: AutomationFlowSpec {
                steps: vec![
                    AutomationStepSpec::Loop {
                        id: "retry".into(),
                        loop_spec: AutomationLoopSpec::Repeat {
                            input: AutomationLoopInput::Trigger,
                            stop_when: AutomationLoopStopSpec::OutputEquals {
                                path: "$.done".into(),
                                value: json!(true),
                            },
                            max_iterations: 2,
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
            ..linear_spec("Try until complete.")
        }
    }

    fn foreach_loop_spec(max_iterations: Option<u32>) -> AutomationSpec {
        AutomationSpec {
            flow: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::Loop {
                    id: "items".into(),
                    loop_spec: AutomationLoopSpec::ForEach {
                        input: AutomationLoopInput::StepOutput {
                            step_id: "root".into(),
                            path: Some("$.items".into()),
                        },
                        item_alias: "item".into(),
                        max_iterations,
                    },
                    body: AutomationFlowSpec {
                        steps: vec![AutomationStepSpec::AgentEnvNode {
                            id: "visit".into(),
                            node: node("transform_js"),
                            summary: None,
                        }],
                    },
                    summary: None,
                }],
            },
            ..linear_spec("Visit items.")
        }
    }

    fn repeat_agentenv_stop_spec() -> AutomationSpec {
        AutomationSpec {
            flow: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::Loop {
                    id: "retry".into(),
                    loop_spec: AutomationLoopSpec::Repeat {
                        input: AutomationLoopInput::Trigger,
                        stop_when: AutomationLoopStopSpec::AgentEnvNode { node: node("stop") },
                        max_iterations: 1,
                    },
                    body: AutomationFlowSpec {
                        steps: vec![AutomationStepSpec::AgentEnvNode {
                            id: "attempt".into(),
                            node: node("transform_js"),
                            summary: None,
                        }],
                    },
                    summary: None,
                }],
            },
            ..linear_spec("Try with delegated stop.")
        }
    }

    fn configure_runtime(paths: &ConfigPaths, api_url: String) {
        ensure_workspace_dirs(paths).expect("workspace dirs");
        let mut config = PufferConfig::default();
        save_workflow_backend_settings(
            paths,
            &mut config,
            SaveWorkflowBackendSettingsParams {
                mode: WorkflowBackendMode::AgentEnvCloud,
                api_url,
                ui_url: "http://localhost:5173".into(),
                workspace_id: "workspace-automation-test".into(),
                api_token: Some("runtime-token".into()),
                keep_token: false,
            },
        )
        .expect("save workflow backend settings");
    }

    struct TestSubscriptionManager {
        _runtime: tokio::runtime::Runtime,
        _tempdir: tempfile::TempDir,
        manager: Arc<SubscriptionManager>,
    }

    static TEST_SUBSCRIPTION_MANAGER: OnceLock<TestSubscriptionManager> = OnceLock::new();

    fn test_subscription_manager() -> Arc<SubscriptionManager> {
        if let Ok(manager) = subscription_manager() {
            return manager;
        }
        let state = TEST_SUBSCRIPTION_MANAGER.get_or_init(|| {
            let tempdir = tempfile::tempdir().unwrap();
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(1)
                .thread_name("puffer-automation-runtime-test")
                .build()
                .unwrap();
            let manager = Arc::new(
                SubscriptionManagerBuilder::new(tempdir.path().join("subscriptions.json"))
                    .build(runtime.handle().clone())
                    .unwrap(),
            );
            let _ = install_subscription_manager(manager.clone());
            TestSubscriptionManager {
                _runtime: runtime,
                _tempdir: tempdir,
                manager,
            }
        });
        subscription_manager().unwrap_or_else(|_| state.manager.clone())
    }

    fn unavailable_runtime_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused runtime port");
        let address = listener.local_addr().expect("unused runtime address");
        drop(listener);
        format!("http://{address}")
    }

    fn deployed_runtime(
        record: &AutomationRecord,
        workflows: Vec<CompiledAgentEnvWorkflow>,
    ) -> AutomationRuntimeState {
        AutomationRuntimeState {
            spec_hash: Some(automation_spec_hash(&record.spec).unwrap()),
            compiled_revision: Some(record.revision),
            status: AutomationRuntimeStatus::Deployed,
            agentenv_workflows: workflows,
            puffer_bindings: Vec::new(),
            last_error: None,
        }
    }

    fn root_workflow(id: &str) -> CompiledAgentEnvWorkflow {
        CompiledAgentEnvWorkflow {
            role: CompiledWorkflowRole::Root,
            workflow_id: Some(id.into()),
            definition_hash: None,
            deployed: true,
        }
    }

    fn loop_workflow(step_id: &str, id: &str) -> CompiledAgentEnvWorkflow {
        CompiledAgentEnvWorkflow {
            role: CompiledWorkflowRole::LoopBody {
                step_id: step_id.into(),
            },
            workflow_id: Some(id.into()),
            definition_hash: None,
            deployed: true,
        }
    }

    struct MockRuntimeResponse {
        status: u16,
        body: Value,
    }

    impl MockRuntimeResponse {
        fn ok(body: Value) -> Self {
            Self { status: 200, body }
        }

        fn error(message: &str) -> Self {
            Self {
                status: 500,
                body: json!({ "error": { "message": message } }),
            }
        }

        fn not_found(message: &str) -> Self {
            Self {
                status: 404,
                body: json!({ "error": { "message": message } }),
            }
        }
    }

    fn spawn_runtime_server(
        responses: Vec<MockRuntimeResponse>,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock runtime");
        let address = listener.local_addr().expect("mock runtime address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept runtime request");
                let request = read_http_request(&mut stream);
                captured.lock().expect("requests lock").push(request);
                write_http_json(&mut stream, response.status, response.body);
            }
        });
        (format!("http://{address}"), requests, handle)
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set read timeout");
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 1024];
        let header_end = loop {
            let read = stream.read(&mut buffer).expect("read request");
            assert!(read > 0, "request ended before headers");
            bytes.extend_from_slice(&buffer[..read]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let head = String::from_utf8(bytes[..header_end].to_vec()).expect("request head utf8");
        let content_length = content_length(&head);
        while bytes.len() < header_end + content_length {
            let read = stream.read(&mut buffer).expect("read request body");
            assert!(read > 0, "request ended before body");
            bytes.extend_from_slice(&buffer[..read]);
        }
        String::from_utf8(bytes).expect("request utf8")
    }

    fn content_length(head: &str) -> usize {
        head.lines()
            .filter_map(|line| line.split_once(':'))
            .find_map(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("content length"))
            })
            .unwrap_or(0)
    }

    fn write_http_json(stream: &mut TcpStream, status: u16, value: Value) {
        let body = value.to_string();
        let reason = if status == 200 {
            "OK"
        } else {
            "Internal Server Error"
        };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    }

    #[test]
    fn daemon_automation_runtime_compile_failure_writes_error_runtime_without_success_hash() {
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        store
            .create(
                "reply-helper",
                loop_continuation_spec(),
                AutomationStatus::Enabled,
            )
            .unwrap();

        let error =
            compile_and_deploy_with_store(&paths, &store, "reply-helper", Some(1)).unwrap_err();

        assert!(
            format!("{error:#}").contains("loop continuation compilation is not implemented yet")
        );
        let record = store.get("reply-helper").unwrap();
        assert_eq!(record.runtime.status, AutomationRuntimeStatus::Error);
        assert_eq!(record.runtime.spec_hash, None);
        assert_eq!(record.runtime.compiled_revision, None);
        assert!(record
            .runtime
            .last_error
            .as_deref()
            .unwrap()
            .contains("loop continuation compilation is not implemented yet"));
    }

    #[test]
    fn daemon_automation_runtime_agentenv_unavailable_writes_error_runtime() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, unavailable_runtime_url());
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        store
            .create(
                "reply-helper",
                linear_spec("Draft a reply."),
                AutomationStatus::Enabled,
            )
            .unwrap();

        let error =
            compile_and_deploy_with_store(&paths, &store, "reply-helper", Some(1)).unwrap_err();

        assert!(format!("{error:#}").contains("create workflow artifact"));
        let record = store.get("reply-helper").unwrap();
        assert_eq!(record.runtime.status, AutomationRuntimeStatus::Error);
        assert!(record
            .runtime
            .last_error
            .as_deref()
            .unwrap()
            .contains("create workflow artifact"));
    }

    #[test]
    fn daemon_automation_runtime_public_error_hides_runtime_url() {
        let error = anyhow::anyhow!(
            "create workflow artifact: runtime unreachable: error sending request for url (http://127.0.0.1:3000/v1/workflows)"
        );

        let message = public_automation_error_message(&error);

        assert_eq!(
            message,
            "Automation runtime is unreachable. Start the selected runtime, then try again."
        );
        assert!(!message.contains("127.0.0.1"));
        assert!(!message.contains("/v1/"));
        assert!(!message.contains("workflow artifact"));
    }

    #[test]
    fn daemon_automation_runtime_create_workflow_failure_writes_error_runtime() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let (api_url, requests, handle) =
            spawn_runtime_server(vec![MockRuntimeResponse::error("create failed")]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        store
            .create(
                "reply-helper",
                linear_spec("Draft a reply."),
                AutomationStatus::Enabled,
            )
            .unwrap();

        let error =
            compile_and_deploy_with_store(&paths, &store, "reply-helper", Some(1)).unwrap_err();
        handle.join().expect("mock runtime joined");

        let message = format!("{error:#}");
        assert!(message.contains("create workflow artifact"));
        assert!(message.contains("create failed"));
        assert_eq!(requests.lock().unwrap().len(), 1);
        let record = store.get("reply-helper").unwrap();
        assert_eq!(record.runtime.status, AutomationRuntimeStatus::Error);
        assert!(record
            .runtime
            .last_error
            .as_deref()
            .unwrap()
            .contains("create failed"));
    }

    #[test]
    fn daemon_automation_runtime_deploy_workflow_failure_writes_error_runtime() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let (api_url, requests, handle) = spawn_runtime_server(vec![
            MockRuntimeResponse::ok(json!({ "data": { "id": "wf-deploy-fail" } })),
            MockRuntimeResponse::error("deploy failed"),
        ]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        store
            .create(
                "reply-helper",
                linear_spec("Draft a reply."),
                AutomationStatus::Enabled,
            )
            .unwrap();

        let error =
            compile_and_deploy_with_store(&paths, &store, "reply-helper", Some(1)).unwrap_err();
        handle.join().expect("mock runtime joined");

        let message = format!("{error:#}");
        assert!(message.contains("deploy workflow `wf-deploy-fail`"));
        assert!(message.contains("deploy failed"));
        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert!(captured[0].starts_with("POST /v1/workflows "));
        assert!(captured[1].starts_with("POST /v1/workflows/wf-deploy-fail/deploy "));
        let record = store.get("reply-helper").unwrap();
        assert_eq!(record.runtime.status, AutomationRuntimeStatus::Error);
        assert!(record
            .runtime
            .last_error
            .as_deref()
            .unwrap()
            .contains("deploy failed"));
    }

    #[test]
    fn daemon_automation_runtime_preview_sync_does_not_deploy_live_bindings() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let manager = test_subscription_manager();
        let slug = "automation-preview-helper-incoming";
        let _ = manager.store().delete(slug);
        let (api_url, requests, handle) =
            spawn_runtime_server(vec![MockRuntimeResponse::ok(json!({
                "data": { "id": "wf-preview-sync" }
            }))]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        store
            .create(
                "preview-helper",
                connector_trigger_spec("Draft a reply.", "telegram-user"),
                AutomationStatus::Enabled,
            )
            .unwrap();

        let record = sync_preview_with_store(&paths, &store, "preview-helper", Some(1)).unwrap();
        handle.join().expect("mock runtime joined");

        assert_eq!(record.runtime.status, AutomationRuntimeStatus::DraftSynced);
        assert!(record.runtime.puffer_bindings.is_empty());
        assert!(manager.store().get(slug).is_none());
        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert!(captured[0].starts_with("POST /v1/workflows "));
        assert!(!captured[0].contains("/deploy "));
    }

    #[test]
    fn daemon_automation_runtime_preview_sync_keeps_current_live_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let record = store
            .create(
                "preview-live-helper",
                connector_trigger_spec("Draft a reply.", "telegram-user"),
                AutomationStatus::Enabled,
            )
            .unwrap();
        store
            .replace_runtime(
                "preview-live-helper",
                record.revision,
                AutomationRuntimeState {
                    spec_hash: Some(automation_spec_hash(&record.spec).unwrap()),
                    compiled_revision: Some(record.revision),
                    status: AutomationRuntimeStatus::Deployed,
                    agentenv_workflows: vec![root_workflow("wf-live")],
                    puffer_bindings: vec![CompiledPufferBinding {
                        trigger_id: "incoming".into(),
                        binding_slug: "automation-preview-live-helper-incoming".into(),
                    }],
                    last_error: None,
                },
            )
            .unwrap();

        let synced =
            sync_preview_with_store(&paths, &store, "preview-live-helper", Some(record.revision))
                .unwrap();

        assert_eq!(synced.runtime.status, AutomationRuntimeStatus::Deployed);
        assert_eq!(synced.runtime.puffer_bindings.len(), 1);
        assert_eq!(
            synced.runtime.puffer_bindings[0].binding_slug,
            "automation-preview-live-helper-incoming"
        );
    }

    #[test]
    fn daemon_automation_runtime_preview_execute_failure_keeps_deployed_runtime() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let (api_url, requests, handle) =
            spawn_runtime_server(vec![MockRuntimeResponse::error("execute failed")]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let record = store
            .create(
                "reply-helper",
                linear_spec("Draft a reply."),
                AutomationStatus::Enabled,
            )
            .unwrap();
        store
            .replace_runtime(
                "reply-helper",
                record.revision,
                deployed_runtime(&record, vec![root_workflow("wf-root")]),
            )
            .unwrap();

        let error = run_automation_preview_with_store(
            &paths,
            &store,
            "reply-helper",
            json!({ "text": "hello" }),
        )
        .unwrap_err();
        handle.join().expect("mock runtime joined");

        let message = format!("{error:#}");
        assert!(message.contains("execute in-memory workflow"));
        assert!(message.contains("execute failed"));
        assert_eq!(requests.lock().unwrap().len(), 1);
        let record = store.get("reply-helper").unwrap();
        assert_eq!(record.runtime.status, AutomationRuntimeStatus::Deployed);
        assert_eq!(record.runtime.last_error, None);
    }

    #[test]
    fn daemon_automation_runtime_preview_falls_back_when_in_memory_endpoint_is_missing() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let (api_url, requests, handle) = spawn_runtime_server(vec![
            MockRuntimeResponse::not_found("not found"),
            MockRuntimeResponse::ok(json!({
                "data": { "status": "completed", "output": { "fallback": true } }
            })),
        ]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let mut spec = linear_spec("Draft a reply.");
        spec.run_location = AutomationRunLocation::AgentEnvCloud;
        let record = store
            .create("reply-helper", spec, AutomationStatus::Enabled)
            .unwrap();
        store
            .replace_runtime(
                "reply-helper",
                record.revision,
                deployed_runtime(&record, vec![root_workflow("wf-root")]),
            )
            .unwrap();

        let output = run_automation_preview_with_store(
            &paths,
            &store,
            "reply-helper",
            json!({ "text": "hello" }),
        )
        .unwrap();
        handle.join().expect("mock runtime joined");

        assert_eq!(output.result["status"], "completed");
        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert!(captured[0].starts_with("POST /v1/workflows/execute-in-memory "));
        assert!(captured[1].starts_with("POST /v1/workflows/wf-root/execute "));
    }

    #[test]
    fn daemon_automation_runtime_preview_run_uses_in_memory_execution() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let (api_url, requests, handle) =
            spawn_runtime_server(vec![MockRuntimeResponse::ok(json!({
                "data": { "status": "completed", "output": { "ok": true } }
            }))]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let mut spec = linear_spec("Draft a reply.");
        spec.run_location = AutomationRunLocation::AgentEnvCloud;
        let record = store
            .create("reply-helper", spec, AutomationStatus::Enabled)
            .unwrap();
        store
            .replace_runtime(
                "reply-helper",
                record.revision,
                AutomationRuntimeState {
                    spec_hash: Some(automation_spec_hash(&record.spec).unwrap()),
                    compiled_revision: Some(record.revision),
                    status: AutomationRuntimeStatus::DraftSynced,
                    agentenv_workflows: vec![root_workflow("wf-draft")],
                    puffer_bindings: Vec::new(),
                    last_error: None,
                },
            )
            .unwrap();

        run_automation_preview_with_store(
            &paths,
            &store,
            "reply-helper",
            json!({ "text": "hello" }),
        )
        .unwrap();
        handle.join().expect("mock runtime joined");

        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert!(captured[0].starts_with("POST /v1/workflows/execute-in-memory "));
        assert!(captured[0].contains(r#""triggerNodeId":"manual""#));
        assert!(captured[0].contains(r#""trigger":{"text":"hello"}"#));
    }

    #[test]
    fn daemon_automation_runtime_save_spec_change_marks_runtime_stale() {
        let temp = tempfile::tempdir().unwrap();
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let record = store
            .create(
                "reply-helper",
                linear_spec("Draft a reply."),
                AutomationStatus::Enabled,
            )
            .unwrap();
        let spec_hash = automation_spec_hash(&record.spec).unwrap();
        store
            .replace_runtime(
                "reply-helper",
                record.revision,
                AutomationRuntimeState {
                    spec_hash: Some(spec_hash),
                    compiled_revision: Some(record.revision),
                    status: AutomationRuntimeStatus::Deployed,
                    agentenv_workflows: vec![CompiledAgentEnvWorkflow {
                        role: CompiledWorkflowRole::Root,
                        workflow_id: Some("automation-reply-helper-root".into()),
                        definition_hash: None,
                        deployed: true,
                    }],
                    puffer_bindings: Vec::new(),
                    last_error: None,
                },
            )
            .unwrap();

        let updated = store
            .save_spec(
                "reply-helper",
                record.revision,
                linear_spec("Draft a reply with context."),
            )
            .unwrap();

        assert_eq!(updated.revision, record.revision + 1);
        assert_eq!(updated.runtime.status, AutomationRuntimeStatus::Stale);
        assert!(updated.runtime.agentenv_workflows.is_empty());
        assert!(updated.runtime.puffer_bindings.is_empty());
        assert_eq!(updated.runtime.last_error, None);
    }

    #[test]
    fn daemon_automation_runtime_stale_spec_preview_refuses_remote_sync() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let (api_url, requests, handle) = spawn_runtime_server(vec![]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let record = store
            .create(
                "reply-helper",
                linear_spec("Draft a reply."),
                AutomationStatus::Enabled,
            )
            .unwrap();
        store
            .replace_runtime(
                "reply-helper",
                record.revision,
                deployed_runtime(&record, vec![root_workflow("wf-old")]),
            )
            .unwrap();
        let updated = store
            .save_spec(
                "reply-helper",
                record.revision,
                linear_spec("Draft a reply with context."),
            )
            .unwrap();

        let error = run_automation_preview_with_store(
            &paths,
            &store,
            "reply-helper",
            json!({ "text": "hello" }),
        )
        .unwrap_err();
        handle.join().expect("mock runtime joined");

        assert!(format!("{error:#}").contains("deploy before running a test preview"));
        let record = store.get("reply-helper").unwrap();
        assert_eq!(record.revision, updated.revision);
        assert_eq!(record.runtime.status, AutomationRuntimeStatus::Stale);
        let captured = requests.lock().unwrap();
        assert!(captured.is_empty());
    }

    #[test]
    fn daemon_automation_runtime_preview_refuses_approval_gated_connector_action() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let (api_url, requests, handle) = spawn_runtime_server(vec![]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let record = store
            .create(
                "reply-helper",
                connector_action_spec(true),
                AutomationStatus::Enabled,
            )
            .unwrap();
        store
            .replace_runtime(
                "reply-helper",
                record.revision,
                deployed_runtime(&record, vec![root_workflow("wf-root")]),
            )
            .unwrap();

        let error = run_automation_preview_with_store(
            &paths,
            &store,
            "reply-helper",
            json!({ "text": "hello" }),
        )
        .unwrap_err();
        handle.join().expect("mock runtime joined");

        assert!(format!("{error:#}").contains("test preview does not execute outward actions"));
        assert!(requests.lock().unwrap().is_empty());
    }

    #[test]
    fn daemon_automation_runtime_stale_enabled_save_removes_old_binding() {
        let manager = test_subscription_manager();
        let slug = "automation-stale-binding-helper-incoming";
        let _ = manager.store().delete(slug);
        let temp = tempfile::tempdir().unwrap();
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let record = store
            .create(
                "stale-binding-helper",
                connector_trigger_spec("Draft a reply.", "telegram-old"),
                AutomationStatus::Enabled,
            )
            .unwrap();
        store
            .replace_runtime(
                "stale-binding-helper",
                record.revision,
                AutomationRuntimeState {
                    spec_hash: Some(automation_spec_hash(&record.spec).unwrap()),
                    compiled_revision: Some(record.revision),
                    status: AutomationRuntimeStatus::Deployed,
                    agentenv_workflows: Vec::new(),
                    puffer_bindings: vec![CompiledPufferBinding {
                        trigger_id: "incoming".into(),
                        binding_slug: slug.into(),
                    }],
                    last_error: None,
                },
            )
            .unwrap();
        manager
            .store()
            .upsert(WorkflowBindingSpec {
                slug: slug.into(),
                description: "old generated automation binding".into(),
                connection_slug: "telegram-old".into(),
                connector_slug: Some("telegram-login".into()),
                status: WorkflowBindingStatus::Enabled,
                filter: None,
                ignore_filters: Vec::new(),
                contact_ids: Vec::new(),
                classify_prompt: None,
                classify_model: None,
                action: ActionSpec::RunAutomation {
                    automation_id: "stale-binding-helper".into(),
                },
                created_at_ms: puffer_subscriptions::now_ms(),
            })
            .unwrap();
        let previous = store.get("stale-binding-helper").unwrap();
        let updated = store
            .save_spec(
                "stale-binding-helper",
                previous.revision,
                connector_trigger_spec("Draft a reply.", "telegram-new"),
            )
            .unwrap();

        sync_automation_bindings_after_save(Some(&previous), &updated).unwrap();

        assert_eq!(updated.status, AutomationStatus::Enabled);
        assert_eq!(updated.runtime.status, AutomationRuntimeStatus::Stale);
        assert!(manager.store().get(slug).is_none());
    }

    #[test]
    fn daemon_automation_runtime_agentenv_trigger_sets_trigger_node_id() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let (api_url, requests, handle) =
            spawn_runtime_server(vec![MockRuntimeResponse::ok(json!({
                "data": { "status": "completed" }
            }))]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let spec = AutomationSpec {
            triggers: vec![agentenv_trigger("webhook")],
            flow: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "transform".into(),
                    node: node("transform_js"),
                    summary: None,
                }],
            },
            ..linear_spec("Transform webhook input.")
        };
        let record = store
            .create("webhook-helper", spec, AutomationStatus::Enabled)
            .unwrap();
        store
            .replace_runtime(
                "webhook-helper",
                record.revision,
                deployed_runtime(&record, vec![root_workflow("wf-webhook")]),
            )
            .unwrap();

        run_automation_preview_with_store(
            &paths,
            &store,
            "webhook-helper",
            json!({ "text": "hello" }),
        )
        .unwrap();
        handle.join().expect("mock runtime joined");

        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert!(captured[0].contains(r#""triggerNodeId":"incoming""#));
        assert!(captured[0].contains(r#""trigger":{"text":"hello"}"#));
    }

    #[test]
    fn daemon_automation_runtime_foreach_loop_respects_max_iterations() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let (api_url, requests, handle) = spawn_runtime_server(vec![
            MockRuntimeResponse::ok(json!({ "data": { "items": [1, 2, 3] } })),
            MockRuntimeResponse::ok(json!({ "data": { "seen": 1 } })),
            MockRuntimeResponse::ok(json!({ "data": { "seen": 2 } })),
        ]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let record = store
            .create(
                "loop-helper",
                foreach_loop_spec(Some(2)),
                AutomationStatus::Enabled,
            )
            .unwrap();
        store
            .replace_runtime(
                "loop-helper",
                record.revision,
                deployed_runtime(
                    &record,
                    vec![root_workflow("wf-root"), loop_workflow("items", "wf-loop")],
                ),
            )
            .unwrap();

        let output = run_automation_preview_with_store(
            &paths,
            &store,
            "loop-helper",
            json!({ "text": "hello" }),
        )
        .unwrap();
        handle.join().expect("mock runtime joined");

        assert_eq!(output.result, json!({ "seen": 2 }));
        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 3);
        assert!(captured[1].contains(r#""iteration":0"#));
        assert!(captured[1].contains(r#""item":1"#));
        assert!(captured[2].contains(r#""iteration":1"#));
        assert!(captured[2].contains(r#""item":2"#));
        assert!(!captured
            .iter()
            .any(|request| request.contains(r#""item":3"#)));
    }

    #[test]
    fn daemon_automation_runtime_loop_agentenv_stop_condition_fails_explicitly() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let (api_url, requests, handle) = spawn_runtime_server(vec![
            MockRuntimeResponse::ok(json!({ "data": { "root": true } })),
            MockRuntimeResponse::ok(json!({ "data": { "done": false } })),
        ]);
        let temp = tempfile::tempdir().unwrap();
        let paths = temp_paths(&temp);
        configure_runtime(&paths, api_url);
        let store = AutomationStore::load(temp.path().join("automations.json")).unwrap();
        let record = store
            .create(
                "loop-helper",
                repeat_agentenv_stop_spec(),
                AutomationStatus::Enabled,
            )
            .unwrap();
        store
            .replace_runtime(
                "loop-helper",
                record.revision,
                deployed_runtime(
                    &record,
                    vec![root_workflow("wf-root"), loop_workflow("retry", "wf-loop")],
                ),
            )
            .unwrap();

        let error = run_automation_preview_with_store(
            &paths,
            &store,
            "loop-helper",
            json!({ "text": "hello" }),
        )
        .unwrap_err();
        handle.join().expect("mock runtime joined");

        let message = format!("{error:#}");
        assert!(message.contains("run loop `retry`"));
        assert!(message.contains("loop stop_when AgentEnvNode is not implemented"));
        assert_eq!(requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn daemon_automation_runtime_preview_response_shape_hides_runtime_artifacts() {
        let record = AutomationRecord {
            id: "reply-helper".into(),
            status: AutomationStatus::Enabled,
            revision: 3,
            spec: linear_spec("Draft a reply."),
            runtime: AutomationRuntimeState {
                status: AutomationRuntimeStatus::Deployed,
                spec_hash: Some(
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                        .into(),
                ),
                compiled_revision: Some(3),
                agentenv_workflows: vec![CompiledAgentEnvWorkflow {
                    role: CompiledWorkflowRole::Root,
                    workflow_id: Some("internal-root".into()),
                    definition_hash: None,
                    deployed: true,
                }],
                puffer_bindings: vec![CompiledPufferBinding {
                    trigger_id: "incoming".into(),
                    binding_slug: "automation-reply-helper-incoming".into(),
                }],
                last_error: None,
            },
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        let output = AutomationRunOutput {
            compiled: true,
            record,
            result: json!({
                "ok": true,
                "workflowId": "customer-workflow-123",
                "nested": {
                    "binding_slug": "customer-binding"
                }
            }),
            summary: "completed".into(),
        };

        let value = automation_preview_response(&output);

        assert_eq!(value["id"], "reply-helper");
        assert_eq!(value["status"], "completed");
        assert_eq!(
            value["result"],
            json!({
                "ok": true,
                "workflowId": "customer-workflow-123",
                "nested": {
                    "binding_slug": "customer-binding"
                }
            })
        );
        assert_eq!(value["compiled"], true);
        assert_eq!(value["runtime"]["status"], "deployed");
        assert_eq!(value["runtime"]["agentenv_workflow_count"], 1);
        assert_eq!(value["runtime"]["puffer_binding_count"], 1);
        assert!(serde_json::to_string(&value)
            .unwrap()
            .find("internal-root")
            .is_none());
        assert!(serde_json::to_string(&value)
            .unwrap()
            .find("automation-reply-helper-incoming")
            .is_none());
    }
}
