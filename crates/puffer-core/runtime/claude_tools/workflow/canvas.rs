//! `Canvas` workflow tool — render an agent-produced typed spec into a
//! self-contained HTML result page and open it.
//!
//! The agent supplies only the SPEC (semantic data: title, counts, findings,
//! tables). The HTML template embeds the design system (tokens, layout, dark +
//! light), so every canvas is consistent rather than free-form HTML. The page
//! is written under `<cwd>/.puffer/canvas/` and opened best-effort.

use crate::AppState;
use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const TEMPLATE: &str = include_str!("canvas_template.html");
const CANVAS_ID_PREFIX: &str = "canvas-";
const CANVAS_STATE_SUFFIX: &str = ".state.json";
const INLINE_CANVAS_ENV: &str = "PUFFER_DESKTOP_INLINE_CANVAS";

/// Render a canvas spec into a self-contained HTML document.
///
/// `bridge` (when present) lets canvas node actions continue the conversation:
/// the page calls back to the local daemon's `run_agent_turn` with the action's
/// bundled context, so a click starts a new agent turn — no prompt typing. It is
/// `{ url, token, sessionId }`; `null` when no daemon is reachable (CLI mode).
pub fn render_canvas(spec: &Value, bridge: Option<&Value>) -> String {
    let title = spec
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Canvas");
    let spec_json = serde_json::to_string(spec).unwrap_or_else(|_| "{}".to_string());
    let bridge_json = bridge
        .map(|b| serde_json::to_string(b).unwrap_or_else(|_| "null".into()))
        .unwrap_or_else(|| "null".into());
    TEMPLATE
        .replace("__TITLE__", &escape_html(title))
        .replace("__SPEC__", &spec_json)
        .replace("__BRIDGE__", &bridge_json)
}

/// Returns the workspace-local directory where Canvas HTML and state live.
pub fn canvas_dir(cwd: &Path) -> PathBuf {
    cwd.join(".puffer").join("canvas")
}

/// Returns a conservative timestamp in milliseconds for Canvas metadata.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Builds a stable Canvas id from a timestamp.
pub fn canvas_id_from_stamp(stamp: u64) -> String {
    format!("{CANVAS_ID_PREFIX}{stamp}")
}

/// Validates a Canvas id before it is used in a file path.
pub fn validate_canvas_id(canvas_id: &str) -> Result<()> {
    if !canvas_id.starts_with(CANVAS_ID_PREFIX) {
        anyhow::bail!("canvasId must start with `{CANVAS_ID_PREFIX}`");
    }
    if canvas_id.len() <= CANVAS_ID_PREFIX.len() {
        anyhow::bail!("canvasId is empty");
    }
    if !canvas_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        anyhow::bail!("canvasId contains unsupported characters");
    }
    Ok(())
}

/// Returns the state file path for a validated Canvas id.
pub fn canvas_state_path(cwd: &Path, canvas_id: &str) -> Result<PathBuf> {
    validate_canvas_id(canvas_id)?;
    Ok(canvas_dir(cwd).join(format!("{canvas_id}{CANVAS_STATE_SUFFIX}")))
}

/// Extracts initial values for interactive Canvas nodes from the spec tree.
pub fn initial_canvas_values(spec: &Value) -> Value {
    let mut values = Map::new();
    collect_initial_values(spec, &mut values);
    Value::Object(values)
}

fn collect_initial_values(node: &Value, values: &mut Map<String, Value>) {
    match node {
        Value::Array(items) => {
            for item in items {
                collect_initial_values(item, values);
            }
        }
        Value::Object(object) => {
            if let (Some(node_type), Some(id)) = (
                object.get("type").and_then(Value::as_str),
                object.get("id").and_then(Value::as_str),
            ) {
                if interactive_node_type(node_type) {
                    values.insert(id.to_string(), default_value_for_node(node_type, object));
                }
            }
            if let Some(children) = object.get("children") {
                collect_initial_values(children, values);
            }
            if let Some(body) = object.get("body") {
                collect_initial_values(body, values);
            }
        }
        _ => {}
    }
}

fn interactive_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "toggle" | "singleSelect" | "multiSelect" | "slider" | "barSelect" | "textInput"
            | "textarea" | "editableTable" | "mediaPicker" | "dependentSelect"
    )
}

