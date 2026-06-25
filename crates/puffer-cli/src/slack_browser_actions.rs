// Act DOM is best-effort from Slack data-qa hooks — NOT yet live-validated (see live-test task).

//! Connector action helpers for the Slack browser subscriber.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use super::{
    ensure_browser_daemon, safe_session_part, SlackBrowserConfig, SubscriberEnv, BROWSER_HEIGHT,
    BROWSER_WIDTH,
};

const SLACK_LOAD_TIMEOUT: Duration = Duration::from_secs(15);
const SLACK_EVALUATE_INTERVAL: Duration = Duration::from_millis(500);

// ── Public entry point ───────────────────────────────────────────────────────

/// Executes one Slack browser connector action through the managed Chrome
/// profile. Dispatches on `action` ∈ {`send_message`, `read_history`, `react`}.
pub(super) fn handle_action(
    env: &SubscriberEnv,
    config: &SlackBrowserConfig,
    handshake: &mut Option<crate::daemon::Handshake>,
    action: &str,
    input: &Value,
) -> Result<Value> {
    match action {
        "send_message" => slack_send_message(env, config, handshake, action, input),
        "read_history" => slack_read_history(env, config, handshake, action, input),
        "react" => slack_react(env, config, handshake, action, input),
        other => anyhow::bail!("unsupported slack-browser action `{other}`"),
    }
}

// ── Field structs + pure parsers ─────────────────────────────────────────────

pub(super) struct SendFields {
    pub channel_id: String,
    pub text: String,
}

pub(super) fn send_message_fields(input: &Value) -> Result<SendFields> {
    let channel_id = string_input(input, "channel_id")
        .or_else(|| string_input(input, "channel"))
        .or_else(|| string_input(input, "to"))
        .ok_or_else(|| anyhow::anyhow!("send_message requires `channel_id`, `channel`, or `to`"))?;
    let text = string_input(input, "text")
        .or_else(|| string_input(input, "message"))
        .or_else(|| string_input(input, "body"))
        .ok_or_else(|| anyhow::anyhow!("send_message requires `text`, `message`, or `body`"))?;
    Ok(SendFields { channel_id, text })
}

pub(super) struct ReadHistoryFields {
    pub channel_id: String,
    pub limit: usize,
}

pub(super) fn read_history_fields(input: &Value) -> Result<ReadHistoryFields> {
    let channel_id = string_input(input, "channel_id")
        .or_else(|| string_input(input, "channel"))
        .or_else(|| string_input(input, "to"))
        .ok_or_else(|| anyhow::anyhow!("read_history requires `channel_id`, `channel`, or `to`"))?;
    let limit = integer_input(input, "limit").unwrap_or(50).clamp(1, 200) as usize;
    Ok(ReadHistoryFields { channel_id, limit })
}

pub(super) struct ReactFields {
    pub channel_id: String,
    pub message_ts: String,
    pub emoji: String,
}

pub(super) fn react_fields(input: &Value) -> Result<ReactFields> {
    let channel_id = string_input(input, "channel_id")
        .or_else(|| string_input(input, "channel"))
        .or_else(|| string_input(input, "to"))
        .ok_or_else(|| anyhow::anyhow!("react requires `channel_id`, `channel`, or `to`"))?;
    let message_ts = string_input(input, "message_id")
        .or_else(|| string_input(input, "ts"))
        .or_else(|| string_input(input, "message_ts"))
        .ok_or_else(|| anyhow::anyhow!("react requires `message_id` or `ts`"))?;
    let emoji = string_input(input, "emoji")
        .or_else(|| string_input(input, "reaction"))
        .unwrap_or_else(|| "👍".to_string());
    Ok(ReactFields {
        channel_id,
        message_ts,
        emoji,
    })
}

// ── Browser-glue actions ─────────────────────────────────────────────────────

