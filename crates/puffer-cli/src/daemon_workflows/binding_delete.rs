use anyhow::{Context, Result};
use puffer_config::ConfigPaths;
use puffer_core::subscription_manager;
use serde_json::Value;
use std::thread;

/// Deletes one subscription workflow binding and returns the refreshed snapshot.
pub(crate) fn handle_workflow_binding_delete(paths: &ConfigPaths, params: &Value) -> Result<Value> {
    let slug = params
        .get("slug")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("missing slug")?;
    let include_workflows = params
        .get("include_workflows")
        .or_else(|| params.get("includeWorkflows"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let manager = subscription_manager()?;
    manager.store().delete(slug)?;
    let refresh_manager = manager.clone();
    let refresh_slug = slug.to_string();
    if let Err(error) = thread::Builder::new()
        .name("puffer-binding-delete-refresh".to_string())
        .spawn(move || {
            if let Err(error) = refresh_manager.refresh_connection_consumers() {
                tracing::warn!(
                    binding = %refresh_slug,
                    error = %error,
                    "failed to refresh connection consumers after deleting workflow binding"
                );
            }
        })
    {
        tracing::warn!(
            binding = %slug,
            error = %error,
            "failed to spawn connection consumer refresh after deleting workflow binding"
        );
    }
    super::handle_workflow_list_with_runtime(paths, include_workflows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn delete_params_require_slug() {
        let tempdir = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::discover(tempdir.path());

        let error = handle_workflow_binding_delete(&paths, &json!({})).unwrap_err();

        assert!(error.to_string().contains("missing slug"));
    }

    #[test]
    fn delete_params_reject_blank_slug() {
        let tempdir = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::discover(tempdir.path());

        let error = handle_workflow_binding_delete(&paths, &json!({"slug": "  "})).unwrap_err();

        assert!(error.to_string().contains("missing slug"));
    }
}
