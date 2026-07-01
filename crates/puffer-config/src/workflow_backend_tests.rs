use super::*;
use std::fs;
use tempfile::TempDir;

fn temp_paths(temp: &TempDir) -> ConfigPaths {
    let workspace_root = temp.path().join("workspace");
    ConfigPaths {
        workspace_root: workspace_root.clone(),
        workspace_config_dir: workspace_root.join(".puffer"),
        user_config_dir: temp.path().join("home").join(".puffer"),
        builtin_resources_dir: workspace_root.join("resources"),
    }
}

#[test]
fn workflow_backend_defaults_to_local_runtime_urls() {
    let backend = WorkflowBackendConfig::default();

    assert_eq!(backend.mode, WorkflowBackendMode::Local);
    assert_eq!(backend.api_base_url, "http://127.0.0.1:3000");
    assert_eq!(backend.frontend_url, "http://localhost:5173");
    assert!(backend.workspace_id.is_empty());
    assert!(backend.api_token_secret_id.is_empty());
}

#[test]
fn save_user_config_normalizes_workflow_backend_urls() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = temp_paths(&temp);
    ensure_workspace_dirs(&paths).expect("workspace dirs");
    let mut config = PufferConfig::default();
    config.workflow_backend = WorkflowBackendConfig {
        mode: WorkflowBackendMode::AgentEnvCloud,
        api_base_url: "https://api.agentenv.io/v1/workflows".to_string(),
        frontend_url: "https://agentenv.io/".to_string(),
        workspace_id: "  workspace-123  ".to_string(),
        api_token_secret_id: "  sec_workflow  ".to_string(),
    };

    save_user_config(&paths, &config).expect("save config");

    let raw = fs::read_to_string(paths.user_config_file()).expect("read config");
    assert!(raw.contains("https://api.agentenv.io"));
    assert!(!raw.contains("/v1"));

    let loaded = load_config(&paths).expect("load config");
    assert_eq!(
        loaded.workflow_backend.mode,
        WorkflowBackendMode::AgentEnvCloud
    );
    assert_eq!(
        loaded.workflow_backend.api_base_url,
        "https://api.agentenv.io"
    );
    assert_eq!(loaded.workflow_backend.frontend_url, "https://agentenv.io");
    assert_eq!(loaded.workflow_backend.workspace_id, "workspace-123");
    assert_eq!(loaded.workflow_backend.api_token_secret_id, "sec_workflow");
}

#[test]
fn load_config_preserves_user_workflow_backend_over_workspace_defaults() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = temp_paths(&temp);
    ensure_workspace_dirs(&paths).expect("workspace dirs");

    let mut user = PufferConfig::default();
    user.workflow_backend = WorkflowBackendConfig {
        mode: WorkflowBackendMode::AgentEnvCloud,
        api_base_url: "https://api.agentenv.io".to_string(),
        frontend_url: "https://agentenv.io".to_string(),
        workspace_id: "workspace-123".to_string(),
        api_token_secret_id: "sec_workflow".to_string(),
    };
    save_user_config(&paths, &user).expect("save user config");

    let mut workspace = PufferConfig::default();
    workspace.workflow_backend = WorkflowBackendConfig::default();
    save_workspace_config(&paths, &workspace).expect("save workspace config");

    let loaded = load_config(&paths).expect("load config");
    assert_eq!(loaded.workflow_backend, user.workflow_backend);
}