fn default_value_for_node(node_type: &str, object: &Map<String, Value>) -> Value {
    if let Some(value) = object.get("value") {
        return value.clone();
    }
    match node_type {
        "toggle" => Value::Bool(false),
        "multiSelect" => Value::Array(Vec::new()),
        "slider" => object.get("min").cloned().unwrap_or_else(|| json!(0)),
        "singleSelect" | "barSelect" | "dependentSelect" => {
            first_option_id(object).unwrap_or(Value::Null)
        }
        "textInput" | "textarea" => Value::String(String::new()),
        "editableTable" => object
            .get("rows")
            .and_then(Value::as_array)
            .map(|rows| Value::Array(rows.clone()))
            .unwrap_or_else(|| json!([])),
        "mediaPicker" => {
            if object.get("multi").and_then(Value::as_bool) == Some(true) {
                json!([])
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    }
}

fn first_option_id(object: &Map<String, Value>) -> Option<Value> {
    object
        .get("options")
        .and_then(Value::as_array)
        .and_then(|options| options.first())
        .map(|option| {
            option
                .get("id")
                .or_else(|| option.get("label"))
                .cloned()
                .unwrap_or(Value::Null)
        })
}

/// Writes the initial Canvas state file for a rendered Canvas.
pub fn write_canvas_state(
    cwd: &Path,
    session_id: &str,
    canvas_id: &str,
    spec: &Value,
) -> Result<PathBuf> {
    let path = canvas_state_path(cwd, canvas_id)?;
    let now = now_ms();
    let state = json!({
        "canvasId": canvas_id,
        "sessionId": session_id,
        "title": spec.get("title").and_then(Value::as_str).unwrap_or("Canvas"),
        "values": initial_canvas_values(spec),
        "events": [],
        "createdAtMs": now,
        "updatedAtMs": now
    });
    write_state_file(&path, &state)?;
    Ok(path)
}

fn write_state_file(path: &Path, state: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create canvas dir {}", parent.display()))?;
    }
    std::fs::write(path, serde_json::to_string_pretty(state)?)
        .with_context(|| format!("failed to write canvas state {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Applies a browser-originated state patch to an existing Canvas state file.
pub fn apply_canvas_state_patch(
    cwd: &Path,
    session_id: &str,
    canvas_id: &str,
    patch: &Value,
) -> Result<Value> {
    let path = canvas_state_path(cwd, canvas_id)?;
    let mut state = read_canvas_state_file(&path)?;
    let stored_session = state
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if stored_session != session_id {
        anyhow::bail!("canvas state does not belong to session {session_id}");
    }
    let patch = patch
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("canvas state patch must be an object"))?;
    let values = state
        .get_mut("values")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("canvas state values must be an object"))?;
    for (key, value) in patch {
        validate_state_key(key)?;
        values.insert(key.clone(), value.clone());
    }
    let now = now_ms();
    state["updatedAtMs"] = json!(now);
    if let Some(events) = state.get_mut("events").and_then(Value::as_array_mut) {
        events.push(json!({
            "kind": "patch",
            "atMs": now,
            "keys": patch.keys().cloned().collect::<Vec<_>>()
        }));
    }
    write_state_file(&path, &state)?;
    Ok(state)
}

fn validate_state_key(key: &str) -> Result<()> {
    if key.trim().is_empty() {
        anyhow::bail!("canvas state key is empty");
    }
    if key.len() > 128 {
        anyhow::bail!("canvas state key is too long");
    }
    if key.chars().any(char::is_control) {
        anyhow::bail!("canvas state key contains control characters");
    }
    Ok(())
}

fn read_canvas_state_file(path: &Path) -> Result<Value> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read canvas state {}", path.display()))?;
    serde_json::from_str(&raw).context("failed to parse canvas state JSON")
}

/// Reads the current state for a specific Canvas id, or the latest state file.
pub fn read_canvas_state(cwd: &Path, session_id: &str, canvas_id: Option<&str>) -> Result<Value> {
    let path = if let Some(canvas_id) = canvas_id.filter(|id| *id != "latest") {
        canvas_state_path(cwd, canvas_id)?
    } else {
        latest_canvas_state_path(cwd, session_id)?
    };
    let state = read_canvas_state_file(&path)?;
    let stored_session = state
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if stored_session != session_id {
        anyhow::bail!("canvas state does not belong to session {session_id}");
    }
    Ok(state)
}

fn latest_canvas_state_path(cwd: &Path, session_id: &str) -> Result<PathBuf> {
    let dir = canvas_dir(cwd);
    let entries = std::fs::read_dir(&dir)
        .with_context(|| format!("failed to read canvas dir {}", dir.display()))?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(CANVAS_ID_PREFIX) || !name.ends_with(CANVAS_STATE_SUFFIX) {
            continue;
        }
        let state = read_canvas_state_file(&path)?;
        if state
            .get("sessionId")
            .and_then(Value::as_str)
            .map(|stored| stored != session_id)
            .unwrap_or(true)
        {
            continue;
        }
        let updated = state
            .get("updatedAtMs")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if best
            .as_ref()
            .map(|(current, _)| updated > *current)
            .unwrap_or(true)
        {
            best = Some((updated, path));
        }
    }
    best.map(|(_, path)| path)
        .ok_or_else(|| anyhow::anyhow!("no Canvas state files found"))
}

