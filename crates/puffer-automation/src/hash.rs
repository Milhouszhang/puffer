use crate::spec::AutomationSpec;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Returns the hash used to decide whether compiled runtime artifacts are
/// stale. Only canonical [`AutomationSpec`] JSON is hashed; record metadata,
/// runtime artifacts, revision, and timestamps are intentionally excluded.
pub fn automation_spec_hash(spec: &AutomationSpec) -> Result<String, String> {
    let value = serde_json::to_value(spec)
        .map_err(|error| format!("failed to encode automation spec: {error}"))?;
    let canonical = canonical_json(&value)?;
    Ok(format!("sha256:{:x}", Sha256::digest(canonical.as_bytes())))
}

pub(crate) fn canonical_json(value: &Value) -> Result<String, String> {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            serde_json::to_string(value).map_err(|error| format!("canonical json scalar: {error}"))
        }
        Value::Array(items) => {
            let mut out = String::from("[");
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push_str(&canonical_json(item)?);
            }
            out.push(']');
            Ok(out)
        }
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let mut out = String::from("{");
            for (idx, key) in keys.into_iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                let encoded_key = serde_json::to_string(key)
                    .map_err(|error| format!("canonical json key: {error}"))?;
                out.push_str(&encoded_key);
                out.push(':');
                let value = map
                    .get(key)
                    .ok_or_else(|| format!("canonical json missing key `{key}`"))?;
                out.push_str(&canonical_json(value)?);
            }
            out.push('}');
            Ok(out)
        }
    }
}
