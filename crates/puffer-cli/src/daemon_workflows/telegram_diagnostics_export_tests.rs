use super::*;
use puffer_config::ConfigPaths;
use serde_json::{json, Value};

#[test]
fn telegram_diagnostics_export_writes_downloadable_message_observability_report() {
    let temp = tempfile::tempdir().unwrap();
    let paths = ConfigPaths {
        workspace_root: temp.path().join("workspace"),
        workspace_config_dir: temp.path().join("workspace/.puffer"),
        user_config_dir: temp.path().join("home/.puffer"),
        builtin_resources_dir: temp.path().join("resources"),
    };
    let output_dir = temp.path().join("Downloads");
    std::fs::create_dir_all(
        paths
            .user_config_dir
            .join("telegram-accounts/telegram-user"),
    )
    .unwrap();
    std::fs::write(
        paths
            .user_config_dir
            .join("telegram-accounts/telegram-user/message-diagnostics.ndjson"),
        [
            json!({
                "at_ms": 1_700_000_010_000i64,
                "stage": "emitted",
                "chat_id": 42,
                "message_id": 7,
                "date_ms": 1_700_000_000_000i64,
                "source_received_at_ms": 1_700_000_005_000i64,
                "text_prefix": "please help"
            })
            .to_string(),
            json!({
                "at_ms": 1_700_000_020_000i64,
                "stage": "suppressed",
                "chat_id": 42,
                "message_id": 8,
                "date_ms": 1_700_000_015_000i64,
                "notification_muted": true,
                "suppressed": true,
                "text_prefix": "muted message"
            })
            .to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    std::fs::write(
        paths
            .user_config_dir
            .join("telegram-accounts/telegram-user/message-diagnostics.ndjson.1"),
        json!({
            "at_ms": 1_700_000_001_000i64,
            "stage": "emitted",
            "chat_id": 42,
            "message_id": 6,
            "date_ms": 1_699_999_990_000i64,
            "source_received_at_ms": 1_699_999_995_000i64,
            "text_prefix": "rotated"
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        paths.user_config_dir.join("workflow_history.json"),
        serde_json::to_vec_pretty(&json!({
            "next_idx": 2,
            "runs": [{
                "idx": 1,
                "run_id": "run-1",
                "workflow_slug": "monitor-telegram-user",
                "trigger_info": {
                    "connection_slug": "telegram-user",
                    "connector_slug": "telegram-login",
                    "envelope_id": "env-1",
                    "received_at_ms": 1_700_000_011_000i64,
                    "topic": "telegram-user",
                    "kind": "message",
                    "dedup_key": "42:7",
                    "text": "please help with my private launch plan",
                    "payload": {
                        "chat_id": 42,
                        "message_id": 7,
                        "date_ms": 1_700_000_000_000i64,
                        "subscriber_received_at_ms": 1_700_000_005_000i64
                    }
                },
                "action_summary": {
                    "status": "completed",
                    "action": "triage_agent",
                    "summary": "Created task #1."
                },
                "action_log": [{
                    "action": "triage_agent",
                    "status": "completed",
                    "summary": "Created task #1.",
                    "started_at_ms": 1_700_000_012_000i64,
                    "ended_at_ms": 1_700_000_013_000i64
                }],
                "status": "completed",
                "started_at_ms": 1_700_000_011_000i64,
                "ended_at_ms": 1_700_000_013_000i64
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let result = handle_telegram_diagnostics_export(
        &paths,
        &json!({ "output_dir": output_dir, "limit": 10 }),
    )
    .unwrap();
    let path = result.get("path").and_then(Value::as_str).unwrap();
    let report: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(report["privacy_note"]
        .as_str()
        .unwrap()
        .contains("message snippets"));
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("private launch plan"));
    let messages = report.get("messages").and_then(Value::as_array).unwrap();

    let acted = messages
        .iter()
        .find(|message| message.get("dedup_key").and_then(Value::as_str) == Some("42:7"))
        .unwrap();
    assert_eq!(acted["message_time_ms"], json!(1_700_000_000_000i64));
    assert_eq!(acted["received_time_ms"], json!(1_700_000_005_000i64));
    assert_eq!(acted["subscriber"]["received"], json!(true));
    assert_eq!(acted["filter"]["filtered"], json!(false));
    assert_eq!(acted["trust_ai"]["entered"], json!(true));
    assert_eq!(acted["trust_ai"]["result"], json!("Created task #1."));

    let suppressed = messages
        .iter()
        .find(|message| message.get("dedup_key").and_then(Value::as_str) == Some("42:8"))
        .unwrap();
    assert_eq!(suppressed["subscriber"]["received"], json!(true));
    assert_eq!(suppressed["filter"]["filtered"], json!(true));
    assert_eq!(
        suppressed["filter"]["stage"],
        json!("subscriber_suppressed")
    );
    assert_eq!(suppressed["trust_ai"]["entered"], json!(false));

    let rotated = messages
        .iter()
        .find(|message| message.get("dedup_key").and_then(Value::as_str) == Some("42:6"))
        .unwrap();
    assert_eq!(rotated["subscriber"]["received"], json!(true));
    assert_eq!(rotated["filter"]["stage"], json!("no_router_history"));
}
