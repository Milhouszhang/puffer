//! Acts for the Slack browser connector.
use anyhow::{bail, Result};
use serde_json::{json, Value};

pub(crate) fn handle_action(
    _env: &super::SubscriberEnv,
    _config: &super::SlackBrowserConfig,
    _handshake: &mut Option<crate::daemon::Handshake>,
    action: &str,
    _input: &Value,
) -> Result<Value> {
    bail!("slack-browser act `{action}` not implemented yet")
}
