use crate::cli_args::ResourceScope;
use anyhow::{Context, Result};
use puffer_config::ConfigPaths;
use puffer_resources::McpServerSpec;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeDesktopConfig {
    #[serde(default)]
    mcp_servers: BTreeMap<String, ClaudeDesktopStdioServer>,
}

#[derive(Debug, Deserialize)]
struct ClaudeDesktopStdioServer {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// Imports Claude Desktop MCP stdio servers into the selected Puffer scope.
pub(crate) fn import_claude_desktop_mcp_servers(
    paths: &ConfigPaths,
    scope: ResourceScope,
) -> Result<String> {
    let Some(config_path) = discover_claude_desktop_config_path() else {
        return Ok("No MCP servers found in Claude Desktop configuration or configuration file does not exist.".to_string());
    };
    import_claude_desktop_mcp_servers_from_file(paths, scope, &config_path)
}

/// Imports Claude Desktop MCP stdio servers from one explicit config file path.
pub(crate) fn import_claude_desktop_mcp_servers_from_file(
    paths: &ConfigPaths,
    scope: ResourceScope,
    config_path: &Path,
) -> Result<String> {
    if !config_path.exists() {
        return Ok(
            "No MCP servers found in Claude Desktop configuration or configuration file does not exist."
                .to_string(),
        );
    }

    let config: ClaudeDesktopConfig = serde_json::from_str(
        &fs::read_to_string(config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", config_path.display()))?;

    if config.mcp_servers.is_empty() {
        return Ok("No MCP servers found in Claude Desktop configuration or configuration file does not exist.".to_string());
    }

    let mut imported = Vec::new();
    let mut skipped_env = Vec::new();
    let dir = resource_dir(paths, scope, "mcp_servers");
    fs::create_dir_all(&dir)?;

    for (name, server) in config.mcp_servers {
        if server.command.trim().is_empty() {
            continue;
        }
        if !server.env.is_empty() {
            skipped_env.push(name);
            continue;
        }
        let spec = McpServerSpec {
            id: name.clone(),
            display_name: name.clone(),
            transport: "stdio".to_string(),
            endpoint: String::new(),
            target: stdio_target(&server.command, &server.args),
            description: "Imported from Claude Desktop".to_string(),
            env: std::collections::BTreeMap::new(),
            inherit_env: true,
            timeout: None,
            connect_timeout: None,
            headers: std::collections::BTreeMap::new(),
            oauth: None,
        };
        write_yaml(&dir.join(format!("{name}.yaml")), &spec)?;
        imported.push(name);
    }

    if imported.is_empty() && skipped_env.is_empty() {
        return Ok("No MCP servers found in Claude Desktop configuration or configuration file does not exist.".to_string());
    }

    let mut message = if imported.is_empty() {
        format!(
            "Skipped {} Claude Desktop MCP server(s) because Puffer does not yet support importing per-server env settings.",
            skipped_env.len()
        )
    } else {
        format!(
            "Imported {} Claude Desktop MCP server(s) into {}.",
            imported.len(),
            dir.display()
        )
    };
    if !imported.is_empty() {
        message.push_str(&format!("\nImported: {}", imported.join(", ")));
    }
    if !skipped_env.is_empty() {
        message.push_str(&format!(
            "\nSkipped (env unsupported): {}",
            skipped_env.join(", ")
        ));
    }
    Ok(message)
}

fn discover_claude_desktop_config_path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")?;
        return Some(
            Path::new(&home).join("Library/Application Support/Claude/claude_desktop_config.json"),
        );
    }

    if cfg!(target_os = "linux") {
        if let Some(home) = std::env::var_os("USERPROFILE") {
            let normalized = home.to_string_lossy().replace('\\', "/");
            let wsl_path = normalized.replacen("C:", "/mnt/c", 1);
            let candidate = PathBuf::from(format!(
                "{wsl_path}/AppData/Roaming/Claude/claude_desktop_config.json"
            ));
            if candidate.exists() {
                return Some(candidate);
            }
        }
        let users_dir = Path::new("/mnt/c/Users");
        if let Ok(entries) = fs::read_dir(users_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if matches!(
                    name.as_str(),
                    "Public" | "Default" | "Default User" | "All Users"
                ) {
                    continue;
                }
                let candidate = entry
                    .path()
                    .join("AppData/Roaming/Claude/claude_desktop_config.json");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn resource_dir(paths: &ConfigPaths, scope: ResourceScope, kind: &str) -> PathBuf {
    match scope {
        ResourceScope::User => paths.user_config_dir.join("resources").join(kind),
        ResourceScope::Local | ResourceScope::Project => {
            paths.workspace_config_dir.join("resources").join(kind)
        }
    }
}

fn stdio_target(command: &str, args: &[String]) -> String {
    std::iter::once(command.to_string())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_yaml<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_yaml::to_string(value)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::import_claude_desktop_mcp_servers_from_file;
    use crate::cli_args::ResourceScope;
    use puffer_config::{ensure_workspace_dirs, ConfigPaths};
    use tempfile::tempdir;

    #[test]
    fn imports_stdio_servers_from_claude_desktop_config() {
        let tempdir = tempdir().unwrap();
        let paths = ConfigPaths::discover(tempdir.path());
        ensure_workspace_dirs(&paths).unwrap();
        let config_path = tempdir.path().join("claude_desktop_config.json");
        std::fs::write(
            &config_path,
            r#"{"mcpServers":{"docs":{"command":"npx","args":["-y","docs-server"]}}}"#,
        )
        .unwrap();

        let message =
            import_claude_desktop_mcp_servers_from_file(&paths, ResourceScope::Local, &config_path)
                .unwrap();

        let written = paths
            .workspace_config_dir
            .join("resources/mcp_servers/docs.yaml");
        assert!(written.exists());
        assert!(message.contains("Imported 1 Claude Desktop MCP server"));
        assert!(std::fs::read_to_string(written)
            .unwrap()
            .contains("target: npx -y docs-server"));
    }

    #[test]
    fn skips_servers_with_env_blocks() {
        let tempdir = tempdir().unwrap();
        let paths = ConfigPaths::discover(tempdir.path());
        ensure_workspace_dirs(&paths).unwrap();
        let config_path = tempdir.path().join("claude_desktop_config.json");
        std::fs::write(
            &config_path,
            r#"{"mcpServers":{"private":{"command":"node","args":["server.js"],"env":{"API_KEY":"secret"}}}}"#,
        )
        .unwrap();

        let message =
            import_claude_desktop_mcp_servers_from_file(&paths, ResourceScope::Local, &config_path)
                .unwrap();

        let written = paths
            .workspace_config_dir
            .join("resources/mcp_servers/private.yaml");
        assert!(!written.exists());
        assert!(message.contains("Skipped (env unsupported): private"));
    }

    #[test]
    fn imports_into_user_scope_when_requested() {
        let tempdir = tempdir().unwrap();
        let paths = ConfigPaths::discover(tempdir.path());
        ensure_workspace_dirs(&paths).unwrap();
        let config_path = tempdir.path().join("claude_desktop_config.json");
        std::fs::write(
            &config_path,
            r#"{"mcpServers":{"docs":{"command":"npx","args":["-y","docs-server"]}}}"#,
        )
        .unwrap();

        let message =
            import_claude_desktop_mcp_servers_from_file(&paths, ResourceScope::User, &config_path)
                .unwrap();

        let written = paths.user_config_dir.join("resources/mcp_servers/docs.yaml");
        assert!(written.exists());
        assert!(message.contains(&paths.user_config_dir.display().to_string()));
    }
}
