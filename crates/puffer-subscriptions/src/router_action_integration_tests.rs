use crate::{
    process_envelope_result, ActionDispatcher, ActionSpec, BuiltinActionDispatcher, Classifier,
    ConnectorActionExecutor, DropAllSelfGate, FilterSpec, NullClassifier, SelfMessageGate,
    TaggedFilterSpec, WorkflowActionOutput, WorkflowActionRunner, WorkflowBindingSpec,
    WorkflowBindingStatus, WorkflowBindingStore,
};
use anyhow::Result;
use puffer_subscriber_runtime::{Event, EventEnvelope};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex as StdMutex};
use tempfile::{tempdir, TempDir};

struct WorkflowCall {
    workflow_id: String,
    trigger: Value,
}

#[derive(Default)]
struct RecordingWorkflowRunner {
    calls: StdMutex<Vec<WorkflowCall>>,
}

impl WorkflowActionRunner for RecordingWorkflowRunner {
    fn run_workflow(&self, slug: &str, trigger: Value) -> Result<WorkflowActionOutput> {
        self.calls.lock().unwrap().push(WorkflowCall {
            workflow_id: slug.to_string(),
            trigger,
        });
        Ok(WorkflowActionOutput::new(format!("ran {slug}")))
    }
}

struct ConnectorCall {
    connector_slug: String,
    action: String,
    input: Value,
    trigger: Value,
}

#[derive(Default)]
struct RecordingConnectorExecutor {
    calls: StdMutex<Vec<ConnectorCall>>,
}

impl ConnectorActionExecutor for RecordingConnectorExecutor {
    fn run_connector_action(
        &self,
        connector_slug: &str,
        action: &str,
        input: Value,
        trigger: Value,
    ) -> Result<String> {
        self.calls.lock().unwrap().push(ConnectorCall {
            connector_slug: connector_slug.to_string(),
            action: action.to_string(),
            input,
            trigger,
        });
        Ok(format!("ran {connector_slug}.{action}"))
    }
}

#[test]
fn telegram_event_matching_filter_triggers_run_workflow_binding() {
    let (_dir, store) = store_with(binding(
        ActionSpec::RunWorkflow {
            workflow_id: "Wf_01HX.Runtime-123".to_string(),
        },
        Some(FilterSpec::Tagged(TaggedFilterSpec::Jq {
            expression: ".chat_id == 424242".to_string(),
        })),
    ));
    let runner = Arc::new(RecordingWorkflowRunner::default());
    let dispatcher = Arc::new(BuiltinActionDispatcher::new());
    dispatcher.set_workflow_runner(runner.clone());
    let dispatcher: Arc<dyn ActionDispatcher> = dispatcher;
    let classifier: Arc<dyn Classifier> = Arc::new(NullClassifier);
    let gate: Arc<dyn SelfMessageGate> = Arc::new(DropAllSelfGate);
    let payload = json!({
        "chat_id": 424242,
        "sender_id": 777,
        "message_id": 9001,
        "kind": "dm",
        "sender": {
            "username": "alice"
        }
    });

    let result = process_envelope_result(
        &event("ship it", payload.clone()),
        &store,
        None,
        &dispatcher,
        &classifier,
        None,
        &gate,
    );

    assert!(result.matched);
    assert_eq!(result.acted, 1);
    assert_eq!(result.failed, 0);
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].workflow_id, "Wf_01HX.Runtime-123");
    assert_connection_trigger(&calls[0].trigger, "ship it", payload);
    assert_eq!(calls[0].trigger["payload"]["chat_id"], 424242);
    assert_eq!(calls[0].trigger["payload"]["sender_id"], 777);
}

#[test]
fn fake_event_triggers_connector_act_binding() {
    let (_dir, store) = store_with(binding(
        ActionSpec::ConnectorAct {
            connector_slug: "telegram-login".to_string(),
            action: "send_message".to_string(),
            input: json!({
                "chat_id": "{{payload.chat_id}}",
                "text": "Echo {{text}}",
                "metadata": {
                    "sender": "{{payload.sender.name}}"
                }
            }),
        },
        None,
    ));
    let executor = Arc::new(RecordingConnectorExecutor::default());
    let dispatcher = Arc::new(BuiltinActionDispatcher::new());
    dispatcher.set_connector_action_executor(executor.clone());
    let dispatcher: Arc<dyn ActionDispatcher> = dispatcher;
    let classifier: Arc<dyn Classifier> = Arc::new(NullClassifier);
    let gate: Arc<dyn SelfMessageGate> = Arc::new(DropAllSelfGate);
    let payload = json!({
        "chat_id": 42,
        "sender": {
            "name": "Alice"
        }
    });

    let result = process_envelope_result(
        &event("hello", payload.clone()),
        &store,
        None,
        &dispatcher,
        &classifier,
        None,
        &gate,
    );

    assert!(result.matched);
    assert_eq!(result.acted, 1);
    assert_eq!(result.failed, 0);
    let calls = executor.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].connector_slug, "telegram-login");
    assert_eq!(calls[0].action, "send_message");
    assert_eq!(
        calls[0].input,
        json!({
            "chat_id": "42",
            "text": "Echo hello",
            "metadata": {
                "sender": "Alice"
            }
        })
    );
    assert_connection_trigger(&calls[0].trigger, "hello", payload);
}

