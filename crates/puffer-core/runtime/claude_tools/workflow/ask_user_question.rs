use crate::AppState;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::Path;

use super::ask_user_question_types::{
    validate_ask_user_questions, AskUserQuestionInput, AskUserQuestionItem,
};
use super::store::workflow_root;
use crate::runtime::permission_prompt::{prompt_for_user_question, UserQuestionPromptRequest};
use crate::runtime::secrets::register_masked_secret;

/// Executes the live `AskUserQuestion` workflow tool.
pub fn execute_ask_user_question(state: &mut AppState, cwd: &Path, input: Value) -> Result<String> {
    let _ = cwd;
    let mut parsed: AskUserQuestionInput =
        serde_json::from_value(input).context("invalid AskUserQuestion input")?;
    validate_ask_user_questions(&parsed.questions)?;
    if parsed.answers.is_empty() {
        if let Some(response) = prompt_for_user_question(UserQuestionPromptRequest {
            questions: serde_json::to_value(&parsed.questions)?,
            metadata: serde_json::to_value(&parsed.metadata).unwrap_or(Value::Null),
        }) {
            parsed.answers = response.answers;
            for (key, value) in response.annotations {
                parsed.annotations.insert(key, value);
            }
        }
    }
    mask_secret_question_answers(state, &parsed.questions, &mut parsed.answers)?;
    let pending_path = workflow_root(state.session.cwd.as_path())?.join("pending_questions.json");
    let pending = parsed.answers.is_empty();
    if pending {
        std::fs::write(
            &pending_path,
            serde_json::to_string_pretty(&parsed.questions)?,
        )?;
    } else if pending_path.exists() {
        let _ = std::fs::remove_file(&pending_path);
    }
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "questions": parsed.questions,
        "answers": parsed.answers,
        "annotations": parsed.annotations,
        "metadata": parsed.metadata,
        "pending": pending,
        "pendingFile": pending_path.display().to_string()
    }))?)
}

fn mask_secret_question_answers(
    state: &AppState,
    questions: &[AskUserQuestionItem],
    answers: &mut serde_json::Map<String, Value>,
) -> Result<()> {
    for question in questions.iter().filter(|question| question.secret) {
        let Some(answer) = answers.get_mut(&question.question) else {
            continue;
        };
        let Value::String(raw) = answer else {
            bail!(
                "AskUserQuestion secret input `{}` must return a string answer",
                question.header
            );
        };
        if raw.trim().is_empty() {
            continue;
        }
        let token = register_masked_secret(state, raw.clone())?;
        *answer = Value::String(token);
    }
    Ok(())
}
