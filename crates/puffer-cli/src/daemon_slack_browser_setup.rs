//! Daemon-owned connect/login flow for the Slack browser connector.

use crate::daemon::{DaemonState, ServerEnvelope};
use anyhow::{bail, Context, Result};
use puffer_core::{CancelToken, UserQuestionPromptResponse};
use puffer_subscriptions::{ConnectionRecord, ConnectionState};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const SIGN_IN_QUESTION: &str =
    "Sign in to Slack in the window (email + magic link / SSO / workspace pick), then choose Continue.";
const SETUP_TAB_ID: &str = "slack-setup";
const BROWSER_WIDTH: u32 = 1100;
const BROWSER_HEIGHT: u32 = 820;
const LOGIN_POLL_TIMEOUT: Duration = Duration::from_secs(120);
const LOGIN_POLL_INTERVAL: Duration = Duration::from_secs(2);

type PendingQuestions = Arc<Mutex<HashMap<String, mpsc::Sender<UserQuestionPromptResponse>>>>;

struct SetupFlow {
    state: Arc<DaemonState>,
    channel: String,
    turn_id: String,
    next_request_id: Arc<AtomicU64>,
    pending_questions: PendingQuestions,
    cancel: CancelToken,
    session_id: String,
    connection_slug: String,
}

pub(crate) fn connect_args_are_slack_browser(connect_args: &str) -> bool {
    connect_args.split_whitespace().next() == Some("slack-browser")
}

/// Executes daemon-native Slack browser connector setup.
pub(crate) fn execute_slack_browser_setup(
    state: Arc<DaemonState>,
    channel: String,
    turn_id: String,
    connect_args: String,
    next_request_id: Arc<AtomicU64>,
    pending_questions: PendingQuestions,
    cancel: CancelToken,
) -> Result<String> {
    let connection_slug = parse_setup_target(&connect_args)?;
    let session_id = format!("slack-browser-setup-{}", safe_session_part(&turn_id));
    let mut flow = SetupFlow {
        state,
        channel,
        turn_id,
        next_request_id,
        pending_questions,
        cancel,
        session_id,
        connection_slug,
    };
    flow.run()
}

impl SetupFlow {
    fn run(&mut self) -> Result<String> {
        self.cancel.check()?;
        self.open_url("https://app.slack.com/", "Slack")?;
        let href = self.poll_until_logged_in()?;

        // Extract team_id from href like https://app.slack.com/client/T0123ABC/...
        let team_id = extract_team_id(&href);

        // Evaluate self_id script
        let self_id_value = crate::daemon_browser::handle_browser_agent(
            &self.state,
            &json!({
                "action": "evaluate",
                "sessionId": &self.session_id,
                "tabId": SETUP_TAB_ID,
                "width": BROWSER_WIDTH,
                "height": BROWSER_HEIGHT,
                "script": crate::slack_browser_script::SLACK_SELF_ID_JS,
            }),
        )
        .context("evaluate Slack self_id")?;
        let self_id_str = self_id_value
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let self_id_json: Value = serde_json::from_str(self_id_str).unwrap_or(Value::Null);
        let self_id = self_id_json
            .get("self_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        crate::slack_browser::save_config(
            self.state.config_paths(),
            self.state.cwd_path(),
            &self.connection_slug,
            &team_id,
            &self_id,
        )?;
        let registered = upsert_connection(&self.connection_slug)?;
        let action = if registered { "created" } else { "updated" };
        Ok(format!(
            "Connected {} (slack-browser) ({action}).",
            self.connection_slug
        ))
    }

    fn open_url(&self, url: &str, label: &str) -> Result<()> {
        crate::daemon_browser::handle_browser_agent(
            &self.state,
            &json!({
                "action": "open",
                "sessionId": &self.session_id,
                "tabId": SETUP_TAB_ID,
                "label": label,
                "url": url,
                "width": BROWSER_WIDTH,
                "height": BROWSER_HEIGHT,
                "activate": true,
            }),
        )
        .with_context(|| format!("open Slack setup browser at {url}"))?;
        Ok(())
    }

    fn poll_until_logged_in(&mut self) -> Result<String> {
        let deadline = Instant::now() + LOGIN_POLL_TIMEOUT;
        let mut asked = false;
        loop {
            self.cancel.check()?;
            let value = crate::daemon_browser::handle_browser_agent(
                &self.state,
                &json!({
                    "action": "evaluate",
                    "sessionId": &self.session_id,
                    "tabId": SETUP_TAB_ID,
                    "width": BROWSER_WIDTH,
                    "height": BROWSER_HEIGHT,
                    "script": crate::slack_browser_script::SLACK_LOGIN_MARKER_JS,
                }),
            )
            .context("evaluate Slack login marker")?;
            let result_str = value.get("value").and_then(Value::as_str).unwrap_or("{}");
            let result: Value = serde_json::from_str(result_str).unwrap_or(Value::Null);
            let logged_in = result
                .get("loggedIn")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let href = result
                .get("href")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();

            if logged_in {
                return Ok(href);
            }
            if Instant::now() >= deadline {
                bail!(
                    "Slack login timed out after {}s (last URL: {})",
                    LOGIN_POLL_TIMEOUT.as_secs(),
                    href
                );
            }
            if !asked {
                asked = true;
                self.ask_sign_in(&href)?;
            }
            std::thread::sleep(LOGIN_POLL_INTERVAL);
        }
    }