#[test]
fn unmatched_filter_does_not_trigger_action() {
    let (_dir, store) = store_with(binding(
        ActionSpec::RunWorkflow {
            workflow_id: "deploy-followup".to_string(),
        },
        Some(FilterSpec::Tagged(TaggedFilterSpec::Regex {
            pattern: "ship".to_string(),
            case_insensitive: false,
        })),
    ));
    let runner = Arc::new(RecordingWorkflowRunner::default());
    let dispatcher = Arc::new(BuiltinActionDispatcher::new());
    dispatcher.set_workflow_runner(runner.clone());
    let dispatcher: Arc<dyn ActionDispatcher> = dispatcher;
    let classifier: Arc<dyn Classifier> = Arc::new(NullClassifier);
    let gate: Arc<dyn SelfMessageGate> = Arc::new(DropAllSelfGate);

    let result = process_envelope_result(
        &event("ignore this", json!({"chat_id": 42})),
        &store,
        None,
        &dispatcher,
        &classifier,
        None,
        &gate,
    );

    assert!(!result.matched);
    assert_eq!(result.acted, 0);
    assert_eq!(result.failed, 0);
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[test]
fn regex_filter_matching_text_triggers_run_workflow_binding() {
    let (_dir, store) = store_with(binding(
        ActionSpec::RunWorkflow {
            workflow_id: "Wf_REGEX.Runtime-1".to_string(),
        },
        Some(FilterSpec::Tagged(TaggedFilterSpec::Regex {
            pattern: "ship it".to_string(),
            case_insensitive: true,
        })),
    ));
    let runner = Arc::new(RecordingWorkflowRunner::default());
    let dispatcher = Arc::new(BuiltinActionDispatcher::new());
    dispatcher.set_workflow_runner(runner.clone());
    let dispatcher: Arc<dyn ActionDispatcher> = dispatcher;
    let classifier: Arc<dyn Classifier> = Arc::new(NullClassifier);
    let gate: Arc<dyn SelfMessageGate> = Arc::new(DropAllSelfGate);

    let result = process_envelope_result(
        &event("SHIP IT now", json!({"chat_id": 1})),
        &store,
        None,
        &dispatcher,
        &classifier,
        None,
        &gate,
    );

    assert!(result.matched);
    assert_eq!(result.acted, 1);
    assert_eq!(result.failed, 0);
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].workflow_id, "Wf_REGEX.Runtime-1");
}

#[test]
fn json_shape_filter_matching_payload_triggers_run_workflow_binding() {
    let (_dir, store) = store_with(binding(
        ActionSpec::RunWorkflow {
            workflow_id: "Wf_JSON.Runtime-1".to_string(),
        },
        Some(FilterSpec::Json(json!({
            "chat_id": 424242,
            "sender": {"username": "alice"}
        }))),
    ));
    let runner = Arc::new(RecordingWorkflowRunner::default());
    let dispatcher = Arc::new(BuiltinActionDispatcher::new());
    dispatcher.set_workflow_runner(runner.clone());
    let dispatcher: Arc<dyn ActionDispatcher> = dispatcher;
    let classifier: Arc<dyn Classifier> = Arc::new(NullClassifier);
    let gate: Arc<dyn SelfMessageGate> = Arc::new(DropAllSelfGate);
    // The filter only requires a subset match, so extra payload fields
    // (sender.id) must not prevent the match.
    let payload = json!({
        "chat_id": 424242,
        "sender": {"username": "alice", "id": 777}
    });

    let result = process_envelope_result(
        &event("hi", payload),
        &store,
        None,
        &dispatcher,
        &classifier,
        None,
        &gate,
    );

    assert!(result.matched);
    assert_eq!(result.acted, 1);
    assert_eq!(runner.calls.lock().unwrap().len(), 1);
}