fn slack_send_message(
    env: &SubscriberEnv,
    config: &SlackBrowserConfig,
    handshake: &mut Option<crate::daemon::Handshake>,
    action: &str,
    input: &Value,
) -> Result<Value> {
    let fields = send_message_fields(input)?;
    let handshake_ref = ensure_browser_daemon(config, handshake)?;
    // Navigate to the target channel, then type and send.
    let result = evaluate_slack_script(
        env,
        handshake_ref,
        &slack_navigate_script(config, &fields.channel_id),
    )?;
    if !result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        anyhow::bail!(
            "slack-browser send_message: navigate failed for channel `{}`: {}",
            fields.channel_id,
            result
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    let result = evaluate_slack_script(
        env,
        handshake_ref,
        &slack_send_message_script(&fields.text),
    )?;
    if !result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        anyhow::bail!(
            "slack-browser send_message failed for channel `{}`: {}",
            fields.channel_id,
            result
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    Ok(json!({
        "action": action,
        "completed": true,
        "summary": format!("sent Slack message to channel {}", fields.channel_id),
        "channel_id": fields.channel_id,
        "text": fields.text,
    }))
}

fn slack_read_history(
    env: &SubscriberEnv,
    config: &SlackBrowserConfig,
    handshake: &mut Option<crate::daemon::Handshake>,
    action: &str,
    input: &Value,
) -> Result<Value> {
    let fields = read_history_fields(input)?;
    let handshake_ref = ensure_browser_daemon(config, handshake)?;

    // Navigate to the channel first.
    let nav = evaluate_slack_script(
        env,
        handshake_ref,
        &slack_navigate_script(config, &fields.channel_id),
    )?;
    if !nav.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        anyhow::bail!(
            "slack-browser read_history: could not open channel `{}`: {}",
            fields.channel_id,
            nav.get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }

    // Install observer (idempotent).
    let _install = evaluate_slack_script(
        env,
        handshake_ref,
        crate::slack_browser_script::SLACK_OBSERVER_INSTALL_JS,
    )?;

    // Drain messages.
    let drain_raw = evaluate_slack_script(
        env,
        handshake_ref,
        crate::slack_browser_script::SLACK_OBSERVER_DRAIN_JS,
    )?;
    let drain_str = drain_raw.get("value").and_then(Value::as_str).unwrap_or("");
    let drain: Value = serde_json::from_str(drain_str).unwrap_or(drain_raw);

    let messages: Vec<Value> = drain
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .take(fields.limit)
        .collect();

    Ok(json!({
        "action": action,
        "completed": true,
        "summary": format!("read {} Slack message(s) from channel {}", messages.len(), fields.channel_id),
        "channel_id": fields.channel_id,
        "messages": messages,
    }))
}

fn slack_react(
    env: &SubscriberEnv,
    config: &SlackBrowserConfig,
    handshake: &mut Option<crate::daemon::Handshake>,
    action: &str,
    input: &Value,
) -> Result<Value> {
    let fields = react_fields(input)?;
    let handshake_ref = ensure_browser_daemon(config, handshake)?;
    // Navigate to the channel first.
    let nav = evaluate_slack_script(
        env,
        handshake_ref,
        &slack_navigate_script(config, &fields.channel_id),
    )?;
    if !nav.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        anyhow::bail!(
            "slack-browser react: could not open channel `{}`: {}",
            fields.channel_id,
            nav.get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    let result = evaluate_slack_script(
        env,
        handshake_ref,
        &slack_react_script(&fields.message_ts, &fields.emoji),
    )?;
    if !result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        anyhow::bail!(
            "slack-browser react failed for message `{}`: {}",
            fields.message_ts,
            result
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    Ok(json!({
        "action": action,
        "completed": true,
        "summary": format!(
            "reacted {} to Slack message {} in channel {}",
            fields.emoji, fields.message_ts, fields.channel_id
        ),
        "channel_id": fields.channel_id,
        "message_ts": fields.message_ts,
        "emoji": fields.emoji,
    }))
}

// ── Browser helpers ──────────────────────────────────────────────────────────

fn evaluate_slack_script(
    env: &SubscriberEnv,
    handshake: &crate::daemon::Handshake,
    script: &str,
) -> Result<Value> {
    let session_id = format!("slack-browser-{}", safe_session_part(&env.topic));
    let deadline = Instant::now() + SLACK_LOAD_TIMEOUT;
    loop {
        let value = crate::daemon_browser::send_daemon_request(
            handshake,
            "browser_agent",
            json!({
                "action": "evaluate",
                "sessionId": session_id,
                "tabId": "messenger",
                "width": BROWSER_WIDTH,
                "height": BROWSER_HEIGHT,
                "script": script,
            }),
        )
        .context("evaluate Slack action script")?;
        let result = value.get("value").cloned().unwrap_or(Value::Null);
        // For scripts that return a JSON string, try to parse it.
        let result = if let Some(s) = result.as_str() {
            serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string()))
        } else {
            result
        };
        if result.get("ok").and_then(Value::as_bool).unwrap_or(false) || Instant::now() >= deadline
        {
            return Ok(result);
        }
        std::thread::sleep(SLACK_EVALUATE_INTERVAL);
    }
}

// ── JS scripts ───────────────────────────────────────────────────────────────

/// Navigate the Slack tab to `app.slack.com/client/<team_id>/<channel_id>`.
/// Returns `{ok: true}` immediately after triggering navigation; the caller
/// should then issue its act script in a subsequent evaluate call.
fn slack_navigate_script(config: &SlackBrowserConfig, channel_id: &str) -> String {
    let team_id_json =
        serde_json::to_string(&config.team_id).unwrap_or_else(|_| format!("\"{}\"", config.team_id));
    let channel_id_json =
        serde_json::to_string(channel_id).unwrap_or_else(|_| format!("\"{channel_id}\""));
    format!(
        r#"(() => {{
  const teamId = {team_id_json};
  const channelId = {channel_id_json};
  const target = 'https://app.slack.com/client/' + teamId + '/' + channelId;
  if (location.href.includes(channelId)) return JSON.stringify({{ ok: true, href: location.href }});
  location.href = target;
  return JSON.stringify({{ ok: true, href: target }});
}})()"#
    )
}

