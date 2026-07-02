//! GitHub Copilot runtime auth.
//!
//! Copilot's chat endpoint (`api.githubcopilot.com`) speaks OpenAI Chat
//! Completions, but the bearer it accepts is NOT the stored GitHub OAuth token.
//! The GitHub token (`gho_`/`ghu_`, obtained via the device-flow login) must be
//! exchanged for a short-lived Copilot token at `copilot_internal/v2/token`.
//! We cache the exchanged token per GitHub token until shortly before it
//! expires so we don't hit the exchange endpoint on every request.

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";
/// Refresh this many seconds before the server-reported expiry.
const EXPIRY_SKEW_SECS: u64 = 120;
/// Fallback lifetime when the response omits `expires_at` (Copilot tokens are
/// typically ~30 min).
const FALLBACK_LIFETIME_SECS: u64 = 25 * 60;

/// Default Copilot API host when the token response omits `endpoints.api`.
const DEFAULT_COPILOT_API: &str = "https://api.githubcopilot.com";

/// A short-lived Copilot bearer plus the account-specific API base URL. The
/// endpoint varies by SKU (individual / business / enterprise), so we always
/// use the `endpoints.api` the exchange returns rather than a hardcoded host.
#[derive(Clone)]
pub(crate) struct CopilotAuth {
    pub token: String,
    pub api_url: String,
}

struct CachedToken {
    auth: CopilotAuth,
    expires_at_secs: u64,
}

fn cache() -> &'static Mutex<HashMap<String, CachedToken>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedToken>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Exchanges a stored GitHub token for a short-lived Copilot bearer token plus
/// the account-specific API endpoint, caching until shortly before it expires.
pub(crate) fn copilot_bearer_token(github_token: &str) -> Result<CopilotAuth> {
    let now = now_secs();
    if let Some(cached) = cache().lock().unwrap().get(github_token) {
        if cached.expires_at_secs > now + EXPIRY_SKEW_SECS {
            return Ok(cached.auth.clone());
        }
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("building Copilot token-exchange client")?;
    let response = client
        .get(COPILOT_TOKEN_URL)
        .header("Authorization", format!("token {github_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "GitHubCopilotChat/0.31.0")
        .header("Editor-Version", "vscode/1.104.0")
        .header("Editor-Plugin-Version", "copilot-chat/0.31.0")
        .header("Copilot-Integration-Id", "vscode-chat")
        .header("X-GitHub-Api-Version", "2025-10-01")
        .send()
        .context("requesting Copilot token")?;

    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        bail!("GitHub Copilot token exchange failed ({status}): {body}");
    }

    let value: serde_json::Value =
        serde_json::from_str(&body).context("parsing Copilot token response")?;
    let token = value
        .get("token")
        .and_then(|t| t.as_str())
        .context("Copilot token response missing `token`")?
        .to_string();
    let expires_at = value
        .get("expires_at")
        .and_then(|e| e.as_u64())
        .unwrap_or(now + FALLBACK_LIFETIME_SECS);
    // The account-specific API host (individual / business / enterprise).
    let api_url = value
        .get("endpoints")
        .and_then(|endpoints| endpoints.get("api"))
        .and_then(|api| api.as_str())
        .unwrap_or(DEFAULT_COPILOT_API)
        .trim_end_matches('/')
        .to_string();

    let auth = CopilotAuth { token, api_url };
    cache().lock().unwrap().insert(
        github_token.to_string(),
        CachedToken {
            auth: auth.clone(),
            expires_at_secs: expires_at,
        },
    );
    Ok(auth)
}
