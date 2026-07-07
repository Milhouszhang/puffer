//! GitHub Copilot device-flow login.
//!
//! Copilot does not use the PKCE redirect flow that openai/anthropic use.
//! Instead we run GitHub's OAuth *device flow*: request a device+user code,
//! the user enters the user code at github.com/login/device, and we poll until
//! GitHub returns a `gho_`/`ghu_` access token. That token is stored as the
//! provider credential; the runtime later exchanges it for a short-lived
//! Copilot bearer (see `puffer-core/runtime/copilot.rs`).

use anyhow::{anyhow, bail, Context, Result};
use puffer_provider_registry::COPILOT_USER_AGENT;
use serde::Deserialize;
use std::time::Duration;

/// Public client id of the VS Code Copilot extension. Copilot gates model
/// access on the client id, so we present the same identity the editors use.
pub(crate) const COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

fn http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("building GitHub device-flow client")
}

/// Result of starting the device flow — shown to the user so they can authorize.
#[derive(Debug, Clone)]
pub(crate) struct DeviceFlowStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// Requests a device + user code from GitHub.
pub(crate) fn start_device_flow() -> Result<DeviceFlowStart> {
    #[derive(Deserialize)]
    struct Resp {
        device_code: String,
        user_code: String,
        verification_uri: String,
        #[serde(default)]
        interval: u64,
        #[serde(default)]
        expires_in: u64,
    }
    let client = http_client()?;
    let response = client
        .post(DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .header("User-Agent", COPILOT_USER_AGENT)
        .form(&[("client_id", COPILOT_CLIENT_ID), ("scope", "read:user")])
        .send()
        .context("requesting GitHub device code")?;
    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        bail!("GitHub device-code request failed ({status}): {body}");
    }
    let parsed: Resp =
        serde_json::from_str(&body).context("parsing GitHub device-code response")?;
    Ok(DeviceFlowStart {
        device_code: parsed.device_code,
        user_code: parsed.user_code,
        verification_uri: parsed.verification_uri,
        interval: if parsed.interval == 0 {
            5
        } else {
            parsed.interval
        },
        expires_in: parsed.expires_in,
    })
}

/// Outcome of a single poll of the device-flow token endpoint.
#[derive(Debug)]
pub(crate) enum DeviceFlowPoll {
    /// User has not authorized yet — keep polling.
    Pending,
    /// GitHub asked us to slow down — increase the interval, keep polling.
    SlowDown,
    /// Authorized — the GitHub access token.
    Done(String),
    /// Terminal failure (expired / denied / etc.).
    Failed(String),
}

struct DevicePollHttpResponse {
    status: reqwest::StatusCode,
    body: String,
}

#[derive(Deserialize)]
struct DevicePollResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

fn classify_device_flow_poll_response(
    response: Result<DevicePollHttpResponse>,
) -> Result<DeviceFlowPoll> {
    let response = response?;
    if !response.status.is_success() {
        bail!(
            "GitHub device-flow token poll failed ({}): {}",
            response.status,
            response.body
        );
    }
    let parsed: DevicePollResponse =
        serde_json::from_str(&response.body).context("parsing GitHub device-flow poll response")?;
    if let Some(token) = parsed.access_token.filter(|t| !t.is_empty()) {
        return Ok(DeviceFlowPoll::Done(token));
    }
    match parsed.error.as_deref() {
        // Keep polling.
        Some("authorization_pending") | None => Ok(DeviceFlowPoll::Pending),
        Some("slow_down") => Ok(DeviceFlowPoll::SlowDown),
        // GitHub's documented terminal device-flow errors.
        Some(
            err @ ("access_denied"
            | "expired_token"
            | "unsupported_grant_type"
            | "incorrect_client_credentials"
            | "incorrect_device_code"
            | "device_flow_disabled"),
        ) => Ok(DeviceFlowPoll::Failed(err.to_string())),
        // Unknown GitHub error code — treat as transient rather than aborting.
        Some(_) => Ok(DeviceFlowPoll::Pending),
    }
}

/// Polls the token endpoint once with the device code.
pub(crate) fn poll_device_flow(device_code: &str) -> Result<DeviceFlowPoll> {
    let client = http_client()?;
    // GitHub's device-flow protocol has explicit non-terminal states
    // (`authorization_pending`, `slow_down`). Transport failures are not one of
    // them: surface those as poll errors so desktop callers can use their
    // consecutive-error guard instead of waiting until device-code expiry.
    let response = match client
        .post(ACCESS_TOKEN_URL)
        .header("Accept", "application/json")
        .header("User-Agent", COPILOT_USER_AGENT)
        .form(&[
            ("client_id", COPILOT_CLIENT_ID),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
    {
        Ok(response) => {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            Ok(DevicePollHttpResponse { status, body })
        }
        Err(error) => Err(anyhow!("GitHub device-flow poll network error: {error}")),
    };
    classify_device_flow_poll_response(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use reqwest::StatusCode;

    fn classify_ok(body: &str) -> Result<DeviceFlowPoll> {
        classify_device_flow_poll_response(Ok(DevicePollHttpResponse {
            status: StatusCode::OK,
            body: body.to_string(),
        }))
    }

    #[test]
    fn authorization_pending_remains_pending() {
        let result = classify_ok(r#"{"error":"authorization_pending"}"#).unwrap();
        assert!(matches!(result, DeviceFlowPoll::Pending));
    }

    #[test]
    fn slow_down_remains_slow_down() {
        let result = classify_ok(r#"{"error":"slow_down"}"#).unwrap();
        assert!(matches!(result, DeviceFlowPoll::SlowDown));
    }

    #[test]
    fn transport_errors_are_not_mapped_to_pending() {
        let error = classify_device_flow_poll_response(Err(anyhow!("connection refused")))
            .expect_err("transport failures must reject the poll RPC");
        assert!(error.to_string().contains("connection refused"));
    }

    #[test]
    fn malformed_poll_response_is_not_mapped_to_pending() {
        let error = classify_ok("<html>bad gateway</html>")
            .expect_err("malformed poll responses are not protocol pending states");
        assert!(
            error
                .to_string()
                .contains("parsing GitHub device-flow poll response"),
            "{error:#}"
        );
    }
}