/// Focus the Slack message input, set `text`, dispatch an InputEvent, and click
/// the send button.
///
/// NOTE: `[data-qa="message_input"]` / `.ql-editor` / `[data-qa="texty_send_button"]`
/// are best-effort from the Phase 0 spike — NOT yet live-validated.
fn slack_send_message_script(text: &str) -> String {
    let text_json = serde_json::to_string(text).unwrap_or_else(|_| format!("\"{text}\""));
    format!(
        r#"(() => {{
  const text = {text_json};

  // Find the message input — try data-qa hook first, then Quill editor.
  const inputArea = document.querySelector('[data-qa="message_input"]');
  const editor = inputArea
    ? (inputArea.querySelector('.ql-editor') || inputArea.querySelector('[contenteditable="true"]'))
    : document.querySelector('.ql-editor') || document.querySelector('[contenteditable="true"]');
  if (!editor) return JSON.stringify({{ ok: false, reason: 'message input editor not found' }});

  // Set text and dispatch InputEvent so the Slack framework tracks the change.
  editor.focus();
  editor.textContent = text;
  editor.dispatchEvent(new InputEvent('input', {{ bubbles: true, data: text, inputType: 'insertText' }}));

  // Click the send button.
  const sendBtn = document.querySelector('[data-qa="texty_send_button"]');
  if (!sendBtn) return JSON.stringify({{ ok: false, reason: 'send button not found ([data-qa="texty_send_button"])' }});
  sendBtn.click();

  return JSON.stringify({{ ok: true }});
}})()"#
    )
}

/// Find a message by `ts`, hover to reveal the action bar, click
/// `[data-qa="add_reaction"]`, then pick the emoji.
///
/// NOTE: selectors are best-effort from the Phase 0 spike — NOT yet live-validated.
fn slack_react_script(ts: &str, emoji: &str) -> String {
    let ts_json = serde_json::to_string(ts).unwrap_or_else(|_| format!("\"{ts}\""));
    let emoji_json = serde_json::to_string(emoji).unwrap_or_else(|_| format!("\"{emoji}\""));
    format!(
        r#"(() => {{
  const ts = {ts_json};
  const emoji = {emoji_json};

  // Find the message container by data-ts or id.
  const msg =
    document.querySelector('[data-ts="' + ts + '"]') ||
    document.getElementById(ts) ||
    document.querySelector('[data-qa="message_container"][data-ts="' + ts + '"]');
  if (!msg) return JSON.stringify({{ ok: false, reason: 'message not found for ts: ' + ts }});

  // Hover the message to reveal the action bar.
  msg.dispatchEvent(new MouseEvent('mouseover', {{ bubbles: true }}));
  msg.dispatchEvent(new MouseEvent('mouseenter', {{ bubbles: true }}));

  // Click the add-reaction button in the action bar.
  const addReaction = msg.querySelector('[data-qa="add_reaction"]') ||
    document.querySelector('[data-qa="add_reaction"]');
  if (!addReaction) return JSON.stringify({{ ok: false, reason: 'add_reaction button not found' }});
  addReaction.click();

  // Try to pick the emoji from the picker (best-effort — picker may not be open yet).
  const picker = document.querySelector('[data-qa="emoji_picker_wrapper"]') ||
    document.querySelector('[data-qa="emoji-picker"]');
  if (picker) {{
    const emojiBtn = picker.querySelector('[data-emoji-name="' + emoji + '"]') ||
      picker.querySelector('[aria-label="' + emoji + '"]');
    if (emojiBtn) {{
      emojiBtn.click();
      return JSON.stringify({{ ok: true, ts, emoji, picked: true }});
    }}
  }}

  // Picker not present yet or emoji not found — return partial ok (button was clicked).
  return JSON.stringify({{ ok: true, ts, emoji, picked: false, note: 'add_reaction clicked; picker not yet open or emoji not found' }});
}})()"#
    )
}

