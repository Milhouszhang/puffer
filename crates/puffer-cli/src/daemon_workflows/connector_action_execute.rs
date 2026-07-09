use anyhow::{anyhow, bail, Context, Result};
use puffer_automation::AutomationStore;
use puffer_config::ConfigPaths;
use puffer_subscriptions::installed_connector_action_executor;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::subscriptions::send_authorization_for_send_message_input_with_source;

#[derive(Debug, Deserialize)]
struct ConnectorActionExecuteParams {
    #[serde(alias = "draftId")]
    draft_id: String,
    version: u64,
    #[serde(alias = "approvedMessage")]
    approved_message: Option<String>,
    /// Edited action input for non-`send_message` (`exact_action`) drafts. The
    /// human approves this exact input; its hash becomes the recorded
    /// `content_hash`. The destination fields must match the draft.
    #[serde(alias = "approvedInput")]
    approved_input: Option<Value>,
    #[serde(alias = "clientRequestId")]
    client_request_id: String,
}

/// Fields that identify the destination of a connector action. Edits on
/// approval may change body-like fields but never these.
const RECIPIENT_KEYS: &[&str] = &[
    "to",
    "target",
    "channel",
    "chat_id",
    "open_id",
    "user",
    "receive_id",
];

#[derive(Debug, Deserialize)]
struct ConnectorActionDraftStatusParams {
    #[serde(alias = "draftId")]
    draft_id: String,
    version: u64,
}

#[derive(Debug, Deserialize)]
struct AutomationPendingActionGetParams {
    #[serde(alias = "draftId")]
    draft_id: String,
    version: u64,
}

#[derive(Debug, Deserialize)]
struct AutomationPendingActionRejectParams {
    #[serde(alias = "draftId")]
    draft_id: String,
    version: u64,
    reason: String,
}

