//! Single source of truth for the GitHub Copilot client-identity headers.
//!
//! GitHub gates and personalizes the Copilot API by client identity and
//! `X-GitHub-Api-Version`, and reshuffles that policy often. Every Copilot-bound
//! request must therefore present a *consistent* identity: the OAuth device-flow
//! login (`puffer-cli`), the token exchange + auto-session mint (`puffer-core`),
//! and model discovery (below) all reference these constants, while
//! `resources/providers/github-copilot.yaml` mirrors them for the chat path.
//! The `yaml_headers_match_constants` test fails if the descriptor drifts from
//! these values, so bumping the client version is a one-line change here plus the
//! YAML. Versions mirror current third-party Copilot clients as of 2026-07.

use reqwest::blocking::RequestBuilder;

/// Copilot token-exchange endpoint: a stored GitHub token (`gho_`/`ghu_`) is
/// swapped here for a short-lived Copilot bearer. Shared by discovery and the
/// runtime bearer cache so the two can never point at different hosts.
pub const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";

pub const COPILOT_USER_AGENT: &str = "GitHubCopilotChat/0.31.0";
pub const COPILOT_EDITOR_VERSION: &str = "vscode/1.104.0";
pub const COPILOT_EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.31.0";
pub const COPILOT_INTEGRATION_ID: &str = "vscode-chat";
pub const COPILOT_API_VERSION: &str = "2025-10-01";

/// Attaches the full Copilot client-identity header set required by the token
/// exchange, auto-session mint, and model-discovery requests. (GitHub's OAuth
/// device-flow endpoints only require `COPILOT_USER_AGENT`; set that directly.)
pub fn apply_copilot_client_identity(request: RequestBuilder) -> RequestBuilder {
    request
        .header("User-Agent", COPILOT_USER_AGENT)
        .header("Editor-Version", COPILOT_EDITOR_VERSION)
        .header("Editor-Plugin-Version", COPILOT_EDITOR_PLUGIN_VERSION)
        .header("Copilot-Integration-Id", COPILOT_INTEGRATION_ID)
        .header("X-GitHub-Api-Version", COPILOT_API_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The chat path sends identity headers from the provider descriptor, while
    // the OAuth / token-exchange / discovery paths send the constants above.
    // Guard against the two drifting apart: the descriptor's headers must equal
    // the constants (a drift would gate discovery or chat independently).
    #[test]
    fn yaml_headers_match_constants() {
        let yaml = include_str!("../../../resources/providers/github-copilot.yaml");
        let doc: serde_yaml::Value = serde_yaml::from_str(yaml).expect("parse github-copilot.yaml");
        let headers = doc
            .get("headers")
            .expect("github-copilot.yaml must declare headers");
        let get = |key: &str| {
            headers
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("github-copilot.yaml missing header {key}"))
                .to_string()
        };
        assert_eq!(get("User-Agent"), COPILOT_USER_AGENT);
        assert_eq!(get("Editor-Version"), COPILOT_EDITOR_VERSION);
        assert_eq!(get("Editor-Plugin-Version"), COPILOT_EDITOR_PLUGIN_VERSION);
        assert_eq!(get("Copilot-Integration-Id"), COPILOT_INTEGRATION_ID);
        assert_eq!(get("X-GitHub-Api-Version"), COPILOT_API_VERSION);
    }
}
