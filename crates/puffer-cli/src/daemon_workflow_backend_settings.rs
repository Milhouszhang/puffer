use anyhow::{Context, Result};
use puffer_config::{save_user_config, ConfigPaths, PufferConfig, WorkflowBackendConfig};
use puffer_secrets::{SecretUpsert, SecretVault};
use url::Url;

use crate::desktop_api_types::SaveWorkflowBackendSettingsParams;

const WORKFLOW_RUNTIME_TOKEN_LABEL: &str = "Workflow runtime API token";
const WORKFLOW_RUNTIME_TOKEN_DESCRIPTION: &str = "API token for the configured workflow runtime.";

/// Saves workflow runtime settings to the user config and updates the stored token reference.
///
/// This is the single shared save path; the daemon exposes it only through the
/// `workflow_backend_save_config` RPC in [`crate::daemon_workflow_runtime`].
pub(crate) fn save_workflow_backend_settings(
    paths: &ConfigPaths,
    config: &mut PufferConfig,
    input: SaveWorkflowBackendSettingsParams,
) -> Result<()> {
    let existing_secret_id = non_empty(&config.workflow_backend.api_token_secret_id);
    let mut workflow_backend = WorkflowBackendConfig {
        mode: input.mode,
        api_base_url: input.api_url,
        frontend_url: input.ui_url,
        workspace_id: input.workspace_id,
        api_token_secret_id: String::new(),
    };
    workflow_backend.normalize();
    workflow_backend.api_base_url = validated_api_base_url(&workflow_backend.api_base_url)?;
    workflow_backend.frontend_url = validated_frontend_url(&workflow_backend.frontend_url)?;
    workflow_backend.api_token_secret_id = stored_secret_id(
        paths,
        existing_secret_id,
        &workflow_backend.api_base_url,
        input.api_token.as_deref(),
        input.keep_token,
    )?;
    config.workflow_backend = workflow_backend;
    save_user_config(paths, config).context("save user config")
}

fn stored_secret_id(
    paths: &ConfigPaths,
    existing_secret_id: Option<&str>,
    api_base_url: &str,
    api_token: Option<&str>,
    keep_token: bool,
) -> Result<String> {
    let vault = SecretVault::open(SecretVault::default_path(&paths.user_config_dir))
        .context("open encrypted secret store")?;
    if let Some(value) = non_empty_optional(api_token) {
        let summary = vault.put(SecretUpsert {
            id: existing_secret_id.map(|value| value.to_string()),
            label: WORKFLOW_RUNTIME_TOKEN_LABEL.to_string(),
            description: Some(WORKFLOW_RUNTIME_TOKEN_DESCRIPTION.to_string()),
            value: value.to_string(),
            username: None,
            origin: Some(api_base_url.to_string()),
            source: "manual".to_string(),
        })?;
        return Ok(summary.id);
    }
    if keep_token {
        return Ok(existing_secret_id.unwrap_or_default().to_string());
    }
    if let Some(secret_id) = existing_secret_id {
        let _ = vault.delete(secret_id)?;
    }
    Ok(String::new())
}

fn validated_api_base_url(value: &str) -> Result<String> {
    let parsed = Url::parse(value).context("workflow api_base_url must be a valid URL")?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed.origin().ascii_serialization()),
        other => anyhow::bail!("workflow api_base_url must use http or https, got `{other}`"),
    }
}

fn validated_frontend_url(value: &str) -> Result<String> {
    let mut parsed = Url::parse(value).context("workflow frontend_url must be a valid URL")?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => anyhow::bail!("workflow frontend_url must use http or https, got `{other}`"),
    }
    parsed.set_fragment(None);
    parsed.set_query(None);
    if parsed.path() == "/" {
        Ok(parsed.origin().ascii_serialization())
    } else {
        Ok(parsed.to_string().trim_end_matches('/').to_string())
    }
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn non_empty_optional(value: Option<&str>) -> Option<&str> {
    value.and_then(non_empty)
}