#[derive(Debug, Clone)]
struct AutomationDraftJoinFields {
    automation_id: String,
    automation_run_id: String,
    step_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AutomationConnectorActionDraftParams {
    pub(crate) automation_id: String,
    pub(crate) automation_run_id: String,
    pub(crate) step_id: String,
    pub(crate) connector_slug: String,
    pub(crate) connection_slug: String,
    pub(crate) action: String,
    pub(crate) input: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct CreatedAutomationConnectorActionDraft {
    pub(crate) draft_id: String,
    pub(crate) version: u64,
    pub(crate) status: String,
    pub(crate) connector_slug: String,
    pub(crate) connection_slug: String,
    pub(crate) action: String,
    pub(crate) recipient_stable_id: String,
    pub(crate) message: String,
    pub(crate) content_hash: String,
}

#[derive(Debug, Clone)]
struct ApprovedConnectorAction {
    input: Value,
    approved_message: Option<String>,
    content_hash: String,
    trigger_extra: Map<String, Value>,
}

trait ConnectorActionDraftExecutor: Send + Sync {
    fn execute_connector_action(
        &self,
        connector_slug: &str,
        action: &str,
        input: Value,
        trigger: Value,
    ) -> Result<Value>;
}

struct InstalledConnectorActionDraftExecutor;

impl ConnectorActionDraftExecutor for InstalledConnectorActionDraftExecutor {
    fn execute_connector_action(
        &self,
        connector_slug: &str,
        action: &str,
        input: Value,
        trigger: Value,
    ) -> Result<Value> {
        let executor = installed_connector_action_executor()
            .context("connector action executor is not installed")?;
        let summary = executor.run_connector_action(connector_slug, action, input, trigger)?;
        Ok(json!({
            "success": true,
            "summary": summary,
            "connector_slug": connector_slug,
            "action": action,
        }))
    }
}

static DRAFT_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

pub(crate) fn handle_connector_action_execute(
    state: &crate::daemon::DaemonState,
    params: &Value,
) -> Result<Value> {
    handle_connector_action_execute_with_executor(
        state.config_paths(),
        Some(state),
        params,
        &InstalledConnectorActionDraftExecutor,
    )
}

pub(crate) fn handle_connector_action_draft_status(
    paths: &ConfigPaths,
    params: &Value,
) -> Result<Value> {
    let params: ConnectorActionDraftStatusParams = serde_json::from_value(params.clone())
        .context("invalid connector action draft status params")?;
    let draft_id = non_empty(&params.draft_id)
        .context("missing draft_id")?
        .to_string();

    let path = outbound_action_drafts_path(paths);
    let store = read_outbound_action_draft_store(&path)?;
    let draft = find_draft(&store, &draft_id)?;
    validate_draft_identity(draft, &draft_id, params.version)?;
    validate_draft_provenance(draft)?;

    Ok(json!({
        "draftId": draft_id,
        "version": params.version,
        "status": string_field(draft, &["status"]).unwrap_or("unknown"),
        "error": draft.get("error").cloned().unwrap_or(Value::Null),
        "receipt": draft.get("receipt").cloned().unwrap_or(Value::Null),
        "updatedAtMs": draft
            .get("updated_at_ms")
            .or_else(|| draft.get("updatedAtMs"))
            .cloned()
            .unwrap_or(Value::Null),
    }))
}

pub(crate) fn handle_automation_pending_action_list(
    paths: &ConfigPaths,
    automations: &AutomationStore,
) -> Result<Value> {
    let path = outbound_action_drafts_path(paths);
    let store = read_outbound_action_draft_store(&path)?;
    let mut drafts = store
        .get("drafts")
        .and_then(Value::as_array)
        .context("draft store missing drafts array")?
        .iter()
        .filter_map(Value::as_object)
        .filter(|draft| is_pending_automation_draft(draft))
        .map(|draft| automation_pending_action_row(draft, automations))
        .collect::<Result<Vec<_>>>()?;
    drafts.sort_by(|left, right| {
        draft_sort_time(right)
            .cmp(&draft_sort_time(left))
            .then_with(|| string_sort_key(right, "id").cmp(&string_sort_key(left, "id")))
    });
    Ok(json!({ "drafts": drafts }))
}

pub(crate) fn handle_automation_pending_action_get(
    paths: &ConfigPaths,
    automations: &AutomationStore,
    params: &Value,
) -> Result<Value> {
    let params: AutomationPendingActionGetParams = serde_json::from_value(params.clone())
        .context("invalid automation pending action get params")?;
    let draft_id = non_empty(&params.draft_id)
        .context("missing draft_id")?
        .to_string();

    let path = outbound_action_drafts_path(paths);
    let store = read_outbound_action_draft_store(&path)?;
    let draft = find_draft(&store, &draft_id)?;
    validate_draft_identity(draft, &draft_id, params.version)?;
    validate_draft_provenance(draft)?;
    validate_pending_draft_status(draft)?;
    let join = automation_join_fields(draft)?;

    Ok(json!({
        "draft": automation_pending_action_detail(draft, automations, &join)?,
    }))
}

pub(crate) fn handle_automation_pending_action_reject(
    paths: &ConfigPaths,
    params: &Value,
) -> Result<Value> {
    let params: AutomationPendingActionRejectParams = serde_json::from_value(params.clone())
        .context("invalid automation pending action reject params")?;
    let draft_id = non_empty(&params.draft_id)
        .context("missing draft_id")?
        .to_string();
    let reason = non_empty(&params.reason)
        .context("missing rejection reason")?
        .to_string();

    let path = outbound_action_drafts_path(paths);
    let draft_lock = connector_draft_lock(&path, &draft_id);
    let _guard = draft_lock.lock().unwrap();
    let mut store = read_outbound_action_draft_store(&path)?;
    let draft = find_draft_mut(&mut store, &draft_id)?;
    validate_draft_identity(draft, &draft_id, params.version)?;
    validate_draft_provenance(draft)?;
    validate_pending_draft_status(draft)?;
    let join = automation_join_fields(draft)?;

    let rejected_at = OffsetDateTime::now_utc().to_string();
    crate::daemon_automation_runtime::mark_automation_run_rejected(
        paths,
        &join.automation_id,
        &join.automation_run_id,
        &reason,
    )?;

    draft.insert("status".to_string(), Value::String("rejected".to_string()));
    draft.insert("rejected_reason".to_string(), Value::String(reason.clone()));
    draft.insert(
        "rejected_at".to_string(),
        Value::String(rejected_at.clone()),
    );
    draft.insert("updated_at".to_string(), Value::String(rejected_at));
    draft.insert("updated_at_ms".to_string(), Value::from(now_ms()));
    write_outbound_action_draft_store(&path, &store)?;

    Ok(json!({
        "draft_id": draft_id,
        "version": params.version,
        "status": "rejected",
        "automation_id": join.automation_id,
        "automation_run_id": join.automation_run_id,
        "step_id": join.step_id,
    }))
}

pub(crate) fn create_automation_connector_action_draft(
    paths: &ConfigPaths,
    params: AutomationConnectorActionDraftParams,
) -> Result<CreatedAutomationConnectorActionDraft> {
    let automation_id = non_empty(&params.automation_id)
        .context("missing automation_id")?
        .to_string();
    let automation_run_id = non_empty(&params.automation_run_id)
        .context("missing automation_run_id")?
        .to_string();
    let step_id = non_empty(&params.step_id)
        .context("missing step_id")?
        .to_string();
    let connector_slug = non_empty(&params.connector_slug)
        .context("missing connector_slug")?
        .to_string();
    let connection_slug = non_empty(&params.connection_slug)
        .context("missing connection_slug")?
        .to_string();
    let action = non_empty(&params.action)
        .context("missing action")?
        .to_string();

    let mut action_input = params.input;
    let Some(object) = action_input.as_object_mut() else {
        bail!("Automation connector action draft input must be an object");
    };
    object
        .entry("connection_slug".to_string())
        .or_insert_with(|| Value::String(connection_slug.clone()));
    object
        .entry("connector_slug".to_string())
        .or_insert_with(|| Value::String(connector_slug.clone()));
    object
        .entry("action".to_string())
        .or_insert_with(|| Value::String(action.clone()));

    let is_send_message = action == "send_message";
    let recipient_stable_id = first_value(
        &action_input,
        &[
            "to",
            "target",
            "channel",
            "chat_id",
            "open_id",
            "user",
            "receive_id",
        ],
        true,
    )
    .unwrap_or_else(|| connection_slug.clone());
    let message = first_value(
        &action_input,
        &[
            "message",
            "text",
            "caption",
            "body",
            "summary",
            "description",
        ],
        false,
    )
    .unwrap_or_else(|| format!("{connector_slug}.{action}"));
    if is_send_message {
        first_value(
            &action_input,
            &[
                "to",
                "target",
                "channel",
                "chat_id",
                "open_id",
                "user",
                "receive_id",
            ],
            true,
        )
        .context("Automation connector action draft requires a send recipient")?;
        first_value(
            &action_input,
            &["message", "text", "caption", "body"],
            false,
        )
        .context("Automation connector action draft requires a message body")?;
    }

    let path = outbound_action_drafts_path(paths);
    let mut store = read_outbound_action_draft_store(&path)?;
    if let Some(existing) = store
        .get("drafts")
        .and_then(Value::as_array)
        .and_then(|drafts| {
            drafts.iter().filter_map(Value::as_object).find(|draft| {
                string_field(draft, &["automation_id", "automationId"])
                    == Some(automation_id.as_str())
                    && string_field(draft, &["automation_run_id", "automationRunId"])
                        == Some(automation_run_id.as_str())
                    && string_field(draft, &["step_id", "stepId"]) == Some(step_id.as_str())
                    && matches!(
                        draft.get("status").and_then(Value::as_str),
                        Some("draft_ready" | "send_failed")
                    )
            })
        })
    {
        return created_automation_draft_from_store(existing);
    }

    let drafts = store
        .get_mut("drafts")
        .and_then(Value::as_array_mut)
        .context("outbound action draft store missing drafts array")?;
    let version = drafts
        .iter()
        .filter_map(Value::as_object)
        .filter(|draft| {
            string_field(draft, &["automation_id", "automationId"]) == Some(automation_id.as_str())
                && string_field(draft, &["automation_run_id", "automationRunId"])
                    == Some(automation_run_id.as_str())
        })
        .filter_map(|draft| draft.get("version").and_then(Value::as_u64))
        .max()
        .unwrap_or(0)
        + 1;
    let draft_id = format!("draft-action-automation-{}", Uuid::new_v4());
    let now = now_ms();
    let created_at = OffsetDateTime::now_utc().to_string();
    let content_hash = if is_send_message {
        draft_content_hash(&recipient_stable_id, &message)
    } else {
        connector_action_content_hash(&connector_slug, &connection_slug, &action, &action_input)
    };
    let draft = json!({
        "id": draft_id,
        "created_by": "ConnectorActionDraft",
        "status": "draft_ready",
        "version": version,
        "connector_slug": connector_slug,
        "connection_slug": connection_slug,
        "action": action,
        "input": action_input,
        "recipient_stable_id": recipient_stable_id,
        "message": message,
        "content_hash": content_hash,
        "message_editable": is_send_message,
        "approval_kind": if is_send_message { "editable_message" } else { "exact_action" },
        "session_id": Value::Null,
        "turn_id": Value::Null,
        "created_at": created_at,
        "updated_at": created_at,
        "created_at_ms": now,
        "updated_at_ms": now,
        "approved_message": Value::Null,
        "approved_by": Value::Null,
        "approved_at": Value::Null,
        "client_request_id": Value::Null,
        "send_attempt_id": Value::Null,
        "receipt": Value::Null,
        "error": Value::Null,
        "automation_id": automation_id,
        "automation_run_id": automation_run_id,
        "step_id": step_id,
    });
    drafts.push(draft);
    write_outbound_action_draft_store(&path, &store)?;

    let stored = store
        .get("drafts")
        .and_then(Value::as_array)
        .and_then(|drafts| drafts.last())
        .and_then(Value::as_object)
        .context("created Automation connector action draft was not stored")?;
    created_automation_draft_from_store(stored)
}

/// After an Automation-originated draft is sent, either resume the suspended run
/// (top-level mid-flow gated action → run the continuation) or, when there is no
/// suspension (terminal-position gated action), mark the run completed. Resume
/// needs full daemon context; when it is unavailable (unit tests), fall back to
/// marking the run completed.
fn settle_automation_run_after_send(
    paths: &ConfigPaths,
    state: Option<&crate::daemon::DaemonState>,
    draft_id: &str,
    join: &AutomationDraftJoinFields,
    receipt: Value,
) -> Result<()> {
    if let Some(state) = state {
        let resumed = crate::daemon_automation_runtime::resume_automation_run(
            state,
            draft_id,
            receipt.clone(),
        )?;
        if resumed {
            return Ok(());
        }
    }
    crate::daemon_automation_runtime::mark_automation_run_approved(
        paths,
        &join.automation_id,
        &join.automation_run_id,
        receipt,
    )
}

fn handle_connector_action_execute_with_executor(
    paths: &ConfigPaths,
    state: Option<&crate::daemon::DaemonState>,
    params: &Value,
    executor: &dyn ConnectorActionDraftExecutor,
) -> Result<Value> {
    let params: ConnectorActionExecuteParams = serde_json::from_value(params.clone())
        .context("invalid connector action execute params")?;
    let draft_id = non_empty(&params.draft_id)
        .context("missing draft_id")?
        .to_string();
    let client_request_id = non_empty(&params.client_request_id)
        .context("missing client_request_id")?
        .to_string();

    let path = outbound_action_drafts_path(paths);
    let draft_lock = connector_draft_lock(&path, &draft_id);
    let _guard = draft_lock.lock().unwrap();
    let mut store = read_outbound_action_draft_store(&path)?;
    let draft = find_draft_mut(&mut store, &draft_id)?;
    validate_draft_identity(draft, &draft_id, params.version)?;
    validate_draft_provenance(draft)?;
    match draft.get("status").and_then(Value::as_str) {
        Some("sent") => {
            if let Ok(join) = automation_join_fields(draft) {
                settle_automation_run_after_send(
                    paths,
                    state,
                    &draft_id,
                    &join,
                    draft.get("receipt").cloned().unwrap_or(Value::Null),
                )?;
            }
            return Ok(json!({"status": "already_sent", "draftId": draft_id}));
        }
        Some("sending") | Some("send_uncertain") => bail!("duplicate_risk_ack_required"),
        Some("draft_ready" | "send_failed") => {}
        Some(other) => bail!("draft state `{other}` cannot be sent"),
        None => bail!("draft missing status"),
    }

    let connector_slug = string_field(draft, &["connector_slug", "connectorSlug"])
        .context("draft missing connector_slug")?
        .to_string();
    let connection_slug = string_field(draft, &["connection_slug", "connectionSlug"])
        .context("draft missing connection_slug")?
        .to_string();
    let action = string_field(draft, &["action"])
        .context("draft missing action")?
        .to_string();
    let approved = approved_connector_action(
        draft,
        &connector_slug,
        &connection_slug,
        &action,
        params.approved_message.as_deref(),
        params.approved_input.as_ref(),
        &draft_id,
        params.version,
        &client_request_id,
    )?;
    let attempt_id = Uuid::new_v4().to_string();
    let now = now_ms();
    draft.insert("status".to_string(), Value::String("sending".to_string()));
    draft.insert(
        "approved_message".to_string(),
        approved
            .approved_message
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    draft.insert(
        "approved_by".to_string(),
        Value::String("human".to_string()),
    );
    draft.insert("approved_at".to_string(), Value::from(now));
    draft.insert(
        "client_request_id".to_string(),
        Value::String(client_request_id.clone()),
    );
    draft.insert(
        "send_attempt_id".to_string(),
        Value::String(attempt_id.clone()),
    );
    draft.insert(
        "content_hash".to_string(),
        Value::String(approved.content_hash.clone()),
    );
    draft.insert("error".to_string(), Value::Null);
    write_outbound_action_draft_store(&path, &store)?;

    let mut trigger = json!({
        "type": "connector_action_execute",
        "envelope_id": client_request_id,
        "connection_id": connection_slug,
        "receivedAt": OffsetDateTime::now_utc().to_string(),
        "topic": connection_slug,
        "kind": "connector_action",
        "dedup_key": client_request_id,
        "text": "",
        "payload": approved.input.clone(),
    });
    if let Some(object) = trigger.as_object_mut() {
        object.extend(approved.trigger_extra);
    }
    let result =
        executor.execute_connector_action(&connector_slug, &action, approved.input, trigger);
    let mut store = read_outbound_action_draft_store(&path)?;
    let draft = find_draft_mut(&mut store, &draft_id)?;
    match result {
        Ok(receipt) => {
            draft.insert("status".to_string(), Value::String("sent".to_string()));
            draft.insert("receipt".to_string(), receipt.clone());
            draft.insert("error".to_string(), Value::Null);
            draft.insert("updated_at_ms".to_string(), Value::from(now_ms()));
            let automation_join = automation_join_fields(draft).ok();
            write_outbound_action_draft_store(&path, &store)?;
            if let Some(join) = automation_join {
                settle_automation_run_after_send(paths, state, &draft_id, &join, receipt.clone())?;
            }
            Ok(json!({
                "status": "sent",
                "draftId": draft_id,
                "receipt": receipt,
            }))
        }
        Err(error) => {
            draft.insert(
                "status".to_string(),
                Value::String("send_uncertain".to_string()),
            );
            draft.insert("error".to_string(), Value::String(format!("{error:#}")));
            draft.insert("updated_at_ms".to_string(), Value::from(now_ms()));
            write_outbound_action_draft_store(&path, &store)?;
            Err(anyhow!("connector_action_send_uncertain: {error:#}"))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn approved_connector_action(
    draft: &Map<String, Value>,
    connector_slug: &str,
    connection_slug: &str,
    action: &str,
    approved_message: Option<&str>,
    approved_input: Option<&Value>,
    draft_id: &str,
    version: u64,
    client_request_id: &str,
) -> Result<ApprovedConnectorAction> {
    let mut input = draft
        .get("input")
        .cloned()
        .context("draft missing action input")?;
    let Some(object) = input.as_object_mut() else {
        bail!("draft action input must be an object");
    };
    object.insert(
        "connection_slug".to_string(),
        Value::String(connection_slug.to_string()),
    );
    object.insert(
        "connector_slug".to_string(),
        Value::String(connector_slug.to_string()),
    );
    object.insert("action".to_string(), Value::String(action.to_string()));

    let mut trigger_extra = Map::new();
    if action == "send_message" {
        let approved_message = approved_message
            .and_then(non_empty)
            .context("missing approved_message")?
            .to_string();
        let recipient_stable_id =
            string_field(draft, &["recipient_stable_id", "recipientStableId"])
                .context("draft missing recipient_stable_id")?
                .to_string();
        validate_input_recipient(&input, &recipient_stable_id)?;
        input
            .as_object_mut()
            .context("draft action input must be an object")?
            .insert(
                "message".to_string(),
                Value::String(approved_message.clone()),
            );
        let authorization = send_authorization_for_send_message_input_with_source(
            "connector-action-draft",
            draft_id,
            version,
            action,
            &input,
            &approved_message,
            client_request_id,
        )?;
        let content_hash = authorization.content_hash.clone();
        trigger_extra.insert(
            "send_authorization".to_string(),
            serde_json::to_value(authorization).context("serialize send authorization")?,
        );
        return Ok(ApprovedConnectorAction {
            input,
            approved_message: Some(approved_message),
            content_hash,
            trigger_extra,
        });
    }

    if approved_message.and_then(non_empty).is_some() {
        bail!("approved_message is only supported for send_message connector actions");
    }

    // Editable exact action: the reviewer edited body-like fields before
    // approving. The approved input IS what they saw ("approve what you see"),
    // so its hash becomes the recorded content_hash. The destination is pinned
    // to the draft's so an edit cannot redirect the action.
    if let Some(edited) = approved_input {
        let mut edited_input = edited.clone();
        let object = edited_input
            .as_object_mut()
            .context("approved_input must be an object")?;
        object.insert(
            "connection_slug".to_string(),
            Value::String(connection_slug.to_string()),
        );
        object.insert(
            "connector_slug".to_string(),
            Value::String(connector_slug.to_string()),
        );
        object.insert("action".to_string(), Value::String(action.to_string()));

        // Pin the destination like-for-like: the recipient derived from the
        // edited input must match the one derived from the drafted input. (Using
        // the stored recipient_stable_id would wrongly reject actions whose input
        // carries no recipient field, since that falls back to the connection.)
        let approved_recipient = first_value(&edited_input, RECIPIENT_KEYS, true);
        let draft_recipient = draft
            .get("input")
            .and_then(|input| first_value(input, RECIPIENT_KEYS, true));
        if approved_recipient != draft_recipient {
            bail!("connector action draft destination cannot be changed on approval");
        }

        let content_hash =
            connector_action_content_hash(connector_slug, connection_slug, action, &edited_input);
        trigger_extra.insert(
            "action_approval".to_string(),
            json!({
                "draft_id": draft_id,
                "version": version,
                "connector_slug": connector_slug,
                "connection_slug": connection_slug,
                "action": action,
                "content_hash": content_hash,
                "client_request_id": client_request_id,
            }),
        );
        return Ok(ApprovedConnectorAction {
            input: edited_input,
            approved_message: None,
            content_hash,
            trigger_extra,
        });
    }

    let content_hash =
        connector_action_content_hash(connector_slug, connection_slug, action, &input);
    if string_field(draft, &["content_hash", "contentHash"]) != Some(content_hash.as_str()) {
        bail!("connector action draft input hash mismatch");
    }
    trigger_extra.insert(
        "action_approval".to_string(),
        json!({
            "draft_id": draft_id,
            "version": version,
            "connector_slug": connector_slug,
            "connection_slug": connection_slug,
            "action": action,
            "content_hash": content_hash,
            "client_request_id": client_request_id,
        }),
    );
    Ok(ApprovedConnectorAction {
        input,
        approved_message: None,
        content_hash,
        trigger_extra,
    })
}

fn connector_draft_lock(path: &Path, draft_id: &str) -> Arc<Mutex<()>> {
    let key = format!("{}::{draft_id}", path.display());
    let locks = DRAFT_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks.lock().unwrap();
    locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

pub(crate) fn outbound_action_drafts_path(paths: &ConfigPaths) -> PathBuf {
    paths.user_config_dir.join("outbound_action_drafts.json")
}

pub(crate) fn read_outbound_action_draft_store(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({"drafts": []}));
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut store: Value = serde_json::from_str(&raw)
        .with_context(|| format!("invalid draft store {}", path.display()))?;
    if store.get("drafts").and_then(Value::as_array).is_none() {
        store["drafts"] = json!([]);
    }
    Ok(store)
}

pub(crate) fn write_outbound_action_draft_store(path: &Path, store: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_string_pretty(store)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn find_draft_mut<'a>(store: &'a mut Value, draft_id: &str) -> Result<&'a mut Map<String, Value>> {
    store
        .get_mut("drafts")
        .and_then(Value::as_array_mut)
        .context("draft store missing drafts array")?
        .iter_mut()
        .find(|draft| draft.get("id").and_then(Value::as_str) == Some(draft_id))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("connector action draft `{draft_id}` not found"))
}

fn find_draft<'a>(store: &'a Value, draft_id: &str) -> Result<&'a Map<String, Value>> {
    store
        .get("drafts")
        .and_then(Value::as_array)
        .context("draft store missing drafts array")?
        .iter()
        .find(|draft| draft.get("id").and_then(Value::as_str) == Some(draft_id))
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("connector action draft `{draft_id}` not found"))
}

fn validate_draft_identity(draft: &Map<String, Value>, draft_id: &str, version: u64) -> Result<()> {
    if draft.get("id").and_then(Value::as_str) != Some(draft_id) {
        bail!("draft_id mismatch");
    }
    if draft.get("version").and_then(Value::as_u64) != Some(version) {
        bail!("draft version mismatch");
    }
    Ok(())
}

fn validate_draft_provenance(draft: &Map<String, Value>) -> Result<()> {
    if draft.get("created_by").and_then(Value::as_str) != Some("ConnectorActionDraft") {
        bail!("connector action draft was not created by ConnectorActionDraft");
    }
    Ok(())
}

fn validate_pending_draft_status(draft: &Map<String, Value>) -> Result<()> {
    match draft.get("status").and_then(Value::as_str) {
        Some("draft_ready" | "send_failed") => Ok(()),
        Some(other) => bail!("automation pending action draft state `{other}` cannot be reviewed"),
        None => bail!("automation pending action draft missing status"),
    }
}

fn created_automation_draft_from_store(
    draft: &Map<String, Value>,
) -> Result<CreatedAutomationConnectorActionDraft> {
    Ok(CreatedAutomationConnectorActionDraft {
        draft_id: string_field(draft, &["id"])
            .context("Automation connector action draft missing id")?
            .to_string(),
        version: draft
            .get("version")
            .and_then(Value::as_u64)
            .context("Automation connector action draft missing version")?,
        status: string_field(draft, &["status"])
            .context("Automation connector action draft missing status")?
            .to_string(),
        connector_slug: string_field(draft, &["connector_slug", "connectorSlug"])
            .context("Automation connector action draft missing connector_slug")?
            .to_string(),
        connection_slug: string_field(draft, &["connection_slug", "connectionSlug"])
            .context("Automation connector action draft missing connection_slug")?
            .to_string(),
        action: string_field(draft, &["action"])
            .context("Automation connector action draft missing action")?
            .to_string(),
        recipient_stable_id: string_field(draft, &["recipient_stable_id", "recipientStableId"])
            .context("Automation connector action draft missing recipient_stable_id")?
            .to_string(),
        message: string_field(draft, &["message"])
            .context("Automation connector action draft missing message")?
            .to_string(),
        content_hash: string_field(draft, &["content_hash", "contentHash"])
            .context("Automation connector action draft missing content_hash")?
            .to_string(),
    })
}

fn automation_join_fields(draft: &Map<String, Value>) -> Result<AutomationDraftJoinFields> {
    Ok(AutomationDraftJoinFields {
        automation_id: string_field(draft, &["automation_id", "automationId"])
            .context("draft missing automation_id")?
            .to_string(),
        automation_run_id: string_field(draft, &["automation_run_id", "automationRunId"])
            .context("draft missing automation_run_id")?
            .to_string(),
        step_id: string_field(draft, &["step_id", "stepId"])
            .context("draft missing step_id")?
            .to_string(),
    })
}

fn is_pending_automation_draft(draft: &Map<String, Value>) -> bool {
    draft.get("created_by").and_then(Value::as_str) == Some("ConnectorActionDraft")
        && matches!(
            draft.get("status").and_then(Value::as_str),
            Some("draft_ready" | "send_failed")
        )
        && automation_join_fields(draft).is_ok()
}

fn automation_pending_action_row(
    draft: &Map<String, Value>,
    automations: &AutomationStore,
) -> Result<Value> {
    let join = automation_join_fields(draft)?;
    let draft_id = string_field(draft, &["id"]).context("draft missing id")?;
    Ok(json!({
        "draft_id": draft_id,
        "version": draft.get("version").and_then(Value::as_u64).unwrap_or_default(),
        "status": string_field(draft, &["status"]).unwrap_or("unknown"),
        "automation_id": &join.automation_id,
        "automation_name": automation_name(automations, &join.automation_id),
        "automation_run_id": &join.automation_run_id,
        "step_id": &join.step_id,
        "connector_slug": string_field(draft, &["connector_slug", "connectorSlug"]).unwrap_or(""),
        "connection_slug": string_field(draft, &["connection_slug", "connectionSlug"]).unwrap_or(""),
        "action": string_field(draft, &["action"]).unwrap_or(""),
        "message_editable": draft_message_is_editable(draft),
        "approval_kind": approval_kind(draft),
        "recipient": recipient_label_or_stable_id(draft).unwrap_or_default(),
        "recipient_label": recipient_label(draft).map(Value::String).unwrap_or(Value::Null),
        "recipient_stable_id": recipient_stable_id(draft).map(Value::String).unwrap_or(Value::Null),
        "created_at": cloned_field(draft, &["created_at", "createdAt"]),
        "created_at_ms": cloned_field(draft, &["created_at_ms", "createdAtMs"]),
        "updated_at": cloned_field(draft, &["updated_at", "updatedAt"]),
        "updated_at_ms": cloned_field(draft, &["updated_at_ms", "updatedAtMs"]),
        "preview": preview_text(string_field(draft, &["message"]).unwrap_or("")),
    }))
}

fn automation_pending_action_detail(
    draft: &Map<String, Value>,
    automations: &AutomationStore,
    join: &AutomationDraftJoinFields,
) -> Result<Value> {
    let draft_id = string_field(draft, &["id"]).context("draft missing id")?;
    Ok(json!({
        "draft_id": draft_id,
        "version": draft.get("version").and_then(Value::as_u64).unwrap_or_default(),
        "status": string_field(draft, &["status"]).unwrap_or("unknown"),
        "automation_id": &join.automation_id,
        "automation_name": automation_name(automations, &join.automation_id),
        "automation_run_id": &join.automation_run_id,
        "step_id": &join.step_id,
        "connector_slug": string_field(draft, &["connector_slug", "connectorSlug"]).unwrap_or(""),
        "connection_slug": string_field(draft, &["connection_slug", "connectionSlug"]).unwrap_or(""),
        "action": string_field(draft, &["action"]).unwrap_or(""),
        "message_editable": draft_message_is_editable(draft),
        "approval_kind": approval_kind(draft),
        "recipient": recipient_label_or_stable_id(draft).unwrap_or_default(),
        "recipient_label": recipient_label(draft).map(Value::String).unwrap_or(Value::Null),
        "recipient_stable_id": recipient_stable_id(draft).map(Value::String).unwrap_or(Value::Null),
        "message": string_field(draft, &["message"]).unwrap_or(""),
        // `input` + `message_field` let the reviewer edit an exact_action draft:
        // the frontend updates `message_field` in `input` and sends it back as
        // `approvedInput`. Absent `message_field` means no editable body field.
        "input": draft.get("input").cloned().unwrap_or(Value::Null),
        "message_field": draft_message_field(draft).map(Value::from).unwrap_or(Value::Null),
        "destination_metadata": destination_metadata(draft),
        "content_hash": cloned_field(draft, &["content_hash", "contentHash"]),
        "created_at": cloned_field(draft, &["created_at", "createdAt"]),
        "created_at_ms": cloned_field(draft, &["created_at_ms", "createdAtMs"]),
        "updated_at": cloned_field(draft, &["updated_at", "updatedAt"]),
        "updated_at_ms": cloned_field(draft, &["updated_at_ms", "updatedAtMs"]),
        "error": draft.get("error").cloned().unwrap_or(Value::Null),
    }))
}

fn destination_metadata(draft: &Map<String, Value>) -> Value {
    let mut metadata = Map::new();
    insert_string_metadata(
        &mut metadata,
        "connector_slug",
        string_field(draft, &["connector_slug", "connectorSlug"]),
    );
    insert_string_metadata(
        &mut metadata,
        "connection_slug",
        string_field(draft, &["connection_slug", "connectionSlug"]),
    );
    insert_string_metadata(
        &mut metadata,
        "recipient",
        recipient_label_or_stable_id(draft),
    );
    insert_string_metadata(&mut metadata, "recipient_label", recipient_label(draft));
    insert_string_metadata(
        &mut metadata,
        "recipient_stable_id",
        recipient_stable_id(draft),
    );

    if let Some(input) = draft.get("input").and_then(Value::as_object) {
        for key in [
            "to",
            "target",
            "channel",
            "chat_id",
            "open_id",
            "user",
            "receive_id",
            "recipient_label",
            "recipientLabel",
            "recipient_name",
            "recipientName",
        ] {
            if let Some(value) = input
                .get(key)
                .filter(|value| destination_value_is_scalar(value))
            {
                metadata.insert(key.to_string(), value.clone());
            }
        }
    }

    Value::Object(metadata)
}

fn insert_string_metadata(
    metadata: &mut Map<String, Value>,
    key: &str,
    value: Option<impl Into<String>>,
) {
    if let Some(value) = value {
        metadata.insert(key.to_string(), Value::String(value.into()));
    }
}

fn destination_value_is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null
    )
}

