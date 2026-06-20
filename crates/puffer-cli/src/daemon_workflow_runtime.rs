use anyhow::{Context, Result};
use puffer_config::{ConfigPaths, PufferConfig, WorkflowBackendMode};
use puffer_core::{blocking_client_for_url, HttpPurpose};
use puffer_secrets::SecretVault;
use puffer_workflow::{
    WorkflowRuntimeClient, WorkflowRuntimeClientConfig, WorkflowRuntimeConnectionStep,
    WorkflowRuntimeConnectionStepState, WorkflowRuntimeConnectionTest, WorkflowRuntimeError,
    WorkflowRuntimeErrorKind,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use url::Url;

use crate::daemon::DaemonState;
use crate::daemon_workflow_backend_settings::save_workflow_backend_settings;
use crate::desktop_api::workflow_backend_settings_dto;
use crate::desktop_api_types::SaveWorkflowBackendSettingsParams;

const WORKFLOW_RUNTIME_TIMEOUT: Duration = Duration::from_secs(15);

/// Returns the redacted workflow backend config for desktop callers.
pub(crate) fn handle_workflow_backend_get_config(state: &DaemonState) -> Result<Value> {
    let config = state.config_snapshot();
    workflow_backend_config_value(state.config_paths(), &config)
}

/// Saves workflow backend config and returns the redacted post-save snapshot.
pub(crate) fn handle_workflow_backend_save_config(
    state: &DaemonState,
    params: &Value,
) -> Result<Value> {
    let input: SaveWorkflowBackendSettingsParams =
        serde_json::from_value(params.clone()).context("invalid workflow backend config")?;
    let mut config = state.config_snapshot();
    let response = save_workflow_backend_config_value(state.config_paths(), &mut config, input)?;
    state.replace_config(config);
    Ok(response)
}

/// Runs the workflow backend connection test using saved config.
pub(crate) fn handle_workflow_backend_test_connection(state: &DaemonState) -> Result<Value> {
    let config = state.config_snapshot();
    workflow_backend_test_connection_value(state.config_paths(), &config)
}

/// Opens the configured AgentEnv workflow console in the system browser.
pub(crate) fn handle_workflow_open_ui(state: &DaemonState) -> Result<Value> {
    let config = state.config_snapshot();
    workflow_open_ui_value(&config, true)
}

#[derive(Debug, Clone)]
struct ResolvedWorkflowRuntimeConfig {
    api_base_url: String,
    api_token: String,
    workspace_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowBackendConnectionTestDto {
    success: bool,
    runtime: WorkflowBackendConnectionCheckDto,
    auth: WorkflowBackendConnectionCheckDto,
    workspace: WorkflowBackendConnectionCheckDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowBackendConnectionCheckDto {
    state: WorkflowRuntimeConnectionStepState,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<WorkflowRuntimeErrorDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowRuntimeErrorDto {
    kind: WorkflowRuntimeErrorKind,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status_code: Option<u16>,
}

impl From<WorkflowRuntimeConnectionTest> for WorkflowBackendConnectionTestDto {
    fn from(value: WorkflowRuntimeConnectionTest) -> Self {
        let runtime = runtime_check(&value.api_surface);
        let auth = auth_check(&value.api_surface);
        let workspace = workspace_check(&value.workspace_access);
        Self {
            success: runtime.state == WorkflowRuntimeConnectionStepState::Passed
                && auth.state == WorkflowRuntimeConnectionStepState::Passed
                && workspace.state == WorkflowRuntimeConnectionStepState::Passed,
            runtime,
            auth,
            workspace,
        }
    }
}

impl From<WorkflowRuntimeError> for WorkflowRuntimeErrorDto {
    fn from(value: WorkflowRuntimeError) -> Self {
        Self {
            kind: value.kind,
            message: value.message,
            status_code: value.status_code,
        }
    }
}

fn runtime_check(step: &WorkflowRuntimeConnectionStep) -> WorkflowBackendConnectionCheckDto {
    match step.state {
        WorkflowRuntimeConnectionStepState::Passed => WorkflowBackendConnectionCheckDto {
            state: WorkflowRuntimeConnectionStepState::Passed,
            message: "Workflow runtime API is reachable.".to_string(),
            error: None,
        },
        WorkflowRuntimeConnectionStepState::Failed => {
            let auth_failure = step
                .error
                .as_ref()
                .is_some_and(|error| auth_error_kind(error.kind));
            if auth_failure {
                WorkflowBackendConnectionCheckDto {
                    state: WorkflowRuntimeConnectionStepState::Passed,
                    message: "Workflow runtime API is reachable.".to_string(),
                    error: None,
                }
            } else {
                failed_check("Unable to reach workflow runtime.", step)
            }
        }
        WorkflowRuntimeConnectionStepState::Skipped => skipped_check(
            step.message
                .as_deref()
                .unwrap_or("Skipped because workflow runtime was not checked."),
        ),
    }
}

fn auth_check(step: &WorkflowRuntimeConnectionStep) -> WorkflowBackendConnectionCheckDto {
    match step.state {
        WorkflowRuntimeConnectionStepState::Passed => WorkflowBackendConnectionCheckDto {
            state: WorkflowRuntimeConnectionStepState::Passed,
            message: "Workflow runtime token is accepted.".to_string(),
            error: None,
        },
        WorkflowRuntimeConnectionStepState::Failed => {
            let auth_failure = step
                .error
                .as_ref()
                .is_some_and(|error| auth_error_kind(error.kind));
            if auth_failure {
                failed_check("Workflow runtime authentication failed.", step)
            } else {
                skipped_check("Skipped because workflow runtime was not reachable.")
            }
        }
        WorkflowRuntimeConnectionStepState::Skipped => skipped_check(
            step.message
                .as_deref()
                .unwrap_or("Skipped because workflow runtime was not checked."),
        ),
    }
}

fn workspace_check(step: &WorkflowRuntimeConnectionStep) -> WorkflowBackendConnectionCheckDto {
    match step.state {
        WorkflowRuntimeConnectionStepState::Passed => WorkflowBackendConnectionCheckDto {
            state: WorkflowRuntimeConnectionStepState::Passed,
            message: "Workflow workspace is accessible.".to_string(),
            error: None,
        },
        WorkflowRuntimeConnectionStepState::Failed => {
            failed_check("Workflow workspace access failed.", step)
        }
        WorkflowRuntimeConnectionStepState::Skipped => skipped_check(
            step.message
                .as_deref()
                .unwrap_or("Skipped because authentication did not pass."),
        ),
    }
}

fn failed_check(
    fallback: &str,
    step: &WorkflowRuntimeConnectionStep,
) -> WorkflowBackendConnectionCheckDto {
    WorkflowBackendConnectionCheckDto {
        state: WorkflowRuntimeConnectionStepState::Failed,
        message: step
            .error
            .as_ref()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| fallback.to_string()),
        error: step.error.clone().map(Into::into),
    }
}

fn skipped_check(message: &str) -> WorkflowBackendConnectionCheckDto {
    WorkflowBackendConnectionCheckDto {
        state: WorkflowRuntimeConnectionStepState::Skipped,
        message: message.to_string(),
        error: None,
    }
}

fn auth_error_kind(kind: WorkflowRuntimeErrorKind) -> bool {
    matches!(
        kind,
        WorkflowRuntimeErrorKind::InvalidToken | WorkflowRuntimeErrorKind::PermissionDenied
    )
}

fn workflow_backend_config_value(paths: &ConfigPaths, config: &PufferConfig) -> Result<Value> {
    Ok(serde_json::to_value(workflow_backend_settings_dto(
        paths, config,
    )?)?)
}

fn save_workflow_backend_config_value(
    paths: &ConfigPaths,
    config: &mut PufferConfig,
    input: SaveWorkflowBackendSettingsParams,
) -> Result<Value> {
    save_workflow_backend_settings(paths, config, input)?;
    workflow_backend_config_value(paths, config)
}

fn workflow_backend_test_connection_value(
    paths: &ConfigPaths,
    config: &PufferConfig,
) -> Result<Value> {
    let client = workflow_runtime_client(paths, config)?;
    let report = client.test_connection();
    Ok(serde_json::to_value(
        WorkflowBackendConnectionTestDto::from(report),
    )?)
}

fn workflow_open_ui_value(config: &PufferConfig, open_in_browser: bool) -> Result<Value> {
    let frontend_url = config.workflow_backend.frontend_url.trim();
    if frontend_url.is_empty() {
        anyhow::bail!("workflow backend UI URL is not configured");
    }
    let url = workflow_ui_url(frontend_url)?;
    let opened = open_in_browser && crate::authflow::open_browser(&url);
    Ok(json!({
        "url": url,
        "opened": opened,
    }))
}

pub(crate) fn workflow_runtime_client(
    paths: &ConfigPaths,
    config: &PufferConfig,
) -> Result<WorkflowRuntimeClient> {
    let resolved = resolve_workflow_runtime_config(paths, config)?;
    let http_client = blocking_client_for_url(
        &config.network.proxy,
        HttpPurpose::Discovery,
        &resolved.api_base_url,
        WORKFLOW_RUNTIME_TIMEOUT,
    )
    .context("build workflow runtime HTTP client")?;
    WorkflowRuntimeClient::with_client(
        WorkflowRuntimeClientConfig::new(
            resolved.api_base_url,
            resolved.api_token,
            resolved.workspace_id,
        )
        .with_timeout(WORKFLOW_RUNTIME_TIMEOUT),
        http_client,
    )
    .context("create workflow runtime client")
}

fn resolve_workflow_runtime_config(
    paths: &ConfigPaths,
    config: &PufferConfig,
) -> Result<ResolvedWorkflowRuntimeConfig> {
    let mut backend = config.workflow_backend.clone();
    backend.normalize();
    Ok(ResolvedWorkflowRuntimeConfig {
        api_token: resolve_workflow_runtime_token(
            paths,
            backend.mode,
            &backend.api_token_secret_id,
        )?,
        api_base_url: backend.api_base_url,
        workspace_id: backend.workspace_id,
    })
}

fn resolve_workflow_runtime_token(
    paths: &ConfigPaths,
    mode: WorkflowBackendMode,
    secret_id: &str,
) -> Result<String> {
    let secret_id = required_trimmed("workflow runtime token secret id", secret_id)
        .map_err(|_| anyhow::anyhow!("{} token is not configured", workflow_runtime_name(mode)))?;
    let vault = SecretVault::open(SecretVault::default_path(&paths.user_config_dir))
        .context("open encrypted secret store")?;
    let resolved = vault.reveal(&secret_id).with_context(|| {
        format!(
            "load {} token from secret store",
            workflow_runtime_name(mode)
        )
    })?;
    Ok(resolved.value)
}

fn workflow_ui_url(frontend_url: &str) -> Result<String> {
    let mut url = normalized_frontend_url(frontend_url)?;
    let already_workflows = url
        .path_segments()
        .and_then(|segments| segments.filter(|segment| !segment.is_empty()).last())
        == Some("workflows");
    let mut segments = url
        .path_segments_mut()
        .map_err(|_| anyhow::anyhow!("workflow backend UI URL must be hierarchical"))?;
    segments.pop_if_empty();
    if !already_workflows {
        segments.push("workflows");
    }
    drop(segments);
    Ok(url.to_string())
}

fn normalized_frontend_url(value: &str) -> Result<Url> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("workflow backend UI URL is not configured");
    }
    let mut parsed = Url::parse(trimmed).context("workflow backend UI URL must be a valid URL")?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            anyhow::bail!("workflow backend UI URL must use http or https, got `{other}`")
        }
    }
    parsed.set_query(None);
    parsed.set_fragment(None);
    Ok(parsed)
}