/// Process-wide guard so secret-store tests across modules never race on the
/// shared `PUFFER_SECRET_STORE_KEY` environment variable.
#[cfg(test)]
pub(crate) fn secret_store_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::secret_store_test_lock;
    use puffer_config::ConfigPaths;
    use std::ffi::OsString;
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    const TEST_SECRET_STORE_KEY: &str = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=";

    /// Locks the shared secret-store test guard.
    pub(crate) fn lock_secret_store() -> MutexGuard<'static, ()> {
        secret_store_test_lock()
    }

    /// Restores `PUFFER_SECRET_STORE_KEY` after a test finishes.
    pub(crate) struct ScopedSecretStoreKey {
        old_value: Option<OsString>,
    }

    impl ScopedSecretStoreKey {
        /// Installs the deterministic secret-store key used by workflow tests.
        pub(crate) fn set() -> Self {
            let old_value = std::env::var_os("PUFFER_SECRET_STORE_KEY");
            std::env::set_var("PUFFER_SECRET_STORE_KEY", TEST_SECRET_STORE_KEY);
            Self { old_value }
        }
    }

    impl Drop for ScopedSecretStoreKey {
        fn drop(&mut self) {
            if let Some(value) = self.old_value.take() {
                std::env::set_var("PUFFER_SECRET_STORE_KEY", value);
            } else {
                std::env::remove_var("PUFFER_SECRET_STORE_KEY");
            }
        }
    }

    /// Builds isolated config paths for workflow daemon tests.
    pub(crate) fn temp_paths(temp: &TempDir) -> ConfigPaths {
        let workspace_root = temp.path().join("workspace");
        ConfigPaths {
            workspace_root: workspace_root.clone(),
            workspace_config_dir: workspace_root.join(".puffer"),
            user_config_dir: temp.path().join("home").join(".puffer"),
            builtin_resources_dir: workspace_root.join("resources"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{lock_secret_store, temp_paths, ScopedSecretStoreKey};
    use super::*;
    use crate::desktop_api::workflow_backend_settings_dto;
    use puffer_config::ensure_workspace_dirs;
    use puffer_config::WorkflowBackendMode;
    use std::fs;

    #[test]
    fn save_workflow_backend_settings_stores_token_outside_config() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = temp_paths(&temp);
        ensure_workspace_dirs(&paths).expect("workspace dirs");
        let mut config = PufferConfig::default();

        save_workflow_backend_settings(
            &paths,
            &mut config,
            SaveWorkflowBackendSettingsParams {
                mode: WorkflowBackendMode::AgentEnvCloud,
                api_url: "https://api.agentenv.io/v1/workflows".to_string(),
                ui_url: String::new(),
                workspace_id: "  workspace-123  ".to_string(),
                api_token: Some("agentenv-secret-token".to_string()),
                keep_token: false,
            },
        )
        .expect("save workflow backend settings");

        let raw_config = fs::read_to_string(paths.user_config_file()).expect("read config");
        assert!(!raw_config.contains("agentenv-secret-token"));
        assert!(raw_config.contains("https://api.agentenv.io"));
        assert!(!raw_config.contains("/v1"));

        let vault = SecretVault::open(SecretVault::default_path(&paths.user_config_dir))
            .expect("open secret vault");
        let stored = vault
            .reveal(&config.workflow_backend.api_token_secret_id)
            .expect("reveal stored token");
        assert_eq!(stored.value, "agentenv-secret-token");
        assert_eq!(
            config.workflow_backend.api_base_url,
            "https://api.agentenv.io"
        );
        assert_eq!(config.workflow_backend.frontend_url, "https://agentenv.io");
        assert_eq!(config.workflow_backend.workspace_id, "workspace-123");
    }

    #[test]
    fn workflow_backend_snapshot_only_reports_has_token() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = temp_paths(&temp);
        ensure_workspace_dirs(&paths).expect("workspace dirs");
        let mut config = PufferConfig::default();

        save_workflow_backend_settings(
            &paths,
            &mut config,
            SaveWorkflowBackendSettingsParams {
                mode: WorkflowBackendMode::Local,
                api_url: "http://127.0.0.1:3000/v1".to_string(),
                ui_url: "http://localhost:5173/".to_string(),
                workspace_id: String::new(),
                api_token: Some("local-runtime-token".to_string()),
                keep_token: false,
            },
        )
        .expect("save workflow backend settings");

        let snapshot = workflow_backend_settings_dto(&paths, &config).expect("workflow snapshot");
        let serialized = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(snapshot.has_token);
        assert_eq!(snapshot.api_url, "http://127.0.0.1:3000");
        assert_eq!(snapshot.ui_url, "http://localhost:5173");
        assert!(!serialized.contains("local-runtime-token"));
        assert!(!serialized.contains("apiToken"));
        assert!(!serialized.contains("apiTokenSecretId"));
        assert!(snapshot
            .options
            .iter()
            .any(|option| option.mode == WorkflowBackendMode::Local
                && !option.label.contains("AgentEnv")
                && !option.description.contains("AgentEnv")));
        assert!(snapshot
            .options
            .iter()
            .any(|option| option.mode == WorkflowBackendMode::AgentEnvCloud
                && option.label.contains("AgentEnv Cloud")));
    }

    #[test]
    fn save_workflow_backend_settings_keeps_existing_token_when_requested() {
        let _guard = lock_secret_store();
        let _secret_store_key = ScopedSecretStoreKey::set();
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = temp_paths(&temp);
        ensure_workspace_dirs(&paths).expect("workspace dirs");
        let mut config = PufferConfig::default();

        save_workflow_backend_settings(
            &paths,
            &mut config,
            SaveWorkflowBackendSettingsParams {
                mode: WorkflowBackendMode::AgentEnvCloud,
                api_url: String::new(),
                ui_url: String::new(),
                workspace_id: "workspace-123".to_string(),
                api_token: Some("first-token".to_string()),
                keep_token: false,
            },
        )
        .expect("save initial token");
        let original_secret_id = config.workflow_backend.api_token_secret_id.clone();

        save_workflow_backend_settings(
            &paths,
            &mut config,
            SaveWorkflowBackendSettingsParams {
                mode: WorkflowBackendMode::AgentEnvCloud,
                api_url: "https://api.agentenv.io/v1".to_string(),
                ui_url: "https://agentenv.io".to_string(),
                workspace_id: "workspace-456".to_string(),
                api_token: None,
                keep_token: true,
            },
        )
        .expect("keep existing token");

        assert_eq!(
            config.workflow_backend.api_token_secret_id,
            original_secret_id
        );
        let vault = SecretVault::open(SecretVault::default_path(&paths.user_config_dir))
            .expect("open secret vault");
        let stored = vault
            .reveal(&config.workflow_backend.api_token_secret_id)
            .expect("reveal stored token");
        assert_eq!(stored.value, "first-token");
    }
}