fn draft_message_is_editable(draft: &Map<String, Value>) -> bool {
    draft
        .get("message_editable")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| string_field(draft, &["action"]) == Some("send_message"))
}

/// Body-like fields a draft's preview `message` is derived from, in priority
/// order. Used to tell the reviewer which input field the editable body maps to.
const BODY_KEYS: &[&str] = &["message", "text", "caption", "body", "summary", "description"];

/// The input field holding the editable body of an exact_action draft, if any.
fn draft_message_field(draft: &Map<String, Value>) -> Option<&'static str> {
    let object = draft.get("input").and_then(Value::as_object)?;
    BODY_KEYS
        .iter()
        .copied()
        .find(|key| object.get(*key).is_some_and(|value| !value.is_null()))
}

fn approval_kind(draft: &Map<String, Value>) -> &'static str {
    if draft_message_is_editable(draft) {
        "editable_message"
    } else {
        "exact_action"
    }
}

fn automation_name(automations: &AutomationStore, automation_id: &str) -> String {
    automations
        .get(automation_id)
        .map(|record| record.spec.name)
        .unwrap_or_else(|_| automation_id.to_string())
}

fn recipient_label_or_stable_id(draft: &Map<String, Value>) -> Option<String> {
    recipient_label(draft).or_else(|| recipient_stable_id(draft))
}