fn workflow_runtime_name(mode: WorkflowBackendMode) -> &'static str {
    match mode {
        WorkflowBackendMode::Local => "workflow runtime",
        WorkflowBackendMode::AgentEnvCloud => "AgentEnv Cloud workflow runtime",
    }
}

fn required_trimmed(label: &str, value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("missing {label}");
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon_workflow_backend_settings::test_support::{
        lock_secret_store, temp_paths, ScopedSecretStoreKey,
    };
    use puffer_config::{ensure_workspace_dirs, load_config, WorkflowBackendConfig};
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;

    // Config persists across a save/reload and the token never lands in config,
    // the snapshot, or the RPC response (only `hasToken` is reported).
    #[test]
    fn workflow_backend_config_save_and_get_mask_token() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = temp_paths(&temp);
        ensure_workspace_dirs(&paths).expect("workspace dirs");
        let mut config = PufferConfig::default();

        let saved = save_workflow_backend_config_value(
            &paths,
            &mut config,
            SaveWorkflowBackendSettingsParams {
                mode: WorkflowBackendMode::AgentEnvCloud,
                api_url: "https://api.agentenv.io/v1/workflows".to_string(),
                ui_url: "https://agentenv.io/console/".to_string(),
                workspace_id: " workspace-123 ".to_string(),
                api_token: Some("super-secret-token".to_string()),
                keep_token: false,
            },
        )
        .expect("save config");

        assert_eq!(saved["mode"], "agent_env_cloud");
        assert_eq!(saved["apiUrl"], "https://api.agentenv.io");
        assert_eq!(saved["uiUrl"], "https://agentenv.io/console");
        assert_eq!(saved["workspaceId"], "workspace-123");
        assert_eq!(saved["hasToken"], true);
        assert!(saved.get("apiToken").is_none());

        let raw_config = fs::read_to_string(paths.user_config_file()).expect("read user config");
        assert!(!raw_config.contains("super-secret-token"));
        assert!(!raw_config.contains("\"apiToken\""));
        assert!(raw_config.contains("api_token_secret_id"));

        let loaded = load_config(&paths).expect("load config");
        let fetched = workflow_backend_config_value(&paths, &loaded).expect("get config");
        let serialized = serde_json::to_string(&fetched).expect("serialize config");
        assert_eq!(fetched["hasToken"], true);
        assert!(!serialized.contains("super-secret-token"));
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let read = stream.read(&mut buffer).expect("read request");
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8(bytes).expect("request utf8")
    }

    fn write_json_response(stream: &mut TcpStream, body: &str) {
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    }

    fn spawn_runtime_server() -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test runtime");
        let url = format!("http://{}", listener.local_addr().expect("local addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            for (index, stream) in listener.incoming().take(2).enumerate() {
                let mut stream = stream.expect("accept connection");
                let request = read_request(&mut stream);
                captured.lock().expect("requests lock").push(request);
                let body = if index == 0 {
                    r#"{"data":[{"id":"node-a"},{"id":"node-b"}]}"#
                } else {
                    r#"{"data":[{"id":"workflow-a"}]}"#
                };
                write_json_response(&mut stream, body);
            }
        });
        (url, requests, handle)
    }

    // The daemon test uses a real local HTTP listener so it validates the
    // saved secret, reqwest client wiring, runtime paths, and headers together.
    #[test]
    fn workflow_backend_test_connection_uses_runtime_client_wiring() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = temp_paths(&temp);
        ensure_workspace_dirs(&paths).expect("workspace dirs");
        let (api_url, requests, handle) = spawn_runtime_server();
        let mut config = PufferConfig::default();

        save_workflow_backend_settings(
            &paths,
            &mut config,
            SaveWorkflowBackendSettingsParams {
                mode: WorkflowBackendMode::Local,
                api_url,
                ui_url: "http://localhost:5173".to_string(),
                workspace_id: "workspace-local".to_string(),
                api_token: Some("runtime-token".to_string()),
                keep_token: false,
            },
        )
        .expect("save backend settings");

        let response =
            workflow_backend_test_connection_value(&paths, &config).expect("test connection");
        handle.join().expect("runtime server joined");

        assert_eq!(response["success"], true);
        assert_eq!(response["runtime"]["state"], "passed");
        assert_eq!(response["auth"]["state"], "passed");
        assert_eq!(response["workspace"]["state"], "passed");

        let captured = requests.lock().expect("requests lock");
        assert_eq!(captured.len(), 2);
        assert!(captured[0].starts_with("GET /v1/workflows/node-definitions "));
        assert!(captured[0]
            .to_ascii_lowercase()
            .contains("x-api-key: runtime-token"));
        assert!(!captured[0].to_ascii_lowercase().contains("x-workspace-id"));
        assert!(captured[1].starts_with("GET /v1/workflows "));
        assert!(captured[1]
            .to_ascii_lowercase()
            .contains("x-workspace-id: workspace-local"));
    }

    // A missing token surfaces a clear, mode-named error instead of falling back
    // to any default or environment value.
    #[test]
    fn missing_token_reports_clear_error_without_fallback() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = temp_paths(&temp);
        ensure_workspace_dirs(&paths).expect("workspace dirs");
        let mut config = PufferConfig::default();
        config.workflow_backend = WorkflowBackendConfig {
            mode: WorkflowBackendMode::AgentEnvCloud,
            api_base_url: "https://api.agentenv.io".to_string(),
            frontend_url: "https://agentenv.io".to_string(),
            workspace_id: "workspace-123".to_string(),
            api_token_secret_id: String::new(),
        };

        let error = workflow_backend_test_connection_value(&paths, &config)
            .expect_err("missing token should error");
        assert!(error
            .to_string()
            .contains("AgentEnv Cloud workflow runtime token is not configured"));
    }

    // `open_ui` normalizes the configured UI URL before appending the stable
    // first-phase Workflow Console path.
    #[test]
    fn workflow_open_ui_normalizes_url_paths() {
        let mut config = PufferConfig::default();
        config.workflow_backend = WorkflowBackendConfig {
            mode: WorkflowBackendMode::AgentEnvCloud,
            api_base_url: "https://api.agentenv.io".to_string(),
            frontend_url: "https://agentenv.io/runtime/".to_string(),
            workspace_id: "workspace-123".to_string(),
            api_token_secret_id: "sec_runtime".to_string(),
        };

        let overview = workflow_open_ui_value(&config, false).expect("overview URL");

        assert_eq!(overview["url"], "https://agentenv.io/runtime/workflows");
        assert_eq!(overview["opened"], false);
    }

    #[test]
    fn workflow_open_ui_does_not_duplicate_workflows_path() {
        let mut config = PufferConfig::default();
        config.workflow_backend = WorkflowBackendConfig {
            mode: WorkflowBackendMode::Local,
            api_base_url: "http://127.0.0.1:3000".to_string(),
            frontend_url: "http://localhost:5173/workflows/".to_string(),
            workspace_id: String::new(),
            api_token_secret_id: "sec_runtime".to_string(),
        };

        let overview = workflow_open_ui_value(&config, false).expect("overview URL");

        assert_eq!(overview["url"], "http://localhost:5173/workflows");
    }

    #[test]
    fn workflow_open_ui_requires_configured_ui_url() {
        let mut config = PufferConfig::default();
        config.workflow_backend.frontend_url = String::new();

        let error = workflow_open_ui_value(&config, false).expect_err("missing UI URL");

        assert!(error
            .to_string()
            .contains("workflow backend UI URL is not configured"));
    }
}
