use super::{ensure_workflow_subscriber_started, handle_workflow_list, resolve_binding_trigger};
use anyhow::{Context, Result};
use puffer_config::ConfigPaths;
use puffer_core::subscription_manager;
use puffer_subscriptions::{
    validate_spec as validate_workflow_binding, ActionSpec, ConnectionRecord, ConnectorTemplate,
    FilterSpec, SubscriptionManager, TaggedFilterSpec, WorkflowBindingSpec, WorkflowBindingStatus,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// Creates or updates a connection-triggered workflow binding.
pub(crate) fn handle_workflow_binding_create(paths: &ConfigPaths, params: &Value) -> Result<Value> {
    let parsed: WorkflowBindingCreateParams =
        serde_json::from_value(params.clone()).context("invalid workflow binding create params")?;
    let connection_slug = parsed.connection_slug.trim();
    if connection_slug.is_empty() {
        anyhow::bail!("connection_slug must not be empty");
    }
    if parsed.action.is_none() && !has_workflow_id(&parsed) && !has_file_append_path(&parsed) {
        anyhow::bail!("workflow_binding_create requires action, workflow_id, or file_append_path");
    }
    let requested_connector_slug = parsed
        .connector_slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let manager = subscription_manager()?;
    let (connection, connector_slug, template) = resolve_binding_trigger(
        paths,
        manager.as_ref(),
        connection_slug,
        requested_connector_slug,
    )?;
    let binding = binding_from_params(parsed, connector_slug)?;
    save_workflow_binding(
        manager.as_ref(),
        paths,
        binding,
        connection.as_ref(),
        &template,
    )?;
    handle_workflow_list(paths)
}

fn save_workflow_binding(
    manager: &SubscriptionManager,
    paths: &ConfigPaths,
    binding: WorkflowBindingSpec,
    connection: Option<&ConnectionRecord>,
    template: &ConnectorTemplate,
) -> Result<()> {
    let binding_slug = binding.slug.clone();
    let previous = manager.store().get(&binding_slug);
    manager.store().upsert(binding.clone())?;
    let setup_result = setup_saved_workflow_binding(manager, paths, &binding, connection, template);
    if let Err(error) = setup_result {
        if let Err(rollback_error) =
            rollback_saved_workflow_binding(manager, &binding_slug, previous.as_ref())
        {
            anyhow::bail!(
                "workflow binding `{binding_slug}` setup failed: {error:#}; rollback failed: {rollback_error:#}"
            );
        }
        return Err(error)
            .with_context(|| format!("workflow binding `{binding_slug}` setup failed"));
    }
    Ok(())
}

fn setup_saved_workflow_binding(
    manager: &SubscriptionManager,
    paths: &ConfigPaths,
    binding: &WorkflowBindingSpec,
    connection: Option<&ConnectionRecord>,
    template: &ConnectorTemplate,
) -> Result<()> {
    if binding.status == WorkflowBindingStatus::Enabled {
        if let Some(connection) = connection {
            ensure_workflow_subscriber_started(manager, paths, connection, template)?;
        }
    }
    manager.refresh_connection_consumers()?;
    Ok(())
}

fn rollback_saved_workflow_binding(
    manager: &SubscriptionManager,
    binding_slug: &str,
    previous: Option<&WorkflowBindingSpec>,
) -> Result<()> {
    let rollback_result = if let Some(previous) = previous {
        manager
            .store()
            .upsert(previous.clone())
            .map(|_| ())
            .map_err(anyhow::Error::from)
    } else if manager.store().get(binding_slug).is_some() {
        manager
            .store()
            .delete(binding_slug)
            .map_err(anyhow::Error::from)
    } else {
        Ok(())
    };
    let refresh_result = manager.refresh_connection_consumers();
    match (rollback_result, refresh_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(rollback_error), Ok(())) => Err(rollback_error),
        (Ok(()), Err(refresh_error)) => Err(refresh_error),
        (Err(rollback_error), Err(refresh_error)) => Err(rollback_error).with_context(|| {
            format!("rollback refresh failed after rollback error: {refresh_error:#}")
        }),
    }
}

