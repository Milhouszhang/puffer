use puffer_workflow::WorkflowRuntimeRecord;
use serde_json::Value;

/// Builds a concise human-readable summary for a runtime workflow execution response.
pub(crate) fn workflow_execute_summary(slug: &str, response: &WorkflowRuntimeRecord) -> String {
    let execution = record_string_for_keys(response, &["execution_id", "executionId", "id"]);
    let status = response.get("status").and_then(Value::as_str);
    match (execution.as_deref(), status) {
        (Some(execution), Some(status)) => {
            format!("workflow `{slug}` execution `{execution}` {status}")
        }
        (Some(execution), None) => format!("workflow `{slug}` execution `{execution}` started"),
        (None, Some(status)) => format!("workflow `{slug}` {status}"),
        (None, None) => format!("workflow `{slug}` executed"),
    }
}

/// Returns the first displayable value for one of the candidate runtime record keys.
pub(crate) fn record_string(record: &WorkflowRuntimeRecord, keys: &[&str]) -> String {
    record_string_for_keys(record, keys).unwrap_or_else(|| "-".to_string())
}

/// Returns the first displayable value for one of the candidate runtime record keys.
pub(crate) fn record_string_for_keys(
    record: &WorkflowRuntimeRecord,
    keys: &[&str],
) -> Option<String> {
    keys.iter()
        .find_map(|key| record.get(key).and_then(value_string))
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}
