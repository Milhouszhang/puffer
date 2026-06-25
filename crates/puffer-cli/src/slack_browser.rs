//! Slack web connector backed by daemon-managed CEF sessions.

#[path = "slack_browser_actions.rs"]
mod slack_browser_actions;

use anyhow::{Context, Result};
use puffer_config::ConfigPaths;
use puffer_subscriber_runtime::{Event, SubscriberCommand};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader, Lines};

pub(crate) const CONNECTOR_SLUG: &str = "slack-browser";
pub(crate) const STATE_ROOT: &str = "slack-browser-accounts";
const CONFIG_FILE: &str = "config.toml";
const SEEN_FILE: &str = "seen.json";
const POLL_INTERVAL: Duration = Duration::from_secs(30);
const ERROR_BACKOFF: Duration = Duration::from_secs(10);
const BROWSER_WIDTH: u32 = 1280;
const BROWSER_HEIGHT: u32 = 900;
const WEB_URL: &str = "https://app.slack.com/";

#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub(crate) struct SlackBrowserConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) workspace_root: Option<PathBuf>,
    #[serde(default)]
    pub(crate) connection: String,
    #[serde(default)]
    pub(crate) team_id: String,
    /// The logged-in user's Slack member id (U…). Used to derive is_outgoing.
    #[serde(default)]
    pub(crate) self_id: String,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub(crate) struct SeenState {
    #[serde(default)]
    pub(crate) initialized: bool,
    #[serde(default)]
    pub(crate) seen: BTreeSet<String>,
}

pub(crate) struct SubscriberEnv {
    pub(crate) state_dir: PathBuf,
    pub(crate) topic: String,
}

impl SubscriberEnv {
    fn from_env() -> Self {
        let state_dir = std::env::var_os("PUFFER_SKILL_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./state"));
        let topic = std::env::var("PUFFER_SKILL_TOPIC")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| CONNECTOR_SLUG.to_string());
        Self { state_dir, topic }
    }
}

struct CommandStream {
    lines: Lines<BufReader<tokio::io::Stdin>>,
}

impl CommandStream {
    fn new() -> Self {
        Self {
            lines: BufReader::new(tokio::io::stdin()).lines(),
        }
    }

    async fn next(&mut self) -> Result<Option<SubscriberCommand>> {
        loop {
            let Some(line) = self.lines.next_line().await? else {
                return Ok(None);
            };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<SubscriberCommand>(&line) {
                Ok(command) => return Ok(Some(command)),
                Err(error) => {
                    eprintln!("slack-browser: ignored malformed command: {error}")
                }
            }
        }
    }
}

pub(crate) fn state_dir(paths: &ConfigPaths, connection_slug: &str) -> PathBuf {
    paths.user_config_dir.join(STATE_ROOT).join(connection_slug)
}

pub(crate) fn save_config(
    paths: &ConfigPaths,
    workspace_root: &Path,
    connection_slug: &str,
    team_id: &str,
    self_id: &str,
) -> Result<SlackBrowserConfig> {
    let config = SlackBrowserConfig {
        workspace_root: Some(workspace_root.to_path_buf()),
        connection: connection_slug.to_string(),
        team_id: team_id.to_string(),
        self_id: self_id.to_string(),
    };
    let dir = state_dir(paths, connection_slug);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let raw = toml::to_string_pretty(&config).context("serialize Slack browser config")?;
    let path = dir.join(CONFIG_FILE);
    fs::write(&path, raw).with_context(|| format!("write {}", path.display()))?;
    Ok(config)
}

fn load_config_from_dir(state_dir: &Path) -> Result<Option<SlackBrowserConfig>> {
    let path = state_dir.join(CONFIG_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let config: SlackBrowserConfig =
        toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(config))
}