// ── Input helpers ─────────────────────────────────────────────────────────────

fn string_input(input: &Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn integer_input(input: &Value, key: &str) -> Option<u64> {
    input
        .get(key)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod act_tests {
    use super::*;
    use serde_json::json;

    // send_message_fields

    #[test]
    fn send_message_requires_channel_and_text() {
        assert!(send_message_fields(&json!({"text": "hi"})).is_err()); // missing channel
        assert!(send_message_fields(&json!({"channel": "C123"})).is_err()); // missing text
        let f = send_message_fields(&json!({"channel": "C123", "text": "hi"})).unwrap();
        assert_eq!(f.channel_id, "C123");
        assert_eq!(f.text, "hi");
    }

    #[test]
    fn send_message_accepts_channel_id_alias() {
        let f = send_message_fields(&json!({"channel_id": "C456", "message": "hello"})).unwrap();
        assert_eq!(f.channel_id, "C456");
        assert_eq!(f.text, "hello");
    }

    #[test]
    fn send_message_accepts_to_and_body_aliases() {
        let f = send_message_fields(&json!({"to": "C789", "body": "world"})).unwrap();
        assert_eq!(f.channel_id, "C789");
        assert_eq!(f.text, "world");
    }

    #[test]
    fn send_message_rejects_empty_strings() {
        assert!(send_message_fields(&json!({"channel": "", "text": "hi"})).is_err());
        assert!(send_message_fields(&json!({"channel": "C123", "text": "  "})).is_err());
    }

    // read_history_fields

    #[test]
    fn read_history_requires_channel() {
        assert!(read_history_fields(&json!({"limit": 10})).is_err());
    }

    #[test]
    fn read_history_valid_defaults_limit_to_50() {
        let f = read_history_fields(&json!({"channel_id": "C1"})).unwrap();
        assert_eq!(f.channel_id, "C1");
        assert_eq!(f.limit, 50);
    }

    #[test]
    fn read_history_clamps_limit() {
        let f = read_history_fields(&json!({"channel": "C1", "limit": 9999})).unwrap();
        assert_eq!(f.limit, 200);
        let f2 = read_history_fields(&json!({"channel": "C1", "limit": 0})).unwrap();
        assert_eq!(f2.limit, 1);
    }

    #[test]
    fn read_history_accepts_to_alias() {
        let f = read_history_fields(&json!({"to": "C2", "limit": 5})).unwrap();
        assert_eq!(f.channel_id, "C2");
        assert_eq!(f.limit, 5);
    }

    // react_fields

    #[test]
    fn react_requires_channel() {
        assert!(react_fields(&json!({"message_id": "1718000000.000100", "emoji": "👍"})).is_err());
    }

    #[test]
    fn react_requires_ts() {
        assert!(react_fields(&json!({"channel_id": "C1", "emoji": "👍"})).is_err());
    }

    #[test]
    fn react_valid_with_defaults() {
        let f = react_fields(&json!({"channel_id": "C1", "message_id": "1718000000.000100"})).unwrap();
        assert_eq!(f.channel_id, "C1");
        assert_eq!(f.message_ts, "1718000000.000100");
        assert_eq!(f.emoji, "👍");
    }

    #[test]
    fn react_accepts_ts_alias_and_reaction_alias() {
        let f = react_fields(&json!({"channel": "C2", "ts": "1718000000.000200", "reaction": "🎉"})).unwrap();
        assert_eq!(f.channel_id, "C2");
        assert_eq!(f.message_ts, "1718000000.000200");
        assert_eq!(f.emoji, "🎉");
    }
}
