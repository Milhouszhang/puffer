use serde::{Deserialize, Serialize};
use url::Url;

const LOCAL_API_BASE_URL: &str = "http://127.0.0.1:3000";
const LOCAL_FRONTEND_URL: &str = "http://localhost:5173";
const AGENTENV_CLOUD_API_BASE_URL: &str = "https://api.agentenv.io";
const AGENTENV_CLOUD_FRONTEND_URL: &str = "https://agentenv.io";

/// Selects which workflow runtime backend should execute workflows.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowBackendMode {
    #[default]
    Local,
    #[serde(alias = "agentEnvCloud", alias = "AgentEnvCloud")]
    AgentEnvCloud,
}

/// Configures the selected workflow runtime backend and its stored token reference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowBackendConfig {
    #[serde(default)]
    pub mode: WorkflowBackendMode,
    #[serde(default)]
    pub api_base_url: String,
    #[serde(default)]
    pub frontend_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_token_secret_id: String,
}

impl Default for WorkflowBackendConfig {
    fn default() -> Self {
        Self {
            mode: WorkflowBackendMode::Local,
            api_base_url: Self::default_api_base_url(WorkflowBackendMode::Local).to_string(),
            frontend_url: Self::default_frontend_url(WorkflowBackendMode::Local).to_string(),
            workspace_id: String::new(),
            api_token_secret_id: String::new(),
        }
    }
}

impl WorkflowBackendConfig {
    /// Returns the default API base URL for one workflow backend mode.
    pub fn default_api_base_url(mode: WorkflowBackendMode) -> &'static str {
        match mode {
            WorkflowBackendMode::Local => LOCAL_API_BASE_URL,
            WorkflowBackendMode::AgentEnvCloud => AGENTENV_CLOUD_API_BASE_URL,
        }
    }

    /// Returns the default frontend URL for one workflow backend mode.
    pub fn default_frontend_url(mode: WorkflowBackendMode) -> &'static str {
        match mode {
            WorkflowBackendMode::Local => LOCAL_FRONTEND_URL,
            WorkflowBackendMode::AgentEnvCloud => AGENTENV_CLOUD_FRONTEND_URL,
        }
    }

    /// Normalizes workflow backend URLs, trims identifiers, and fills per-mode defaults.
    pub fn normalize(&mut self) {
        let api_base_url = if self.api_base_url.trim().is_empty() {
            Self::default_api_base_url(self.mode).to_string()
        } else {
            normalize_api_base_url(&self.api_base_url)
        };
        let frontend_url = if self.frontend_url.trim().is_empty() {
            Self::default_frontend_url(self.mode).to_string()
        } else {
            normalize_frontend_url(&self.frontend_url)
        };
        self.api_base_url = api_base_url;
        self.frontend_url = frontend_url;
        self.workspace_id = self.workspace_id.trim().to_string();
        self.api_token_secret_id = self.api_token_secret_id.trim().to_string();
    }
}

fn normalize_api_base_url(value: &str) -> String {
    let trimmed = value.trim();
    Url::parse(trimmed)
        .ok()
        .and_then(|parsed| match parsed.scheme() {
            "http" | "https" => Some(parsed.origin().ascii_serialization()),
            _ => None,
        })
        .unwrap_or_else(|| trimmed.trim_end_matches('/').to_string())
}

fn normalize_frontend_url(value: &str) -> String {
    let trimmed = value.trim();
    let Some(mut parsed) = Url::parse(trimmed).ok() else {
        return trimmed.trim_end_matches('/').to_string();
    };
    parsed.set_fragment(None);
    parsed.set_query(None);
    if parsed.path() == "/" {
        parsed.origin().ascii_serialization()
    } else {
        parsed.to_string().trim_end_matches('/').to_string()
    }
}