#[test]
fn json_shape_filter_mismatched_payload_does_not_trigger_run_workflow_binding() {
    let (_dir, store) = store_with(binding(
        ActionSpec::RunWorkflow {
            workflow_id: "Wf_JSON.Runtime-1".to_string(),
        },
        Some(FilterSpec::Json(json!({"chat_id": 424242}))),
    ));
    let runner = Arc::new(RecordingWorkflowRunner::default());
    let dispatcher = Arc::new(BuiltinActionDispatcher::new());
    dispatcher.set_workflow_runner(runner.clone());
    let dispatcher: Arc<dyn ActionDispatcher> = dispatcher;
    let classifier: Arc<dyn Classifier> = Arc::new(NullClassifier);
    let gate: Arc<dyn SelfMessageGate> = Arc::new(DropAllSelfGate);

    let result = process_envelope_result(
        &event("hi", json!({"chat_id": 1})),
        &store,
        None,
        &dispatcher,
        &classifier,
        None,
        &gate,
    );

    assert!(!result.matched);
    assert_eq!(result.acted, 0);
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[test]
fn any_filter_triggers_run_workflow_when_one_branch_matches() {
    let (_dir, store) = store_with(binding(
        ActionSpec::RunWorkflow {
            workflow_id: "Wf_ANY.Runtime-1".to_string(),
        },
        Some(FilterSpec::Tagged(TaggedFilterSpec::Any {
            filters: vec![
                FilterSpec::Tagged(TaggedFilterSpec::Jq {
                    expression: ".chat_id == 1".to_string(),
                }),
                FilterSpec::Tagged(TaggedFilterSpec::Regex {
                    pattern: "urgent".to_string(),
                    case_insensitive: true,
                }),
            ],
        })),
    ));
    let runner = Arc::new(RecordingWorkflowRunner::default());
    let dispatcher = Arc::new(BuiltinActionDispatcher::new());
    dispatcher.set_workflow_runner(runner.clone());
    let dispatcher: Arc<dyn ActionDispatcher> = dispatcher;
    let classifier: Arc<dyn Classifier> = Arc::new(NullClassifier);
    let gate: Arc<dyn SelfMessageGate> = Arc::new(DropAllSelfGate);

    // chat_id does not satisfy the jq branch, but the text satisfies the
    // regex branch, so the `any` filter as a whole must still match.
    let result = process_envelope_result(
        &event("this is URGENT", json!({"chat_id": 999})),
        &store,
        None,
        &dispatcher,
        &classifier,
        None,
        &gate,
    );

    assert!(result.matched);
    assert_eq!(result.acted, 1);
    assert_eq!(runner.calls.lock().unwrap().len(), 1);
}

fn store_with(binding: WorkflowBindingSpec) -> (TempDir, WorkflowBindingStore) {
    let dir = tempdir().unwrap();
    let store = WorkflowBindingStore::load(dir.path().join("bindings.json")).unwrap();
    store.create(binding).unwrap();
    (dir, store)
}

fn binding(action: ActionSpec, filter: Option<FilterSpec>) -> WorkflowBindingSpec {
    WorkflowBindingSpec {
        slug: "binding-telegram-user-action".to_string(),
        description: "Route telegram events".to_string(),
        connection_slug: "telegram-user".to_string(),
        connector_slug: Some("telegram-login".to_string()),
        status: WorkflowBindingStatus::Enabled,
        filter,
        ignore_filters: Vec::new(),
        contact_ids: Vec::new(),
        classify_prompt: None,
        classify_model: None,
        action,
        created_at_ms: 0,
    }
}

fn event(text: &str, payload: Value) -> EventEnvelope {
    EventEnvelope {
        envelope_id: "env-1".to_string(),
        subscriber_id: "telegram-user".to_string(),
        received_at_ms: 1_700_000_000_000,
        event: Event {
            topic: "telegram-user".to_string(),
            kind: "message".to_string(),
            control: false,
            dedup_key: Some("telegram-user:42".to_string()),
            text: text.to_string(),
            payload,
        },
    }
}

fn assert_connection_trigger(trigger: &Value, text: &str, payload: Value) {
    assert_eq!(
        trigger,
        &json!({
            "type": "connection",
            "envelope_id": "env-1",
            "connection_id": "telegram-user",
            "receivedAt": "2023-11-14T22:13:20Z",
            "topic": "telegram-user",
            "kind": "message",
            "dedup_key": "telegram-user:42",
            "text": text,
            "payload": payload
        })
    );
}