/// Executes the `CanvasState` workflow tool.
pub fn execute_canvas_state(state: &AppState, cwd: &Path, input: Value) -> Result<String> {
    let canvas_id = input
        .get("canvasId")
        .or_else(|| input.get("canvas_id"))
        .and_then(Value::as_str);
    let state = read_canvas_state(cwd, &state.session.id.to_string(), canvas_id)?;
    Ok(serde_json::to_string_pretty(&state)?)
}

/// Resolve a daemon callback bridge from the on-disk handshake, if a daemon is
/// running. Returns `{ url, token, sessionId }`. Security note: this embeds the
/// daemon token into the generated HTML so a browser page can call back; the
/// file is written owner-only (0600). Corbina-native embedding (no token on
/// disk) is the hardened follow-up.
fn resolve_bridge(session_id: &str) -> Option<Value> {
    let home = std::env::var_os("HOME")?;
    let handshake = Path::new(&home).join(".puffer").join("daemon.handshake");
    let raw = std::fs::read_to_string(&handshake).ok()?;
    let line = raw.lines().next()?;
    let hs: Value = serde_json::from_str(line).ok()?;
    let url = hs.get("url").and_then(Value::as_str)?;
    let token = hs.get("token").and_then(Value::as_str)?;
    Some(json!({ "url": url, "token": token, "sessionId": session_id }))
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Best-effort open of a file in the OS default app. Never fails the tool.
fn open_in_os(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    let prog = "open";
    #[cfg(target_os = "linux")]
    let prog = "xdg-open";
    #[cfg(target_os = "windows")]
    let prog = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let prog = "open";
    std::process::Command::new(prog)
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

fn should_open_browser_fallback() -> bool {
    !std::env::var_os(INLINE_CANVAS_ENV)
        .and_then(|value| value.into_string().ok())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

/// Normalizes a Canvas spec so `body` is always a JSON array.
///
/// The model frequently serializes array arguments as a JSON-encoded string;
/// that single case has one unambiguous decoding, so we coerce it. Anything that
/// does not resolve to an array is a contract violation the model must fix, so we
/// return an actionable error instead of rendering nothing and reporting success.
fn normalize_spec(mut input: Value) -> Result<Value> {
    if !input.is_object() {
        anyhow::bail!("Canvas input must be a JSON object (the canvas spec)");
    }
    let coerced = match input.get("body") {
        Some(Value::Array(_)) => None, // already well-formed (empty array is fine)
        Some(Value::String(raw)) => {
            let parsed = serde_json::from_str::<Value>(raw)
                .ok()
                .filter(|value| value.is_array())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Canvas `body` must be a JSON array of node objects, not a \
                         JSON-encoded string. Pass the components as an array."
                    )
                })?;
            Some(parsed)
        }
        _ => anyhow::bail!(
            "Canvas `body` is required and must be a JSON array of node objects \
             (not a string or scalar)."
        ),
    };
    if let Some(parsed) = coerced {
        input["body"] = parsed;
    }
    Ok(input)
}