fn load_seen(state_dir: &Path) -> Result<SeenState> {
    let path = state_dir.join(SEEN_FILE);
    if !path.exists() {
        return Ok(SeenState::default());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

fn save_seen(state_dir: &Path, seen: &SeenState) -> Result<()> {
    fs::create_dir_all(state_dir).with_context(|| format!("create {}", state_dir.display()))?;
    let path = state_dir.join(SEEN_FILE);
    fs::write(&path, serde_json::to_vec_pretty(seen)?)
        .with_context(|| format!("write {}", path.display()))
}

fn safe_session_part(value: &str) -> String {
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
    output.trim_matches('-').to_string()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn emit_event(event: Event) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &event).context("encode subscriber event")?;
    stdout.write_all(b"\n").context("write subscriber event")?;
    stdout.flush().context("flush subscriber event")
}

fn emit_control(topic: &str, kind: &str, payload: Value) -> Result<()> {
    emit_event(Event {
        topic: topic.to_string(),
        kind: kind.to_string(),
        control: true,
        dedup_key: None,
        text: String::new(),
        payload,
    })
}

async fn wait_or_handle_command(
    env: &SubscriberEnv,
    config: Option<&SlackBrowserConfig>,
    handshake: &mut Option<crate::daemon::Handshake>,
    commands: &mut CommandStream,
    delay: Duration,
) -> Result<()> {
    tokio::select! {
        _ = tokio::time::sleep(delay) => Ok(()),
        command = commands.next() => {
            let Some(command) = command? else {
                tokio::time::sleep(delay).await;
                return Ok(());
            };
            handle_command(env, config, handshake, command)
        }
    }
}

fn handle_command(
    env: &SubscriberEnv,
    config: Option<&SlackBrowserConfig>,
    handshake: &mut Option<crate::daemon::Handshake>,
    command: SubscriberCommand,
) -> Result<()> {
    match command {
        SubscriberCommand::Custom { op, args } if op == "slack_browser_act" => {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let input = args.get("input").cloned().unwrap_or_else(|| json!({}));
            let Some(config) = config else {
                emit_control(
                    &env.topic,
                    "slack_browser_action_error",
                    json!({
                        "op": op,
                        "action": action,
                        "error": "slack-browser connector is not configured yet",
                    }),
                )?;
                return Ok(());
            };
            match slack_browser_actions::handle_action(env, config, handshake, action, &input) {
                Ok(payload) => {
                    emit_control(&env.topic, "slack_browser_action_complete", payload)
                }
                Err(error) => emit_control(
                    &env.topic,
                    "slack_browser_action_error",
                    json!({
                        "op": op,
                        "action": action,
                        "error": format!("{error:#}"),
                    }),
                ),
            }
        }
        SubscriberCommand::Custom { op, .. } => emit_control(
            &env.topic,
            "command_ignored",
            json!({ "op": op, "error": "unknown custom op" }),
        ),
        _ => emit_control(
            &env.topic,
            "command_ignored",
            json!({ "error": "slack-browser subscriber only handles slack_browser_act custom commands" }),
        ),
    }
}

pub(crate) async fn run_subscriber() -> anyhow::Result<()> {
    let env = SubscriberEnv::from_env();
    tokio::fs::create_dir_all(&env.state_dir)
        .await
        .with_context(|| format!("create {}", env.state_dir.display()))?;

    let mut seen = load_seen(&env.state_dir)?;
    eprintln!(
        "slack-browser: subscriber_start topic={} state_dir={} seen_initialized={} seen_count={}",
        env.topic,
        env.state_dir.display(),
        seen.initialized,
        seen.seen.len()
    );

    let mut handshake = None;
    let mut commands = CommandStream::new();

    loop {
        let Some(config) = load_config_from_dir(&env.state_dir)? else {
            eprintln!(
                "slack-browser: config_required topic={} state_dir={} reason=missing",
                env.topic,
                env.state_dir.display()
            );
            emit_control(&env.topic, "config_required", json!({}))?;
            wait_or_handle_command(&env, None, &mut handshake, &mut commands, POLL_INTERVAL)
                .await?;
            continue;
        };

        // Skeleton: real poll_once lands in a later task.
        wait_or_handle_command(
            &env,
            Some(&config),
            &mut handshake,
            &mut commands,
            POLL_INTERVAL,
        )
        .await?;
    }
}

fn fingerprint(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.trim().hash(&mut h);
    format!("{:x}", h.finish())
}

fn sidebar_dedup_key(conn: &str, channel_id: &str, fp: &str) -> String {
    format!("{conn}:{channel_id}:{fp}")
}

fn active_dedup_key(conn: &str, channel_id: &str, ts: &str) -> String {
    format!("{conn}:{channel_id}:{ts}")
}

fn should_emit(seen: &SeenState, key: &str) -> bool {
    if seen.seen.contains(key) {
        return false;
    }
    seen.initialized
}

#[allow(clippy::too_many_arguments)]
fn build_message_event(
    platform: &str,
    channel_id: &str,
    channel_name: &str,
    conversation_type: &str,
    sender: &str,
    sender_id: &str,
    text: &str,
    is_outgoing: bool,
    unread: bool,
    mention: bool,
    source: &str,
    ts: &str,
    dedup_key: &str,
) -> Event {
    Event {
        topic: platform.to_string(),
        kind: "message".to_string(),
        control: false,
        dedup_key: Some(dedup_key.to_string()),
        text: format!("{sender}\n{text}").trim().to_string(),
        payload: json!({
            "platform": platform,
            "event_type": "message",
            "channel_id": channel_id,
            "channel_name": channel_name,
            "conversation_type": conversation_type,
            "sender": sender,
            "sender_id": sender_id,
            "is_outgoing": is_outgoing,
            "unread": unread,
            "mention": mention,
            "source": source,
            "ts": ts,
            "receivedAtMs": now_ms(),
        }),
    }
}

#[cfg(test)]
mod emit_tests {
    use super::*;

    #[test]
    fn event_payload_has_monitor_keys_and_no_is_outgoing_in_schema_fields() {
        let ev = build_message_event(
            "slack-browser", "C123", "general", "channel",
            "Alice", "U999", "hi", true, false, false, "active", "1718000000.000100",
            "conn1:C123:1718000000.000100",
        );
        assert_eq!(ev.kind, "message");
        assert_eq!(ev.payload["platform"], "slack-browser");
        assert_eq!(ev.payload["channel_id"], "C123");
        assert_eq!(ev.payload["channel_name"], "general");
        assert_eq!(ev.payload["conversation_type"], "channel");
        assert_eq!(ev.payload["sender_id"], "U999");
        assert_eq!(ev.payload["is_outgoing"], true);
        assert_eq!(ev.payload["event_type"], "message");
        assert_eq!(ev.payload["source"], "active");
        assert_eq!(ev.payload["ts"], "1718000000.000100");
        assert_eq!(ev.dedup_key.as_deref(), Some("conn1:C123:1718000000.000100"));
    }

    #[test]
    fn first_poll_seeds_without_emitting() {
        let mut seen = SeenState::default();
        let key = active_dedup_key("c1", "C1", "1.1");
        assert!(!should_emit(&seen, &key)); // pre-init: no emit
        seen.seen.insert(key.clone());
        seen.initialized = true;
        assert!(should_emit(&seen, &active_dedup_key("c1", "C1", "2.2"))); // new post-init
        assert!(!should_emit(&seen, &key)); // already seen
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;
    use puffer_config::ConfigPaths;

    fn test_paths(tmp: &std::path::Path) -> ConfigPaths {
        ConfigPaths {
            workspace_root: tmp.join("workspace"),
            workspace_config_dir: tmp.join("workspace").join(".puffer"),
            user_config_dir: tmp.join("home").join(".puffer"),
            builtin_resources_dir: tmp.join("resources"),
        }
    }

    #[test]
    fn save_and_load_config_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = test_paths(tmp.path());
        let ws = std::path::Path::new("/workspace/ws");
        save_config(&paths, ws, "myconn", "T123", "U456").unwrap();
        let dir = state_dir(&paths, "myconn");
        assert!(dir.ends_with("slack-browser-accounts/myconn"), "got {}", dir.display());
        assert!(dir.starts_with(&paths.user_config_dir));
        let loaded = load_config_from_dir(&dir).unwrap().expect("config exists");
        assert_eq!(loaded.connection, "myconn");
        assert_eq!(loaded.team_id, "T123");
        assert_eq!(loaded.self_id, "U456");
        assert_eq!(loaded.workspace_root.as_deref(), Some(ws));
    }

    #[test]
    fn load_config_returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = test_paths(tmp.path());
        let dir = state_dir(&paths, "myconn");
        assert!(load_config_from_dir(&dir).unwrap().is_none());
    }
}
