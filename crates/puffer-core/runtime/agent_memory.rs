use anyhow::Result;
use puffer_config::ConfigPaths;
use puffer_resources::{AgentMemoryScope, AgentSpec};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_AGENT_MEMORY_LINES: usize = 200;
const MAX_AGENT_MEMORY_BYTES: usize = 25_000;

/// Builds the persistent memory section for one agent when memory is enabled.
pub(super) fn build_agent_memory_section(cwd: &Path, agent: &AgentSpec) -> Result<Option<String>> {
    let Some(scope) = agent.memory.as_ref() else {
        return Ok(None);
    };
    let entrypoint = agent_memory_entrypoint(cwd, &agent.id, scope);
    if let Some(parent) = entrypoint.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let scope_note = match scope {
        AgentMemoryScope::User => {
            "This memory is user-scope. Keep learnings general enough to help in future projects."
        }
        AgentMemoryScope::Project => {
            "This memory is project-scope. Keep it specific to this repository and its conventions."
        }
        AgentMemoryScope::Local => {
            "This memory is local-scope. Keep it specific to this checkout or machine-local setup."
        }
    };

    let mut lines = vec![
        "# Persistent Agent Memory".to_string(),
        format!("Memory file: {}", entrypoint.display()),
        scope_note.to_string(),
        "Use this memory for durable agent-specific knowledge. Update it when you learn reusable facts that will help the same agent type in future runs.".to_string(),
    ];

    if let Ok(content) = fs::read_to_string(&entrypoint) {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            lines.push(format!(
                "Current memory:\n{}",
                truncate_memory_content(trimmed)
            ));
        }
    }

    Ok(Some(lines.join("\n\n")))
}

fn agent_memory_entrypoint(cwd: &Path, agent_type: &str, scope: &AgentMemoryScope) -> PathBuf {
    let sanitized = sanitize_agent_type(agent_type);
    match scope {
        AgentMemoryScope::User => user_claude_dir(cwd)
            .join("agent-memory")
            .join(sanitized)
            .join("MEMORY.md"),
        AgentMemoryScope::Project => cwd
            .join(".claude")
            .join("agent-memory")
            .join(sanitized)
            .join("MEMORY.md"),
        AgentMemoryScope::Local => cwd
            .join(".claude")
            .join("agent-memory-local")
            .join(sanitized)
            .join("MEMORY.md"),
    }
}

fn sanitize_agent_type(agent_type: &str) -> String {
    agent_type.replace(':', "-")
}

fn user_claude_dir(cwd: &Path) -> PathBuf {
    let paths = ConfigPaths::discover(cwd);
    paths
        .user_config_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| paths.user_config_dir.clone())
        .join(".claude")
}

fn truncate_memory_content(content: &str) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    let line_truncated = lines.len() > MAX_AGENT_MEMORY_LINES;
    let byte_truncated = content.len() > MAX_AGENT_MEMORY_BYTES;

    if !line_truncated && !byte_truncated {
        return content.to_string();
    }

    let mut truncated = if line_truncated {
        lines
            .into_iter()
            .take(MAX_AGENT_MEMORY_LINES)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        content.to_string()
    };

    if truncated.len() > MAX_AGENT_MEMORY_BYTES {
        let cut_at = truncated
            .char_indices()
            .take_while(|(index, _)| *index <= MAX_AGENT_MEMORY_BYTES)
            .map(|(index, _)| index)
            .last()
            .unwrap_or(MAX_AGENT_MEMORY_BYTES);
        truncated.truncate(cut_at);
    }

    truncated.push_str(
        "\n\n[Agent memory truncated to stay within prompt limits. Keep MEMORY.md concise.]",
    );
    truncated
}

#[cfg(test)]
mod tests {
    use super::build_agent_memory_section;
    use puffer_resources::{AgentMemoryScope, AgentSpec};
    use tempfile::tempdir;

    fn agent(memory: AgentMemoryScope) -> AgentSpec {
        AgentSpec {
            id: "reviewer".to_string(),
            description: "Reviews code".to_string(),
            prompt: "You are a reviewer.".to_string(),
            memory: Some(memory),
            ..AgentSpec::default()
        }
    }

    #[test]
    fn build_agent_memory_section_reads_project_memory_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(".claude/agent-memory/reviewer/MEMORY.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "Remember the release checklist.").unwrap();

        let rendered = build_agent_memory_section(temp.path(), &agent(AgentMemoryScope::Project))
            .unwrap()
            .unwrap();

        assert!(rendered.contains("# Persistent Agent Memory"));
        assert!(rendered.contains("Remember the release checklist."));
        assert!(rendered.contains(".claude/agent-memory/reviewer/MEMORY.md"));
    }

    #[test]
    fn build_agent_memory_section_uses_local_scope_path() {
        let temp = tempdir().unwrap();
        let rendered = build_agent_memory_section(temp.path(), &agent(AgentMemoryScope::Local))
            .unwrap()
            .unwrap();

        assert!(rendered.contains(".claude/agent-memory-local/reviewer/MEMORY.md"));
    }

    #[test]
    fn build_agent_memory_section_tolerates_uncreatable_parent() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join(".claude"), "occupied").unwrap();

        let rendered = build_agent_memory_section(temp.path(), &agent(AgentMemoryScope::Project))
            .unwrap()
            .unwrap();

        assert!(rendered.contains("Persistent Agent Memory"));
        assert!(rendered.contains(".claude/agent-memory/reviewer/MEMORY.md"));
    }

    #[test]
    fn build_agent_memory_section_truncates_large_memory_files() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(".claude/agent-memory/reviewer/MEMORY.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let oversized = (0..250)
            .map(|index| format!("line {index}: remember this detail"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, oversized).unwrap();

        let rendered = build_agent_memory_section(temp.path(), &agent(AgentMemoryScope::Project))
            .unwrap()
            .unwrap();

        assert!(rendered.contains("Agent memory truncated to stay within prompt limits"));
    }
}
