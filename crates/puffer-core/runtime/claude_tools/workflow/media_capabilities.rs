use anyhow::{bail, Context, Result};
use puffer_media::{
    list_exact_media_capabilities_with_cache, ExactMediaDiscoveryCache, MediaCapabilityView,
};
use puffer_provider_registry::{AuthStore, ProviderRegistry};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MediaCapabilitiesInput {
    kind: String,
}

/// Lists connected (auth-available) media capabilities for one kind.
pub(crate) fn execute_media_capabilities(
    providers: &ProviderRegistry,
    auth_store: &AuthStore,
    discovery_cache: &ExactMediaDiscoveryCache,
    input: Value,
) -> Result<String> {
    let parsed: MediaCapabilitiesInput =
        serde_json::from_value(input).context("invalid MediaCapabilities input")?;
    let kind = match parsed.kind.trim() {
        "image" => "image",
        "video" => "video",
        other => bail!("unsupported media kind `{other}` (expected image or video)"),
    };
    let views =
        list_exact_media_capabilities_with_cache(providers, auth_store, Some(kind), discovery_cache);
    Ok(media_capabilities_json(kind, views))
}

/// Shapes capability views into the connected-only JSON the skill consumes.
fn media_capabilities_json(kind: &str, views: Vec<MediaCapabilityView>) -> String {
    let capabilities: Vec<Value> = views
        .into_iter()
        .filter(|view| view.status == "available")
        .map(|view| {
            json!({
                "providerId": view.provider_id,
                "providerDisplayName": view.provider_display_name,
                "modelId": view.model_id,
                "modelDisplayName": view.model_display_name,
                "status": view.status,
                "supportsImageSet": view.supports_image_set,
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({ "kind": kind, "capabilities": capabilities }))
        .expect("capability JSON serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use puffer_media::MediaCapabilityView;
    use serde_json::Value;

    fn view(provider: &str, model: &str, status: &str, supports_image_set: bool) -> MediaCapabilityView {
        MediaCapabilityView {
            provider_id: provider.to_string(),
            provider_display_name: format!("{provider} display"),
            model_id: model.to_string(),
            model_display_name: format!("{model} display"),
            kind: "image".to_string(),
            operation: "generate".to_string(),
            adapter: "images_json".to_string(),
            axes: Vec::new(),
            status: status.to_string(),
            source: "static".to_string(),
            reason: None,
            checked_at_ms: 0,
            supports_image_set,
        }
    }

    #[test]
    fn shapes_only_available_capabilities() {
        let out = media_capabilities_json(
            "image",
            vec![
                view("byteplus", "seedream", "available", true),
                view("worldrouter", "blocked", "unavailable", false),
            ],
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["kind"], "image");
        let caps = parsed["capabilities"].as_array().unwrap();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0]["providerId"], "byteplus");
        assert_eq!(caps[0]["modelId"], "seedream");
        assert_eq!(caps[0]["providerDisplayName"], "byteplus display");
        assert_eq!(caps[0]["status"], "available");
        assert_eq!(caps[0]["supportsImageSet"], true);
    }
}
