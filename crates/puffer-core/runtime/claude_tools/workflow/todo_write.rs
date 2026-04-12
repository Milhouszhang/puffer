use crate::AppState;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;

use super::store::{load_store, save_store, todos_path, StoredTodo, TodoStore, TodoWriteInput};
use super::task_runtime::{
    should_emit_verification_nudge_for_todos, validate_todos, VERIFICATION_NUDGE,
};

/// Executes the Claude-compatible `TodoWrite` tool scaffold.
pub fn execute_todo_write(state: &mut AppState, cwd: &Path, input: Value) -> Result<String> {
    let parsed: TodoWriteInput =
        serde_json::from_value(input).context("invalid TodoWrite input")?;
    validate_todos(&parsed.todos)?;
    let mut store = load_store::<TodoStore>(&todos_path(cwd))?;
    let old = store.todos.clone();
    store.todos = parsed
        .todos
        .into_iter()
        .map(|todo| StoredTodo {
            content: todo.content,
            status: todo.status,
            active_form: todo.active_form,
        })
        .collect();
    let verification_nudge_needed =
        state.active_team_name.is_none() && should_emit_verification_nudge_for_todos(&store.todos);
    save_store(&todos_path(cwd), &store)?;
    Ok(serde_json::to_string_pretty(&json!({
        "oldTodos": old,
        "newTodos": store.todos,
        "verificationNudgeNeeded": verification_nudge_needed,
        "note": verification_nudge_needed.then_some(VERIFICATION_NUDGE),
    }))?)
}