/// Executes the `Canvas` workflow tool. `input` is the canvas spec itself.
pub fn execute_canvas(state: &mut AppState, cwd: &Path, input: Value) -> Result<String> {
    let input = normalize_spec(input)?;
    // `normalize_spec` guarantees `body` is a JSON array, so this never panics.
    let node_count = input["body"]
        .as_array()
        .expect("normalize_spec guarantees body is an array")
        .len();
    let stamp = now_ms();
    let canvas_id = canvas_id_from_stamp(stamp);
    let bridge = resolve_bridge(&state.session.id.to_string()).map(|mut bridge| {
        if let Some(object) = bridge.as_object_mut() {
            object.insert("canvasId".to_string(), json!(canvas_id.clone()));
        }
        bridge
    });
    let interactive = bridge.is_some();
    let html = render_canvas(&input, bridge.as_ref());

    let dir = canvas_dir(cwd);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create canvas dir {}", dir.display()))?;
    let path = dir.join(format!("{canvas_id}.html"));
    std::fs::write(&path, &html)
        .with_context(|| format!("failed to write canvas {}", path.display()))?;
    let state_path = write_canvas_state(cwd, &state.session.id.to_string(), &canvas_id, &input)?;
    // Owner-only: the page may embed the daemon token for action callbacks.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    let browser_fallback = should_open_browser_fallback();
    let opened = browser_fallback && open_in_os(&path);
    let note = if opened {
        "Canvas rendered and opened in the default browser."
    } else if browser_fallback {
        "Canvas rendered; open the file path in a browser to view it."
    } else {
        "Canvas rendered inline in Puffer Desktop."
    };

    Ok(serde_json::to_string_pretty(&json!({
        "status": "rendered",
        "canvasId": canvas_id,
        "path": path.display().to_string(),
        "statePath": state_path.display().to_string(),
        "nodes": node_count,
        "interactive": interactive,
        "opened": opened,
        "inline": !browser_fallback,
        "note": note
    }))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use puffer_config::PufferConfig;
    use puffer_session_store::SessionMetadata;
    use serde_json::json;
    use uuid::Uuid;

    fn temp_state(cwd: std::path::PathBuf) -> AppState {
        let session = SessionMetadata {
            id: Uuid::new_v4(),
            display_name: None,
            generated_title: None,
            cwd: cwd.clone(),
            created_at_ms: 0,
            updated_at_ms: 0,
            parent_session_id: None,
            slug: None,
            tags: Vec::new(),
            note: None,
        };
        AppState::new(PufferConfig::default(), cwd, session)
    }

    #[test]
    fn render_embeds_title_and_spec() {
        let spec = json!({
            "title": "Demo <Review>",
            "body": [{ "type": "finding", "severity": "high", "title": "Issue" }]
        });
        let html = render_canvas(&spec, None);
        assert!(
            html.contains("Demo &lt;Review&gt;"),
            "title escaped into <title>"
        );
        assert!(html.contains("\"finding\""), "spec JSON embedded");
        assert!(html.contains("application/json"), "spec script tag present");
        // the design-system renderer (component registry) is present
        assert!(html.contains("const COMP="), "component registry embedded");
        // no daemon bridge -> actions degrade
        assert!(
            html.contains("=null") || html.contains("= null") || html.contains(":null"),
            "bridge is null without a daemon"
        );
    }

    #[test]
    fn render_embeds_bridge_for_actions() {
        let spec = json!({ "title": "T", "body": [] });
        let bridge =
            json!({ "url": "ws://127.0.0.1:5555/ws", "token": "tk", "sessionId": "sess-1" });
        let html = render_canvas(&spec, Some(&bridge));
        assert!(
            html.contains("ws://127.0.0.1:5555/ws"),
            "daemon url embedded for callback"
        );
        assert!(
            html.contains("sess-1"),
            "session id embedded so actions continue the turn"
        );
    }

    #[test]
    fn execute_writes_html_file() {
        let tempdir = tempfile::tempdir().unwrap();
        let cwd = tempdir.path().to_path_buf();
        let mut state = temp_state(cwd.clone());
        let spec = json!({
            "title": "T",
            "body": [
                { "type": "metrics", "items": [{ "value": "1", "label": "x" }] },
                { "type": "section", "title": "S", "children": [
                    { "type": "finding", "severity": "high", "title": "y" }
                ]}
            ]
        });
        let out = execute_canvas(&mut state, &cwd, spec).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["status"], "rendered");
        assert_eq!(parsed["nodes"], 2);
        let path = parsed["path"].as_str().unwrap();
        assert!(std::path::Path::new(path).exists(), "html file written");
        let body = std::fs::read_to_string(path).unwrap();
        assert!(body.contains("<!doctype html>"));
        let state_path = parsed["statePath"].as_str().unwrap();
        assert!(
            std::path::Path::new(state_path).exists(),
            "state file written"
        );
        assert!(parsed["canvasId"].as_str().unwrap().starts_with("canvas-"));
    }

    #[test]
    fn normalize_spec_passes_through_array_body() {
        let spec = json!({ "title": "T", "body": [{ "type": "text", "value": "hi" }] });
        let out = normalize_spec(spec).unwrap();
        assert!(out["body"].is_array());
        assert_eq!(out["body"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn execute_coerces_stringified_array_body() {
        let tempdir = tempfile::tempdir().unwrap();
        let cwd = tempdir.path().to_path_buf();
        let mut state = temp_state(cwd.clone());
        // body arrives as a JSON-ENCODED STRING (the bug), not an array.
        let spec = json!({
            "title": "Script draft",
            "body": "[{\"type\": \"textarea\", \"id\": \"script\", \"rows\": 14, \"value\": \"hello\"}]"
        });
        let out = execute_canvas(&mut state, &cwd, spec).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["status"], "rendered");
        assert_eq!(parsed["nodes"], 1, "stringified array counted as one node");
        // state.json initial values include the interactive node's value.
        let state_path = parsed["statePath"].as_str().unwrap();
        let state_json: Value =
            serde_json::from_str(&std::fs::read_to_string(state_path).unwrap()).unwrap();
        assert_eq!(state_json["values"]["script"], "hello");
    }

    #[test]
    fn execute_rejects_non_array_string_body() {
        let tempdir = tempfile::tempdir().unwrap();
        let cwd = tempdir.path().to_path_buf();
        let mut state = temp_state(cwd.clone());
        let spec = json!({ "title": "T", "body": "not even json" });
        let err = execute_canvas(&mut state, &cwd, spec).unwrap_err();
        assert!(err.to_string().contains("body"), "error names the body field");
        assert!(!canvas_dir(&cwd).exists(), "no partial canvas artifacts written");
    }

    #[test]
    fn execute_rejects_missing_body() {
        let tempdir = tempfile::tempdir().unwrap();
        let cwd = tempdir.path().to_path_buf();
        let mut state = temp_state(cwd.clone());
        let spec = json!({ "title": "T" });
        let err = execute_canvas(&mut state, &cwd, spec).unwrap_err();
        assert!(err.to_string().contains("body"));
        assert!(!canvas_dir(&cwd).exists());
    }

    #[test]
    fn initial_values_collect_interactive_nodes() {
        let spec = json!({
            "title": "Inputs",
            "body": [{
                "type": "section",
                "children": [
                    { "type": "toggle", "id": "run-tests", "label": "Run tests", "value": true },
                    { "type": "multiSelect", "id": "areas", "label": "Areas", "value": ["auth"] },
                    { "type": "slider", "id": "confidence", "label": "Confidence", "min": 10 }
                ]
            }]
        });
        let values = initial_canvas_values(&spec);
        assert_eq!(values["run-tests"], true);
        assert_eq!(values["areas"], json!(["auth"]));
        assert_eq!(values["confidence"], 10);
    }

    #[test]
    fn initial_values_cover_new_primitives() {
        let spec = json!({ "body": [
            { "type": "textarea", "id": "script" },
            { "type": "editableTable", "id": "sb", "rows": [["shot-001","x"]] },
            { "type": "editableTable", "id": "noRows" },
            { "type": "editableTable", "id": "badRows", "rows": "corrupt" },
            { "type": "mediaPicker", "id": "pickOne" },
            { "type": "mediaPicker", "id": "pickExplicitFalse", "multi": false },
            { "type": "mediaPicker", "id": "pickMany", "multi": true },
        ]});
        let values = initial_canvas_values(&spec);
        assert_eq!(values["script"], json!(""));
        assert_eq!(values["sb"], json!([["shot-001","x"]]));
        assert_eq!(values["noRows"], json!([]));
        assert_eq!(values["badRows"], json!([]));
        assert_eq!(values["pickOne"], Value::Null);
        assert_eq!(values["pickExplicitFalse"], Value::Null);
        assert_eq!(values["pickMany"], json!([]));
    }

    #[test]
    fn initial_values_media_picker_kind_path_items_are_inert() {
        // `kind`/`path` on mediaPicker items are a pure frontend concern; the
        // backend only branches on `multi`, so video items must not perturb the
        // default `value` (empty array for multi:true, null otherwise).
        let spec = json!({ "body": [
            { "type": "mediaPicker", "id": "shotsMany", "multi": true, "items": [
                { "id": "shot-001", "kind": "video", "path": ".puffer/media/videos/a/1.mp4" },
                { "id": "shot-002", "kind": "video", "path": ".puffer/media/videos/a/2.mp4" },
            ] },
            { "type": "mediaPicker", "id": "shotsOne", "items": [
                { "id": "shot-003", "kind": "video", "path": ".puffer/media/videos/a/3.mp4" },
            ] },
        ]});
        let values = initial_canvas_values(&spec);
        assert_eq!(values["shotsMany"], json!([]));
        assert_eq!(values["shotsOne"], Value::Null);
    }

    #[test]
    fn initial_values_seed_dependent_select_first_option() {
        let spec = json!({
            "body": [
                { "type": "singleSelect", "id": "imgProvider",
                  "options": [{ "id": "byteplus", "label": "BytePlus" }] },
                { "type": "dependentSelect", "id": "imgModel", "dependsOn": "imgProvider",
                  "options": [
                      { "id": "seedream", "label": "Seedream", "group": "byteplus" },
                      { "id": "other", "label": "Other", "group": "elsewhere" }
                  ] }
            ]
        });
        let values = initial_canvas_values(&spec);
        assert_eq!(values["imgProvider"], json!("byteplus"));
        assert_eq!(values["imgModel"], json!("seedream"));
    }

    #[test]
    fn canvas_state_patch_updates_values() {
        let tempdir = tempfile::tempdir().unwrap();
        let cwd = tempdir.path().to_path_buf();
        let session_id = Uuid::new_v4().to_string();
        let spec = json!({
            "title": "Inputs",
            "body": [{ "type": "toggle", "id": "run-tests", "label": "Run tests" }]
        });
        write_canvas_state(&cwd, &session_id, "canvas-1", &spec).unwrap();

        let updated =
            apply_canvas_state_patch(&cwd, &session_id, "canvas-1", &json!({ "run-tests": true }))
                .unwrap();

        assert_eq!(updated["values"]["run-tests"], true);
        assert_eq!(updated["events"][0]["kind"], "patch");
    }

    #[test]
    fn canvas_state_patch_allows_natural_control_ids() {
        let tempdir = tempfile::tempdir().unwrap();
        let cwd = tempdir.path().to_path_buf();
        let session_id = Uuid::new_v4().to_string();
        let spec = json!({
            "title": "Inputs",
            "body": [{ "type": "toggle", "id": "Run tests?", "label": "Run tests?" }]
        });
        write_canvas_state(&cwd, &session_id, "canvas-1", &spec).unwrap();

        let updated = apply_canvas_state_patch(
            &cwd,
            &session_id,
            "canvas-1",
            &json!({ "Run tests?": true }),
        )
        .unwrap();

        assert_eq!(updated["values"]["Run tests?"], true);
    }

    #[test]
    fn canvas_state_rejects_path_escape_id() {
        let tempdir = tempfile::tempdir().unwrap();
        let cwd = tempdir.path().to_path_buf();
        let error = canvas_state_path(&cwd, "canvas-../oops").unwrap_err();
        assert!(error.to_string().contains("unsupported characters"));
    }

    #[test]
    fn canvas_state_read_requires_matching_session() {
        let tempdir = tempfile::tempdir().unwrap();
        let cwd = tempdir.path().to_path_buf();
        let spec = json!({ "title": "Inputs", "body": [] });
        write_canvas_state(&cwd, "session-a", "canvas-2", &spec).unwrap();

        let error = read_canvas_state(&cwd, "session-b", Some("canvas-2")).unwrap_err();
        assert!(error.to_string().contains("does not belong to session"));
    }

    #[test]
    fn execute_canvas_state_reads_latest_state() {
        let tempdir = tempfile::tempdir().unwrap();
        let cwd = tempdir.path().to_path_buf();
        let state = temp_state(cwd.clone());
        let session_id = state.session.id.to_string();
        let spec = json!({
            "title": "Inputs",
            "body": [{ "type": "textInput", "id": "note", "label": "Note", "value": "hello" }]
        });
        write_canvas_state(&cwd, &session_id, "canvas-3", &spec).unwrap();

        let out = execute_canvas_state(&state, &cwd, json!({ "canvasId": "latest" })).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(parsed["canvasId"], "canvas-3");
        assert_eq!(parsed["values"]["note"], "hello");
    }
}