#[derive(Debug, Deserialize)]
struct WorkflowBindingCreateParams {
    #[serde(default)]
    slug: Option<String>,
    connection_slug: String,
    #[serde(default)]
    connector_slug: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "include_filter", alias = "prefilter")]
    filter: Option<FilterSpec>,
    #[serde(default)]
    ignore_filters: Vec<FilterSpec>,
    #[serde(default)]
    contact_ids: Vec<String>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default, alias = "path")]
    file_append_path: Option<String>,
    #[serde(default)]
    action: Option<ActionSpec>,
    #[serde(
        default,
        alias = "workflowId",
        alias = "workflow_slug",
        alias = "workflowSlug"
    )]
    workflow_id: Option<String>,
    #[serde(default)]
    classify_prompt: Option<String>,
    #[serde(default)]
    classify_model: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

fn binding_from_params(
    params: WorkflowBindingCreateParams,
    connector_slug: String,
) -> Result<WorkflowBindingSpec> {
    let binding = if params.action.is_some() || has_workflow_id(&params) {
        generic_binding_from_params(params, connector_slug)?
    } else if has_file_append_path(&params) {
        file_append_binding_from_params(params, connector_slug)?
    } else {
        anyhow::bail!("workflow_binding_create requires action, workflow_id, or file_append_path");
    };
    validate_workflow_binding(&binding)
        .map_err(anyhow::Error::msg)
        .context("invalid workflow binding")?;
    Ok(binding)
}

fn has_file_append_path(params: &WorkflowBindingCreateParams) -> bool {
    params
        .file_append_path
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn has_workflow_id(params: &WorkflowBindingCreateParams) -> bool {
    params
        .workflow_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn file_append_binding_from_params(
    params: WorkflowBindingCreateParams,
    connector_slug: String,
) -> Result<WorkflowBindingSpec> {
    let connection_slug = params.connection_slug.trim();
    if connection_slug.is_empty() {
        anyhow::bail!("connection_slug must not be empty");
    }
    let path = params
        .file_append_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("missing file_append_path")?
        .to_string();
    let slug = params
        .slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| default_file_append_slug(connection_slug, &path));
    let filter = binding_filter_from_params(params.filter, params.pattern);
    let action = serde_json::from_value::<ActionSpec>(json!({
        "type": "file_append",
        "path": path,
        "format": "text",
    }))
    .context("build file_append action")?;
    Ok(WorkflowBindingSpec {
        slug,
        description: params
            .description
            .unwrap_or_else(|| format!("Append {connection_slug} messages to {path}")),
        connection_slug: connection_slug.to_string(),
        connector_slug: Some(connector_slug),
        status: binding_status(params.enabled.unwrap_or(true)),
        filter,
        ignore_filters: params.ignore_filters,
        contact_ids: params.contact_ids,
        classify_prompt: non_empty_string(params.classify_prompt),
        classify_model: non_empty_string(params.classify_model),
        action,
        created_at_ms: puffer_subscriptions::now_ms(),
    })
}

fn generic_binding_from_params(
    params: WorkflowBindingCreateParams,
    connector_slug: String,
) -> Result<WorkflowBindingSpec> {
    let connection_slug = params.connection_slug.trim();
    if connection_slug.is_empty() {
        anyhow::bail!("connection_slug must not be empty");
    }
    let action = parse_binding_action(params.action, params.workflow_id)?;
    let slug = params
        .slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| default_binding_slug(connection_slug, &action));
    Ok(WorkflowBindingSpec {
        slug,
        description: params
            .description
            .unwrap_or_else(|| default_binding_description(connection_slug, &action)),
        connection_slug: connection_slug.to_string(),
        connector_slug: Some(connector_slug),
        status: binding_status(params.enabled.unwrap_or(true)),
        filter: binding_filter_from_params(params.filter, params.pattern),
        ignore_filters: params.ignore_filters,
        contact_ids: params.contact_ids,
        classify_prompt: non_empty_string(params.classify_prompt),
        classify_model: non_empty_string(params.classify_model),
        action,
        created_at_ms: puffer_subscriptions::now_ms(),
    })
}