fn recipient_label(draft: &Map<String, Value>) -> Option<String> {
    string_field(
        draft,
        &[
            "recipient_label",
            "recipientLabel",
            "recipient_name",
            "recipientName",
        ],
    )
    .map(ToString::to_string)
    .or_else(|| {
        draft
            .get("input")
            .and_then(Value::as_object)
            .and_then(|input| {
                string_field(
                    input,
                    &[
                        "recipient_label",
                        "recipientLabel",
                        "recipient_name",
                        "recipientName",
                    ],
                )
                .map(ToString::to_string)
            })
    })
}

fn recipient_stable_id(draft: &Map<String, Value>) -> Option<String> {
    string_field(draft, &["recipient_stable_id", "recipientStableId"])
        .map(ToString::to_string)
        .or_else(|| {
            draft.get("input").and_then(|input| {
                first_value(
                    input,
                    &[
                        "to",
                        "target",
                        "channel",
                        "chat_id",
                        "open_id",
                        "user",
                        "receive_id",
                    ],
                    true,
                )
            })
        })
}

fn cloned_field(object: &Map<String, Value>, keys: &[&str]) -> Value {
    keys.iter()
        .find_map(|key| object.get(*key).cloned())
        .unwrap_or(Value::Null)
}

fn preview_text(message: &str) -> String {
    let collapsed = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = String::new();
    let mut truncated = false;
    for (index, ch) in collapsed.chars().enumerate() {
        if index >= 120 {
            truncated = true;
            break;
        }
        preview.push(ch);
    }
    if truncated {
        preview.push_str("...");
    }
    preview
}

