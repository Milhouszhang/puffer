use crate::{RegisterOptions, WorkflowDefinition, WorkflowRun};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// File-backed workflow store rooted in a workspace `.puffer` directory.
pub struct WorkflowStore {
    root: PathBuf,
    lock: Mutex<()>,
}

/// Snapshot of definitions and runs for daemon/UI consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStoreSnapshot {
    /// Registered workflow definitions.
    pub workflows: Vec<WorkflowDefinition>,
    /// Persisted workflow runs.
    pub runs: Vec<WorkflowRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DefinitionsFile {
    workflows: Vec<WorkflowDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunsFile {
    next_idx: u64,
    runs: Vec<WorkflowRun>,
}

impl Default for RunsFile {
    fn default() -> Self {
        Self {
            next_idx: 1,
            runs: Vec::new(),
        }
    }
}

impl WorkflowStore {
    /// Creates a file-backed workflow store from a workspace config directory.
    pub fn new(workspace_config_dir: impl Into<PathBuf>) -> Self {
        Self {
            root: workspace_config_dir.into().join("workflows"),
            lock: Mutex::new(()),
        }
    }

    /// Registers or replaces a workflow parsed from a JSON value.
    pub fn register_json(
        &self,
        value: Value,
        options: RegisterOptions,
    ) -> Result<WorkflowDefinition> {
        let definition = WorkflowDefinition::from_json(value, options)?;
        self.upsert(definition)
    }

    /// Registers or replaces a workflow definition.
    pub fn upsert(&self, definition: WorkflowDefinition) -> Result<WorkflowDefinition> {
        let _guard = self.lock.lock().unwrap();
        let mut file = self.load_definitions_unlocked()?;
        file.workflows.retain(|item| item.slug != definition.slug);
        file.workflows.push(definition.clone());
        file.workflows.sort_by(|a, b| a.slug.cmp(&b.slug));
        self.write_json(&self.definitions_path(), &file)?;
        Ok(definition)
    }

    /// Returns all workflow definitions sorted by slug.
    pub fn list(&self) -> Result<Vec<WorkflowDefinition>> {
        let _guard = self.lock.lock().unwrap();
        Ok(self.load_definitions_unlocked()?.workflows)
    }

    /// Returns one workflow definition by slug.
    pub fn get(&self, slug: &str) -> Result<Option<WorkflowDefinition>> {
        Ok(self.list()?.into_iter().find(|item| item.slug == slug))
    }

    /// Appends a run and assigns the next global run index.
    pub fn append_run(&self, mut run: WorkflowRun) -> Result<WorkflowRun> {
        let _guard = self.lock.lock().unwrap();
        let mut file = self.load_runs_unlocked()?;
        run.idx = file.next_idx;
        file.next_idx += 1;
        file.runs.push(run.clone());
        self.write_json(&self.runs_path(), &file)?;
        Ok(run)
    }

    /// Returns all runs, newest first.
    pub fn list_runs(&self) -> Result<Vec<WorkflowRun>> {
        let _guard = self.lock.lock().unwrap();
        let mut runs = self.load_runs_unlocked()?.runs;
        runs.sort_by(|a, b| b.idx.cmp(&a.idx));
        Ok(runs)
    }

    /// Returns runs for one workflow, newest first.
    pub fn list_runs_for(&self, workflow_slug: &str) -> Result<Vec<WorkflowRun>> {
        Ok(self
            .list_runs()?
            .into_iter()
            .filter(|run| run.workflow_slug == workflow_slug)
            .collect())
    }

    /// Returns one run by global index.
    pub fn get_run(&self, idx: u64) -> Result<Option<WorkflowRun>> {
        Ok(self.list_runs()?.into_iter().find(|run| run.idx == idx))
    }

    /// Returns definitions and runs in one read.
    pub fn snapshot(&self) -> Result<WorkflowStoreSnapshot> {
        Ok(WorkflowStoreSnapshot {
            workflows: self.list()?,
            runs: self.list_runs()?,
        })
    }

    fn definitions_path(&self) -> PathBuf {
        self.root.join("definitions.json")
    }

    fn runs_path(&self) -> PathBuf {
        self.root.join("runs.json")
    }

    fn load_definitions_unlocked(&self) -> Result<DefinitionsFile> {
        read_json_or_default(&self.definitions_path())
    }

    fn load_runs_unlocked(&self) -> Result<RunsFile> {
        read_json_or_default(&self.runs_path())
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create workflow store {}", self.root.display()))?;
        let tmp = path.with_extension("tmp");
        let text = serde_json::to_string_pretty(value)?;
        fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, path)
            .with_context(|| format!("rename {} to {}", tmp.display(), path.display()))?;
        Ok(())
    }
}

fn read_json_or_default<T>(path: &Path) -> Result<T>
where
    T: Default + for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TriggerSpec, WorkflowRunNode, WorkflowRunStatus};
    use serde_json::json;

    #[test]
    fn store_roundtrip_and_indexes() {
        let temp = tempfile::tempdir().unwrap();
        let store = WorkflowStore::new(temp.path());
        store
            .register_json(
                json!({"name":"X","nodes":[{"id":"a","prompt":"go"}]}),
                RegisterOptions {
                    slug: Some("x".into()),
                    trigger: Some(TriggerSpec::Cron {
                        cron: "* * * * *".into(),
                    }),
                },
            )
            .unwrap();
        assert_eq!(store.list().unwrap()[0].slug, "x");
        let run = WorkflowRun {
            idx: 0,
            workflow_slug: "x".into(),
            run_id: "r".into(),
            trigger: json!({}),
            status: WorkflowRunStatus::Completed,
            started_at_ms: 1,
            ended_at_ms: Some(2),
            nodes: vec![WorkflowRunNode {
                id: "a".into(),
                status: WorkflowRunStatus::Completed,
                started_at_ms: Some(1),
                ended_at_ms: Some(2),
                output: Some("ok".into()),
                error: None,
            }],
            error: None,
            trigger_key: None,
        };
        assert_eq!(store.append_run(run.clone()).unwrap().idx, 1);
        assert_eq!(store.append_run(run).unwrap().idx, 2);
    }
}