fn parse_binding_action(
    action: Option<ActionSpec>,
    workflow_id: Option<String>,
) -> Result<ActionSpec> {
    match action {
        Some(action) => Ok(action),
        None => workflow_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|workflow_id| ActionSpec::RunWorkflow {
                workflow_id: workflow_id.to_string(),
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "workflow_binding_create requires action, workflow_id, or file_append_path"
                )
            }),
    }
}

fn binding_filter_from_params(
    filter: Option<FilterSpec>,
    pattern: Option<String>,
) -> Option<FilterSpec> {
    if let Some(filter) = filter {
        return Some(filter);
    }
    pattern
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != ".*")
        .map(|pattern| {
            FilterSpec::Tagged(TaggedFilterSpec::Regex {
                pattern: pattern.to_string(),
                case_insensitive: true,
            })
        })
}

fn binding_status(enabled: bool) -> WorkflowBindingStatus {
    if enabled {
        WorkflowBindingStatus::Enabled
    } else {
        WorkflowBindingStatus::Paused
    }
}

fn default_binding_slug(connection_slug: &str, action: &ActionSpec) -> String {
    match action {
        ActionSpec::RunWorkflow { workflow_id } => {
            format!("run-{connection_slug}-{}", slug_fragment(workflow_id))
        }
        ActionSpec::RunAutomation { automation_id } => {
            format!("run-{connection_slug}-{}", slug_fragment(automation_id))
        }
        _ => format!(
            "binding-{connection_slug}-{}",
            slug_fragment(binding_action_type(action))
        ),
    }
}

fn default_binding_description(connection_slug: &str, action: &ActionSpec) -> String {
    match action {
        ActionSpec::RunWorkflow { workflow_id } => {
            format!("Run workflow {workflow_id} for {connection_slug} messages")
        }
        ActionSpec::RunAutomation { automation_id } => {
            format!("Run automation {automation_id} for {connection_slug} messages")
        }
        ActionSpec::ConnectorAct {
            connector_slug,
            action,
            ..
        } => format!("Run {connector_slug}.{action} for {connection_slug} messages"),
        ActionSpec::ToolCall { tool, .. } => {
            format!("Call tool {tool} for {connection_slug} messages")
        }
        ActionSpec::FileAppend { path, .. } => {
            format!("Append {connection_slug} messages to {path}")
        }
        ActionSpec::SqliteInsert { table, .. } => {
            format!("Insert {connection_slug} messages into {table}")
        }
        ActionSpec::ForwardMessage { target, .. } => {
            format!("Forward {connection_slug} messages to {target}")
        }
        ActionSpec::TriageAgent { .. } => format!("Triage {connection_slug} messages"),
        ActionSpec::Graph { .. } => format!("Run action graph for {connection_slug} messages"),
        ActionSpec::Unknown => format!("Run workflow action for {connection_slug} messages"),
    }
}

fn binding_action_type(action: &ActionSpec) -> &'static str {
    match action {
        ActionSpec::SqliteInsert { .. } => "sqlite_insert",
        ActionSpec::FileAppend { .. } => "file_append",
        ActionSpec::ForwardMessage { .. } => "forward_message",
        ActionSpec::RunWorkflow { .. } => "run_workflow",
        ActionSpec::RunAutomation { .. } => "run_automation",
        ActionSpec::ConnectorAct { .. } => "connector_act",
        ActionSpec::ToolCall { .. } => "tool_call",
        ActionSpec::TriageAgent { .. } => "triage_agent",
        ActionSpec::Graph { .. } => "graph",
        ActionSpec::Unknown => "unknown",
    }
}

fn default_file_append_slug(connection_slug: &str, path: &str) -> String {
    let path_leaf = path
        .rsplit('/')
        .find(|part| !part.trim().is_empty())
        .unwrap_or("events");
    format!("append-{connection_slug}-{}", slug_fragment(path_leaf))
}