fn draft_sort_time(draft: &Value) -> i128 {
    draft
        .get("created_at_ms")
        .or_else(|| draft.get("createdAtMs"))
        .and_then(Value::as_i64)
        .map(i128::from)
        .or_else(|| {
            draft
                .get("updated_at_ms")
                .or_else(|| draft.get("updatedAtMs"))
                .and_then(Value::as_i64)
                .map(i128::from)
        })
        .unwrap_or_default()
}

fn string_sort_key<'a>(draft: &'a Value, key: &str) -> &'a str {
    draft.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn validate_input_recipient(input: &Value, recipient_stable_id: &str) -> Result<()> {
    let target = first_value(
        input,
        &[
            "to",
            "target",
            "channel",
            "chat_id",
            "open_id",
            "user",
            "receive_id",
        ],
        true,
    )
    .context("draft input missing recipient")?;
    if target != recipient_stable_id {
        bail!("draft recipient no longer matches approved recipient");
    }
    Ok(())
}

fn string_field<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn first_value(input: &Value, keys: &[&str], accept_numbers: bool) -> Option<String> {
    keys.iter()
        .filter_map(|key| input.get(*key))
        .find_map(|value| match value {
            Value::String(value) => non_empty(value).map(ToString::to_string),
            Value::Number(value) if accept_numbers => Some(value.to_string()),
            _ => None,
        })
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn now_ms() -> u64 {
    OffsetDateTime::now_utc().unix_timestamp_nanos() as u64 / 1_000_000
}

fn draft_content_hash(recipient_stable_id: &str, text: &str) -> String {
    let canonical = json!({
        "recipient_stable_id": recipient_stable_id,
        "text": text,
        "media": [],
    });
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn connector_action_content_hash(
    connector_slug: &str,
    connection_slug: &str,
    action: &str,
    input: &Value,
) -> String {
    let canonical = json!({
        "connector_slug": connector_slug,
        "connection_slug": connection_slug,
        "action": action,
        "input": canonical_value(input),
    });
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
        Value::Object(object) => {
            let sorted = object
                .iter()
                .map(|(key, value)| (key.clone(), canonical_value(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use puffer_automation::{
        AgentEnvNodeRef, AutomationFlowSpec, AutomationReviewSpec, AutomationSource,
        AutomationSpec, AutomationStatus, AutomationStepSpec, AutomationTriggerSpec,
        AUTOMATION_SPEC_VERSION,
    };
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingExecutor {
        calls: Arc<Mutex<Vec<(String, String, Value, Value)>>>,
    }

    impl ConnectorActionDraftExecutor for RecordingExecutor {
        fn execute_connector_action(
            &self,
            connector_slug: &str,
            action: &str,
            input: Value,
            trigger: Value,
        ) -> Result<Value> {
            self.calls.lock().unwrap().push((
                connector_slug.to_string(),
                action.to_string(),
                input,
                trigger,
            ));
            Ok(json!({"ok": true}))
        }
    }

    #[derive(Default)]
    struct FailingExecutor {
        calls: Arc<Mutex<Vec<(String, String, Value, Value)>>>,
    }

    impl ConnectorActionDraftExecutor for FailingExecutor {
        fn execute_connector_action(
            &self,
            connector_slug: &str,
            action: &str,
            input: Value,
            trigger: Value,
        ) -> Result<Value> {
            self.calls.lock().unwrap().push((
                connector_slug.to_string(),
                action.to_string(),
                input,
                trigger,
            ));
            bail!("simulated send failure")
        }
    }

    fn write_draft_store(paths: &ConfigPaths, store: Value) {
        let path = outbound_action_drafts_path(paths);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, serde_json::to_string_pretty(&store).unwrap()).unwrap();
    }

    fn test_paths(root: &Path) -> ConfigPaths {
        ConfigPaths {
            workspace_root: root.join("workspace"),
            workspace_config_dir: root.join("workspace/.puffer"),
            user_config_dir: root.join("home/.puffer"),
            builtin_resources_dir: root.join("resources"),
        }
    }

    fn automation_store(paths: &ConfigPaths) -> AutomationStore {
        let store = AutomationStore::load(paths.user_config_dir.join("automations.json")).unwrap();
        store
            .create(
                "automation-1",
                sample_automation_spec("Morning Review"),
                AutomationStatus::Enabled,
            )
            .unwrap();
        store
    }

    fn sample_automation_spec(name: &str) -> AutomationSpec {
        AutomationSpec {
            spec_version: AUTOMATION_SPEC_VERSION,
            name: name.to_string(),
            description: None,
            source: AutomationSource::Blank,
            instructions: "Draft a reviewed reply.".to_string(),
            run_location: Default::default(),
            triggers: vec![AutomationTriggerSpec::PufferConnection {
                id: "incoming".to_string(),
                connection_slug: "telegram-user".to_string(),
                connector_slug: Some("telegram-login".to_string()),
                filter: None,
                ignore_filters: Vec::new(),
                contact_ids: Vec::new(),
                summary: None,
            }],
            flow: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "draft".to_string(),
                    node: AgentEnvNodeRef {
                        node_type: "transform_js".to_string(),
                        name: Some("Transform".to_string()),
                        trusted: Some(true),
                        config: BTreeMap::new(),
                    },
                    summary: None,
                }],
            },
            review: AutomationReviewSpec::default(),
        }
    }

    fn draft_store() -> Value {
        json!({
            "drafts": [{
                "id": "draft-1",
                "created_by": "ConnectorActionDraft",
                "status": "draft_ready",
                "version": 1,
                "connector_slug": "telegram-login",
                "connection_slug": "telegram-user",
                "action": "send_message",
                "input": {
                    "chat_id": 123456789,
                    "message": "draft body"
                },
                "recipient_stable_id": "123456789",
                "message": "draft body",
                "content_hash": "sha256:old"
            }]
        })
    }

    fn automation_draft_store() -> Value {
        json!({
            "drafts": [
                {
                    "id": "draft-auto-1",
                    "created_by": "ConnectorActionDraft",
                    "status": "draft_ready",
                    "version": 3,
                    "connector_slug": "telegram-login",
                    "connection_slug": "telegram-user",
                    "action": "send_message",
                    "input": {
                        "chat_id": 42,
                        "message": "automation body",
                        "trigger": {
                            "text": "source event body"
                        },
                        "root_output": {
                            "internal": "root"
                        },
                        "previous_output": {
                            "internal": "previous"
                        }
                    },
                    "recipient_stable_id": "42",
                    "message": "automation body",
                    "content_hash": "sha256:auto",
                    "created_at": "2026-07-08T12:00:00Z",
                    "created_at_ms": 1783512000000i64,
                    "automation_id": "automation-1",
                    "automation_run_id": "run-1",
                    "step_id": "send-step"
                },
                {
                    "id": "draft-chat-1",
                    "created_by": "ConnectorActionDraft",
                    "status": "draft_ready",
                    "version": 1,
                    "connector_slug": "telegram-login",
                    "connection_slug": "telegram-user",
                    "action": "send_message",
                    "input": {
                        "chat_id": 99,
                        "message": "chat body"
                    },
                    "recipient_stable_id": "99",
                    "message": "chat body",
                    "content_hash": "sha256:chat"
                },
                {
                    "id": "draft-auto-sent",
                    "created_by": "ConnectorActionDraft",
                    "status": "sent",
                    "version": 1,
                    "connector_slug": "telegram-login",
                    "connection_slug": "telegram-user",
                    "action": "send_message",
                    "input": {
                        "chat_id": 77,
                        "message": "sent body"
                    },
                    "recipient_stable_id": "77",
                    "message": "sent body",
                    "receipt": {
                        "ok": true,
                        "attempt": "sent-before-retry"
                    },
                    "automation_id": "automation-1",
                    "automation_run_id": "run-sent",
                    "step_id": "send-step"
                }
            ]
        })
    }

    fn generic_automation_draft_store() -> Value {
        let input = json!({
            "connection_slug": "demo-account",
            "connector_slug": "demo-connector",
            "action": "read_status",
            "query": "latest",
            "trigger": { "text": "source event body" },
            "root_output": { "internal": "root" },
        });
        let content_hash =
            connector_action_content_hash("demo-connector", "demo-account", "read_status", &input);
        json!({
            "drafts": [{
                "id": "draft-auto-generic",
                "created_by": "ConnectorActionDraft",
                "status": "draft_ready",
                "version": 1,
                "connector_slug": "demo-connector",
                "connection_slug": "demo-account",
                "action": "read_status",
                "input": input,
                "recipient_stable_id": "demo-account",
                "message": "demo-connector.read_status",
                "content_hash": content_hash,
                "message_editable": false,
                "approval_kind": "exact_action",
                "automation_id": "automation-1",
                "automation_run_id": "run-1",
                "step_id": "read-step"
            }]
        })
    }

    fn write_run_history(paths: &ConfigPaths) {
        write_run_history_for(paths, "run-1", "draft-auto-1");
    }

    fn write_run_history_for(paths: &ConfigPaths, run_id: &str, draft_id: &str) {
        let path = paths.user_config_dir.join("automation_runs.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "runs": [{
                    "id": run_id,
                    "automation_id": "automation-1",
                    "title": "Live run",
                    "status": "awaiting_approval",
                    "started_at_ms": 1783512000000i64,
                    "duration_ms": 0,
                    "summary": "Awaiting approval",
                    "source_event": "connector_event",
                    "compiled": true,
                    "runtime_status": "deployed",
                    "result": {
                        "draft_id": draft_id
                    },
                    "approval": {
                        "required": true,
                        "status": "draft_ready"
                    }
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn read_run_history(paths: &ConfigPaths) -> Value {
        let path = paths.user_config_dir.join("automation_runs.json");
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn connector_action_execute_sends_with_bound_authorization_and_consumes_draft() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        write_draft_store(&paths, draft_store());
        let executor = RecordingExecutor::default();

        let result = handle_connector_action_execute_with_executor(
            &paths,
            None,
            &json!({
                "draftId": "draft-1",
                "version": 1,
                "approvedMessage": "draft body",
                "clientRequestId": "request-1"
            }),
            &executor,
        )
        .unwrap();

        assert_eq!(result["status"], "sent");
        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "telegram-login");
        assert_eq!(calls[0].1, "send_message");
        assert_eq!(calls[0].2["message"], "draft body");
        let auth = &calls[0].3["send_authorization"];
        assert_eq!(auth["draft_id"], "draft-1");
        assert_eq!(auth["version"], 1);
        assert_eq!(auth["recipient_stable_id"], "123456789");
        assert_eq!(auth["action"], "send_message");

        let store = read_outbound_action_draft_store(&outbound_action_drafts_path(&paths)).unwrap();
        assert_eq!(store["drafts"][0]["status"], "sent");
    }

    #[test]
    fn connector_action_execute_does_not_send_twice_after_success() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut store = draft_store();
        store["drafts"][0]["status"] = json!("sent");
        write_draft_store(&paths, store);
        let executor = RecordingExecutor::default();

        let result = handle_connector_action_execute_with_executor(
            &paths,
            None,
            &json!({
                "draftId": "draft-1",
                "version": 1,
                "approvedMessage": "draft body",
                "clientRequestId": "request-2"
            }),
            &executor,
        )
        .unwrap();

        assert_eq!(result["status"], "already_sent");
        assert!(executor.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn connector_action_execute_approved_automation_draft_completes_run_history() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        write_draft_store(&paths, automation_draft_store());
        write_run_history(&paths);
        let executor = RecordingExecutor::default();

        let result = handle_connector_action_execute_with_executor(
            &paths,
            None,
            &json!({
                "draftId": "draft-auto-1",
                "version": 3,
                "approvedMessage": "automation body",
                "clientRequestId": "request-auto-approve"
            }),
            &executor,
        )
        .unwrap();

        assert_eq!(result["status"], "sent");
        assert_eq!(result["receipt"], json!({"ok": true}));
        let store = read_outbound_action_draft_store(&outbound_action_drafts_path(&paths)).unwrap();
        let draft = store["drafts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|draft| draft["id"] == "draft-auto-1")
            .unwrap();
        assert_eq!(draft["status"], "sent");

        let history = read_run_history(&paths);
        assert_eq!(history["runs"][0]["status"], "completed");
        assert_eq!(history["runs"][0]["approval"]["status"], "approved");
        assert_eq!(history["runs"][0]["result"]["draft_id"], "draft-auto-1");
        assert_eq!(history["runs"][0]["result"]["receipt"], json!({"ok": true}));
    }

    #[test]
    fn connector_action_execute_already_sent_automation_draft_is_idempotent_for_run_history() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        write_draft_store(&paths, automation_draft_store());
        write_run_history_for(&paths, "run-sent", "draft-auto-sent");
        let executor = RecordingExecutor::default();

        let result = handle_connector_action_execute_with_executor(
            &paths,
            None,
            &json!({
                "draftId": "draft-auto-sent",
                "version": 1,
                "approvedMessage": "sent body",
                "clientRequestId": "request-auto-already-sent"
            }),
            &executor,
        )
        .unwrap();

        assert_eq!(result["status"], "already_sent");
        assert!(executor.calls.lock().unwrap().is_empty());
        let history = read_run_history(&paths);
        assert_eq!(history["runs"][0]["status"], "completed");
        assert_eq!(history["runs"][0]["approval"]["status"], "approved");
        assert_eq!(
            history["runs"][0]["result"]["receipt"],
            json!({
                "ok": true,
                "attempt": "sent-before-retry"
            })
        );
    }

    #[test]
    fn connector_action_execute_send_failure_does_not_complete_automation_run() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        write_draft_store(&paths, automation_draft_store());
        write_run_history(&paths);
        let executor = FailingExecutor::default();

        let err = handle_connector_action_execute_with_executor(
            &paths,
            None,
            &json!({
                "draftId": "draft-auto-1",
                "version": 3,
                "approvedMessage": "automation body",
                "clientRequestId": "request-auto-fail"
            }),
            &executor,
        )
        .unwrap_err();

        assert!(err.to_string().contains("connector_action_send_uncertain"));
        assert_eq!(executor.calls.lock().unwrap().len(), 1);
        let store = read_outbound_action_draft_store(&outbound_action_drafts_path(&paths)).unwrap();
        let draft = store["drafts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|draft| draft["id"] == "draft-auto-1")
            .unwrap();
        assert_eq!(draft["status"], "send_uncertain");

        let history = read_run_history(&paths);
        assert_eq!(history["runs"][0]["status"], "awaiting_approval");
        assert_eq!(history["runs"][0]["approval"]["status"], "draft_ready");
        assert!(history["runs"][0]["result"].get("receipt").is_none());
    }

    #[test]
    fn connector_action_execute_approves_generic_automation_action_exact_input() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        write_draft_store(&paths, generic_automation_draft_store());
        write_run_history_for(&paths, "run-1", "draft-auto-generic");
        let executor = RecordingExecutor::default();

        let result = handle_connector_action_execute_with_executor(
            &paths,
            None,
            &json!({
                "draftId": "draft-auto-generic",
                "version": 1,
                "clientRequestId": "request-generic-approve"
            }),
            &executor,
        )
        .unwrap();

        assert_eq!(result["status"], "sent");
        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "demo-connector");
        assert_eq!(calls[0].1, "read_status");
        assert_eq!(calls[0].2["query"], "latest");
        assert_eq!(calls[0].2["trigger"]["text"], "source event body");
        assert!(calls[0].3.get("send_authorization").is_none());
        assert_eq!(
            calls[0].3["action_approval"]["draft_id"],
            "draft-auto-generic"
        );
        assert_eq!(calls[0].3["action_approval"]["action"], "read_status");

        let store = read_outbound_action_draft_store(&outbound_action_drafts_path(&paths)).unwrap();
        let draft = &store["drafts"][0];
        assert_eq!(draft["status"], "sent");
        assert_eq!(draft["approved_message"], Value::Null);

        let history = read_run_history(&paths);
        assert_eq!(history["runs"][0]["status"], "completed");
        assert_eq!(history["runs"][0]["approval"]["status"], "approved");
    }

    #[test]
    fn connector_action_execute_approves_edited_generic_action_input() {
        // A reviewer edits a body field of an exact_action draft before
        // approving; the executor receives the edited input and the recorded
        // content_hash reflects what was approved ("approve what you see").
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        write_draft_store(&paths, generic_automation_draft_store());
        write_run_history_for(&paths, "run-1", "draft-auto-generic");
        let executor = RecordingExecutor::default();

        let result = handle_connector_action_execute_with_executor(
            &paths,
            None,
            &json!({
                "draftId": "draft-auto-generic",
                "version": 1,
                "approvedInput": {
                    "connection_slug": "demo-account",
                    "connector_slug": "demo-connector",
                    "action": "read_status",
                    "query": "edited query",
                },
                "clientRequestId": "request-generic-edit"
            }),
            &executor,
        )
        .unwrap();

        assert_eq!(result["status"], "sent");
        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls[0].2["query"], "edited query");
        let approved_hash = connector_action_content_hash(
            "demo-connector",
            "demo-account",
            "read_status",
            &json!({
                "connection_slug": "demo-account",
                "connector_slug": "demo-connector",
                "action": "read_status",
                "query": "edited query",
            }),
        );
        assert_eq!(calls[0].3["action_approval"]["content_hash"], approved_hash);
    }

    #[test]
    fn connector_action_execute_rejects_edited_action_destination_change() {
        // An edit may change body fields but never the destination.
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        write_draft_store(&paths, generic_automation_draft_store());
        write_run_history_for(&paths, "run-1", "draft-auto-generic");
        let executor = RecordingExecutor::default();

        let err = handle_connector_action_execute_with_executor(
            &paths,
            None,
            &json!({
                "draftId": "draft-auto-generic",
                "version": 1,
                "approvedInput": {
                    "connection_slug": "demo-account",
                    "connector_slug": "demo-connector",
                    "action": "read_status",
                    "query": "latest",
                    "channel": "somewhere-else",
                },
                "clientRequestId": "request-generic-redirect"
            }),
            &executor,
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("destination cannot be changed"));
        assert!(executor.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn connector_action_draft_status_reads_sent_store_state() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut store = draft_store();
        store["drafts"][0]["status"] = json!("sent");
        store["drafts"][0]["receipt"] = json!({"ok": true});
        write_draft_store(&paths, store);

        let result = handle_connector_action_draft_status(
            &paths,
            &json!({
                "draftId": "draft-1",
                "version": 1
            }),
        )
        .unwrap();

        assert_eq!(result["draftId"], "draft-1");
        assert_eq!(result["version"], 1);
        assert_eq!(result["status"], "sent");
        assert_eq!(result["receipt"], json!({"ok": true}));
    }

    #[test]
    fn connector_action_execute_sends_edited_approved_message() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        write_draft_store(&paths, draft_store());
        let executor = RecordingExecutor::default();

        let result = handle_connector_action_execute_with_executor(
            &paths,
            None,
            &json!({
                "draftId": "draft-1",
                "version": 1,
                "approvedMessage": "changed body",
                "clientRequestId": "request-changed"
            }),
            &executor,
        )
        .unwrap();

        assert_eq!(result["status"], "sent");
        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].2["message"], "changed body");
        let store = read_outbound_action_draft_store(&outbound_action_drafts_path(&paths)).unwrap();
        assert_eq!(store["drafts"][0]["status"], "sent");
        assert_eq!(store["drafts"][0]["message"], "draft body");
        assert_eq!(store["drafts"][0]["approved_message"], "changed body");
    }

    #[test]
    fn automation_pending_action_list_only_returns_pending_automation_drafts() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let automations = automation_store(&paths);
        write_draft_store(&paths, automation_draft_store());

        let result = handle_automation_pending_action_list(&paths, &automations).unwrap();

        let drafts = result["drafts"].as_array().unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0]["draft_id"], "draft-auto-1");
        assert_eq!(drafts[0]["version"], 3);
        assert_eq!(drafts[0]["status"], "draft_ready");
        assert_eq!(drafts[0]["automation_id"], "automation-1");
        assert_eq!(drafts[0]["automation_name"], "Morning Review");
        assert_eq!(drafts[0]["automation_run_id"], "run-1");
        assert_eq!(drafts[0]["step_id"], "send-step");
        assert_eq!(drafts[0]["preview"], "automation body");
        assert!(drafts[0].get("message").is_none());
    }

    #[test]
    fn automation_pending_action_get_returns_detail_by_draft_id_and_version() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let automations = automation_store(&paths);
        write_draft_store(&paths, automation_draft_store());

        let result = handle_automation_pending_action_get(
            &paths,
            &automations,
            &json!({
                "draft_id": "draft-auto-1",
                "version": 3
            }),
        )
        .unwrap();

        let draft = &result["draft"];
        assert_eq!(draft["draft_id"], "draft-auto-1");
        assert_eq!(draft["version"], 3);
        assert_eq!(draft["automation_name"], "Morning Review");
        assert_eq!(draft["message"], "automation body");
        assert_eq!(draft["recipient_stable_id"], "42");
        assert_eq!(draft["message_editable"], true);
        assert_eq!(draft["approval_kind"], "editable_message");
        // The detail (unlike the list) carries full input so the reviewer can
        // edit an exact_action draft before approving.
        assert_eq!(draft["input"]["chat_id"], 42);
        assert_eq!(draft["destination_metadata"]["chat_id"], 42);
        assert_eq!(draft["destination_metadata"]["recipient_stable_id"], "42");
        assert!(draft["destination_metadata"].get("trigger").is_none());
        assert!(draft["destination_metadata"].get("root_output").is_none());
        assert!(draft["destination_metadata"]
            .get("previous_output")
            .is_none());
    }

    #[test]
    fn automation_pending_action_reject_updates_draft_and_run_history() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        write_draft_store(&paths, automation_draft_store());
        write_run_history(&paths);

        let result = handle_automation_pending_action_reject(
            &paths,
            &json!({
                "draft_id": "draft-auto-1",
                "version": 3,
                "reason": "Needs a clearer answer"
            }),
        )
        .unwrap();

        assert_eq!(result["status"], "rejected");
        let store = read_outbound_action_draft_store(&outbound_action_drafts_path(&paths)).unwrap();
        let draft = store["drafts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|draft| draft["id"] == "draft-auto-1")
            .unwrap();
        assert_eq!(draft["status"], "rejected");
        assert_eq!(draft["rejected_reason"], "Needs a clearer answer");
        assert!(!draft["rejected_at"].as_str().unwrap().is_empty());

        let history = read_run_history(&paths);
        assert_eq!(history["runs"][0]["status"], "rejected");
        assert_eq!(history["runs"][0]["approval"]["status"], "rejected");
        assert!(history["runs"][0]["summary"]
            .as_str()
            .unwrap()
            .contains("Needs a clearer answer"));
    }
}
