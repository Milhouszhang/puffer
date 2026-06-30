use anyhow::{bail, Context, Result};
use puffer_provider_registry::{
    canonical_provider_id, AuthStore, MediaExecutionDescriptor, ProviderDescriptor,
    StoredCredential,
};
use reqwest::blocking::Client;
use std::time::Duration;

/// Controls whether OpenAI media execution can use legacy Codex credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialAliasMode {
    Strict,
    OpenAiCodexAlias,
}

/// Builds an absolute provider media execution URL.
pub(crate) fn provider_execution_url(
    provider: &ProviderDescriptor,
    execution: &MediaExecutionDescriptor,
    label: &str,
) -> Result<reqwest::Url> {
    let base_url = execution.base_url.as_deref().unwrap_or(&provider.base_url);
    let base = format!("{}/", base_url.trim_end_matches('/'));
    let path = execution.path.trim_start_matches('/');
    let mut url = reqwest::Url::parse(&base)
        .and_then(|base| base.join(path))
        .with_context(|| format!("build {label} URL from {} and {}", base_url, execution.path))?;
    if !provider.query_params.is_empty() {
        let mut query = url.query_pairs_mut();
        for (key, value) in &provider.query_params {
            query.append_pair(key, value);
        }
    }
    Ok(url)
}

/// Returns the bearer token for an authenticated provider.
pub(crate) fn bearer_token(
    provider: &ProviderDescriptor,
    auth_store: &AuthStore,
    alias_mode: CredentialAliasMode,
) -> Result<Option<String>> {
    if provider.auth_modes.is_empty() {
        return Ok(None);
    }
    let Some(credential) = provider_credential(provider, auth_store, alias_mode) else {
        bail!(
            "missing credentials configured for provider {}",
            provider.id
        );
    };
    match credential {
        StoredCredential::ApiKey { key } => non_empty_token(key, &provider.id).map(Some),
        StoredCredential::OAuth(credential) => {
            non_empty_token(&credential.access_token, &provider.id).map(Some)
        }
    }
}

/// Collects secret-bearing provider values for error redaction.
pub(crate) fn provider_error_secrets(
    provider: &ProviderDescriptor,
    auth_store: &AuthStore,
    alias_mode: CredentialAliasMode,
) -> Vec<String> {
    let mut secrets = Vec::new();
    if let Some(credential) = provider_credential(provider, auth_store, alias_mode) {
        match credential {
            StoredCredential::ApiKey { key } => secrets.push(key.clone()),
            StoredCredential::OAuth(credential) => {
                secrets.push(credential.access_token.clone());
                secrets.push(credential.refresh_token.clone());
            }
        }
    }
    secrets.extend(provider.headers.values().cloned());
    secrets.extend(provider.query_params.values().cloned());
    secrets
        .into_iter()
        .map(|secret| secret.trim().to_string())
        .filter(|secret| !secret.is_empty())
        .collect()
}

/// Redacts all known provider secrets from an error string.
pub(crate) fn redact_secrets(text: &str, secrets: &[String]) -> String {
    secrets.iter().fold(text.to_string(), |redacted, secret| {
        redacted.replace(secret, "[redacted]")
    })
}

/// Attempts before giving up on a media download, and the base backoff between
/// attempts (scaled by attempt index).
const DOWNLOAD_MAX_ATTEMPTS: u32 = 4;
const DOWNLOAD_RETRY_BACKOFF: Duration = Duration::from_secs(2);

/// Builds a blocking HTTP client for downloading generated media bytes.
///
/// Carries a generous per-request timeout (the default client has none) so a
/// stalled transfer is bounded and the retry loop in [`download_image_url`] can
/// re-attempt instead of hanging indefinitely. Proxy handling is left at the
/// client default — any ambient `HTTP(S)_PROXY` is honored as-is.
pub(crate) fn media_download_client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(180))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .context("build media download HTTP client")
}

/// Worth a retry vs. not, so [`download_image_url`] can fail fast on a definitive
/// 4xx but keep trying through transient network and server hiccups.
enum DownloadError {
    Transient(anyhow::Error),
    Definitive(anyhow::Error),
}

/// Downloads media bytes from a remote URL, retrying transient transport and
/// server errors with backoff. Plain HTTP is rejected unless the host is
/// loopback. A definitive client error (4xx other than 408/429) fails
/// immediately — retrying a 404/403 is pointless.
pub(crate) fn download_image_url(client: &Client, url: &str, label: &str) -> Result<Vec<u8>> {
    let parsed =
        reqwest::Url::parse(url).with_context(|| format!("{label} URL must be absolute"))?;
    match parsed.scheme() {
        "https" => {}
        "http" if url_host_is_loopback(&parsed) => {}
        other => bail!("unsupported {label} URL scheme `{other}`"),
    }
    let mut last_error = None;
    for attempt in 0..DOWNLOAD_MAX_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(DOWNLOAD_RETRY_BACKOFF * attempt);
        }
        match try_download_once(client, parsed.clone(), label) {
            Ok(bytes) => return Ok(bytes),
            Err(DownloadError::Definitive(error)) => return Err(error),
            Err(DownloadError::Transient(error)) => last_error = Some(error),
        }
    }
    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("download {label} failed"))
        .context(format!(
            "download {label} failed after {DOWNLOAD_MAX_ATTEMPTS} attempts"
        )))
}

/// Performs one download attempt, classifying any failure as transient or
/// definitive for the caller's retry loop.
fn try_download_once(
    client: &Client,
    url: reqwest::Url,
    label: &str,
) -> std::result::Result<Vec<u8>, DownloadError> {
    let response = match client.get(url).send() {
        Ok(response) => response,
        // A transport failure (timeout, connection reset, proxy sever) is
        // always worth retrying.
        Err(error) => {
            return Err(DownloadError::Transient(
                anyhow::Error::new(error).context(format!("download {label}")),
            ))
        }
    };
    let status = response.status();
    if !status.is_success() {
        let error = anyhow::anyhow!("download {label} failed with status {}", status.as_u16());
        if status.is_server_error()
            || status == reqwest::StatusCode::REQUEST_TIMEOUT
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            return Err(DownloadError::Transient(error));
        }
        return Err(DownloadError::Definitive(error));
    }
    response
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|error| {
            DownloadError::Transient(anyhow::Error::new(error).context(format!("read {label} bytes")))
        })
}

fn provider_credential<'a>(
    provider: &ProviderDescriptor,
    auth_store: &'a AuthStore,
    alias_mode: CredentialAliasMode,
) -> Option<&'a StoredCredential> {
    let canonical = canonical_provider_id(&provider.id);
    auth_store
        .get(&provider.id)
        .or_else(|| {
            (canonical != provider.id.as_str())
                .then(|| auth_store.get(&canonical))
                .flatten()
        })
        .or_else(|| {
            (alias_mode == CredentialAliasMode::OpenAiCodexAlias && canonical == "openai")
                .then(|| auth_store.get("codex"))
                .flatten()
        })
}

fn non_empty_token(value: &str, provider_id: &str) -> Result<String> {
    let token = value.trim();
    if token.is_empty() {
        bail!("empty credentials configured for provider {provider_id}");
    }
    Ok(token.to_string())
}

fn url_host_is_loopback(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}
