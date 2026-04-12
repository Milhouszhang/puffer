use anyhow::Result;
use puffer_config::ConfigPaths;
use puffer_resources::{AgentMemoryScope, AgentSpec};
use std::fs;
use std::path::{Path, PathBuf};

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
            lines.push(format!("Current memory:\n{trimmed}"));
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
}
