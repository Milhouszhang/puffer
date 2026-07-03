//! Resolves installed Claude plugin resource roots from
//! `~/.claude/plugins/installed_plugins.json` (schema version 2).
//!
//! Only the manifest decides which cached plugin versions are live —
//! stale directories under `plugins/cache` are never scanned.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct InstalledPluginsFile {
    #[serde(default)]
    plugins: BTreeMap<String, Vec<InstalledPluginEntry>>,
}

#[derive(Debug, Deserialize)]
struct InstalledPluginEntry {
    #[serde(rename = "installPath")]
    install_path: Option<PathBuf>,
}

/// Returns the resource root of every installed plugin entry.
///
/// Missing manifest → empty (plugins are optional). Unreadable or
/// unparsable manifest → empty plus one diagnostic; never an error.
pub(crate) fn installed_plugin_roots(
    claude_dir: &Path,
    diagnostics: &mut Vec<String>,
) -> Vec<PathBuf> {
    let manifest_path = claude_dir.join("plugins").join("installed_plugins.json");
    let raw = match fs::read_to_string(&manifest_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            diagnostics.push(format!(
                "skipped plugin resources: failed to read {}: {error}",
                manifest_path.display()
            ));
            return Vec::new();
        }
    };
    let manifest: InstalledPluginsFile = match serde_json::from_str(&raw) {
        Ok(manifest) => manifest,
        Err(error) => {
            diagnostics.push(format!(
                "skipped plugin resources: failed to parse {}: {error}",
                manifest_path.display()
            ));
            return Vec::new();
        }
    };
    let mut roots: Vec<PathBuf> = manifest
        .plugins
        .into_values()
        .flatten()
        .filter_map(|entry| entry.install_path)
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_manifest(claude_dir: &Path, body: &str) {
        let plugins_dir = claude_dir.join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();
        fs::write(plugins_dir.join("installed_plugins.json"), body).unwrap();
    }

    #[test]
    fn resolves_install_paths_across_plugins_and_scopes() {
        let temp = tempdir().unwrap();
        let claude = temp.path().join(".claude");
        write_manifest(
            &claude,
            r#"{
                "version": 2,
                "plugins": {
                    "ecc@market": [
                        {"scope": "user", "installPath": "/cache/ecc/2.0.0", "version": "2.0.0"},
                        {"scope": "project", "installPath": "/cache/ecc/2.0.0"},
                        {"scope": "user"}
                    ],
                    "superpowers@market": [
                        {"scope": "user", "installPath": "/cache/superpowers/6.1.1"}
                    ]
                }
            }"#,
        );
        let mut diagnostics = Vec::new();
        let roots = installed_plugin_roots(&claude, &mut diagnostics);
        assert_eq!(
            roots,
            vec![
                PathBuf::from("/cache/ecc/2.0.0"),
                PathBuf::from("/cache/superpowers/6.1.1"),
            ]
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn missing_manifest_is_silent() {
        let temp = tempdir().unwrap();
        let mut diagnostics = Vec::new();
        let roots = installed_plugin_roots(&temp.path().join(".claude"), &mut diagnostics);
        assert!(roots.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn corrupt_manifest_yields_single_diagnostic() {
        let temp = tempdir().unwrap();
        let claude = temp.path().join(".claude");
        write_manifest(&claude, "{not json");
        let mut diagnostics = Vec::new();
        let roots = installed_plugin_roots(&claude, &mut diagnostics);
        assert!(roots.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("installed_plugins.json"));
    }
}