fn slug_fragment(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            slug.push(ch);
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
        if slug.len() >= 48 {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "events".to_string()
    } else {
        slug
    }
}

fn non_empty_string(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use puffer_subscriptions::{ConnectionState, SubscriptionManagerBuilder};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    #[test]
    fn old_file_append_request_still_builds_file_append_binding() {
        let params = serde_json::from_value::<WorkflowBindingCreateParams>(json!({
            "slug": "append-telegram-user-hi",
            "connection_slug": "telegram-user",
            "connector_slug": "telegram-login",
            "pattern": "hi",
            "file_append_path": "/tmp/hi",
            "enabled": true
        }))
        .unwrap();

        let binding = binding_from_params(params, "telegram-login".to_string()).unwrap();

        assert_eq!(binding.slug, "append-telegram-user-hi");
        assert_eq!(
            binding.description,
            "Append telegram-user messages to /tmp/hi"
        );
        assert_eq!(binding.connection_slug, "telegram-user");
        assert_eq!(binding.connector_slug.as_deref(), Some("telegram-login"));
        assert_eq!(binding.status, WorkflowBindingStatus::Enabled);
        assert!(matches!(
            binding.filter,
            Some(FilterSpec::Tagged(TaggedFilterSpec::Regex {
                ref pattern,
                case_insensitive: true
            })) if pattern == "hi"
        ));
        assert!(matches!(
            binding.action,
            ActionSpec::FileAppend {
                ref path,
                ..
            } if path == "/tmp/hi"
        ));
    }

    #[test]
    fn new_run_workflow_request_builds_generic_binding() {
        let params = serde_json::from_value::<WorkflowBindingCreateParams>(json!({
            "connection_slug": "telegram-user",
            "pattern": "old-filter",
            "filter": {
                "type": "jq",
                "expression": ".topic == \"deploy\""
            },
            "ignore_filters": [{
                "type": "regex",
                "pattern": "ignore me",
                "case_insensitive": false
            }],
            "contact_ids": ["telegram@alice"],
            "classify_prompt": "Only route deployment requests.",
            "classify_model": "openai/gpt-5.4",
            "action": {
                "type": "run_workflow",
                "slug": "deploy-followup"
            }
        }))
        .unwrap();

        let binding = binding_from_params(params, "telegram-login".to_string()).unwrap();

        assert_eq!(binding.slug, "run-telegram-user-deploy-followup");
        assert_eq!(
            binding.description,
            "Run workflow deploy-followup for telegram-user messages"
        );
        assert_eq!(binding.contact_ids, ["telegram@alice"]);
        assert_eq!(
            binding.classify_prompt.as_deref(),
            Some("Only route deployment requests.")
        );
        assert_eq!(binding.classify_model.as_deref(), Some("openai/gpt-5.4"));
        assert!(matches!(
            binding.filter,
            Some(FilterSpec::Tagged(TaggedFilterSpec::Jq { ref expression }))
                if expression == ".topic == \"deploy\""
        ));
        assert_eq!(binding.ignore_filters.len(), 1);
        assert!(matches!(
            binding.action,
            ActionSpec::RunWorkflow { ref workflow_id } if workflow_id == "deploy-followup"
        ));
    }

    #[test]
    fn new_filter_request_deserializes_directly() {
        let params = serde_json::from_value::<WorkflowBindingCreateParams>(json!({
            "connection_slug": "telegram-user",
            "filter": {
                "type": "regex",
                "pattern": "ship",
                "case_insensitive": false
            },
            "action": {
                "type": "run_workflow",
                "slug": "deploy-followup"
            }
        }))
        .unwrap();

        let binding = binding_from_params(params, "telegram-login".to_string()).unwrap();

        assert!(matches!(
            binding.filter,
            Some(FilterSpec::Tagged(TaggedFilterSpec::Regex {
                ref pattern,
                case_insensitive: false
            })) if pattern == "ship"
        ));
    }

    #[test]
    fn new_connector_act_request_uses_action_even_with_file_append_path() {
        let params = serde_json::from_value::<WorkflowBindingCreateParams>(json!({
            "slug": "send-telegram-reply",
            "connection_slug": "telegram-user",
            "file_append_path": "/tmp/ignored",
            "pattern": "ship",
            "enabled": false,
            "action": {
                "type": "connector_act",
                "connector_slug": "telegram-login",
                "action": "send_message",
                "input": {
                    "chat_id": "{{payload.chat_id}}",
                    "text": "Seen: {{text}}"
                }
            }
        }))
        .unwrap();

        let binding = binding_from_params(params, "telegram-login".to_string()).unwrap();

        assert_eq!(binding.slug, "send-telegram-reply");
        assert_eq!(binding.status, WorkflowBindingStatus::Paused);
        assert!(matches!(
            binding.filter,
            Some(FilterSpec::Tagged(TaggedFilterSpec::Regex {
                ref pattern,
                case_insensitive: true
            })) if pattern == "ship"
        ));
        let ActionSpec::ConnectorAct {
            connector_slug,
            action,
            input,
        } = binding.action
        else {
            panic!("expected connector_act action");
        };
        assert_eq!(connector_slug, "telegram-login");
        assert_eq!(action, "send_message");
        assert_eq!(input["chat_id"], json!("{{payload.chat_id}}"));
        assert_eq!(input["text"], json!("Seen: {{text}}"));
    }

    #[test]
    fn generic_binding_accepts_agentenv_runtime_workflow_id() {
        let params = serde_json::from_value::<WorkflowBindingCreateParams>(json!({
            "connection_slug": "telegram-user",
            "action": {
                "type": "run_workflow",
                "slug": "Wf_01HX.Runtime:123"
            }
        }))
        .unwrap();

        let binding = binding_from_params(params, "telegram-login".to_string()).unwrap();

        assert!(matches!(
            binding.action,
            ActionSpec::RunWorkflow { ref workflow_id } if workflow_id == "Wf_01HX.Runtime:123"
        ));
    }

    #[test]
    fn workflow_id_request_builds_run_workflow_binding_without_action_shape() {
        let params = serde_json::from_value::<WorkflowBindingCreateParams>(json!({
            "connection_slug": "telegram-user",
            "workflowId": "Wf_01HX.Runtime:123",
            "pattern": "ship"
        }))
        .unwrap();

        let binding = binding_from_params(params, "telegram-login".to_string()).unwrap();

        assert_eq!(binding.slug, "run-telegram-user-wf-01hx-runtime-123");
        assert_eq!(
            binding.description,
            "Run workflow Wf_01HX.Runtime:123 for telegram-user messages"
        );
        assert!(matches!(
            binding.filter,
            Some(FilterSpec::Tagged(TaggedFilterSpec::Regex {
                ref pattern,
                case_insensitive: true
            })) if pattern == "ship"
        ));
        assert!(matches!(
            binding.action,
            ActionSpec::RunWorkflow { ref workflow_id } if workflow_id == "Wf_01HX.Runtime:123"
        ));
    }

    #[test]
    fn generic_binding_rejects_invalid_runtime_workflow_id() {
        let params = serde_json::from_value::<WorkflowBindingCreateParams>(json!({
            "connection_slug": "telegram-user",
            "action": {
                "type": "run_workflow",
                "slug": "Bad Slug"
            }
        }))
        .unwrap();

        let error = binding_from_params(params, "telegram-login".to_string()).unwrap_err();

        assert!(format!("{error:#}").contains("AgentEnv workflow runtime id"));
    }

    #[test]
    fn missing_action_and_file_append_path_errors_clearly() {
        let params = serde_json::from_value::<WorkflowBindingCreateParams>(json!({
            "connection_slug": "telegram-user"
        }))
        .unwrap();

        let error = binding_from_params(params, "telegram-login".to_string()).unwrap_err();

        assert!(error
            .to_string()
            .contains("requires action, workflow_id, or file_append_path"));
    }

    #[test]
    fn save_enabled_binding_refreshes_connection_consumer() {
        let tempdir = tempfile::tempdir().unwrap();
        let paths = test_paths(tempdir.path());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .unwrap();
        let manager = SubscriptionManagerBuilder::new(tempdir.path().join("subscriptions.json"))
            .build(runtime.handle().clone())
            .unwrap();
        manager
            .connection_store()
            .create(ConnectionRecord::authenticated(
                "chat",
                "test-connector",
                "Test chat",
            ))
            .unwrap();
        let connection = manager.connection_store().get("chat").unwrap();
        let template = plain_template("test-connector");

        save_workflow_binding(
            &manager,
            &paths,
            test_binding("run-chat-demo", WorkflowBindingStatus::Enabled),
            Some(&connection),
            &template,
        )
        .unwrap();

        let updated = manager.connection_store().get("chat").unwrap();
        assert!(updated.has_consumer);
        assert_eq!(updated.state, ConnectionState::Active);
        manager.shutdown();
    }

    #[test]
    fn save_paused_binding_does_not_start_command_stream() {
        let tempdir = tempfile::tempdir().unwrap();
        let paths = test_paths(tempdir.path());
        let (script, log) = write_stream_logger(tempdir.path());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .unwrap();
        let manager = SubscriptionManagerBuilder::new(tempdir.path().join("subscriptions.json"))
            .build(runtime.handle().clone())
            .unwrap();
        let template = stream_template("test-connector", &script, &log);
        manager.connector_store().upsert(template.clone()).unwrap();
        manager
            .connection_store()
            .create(ConnectionRecord::authenticated(
                "chat",
                "test-connector",
                "Test chat",
            ))
            .unwrap();
        let connection = manager.connection_store().get("chat").unwrap();

        save_workflow_binding(
            &manager,
            &paths,
            test_binding("run-chat-demo", WorkflowBindingStatus::Paused),
            Some(&connection),
            &template,
        )
        .unwrap();
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(50)).await });

        let updated = manager.connection_store().get("chat").unwrap();
        assert!(!updated.has_consumer);
        assert_eq!(read_stream_log(&log), Vec::<String>::new());
        manager.shutdown();
    }

    #[test]
    fn setup_failure_deletes_new_binding_and_refreshes_connection() {
        let tempdir = tempfile::tempdir().unwrap();
        let paths = test_paths(tempdir.path());
        write_failing_subscriber_manifest(&paths, "chat", tempdir.path());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .unwrap();
        let manager = SubscriptionManagerBuilder::new(tempdir.path().join("subscriptions.json"))
            .build(runtime.handle().clone())
            .unwrap();
        manager
            .connection_store()
            .create(ConnectionRecord::authenticated(
                "chat",
                "test-connector",
                "Test chat",
            ))
            .unwrap();
        let connection = manager.connection_store().get("chat").unwrap();
        let template = subscriber_template("test-connector");

        let error = save_workflow_binding(
            &manager,
            &paths,
            test_binding("run-chat-demo", WorkflowBindingStatus::Enabled),
            Some(&connection),
            &template,
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("workflow binding `run-chat-demo` setup failed"));
        assert!(manager.store().get("run-chat-demo").is_none());
        let updated = manager.connection_store().get("chat").unwrap();
        assert!(!updated.has_consumer);
        assert_eq!(updated.state, ConnectionState::Authenticated);
        manager.shutdown();
    }

    #[test]
    fn setup_failure_restores_old_binding_and_refreshes_connection() {
        let tempdir = tempfile::tempdir().unwrap();
        let paths = test_paths(tempdir.path());
        write_failing_subscriber_manifest(&paths, "chat", tempdir.path());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .unwrap();
        let manager = SubscriptionManagerBuilder::new(tempdir.path().join("subscriptions.json"))
            .build(runtime.handle().clone())
            .unwrap();
        manager
            .connection_store()
            .create(ConnectionRecord::authenticated(
                "chat",
                "test-connector",
                "Test chat",
            ))
            .unwrap();
        let previous = test_binding("run-chat-demo", WorkflowBindingStatus::Enabled);
        manager.store().upsert(previous.clone()).unwrap();
        manager.refresh_connection_consumers().unwrap();
        let connection = manager.connection_store().get("chat").unwrap();
        assert!(connection.has_consumer);
        let template = subscriber_template("test-connector");
        let mut replacement = test_binding("run-chat-demo", WorkflowBindingStatus::Enabled);
        replacement.description = "replacement".to_string();

        let error =
            save_workflow_binding(&manager, &paths, replacement, Some(&connection), &template)
                .unwrap_err();

        assert!(format!("{error:#}").contains("workflow binding `run-chat-demo` setup failed"));
        let restored = manager.store().get("run-chat-demo").unwrap();
        assert_eq!(restored.description, previous.description);
        let updated = manager.connection_store().get("chat").unwrap();
        assert!(updated.has_consumer);
        assert_eq!(updated.state, ConnectionState::Active);
        manager.shutdown();
    }

    fn test_paths(root: &Path) -> ConfigPaths {
        ConfigPaths {
            workspace_root: root.to_path_buf(),
            workspace_config_dir: root.join(".puffer"),
            user_config_dir: root.join("home").join(".puffer"),
            builtin_resources_dir: root.join("resources"),
        }
    }

    fn plain_template(slug: &str) -> ConnectorTemplate {
        ConnectorTemplate {
            slug: slug.to_string(),
            description: "Test connector".to_string(),
            skill: "test".to_string(),
            binary: "test".to_string(),
            command: Vec::new(),
            requires_auth: false,
            can_subscribe: false,
            can_proxy_agent: false,
            subscriber: None,
            output_schema: Value::Null,
            actions: BTreeMap::new(),
        }
    }

    fn subscriber_template(slug: &str) -> ConnectorTemplate {
        ConnectorTemplate {
            can_subscribe: true,
            ..plain_template(slug)
        }
    }

    fn stream_template(slug: &str, script: &Path, log: &Path) -> ConnectorTemplate {
        ConnectorTemplate {
            command: vec![
                "sh".to_string(),
                script.display().to_string(),
                log.display().to_string(),
            ],
            can_subscribe: true,
            ..plain_template(slug)
        }
    }

    fn test_binding(slug: &str, status: WorkflowBindingStatus) -> WorkflowBindingSpec {
        WorkflowBindingSpec {
            slug: slug.to_string(),
            description: "test binding".to_string(),
            connection_slug: "chat".to_string(),
            connector_slug: Some("test-connector".to_string()),
            status,
            filter: None,
            ignore_filters: Vec::new(),
            contact_ids: Vec::new(),
            classify_prompt: None,
            classify_model: None,
            action: ActionSpec::RunWorkflow {
                workflow_id: "demo".to_string(),
            },
            created_at_ms: 0,
        }
    }

    fn write_stream_logger(dir: &Path) -> (PathBuf, PathBuf) {
        let script = dir.join("stream.sh");
        let log = dir.join("subscribes.ndjson");
        fs::write(
            &script,
            r#"log="$1"
IFS= read -r line || exit 0
printf '%s\n' "$line" >> "$log"
while IFS= read -r _line; do
  :
done
"#,
        )
        .unwrap();
        (script, log)
    }

    fn read_stream_log(path: &Path) -> Vec<String> {
        fs::read_to_string(path)
            .ok()
            .map(|raw| raw.lines().map(ToOwned::to_owned).collect())
            .unwrap_or_default()
    }

    fn write_failing_subscriber_manifest(paths: &ConfigPaths, topic: &str, root: &Path) {
        let manifest_dir = paths.workspace_config_dir.join("subscribers").join(topic);
        fs::create_dir_all(&manifest_dir).unwrap();
        let missing_binary = root.join("missing-subscriber-binary");
        fs::write(
            manifest_dir.join("manifest.toml"),
            format!(
                r#"manifest_version = 1
id = "{topic}"
kind = "subscriber"
topic = "{topic}"

[run]
cmd = ["{}"]
"#,
                missing_binary.display()
            ),
        )
        .unwrap();
    }
}
