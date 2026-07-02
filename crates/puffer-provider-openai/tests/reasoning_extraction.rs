use puffer_provider_openai::{
    extract_chat_completions_reasoning, extract_chat_completions_visible_text,
    parse_chat_completions_response,
};

#[test]
fn picks_up_dedicated_reasoning_content_field() {
    let payload = r#"{"id":"x","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"hi","reasoning_content":"thoughts"},"finish_reason":"stop"}]}"#;
    let parsed = parse_chat_completions_response(payload).unwrap();
    assert_eq!(
        extract_chat_completions_reasoning(&parsed),
        Some("thoughts".to_string())
    );
    assert_eq!(extract_chat_completions_visible_text(&parsed), "hi");
}

#[test]
fn picks_up_reasoning_alias_used_by_openrouter() {
    let payload = r#"{"id":"x","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"hi","reasoning":"thoughts2"},"finish_reason":"stop"}]}"#;
    let parsed = parse_chat_completions_response(payload).unwrap();
    assert_eq!(
        extract_chat_completions_reasoning(&parsed),
        Some("thoughts2".to_string())
    );
}

#[test]
fn falls_back_to_think_tag_inside_content() {
    let payload = r#"{"id":"x","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"<think>step 1\nstep 2</think>visible answer"},"finish_reason":"stop"}]}"#;
    let parsed = parse_chat_completions_response(payload).unwrap();
    assert_eq!(
        extract_chat_completions_reasoning(&parsed),
        Some("step 1\nstep 2".to_string())
    );
    assert_eq!(
        extract_chat_completions_visible_text(&parsed),
        "visible answer"
    );
}

#[test]
fn handles_uppercase_think_tag() {
    let payload = r#"{"id":"x","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"<Think>thoughts</Think>answer"},"finish_reason":"stop"}]}"#;
    let parsed = parse_chat_completions_response(payload).unwrap();
    assert_eq!(
        extract_chat_completions_reasoning(&parsed),
        Some("thoughts".to_string())
    );
    assert_eq!(extract_chat_completions_visible_text(&parsed), "answer");
}

#[test]
fn no_reasoning_returns_none_and_full_text() {
    let payload = r#"{"id":"x","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"plain answer"},"finish_reason":"stop"}]}"#;
    let parsed = parse_chat_completions_response(payload).unwrap();
    assert_eq!(extract_chat_completions_reasoning(&parsed), None);
    assert_eq!(
        extract_chat_completions_visible_text(&parsed),
        "plain answer"
    );
}

#[test]
fn empty_reasoning_content_returns_none() {
    let payload = r#"{"id":"x","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"answer","reasoning_content":""},"finish_reason":"stop"}]}"#;
    let parsed = parse_chat_completions_response(payload).unwrap();
    assert_eq!(extract_chat_completions_reasoning(&parsed), None);
}

#[test]
fn strips_nul_and_control_bytes_but_keeps_whitespace() {
    // Kimi has been observed embedding a NUL plus a stray C0 control
    // byte (BEL) and a DEL inside its reasoning_content, then rejecting
    // the same string on replay. Tab and newline must survive. The \u
    // escapes below are JSON string escapes, so serde materializes the
    // real control chars at parse time.
    let payload = r#"{"id":"x","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"answer","reasoning_content":"a\u0000b\u0007c\u007f\ttab\nline"},"finish_reason":"stop"}]}"#;
    let parsed = parse_chat_completions_response(payload).unwrap();
    assert_eq!(
        extract_chat_completions_reasoning(&parsed),
        Some("abc\ttab\nline".to_string())
    );
}

#[test]
fn all_control_reasoning_content_returns_none() {
    // reasoning_content that is nothing but control bytes sanitizes to
    // empty, which must collapse back to `None` rather than `Some("")`.
    let payload = r#"{"id":"x","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"answer","reasoning_content":"\u0000\u0007\u007f"},"finish_reason":"stop"}]}"#;
    let parsed = parse_chat_completions_response(payload).unwrap();
    assert_eq!(extract_chat_completions_reasoning(&parsed), None);
}