    fn ask_sign_in(&self, href: &str) -> Result<()> {
        self.ask_questions(
            json!([{
                "type": "choice",
                "header": "Slack sign in",
                "question": SIGN_IN_QUESTION,
                "multiSelect": false,
                "options": []
            }]),
            json!({
                "browserSessionId": &self.session_id,
                "browserTabId": SETUP_TAB_ID,
                "browserUrl": href,
            }),
        )?;
        Ok(())
    }

    fn ask_questions(&self, questions: Value, extras: Value) -> Result<UserQuestionPromptResponse> {
        let request_id = self
            .next_request_id
            .fetch_add(1, Ordering::SeqCst)
            .to_string();
        let (tx, rx) = mpsc::channel();
        self.pending_questions
            .lock()
            .unwrap()
            .insert(request_id.clone(), tx);

        let mut payload = Map::new();
        payload.insert("type".to_string(), json!("user-question-request"));
        payload.insert("turnId".to_string(), json!(self.turn_id));
        payload.insert("requestId".to_string(), json!(request_id));
        payload.insert("questions".to_string(), questions);
        if let Some(extra) = extras.as_object() {
            for (key, value) in extra {
                payload.insert(key.clone(), value.clone());
            }
        }
        self.state.publish_event(ServerEnvelope::Event {
            event: self.channel.clone(),
            payload: Value::Object(payload),
        });

        rx.recv()
            .map_err(|_| anyhow::anyhow!("connector setup question channel closed"))
    }
}

/// Parse the connection slug from connect args.
/// Falls back to "slack-browser" if no second token is given.
pub(crate) fn parse_setup_target(connect_args: &str) -> Result<String> {
    let mut parts = connect_args.split_whitespace();
    let _connector = parts.next().unwrap_or_default();
    let connection_slug = parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("slack-browser")
        .to_string();
    if parts.next().is_some() {
        bail!("Usage: /connect slack-browser <connection-name>");
    }
    Ok(connection_slug)
}

fn upsert_connection(connection: &str) -> Result<bool> {
    let manager = puffer_core::subscription_manager()?;
    let description = "Slack Browser".to_string();
    let registered = if let Some(existing) = manager.connection_store().get(connection) {
        if existing.connector_slug != "slack-browser" {
            bail!(
                "connection `{connection}` already exists for connector `{}`",
                existing.connector_slug
            );
        }
        manager.connection_store().update(connection, |record| {
            record.description = description.clone();
            record.state = ConnectionState::Authenticated;
            record.auth_failure_notified = false;
        })?;
        false
    } else {
        manager
            .connection_store()
            .create(ConnectionRecord::authenticated(
                connection,
                "slack-browser",
                description,
            ))?;
        true
    };
    manager.refresh_connection_consumers()?;
    manager.refresh_connection_auth()?;
    Ok(registered)
}

fn extract_team_id(href: &str) -> String {
    let re = Regex::new(r"/client/(T[A-Z0-9]+)").expect("valid regex");
    re.captures(href)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

pub(crate) fn safe_session_part(value: &str) -> String {
    let mut output = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    while output.contains("--") {
        output = output.replace("--", "-");
    }
    let trimmed = output.trim_matches('-');
    if trimmed.is_empty() {
        "setup".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_slug_only() {
        assert!(connect_args_are_slack_browser("slack-browser"));
        assert!(connect_args_are_slack_browser("slack-browser work"));
        assert!(!connect_args_are_slack_browser("gmail-browser"));
        assert!(!connect_args_are_slack_browser(""));
    }

    #[test]
    fn parse_target_defaults_to_slug() {
        assert_eq!(
            parse_setup_target("slack-browser").unwrap(),
            "slack-browser"
        );
        assert_eq!(parse_setup_target("slack-browser my-ws").unwrap(), "my-ws");
        assert!(parse_setup_target("slack-browser a b").is_err());
    }

    #[test]
    fn safe_session_part_sanitizes() {
        assert_eq!(safe_session_part("abc-def_123"), "abc-def_123");
        assert_eq!(safe_session_part("a b c"), "a-b-c");
        assert_eq!(safe_session_part("  "), "setup");
        assert_eq!(safe_session_part("a--b"), "a-b");
    }

    #[test]
    fn extract_team_id_from_href() {
        assert_eq!(
            extract_team_id("https://app.slack.com/client/T0123ABCD/C456"),
            "T0123ABCD"
        );
        assert_eq!(extract_team_id("https://app.slack.com/signin"), "");
        assert_eq!(extract_team_id(""), "");
    }
}
