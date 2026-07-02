use super::*;
use serde_json::json;

fn snapshot_for(action: ActionSpec) -> Value {
    let tempdir = tempfile::tempdir().unwrap();
    let paths = ConfigPaths::discover(tempdir.path());
    workflow_binding_json(
        &paths,
        WorkflowBindingSpec {
            slug: "binding-telegram-user-action".to_string(),
            description: "Capture action".to_string(),
            connection_slug: "telegram-user".to_string(),
            connector_slug: Some("telegram-login".to_string()),
            status: WorkflowBindingStatus::Enabled,
            filter: None,
            ignore_filters: Vec::new(),
            contact_ids: Vec::new(),
            classify_prompt: None,
            classify_model: None,
            action,
            created_at_ms: 42,
        },
    )
}

#[test]
fn file_append_snapshot_keeps_legacy_fields_and_full_action() {
    let value = snapshot_for(
        serde_json::from_value(json!({
            "type": "file_append",
            "path": "/tmp/hi",
            "format": "text"
        }))
        .unwrap(),
    );

    assert_eq!(value["action_type"], "file_append");
    assert_eq!(value["action_path"], "/tmp/hi");
    assert_eq!(value["action_format"], "text");
    assert_eq!(
        value["action"],
        json!({
            "type": "file_append",
            "path": "/tmp/hi",
            "format": "text"
        })
    );
}

#[test]
fn run_workflow_snapshot_includes_original_action() {
    let value = snapshot_for(ActionSpec::RunWorkflow {
        workflow_id: "Wf_01HX.Runtime-123".to_string(),
    });

    assert_eq!(value["action_type"], "run_workflow");
    assert_eq!(
        value["action"],
        json!({
            "type": "run_workflow",
            "slug": "Wf_01HX.Runtime-123"
        })
    );
    assert!(value.get("input").is_none());
    assert!(value["action"].get("input").is_none());
}

#[test]
fn connector_act_snapshot_includes_complete_input() {
    let value = snapshot_for(ActionSpec::ConnectorAct {
        connector_slug: "telegram-login".to_string(),
        action: "send_message".to_string(),
        input: json!({
            "chat_id": "{{payload.chat_id}}",
            "text": "Seen: {{text}}",
            "metadata": {
                "source": "{{payload.sender.name}}"
            }
        }),
    });

    assert_eq!(value["action_type"], "connector_act");
    assert_eq!(
        value["action"],
        json!({
            "type": "connector_act",
            "connector_slug": "telegram-login",
            "action": "send_message",
            "input": {
                "chat_id": "{{payload.chat_id}}",
                "text": "Seen: {{text}}",
                "metadata": {
                    "source": "{{payload.sender.name}}"
                }
            }
        })
    );
}
