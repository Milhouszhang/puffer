//! [`TurnSession`] impl for the OpenAI Chat Completions API.
//!
//! Streaming requests send `stream: true` and parse Chat Completions
//! SSE (`data: {...}` chunks plus `[DONE]`). Non-SSE JSON responses
//! are still accepted as a compatibility fallback and synthesize the
//! same text/thinking events the old path emitted.

use anyhow::{bail, Context, Result};
use puffer_provider_openai::{
    build_chat_completions_request, build_json_post_request, extract_chat_completions_reasoning,
    extract_chat_completions_tool_calls, extract_chat_completions_visible_text,
    parse_chat_completions_response, OpenAIChatCompletionTool, OpenAIChatCompletionsRequest,
    OpenAIChatMessage, OpenAIChatResponseFormat, OpenAIRequestConfig, OpenAIResponseToolCall,
    OpenAIResponsesToolChoiceMode,
};
use puffer_provider_registry::{
    AuthStore, OpenAiCompletionsCompat, ProviderDescriptor, ThinkingFormat,
};
use puffer_resources::LoadedResources;
use puffer_tools::ToolRegistry;
use reqwest::blocking::Response;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, Read};

use super::conversation::{
    build_system_reminder, generate_openai_summary, items_to_chat_messages,
    managed_system_prompt_1_from_env, ConversationItem,
};
use super::{
    parse_openai_text, parse_openai_text_fallback, send_openai_request_with_refresh,
    send_openai_request_with_refresh_streaming_using_parser, OpenAIExecutionConfig,
};
use crate::permissions::{load_runtime_permission_context_with_inputs, RuntimePermissionInputs};
use crate::runtime::agent_loop::{AssistantTurn, TurnSession};
use crate::runtime::structured_output_support::{
    openai_chat_completion_tools_for_request, openai_chat_response_format, StructuredOutputConfig,
};
use crate::runtime::system_prompt::render_runtime_system_prompt;
use crate::runtime::tool_executor::ToolExecutionBackend;
use crate::runtime::{ToolCallRequest, TurnRequestOptions, TurnStreamEvent};
use crate::AppState;

pub(super) struct OpenAICompletionsTurnSession {
    pub execution: OpenAIExecutionConfig,
    pub tools: Vec<OpenAIChatCompletionTool>,
    pub response_format: Option<OpenAIChatResponseFormat>,
    pub system_prompt: String,
    pub managed_system_prompt_1: Option<String>,
    pub plan_mode_context: Option<String>,
    pub system_reminder: String,
    pub structured_output: Option<StructuredOutputConfig>,
    pub model_id: String,
    /// Resolved compat from `Model.compat` (when `kind = openai-completions`)
    /// — controls reasoning-effort wire format, requires-reasoning-content
    /// flag, and per-effort name remapping. `None` means "use canonical
    /// OpenAI shape with auto-detected defaults".
    pub compat: Option<puffer_provider_registry::OpenAiCompletionsCompat>,
    /// Whether the *model itself* supports reasoning. Gates emission of
    /// any reasoning_effort field even when the relay declares support.
    pub model_supports_reasoning: bool,
}

impl TurnSession for OpenAICompletionsTurnSession {
    fn one_turn_streaming(
        &mut self,
        state: &mut AppState,
        auth_store: &mut AuthStore,
        items: &mut Vec<ConversationItem>,
        on_event: &mut dyn FnMut(TurnStreamEvent),
    ) -> Result<AssistantTurn> {
        let result = self.send_streaming_and_parse(state, auth_store, items, on_event)?;
        Ok(result.into_assistant_turn())
    }

    fn one_turn_blocking(
        &mut self,
        state: &mut AppState,
        auth_store: &mut AuthStore,
        items: &mut Vec<ConversationItem>,
    ) -> Result<AssistantTurn> {
        Ok(self
            .send_and_parse(state, auth_store, items)?
            .into_assistant_turn())
    }

    fn generate_summary(&self, old_context: &str, model_id: &str) -> Option<String> {
        // Same Phase 2 helper Responses uses — issues a single
        // non-streaming summarization request via the OpenAI
        // /responses endpoint. Falls through to Phase 3 (drop oldest)
        // on any failure.
        generate_openai_summary(old_context, model_id, &self.execution.request_config)
    }

    fn tool_execution_backend(&self) -> ToolExecutionBackend<'_> {
        ToolExecutionBackend::OpenAi {
            request_config: &self.execution.request_config,
            structured_output: self.structured_output.as_ref(),
        }
    }
}

/// Internal "rich" result from a Chat Completions round-trip — carries
/// everything `AssistantTurn` carries PLUS the optional reasoning chain
/// so `one_turn_streaming` can synthesize a `ThinkingDelta` event for
/// reasoning-capable providers (Moonshot Kimi, Deepseek, OpenRouter).
struct CompletionsTurnResult {
    pre_tool_items: Vec<ConversationItem>,
    tool_calls: Vec<ToolCallRequest>,
    assistant_text: String,
    reasoning_chain: Option<String>,
    emitted_tool_call_ids: HashSet<String>,
}

impl CompletionsTurnResult {
    fn into_assistant_turn(self) -> AssistantTurn {
        AssistantTurn {
            pre_tool_items: self.pre_tool_items,
            tool_calls: self.tool_calls,
            assistant_text: self.assistant_text,
            input_tokens_hint: None,
            emitted_tool_call_ids: self.emitted_tool_call_ids,
            usage_report: None,
        }
    }
}

impl OpenAICompletionsTurnSession {
    /// Builds the wire body, sends the (non-streaming) request, parses
    /// the response, and pulls out the bits both `one_turn_streaming`
    /// and `one_turn_blocking` need. Stays a private method on the
    /// session so it has access to `&mut self` for execution config
    /// state mutation under OAuth refresh.
    fn send_and_parse(
        &mut self,
        state: &mut AppState,
        auth_store: &mut AuthStore,
        items: &mut Vec<ConversationItem>,
    ) -> Result<CompletionsTurnResult> {
        let prepared = self.prepare_request(state, items);

        let body_for_each_attempt = move |request_config: &OpenAIRequestConfig| {
            build_prepared_chat_completions_request(request_config, &prepared, false)
        };

        let response: Value = send_openai_request_with_refresh(
            auth_store,
            &mut self.execution,
            &state.config.network.proxy,
            body_for_each_attempt,
        )?;

        Self::result_from_response_value(&response, state)
    }

    fn send_streaming_and_parse(
        &mut self,
        state: &mut AppState,
        auth_store: &mut AuthStore,
        items: &mut Vec<ConversationItem>,
        on_event: &mut dyn FnMut(TurnStreamEvent),
    ) -> Result<CompletionsTurnResult> {
        let prepared = self.prepare_request(state, items);

        let body_for_each_attempt = move |request_config: &OpenAIRequestConfig| {
            build_prepared_chat_completions_request(request_config, &prepared, true)
        };

        let streamed = send_openai_request_with_refresh_streaming_using_parser(
            auth_store,
            &mut self.execution,
            &state.config.network.proxy,
            body_for_each_attempt,
            on_event,
            parse_chat_completions_stream_response,
        )?;

        Ok(Self::result_from_stream(streamed, state))
    }

    fn prepare_request(
        &self,
        state: &AppState,
        items: &[ConversationItem],
    ) -> PreparedCompletionsRequest {
        let messages = items_to_chat_messages(
            items,
            Some(&self.system_prompt),
            self.managed_system_prompt_1.as_deref(),
            self.plan_mode_context.as_deref(),
            Some(&self.system_reminder),
        );

        let reasoning_fields = resolve_reasoning_fields(
            self.compat.as_ref(),
            self.model_supports_reasoning,
            &state.effort_level,
        );
        let mut messages = messages;
        if reasoning_fields.requires_reasoning_content_on_assistant_messages
            && self.model_supports_reasoning
        {
            for msg in &mut messages {
                if msg.role == "assistant" && msg.reasoning_content.is_none() {
                    msg.reasoning_content = Some(String::new());
                }
            }
        }

        PreparedCompletionsRequest {
            model_id: self.model_id.clone(),
            messages,
            tools: self.tools.clone(),
            response_format: self.response_format.clone(),
            reasoning_fields,
        }
    }

    fn result_from_response_value(
        response: &Value,
        state: &AppState,
    ) -> Result<CompletionsTurnResult> {
        let parsed = chat_completions_result_from_json_value(response)?;
        Ok(Self::result_from_stream(parsed, state))
    }

    fn result_from_stream(
        streamed: ChatCompletionsStreamResult,
        state: &AppState,
    ) -> CompletionsTurnResult {
        let tool_calls: Vec<ToolCallRequest> = streamed
            .tool_calls
            .iter()
            .map(|tc| ToolCallRequest {
                call_id: tc.call_id.clone(),
                tool_id: tc.name.clone(),
                input: serde_json::to_string(&tc.arguments).unwrap_or_default(),
            })
            .collect();
        let mut pre_tool_items: Vec<ConversationItem> = Vec::new();
        if !streamed.assistant_text.trim().is_empty() {
            pre_tool_items.push(ConversationItem::assistant_message(
                &streamed.assistant_text,
            ));
        }
        for tc in &streamed.tool_calls {
            pre_tool_items.push(ConversationItem::FunctionCall {
                call_id: tc.call_id.clone(),
                name: tc.name.clone(),
                arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
            });
        }

        let final_assistant_text = if tool_calls.is_empty() {
            if streamed.assistant_text.trim().is_empty() {
                parse_openai_text(&streamed.raw_response)
                    .or_else(|_| parse_openai_text_fallback(&streamed.raw_response, state))
                    .unwrap_or_default()
            } else {
                streamed.assistant_text
            }
        } else {
            String::new()
        };

        CompletionsTurnResult {
            pre_tool_items,
            tool_calls,
            assistant_text: final_assistant_text,
            reasoning_chain: streamed.reasoning_chain,
            emitted_tool_call_ids: streamed.emitted_tool_call_ids,
        }
    }
}

struct PreparedCompletionsRequest {
    model_id: String,
    messages: Vec<OpenAIChatMessage>,
    tools: Vec<OpenAIChatCompletionTool>,
    response_format: Option<OpenAIChatResponseFormat>,
    reasoning_fields: ReasoningFields,
}

fn build_prepared_chat_completions_request(
    config: &OpenAIRequestConfig,
    prepared: &PreparedCompletionsRequest,
    stream: bool,
) -> Result<puffer_provider_openai::BuiltOpenAIRequest> {
    let request = OpenAIChatCompletionsRequest {
        model: prepared.model_id.clone(),
        messages: prepared.messages.clone(),
        tools: prepared.tools.clone(),
        tool_choice: if prepared.tools.is_empty() {
            None
        } else {
            Some(OpenAIResponsesToolChoiceMode::Auto)
        },
        response_format: prepared.response_format.clone(),
        reasoning_effort: prepared.reasoning_fields.reasoning_effort.clone(),
        reasoning: prepared.reasoning_fields.reasoning.clone(),
        thinking: prepared.reasoning_fields.thinking.clone(),
        enable_thinking: prepared.reasoning_fields.enable_thinking,
        chat_template_kwargs: prepared.reasoning_fields.chat_template_kwargs.clone(),
    };

    if !stream {
        return build_chat_completions_request(config, &request);
    }

    let path = config
        .chat_completions_path
        .as_deref()
        .unwrap_or("/v1/chat/completions");
    let mut body = serde_json::to_value(&request)?;
    body["stream"] = Value::Bool(true);
    build_json_post_request(config, path, &body)
}

struct ChatCompletionsStreamResult {
    assistant_text: String,
    reasoning_chain: Option<String>,
    tool_calls: Vec<OpenAIResponseToolCall>,
    emitted_tool_call_ids: HashSet<String>,
    raw_response: Value,
}

fn parse_chat_completions_stream_response<G>(
    url: &str,
    response: Response,
    on_event: &mut G,
) -> Result<ChatCompletionsStreamResult>
where
    G: FnMut(TurnStreamEvent) + ?Sized,
{
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        if let Some(quota) =
            super::super::quota::classify_response("openai", status.as_u16(), &text)
        {
            return Err(anyhow::Error::new(quota));
        }
        bail!("request failed with status {}: {}", status, text);
    }

    let mut reader = std::io::BufReader::new(response);
    let looks_like_sse = if is_chat_completions_event_stream(content_type.as_deref(), "") {
        true
    } else {
        let prefix = reader.fill_buf()?;
        let prefix = std::str::from_utf8(prefix).unwrap_or_default();
        is_chat_completions_event_stream(content_type.as_deref(), prefix)
    };

    if looks_like_sse {
        return parse_chat_completions_sse_reader(reader, on_event)
            .with_context(|| format!("failed to parse Chat Completions SSE response from {url}"));
    }

    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    let raw: Value = serde_json::from_str(&text)
        .with_context(|| format!("response from {url} was not valid JSON"))?;
    let result = chat_completions_result_from_json_value(&raw)
        .with_context(|| format!("response from {url} was not a valid Chat Completions payload"))?;
    if let Some(reasoning) = result.reasoning_chain.as_deref() {
        if !reasoning.is_empty() {
            on_event(TurnStreamEvent::ThinkingDelta(reasoning.to_string()));
        }
    }
    if !result.assistant_text.is_empty() {
        on_event(TurnStreamEvent::TextDelta(result.assistant_text.clone()));
    }
    Ok(result)
}

fn chat_completions_result_from_json_value(
    response: &Value,
) -> Result<ChatCompletionsStreamResult> {
    let parsed = parse_chat_completions_response(&serde_json::to_string(response)?)?;
    let tool_calls = extract_chat_completions_tool_calls(&parsed)?;
    Ok(ChatCompletionsStreamResult {
        assistant_text: extract_chat_completions_visible_text(&parsed),
        reasoning_chain: extract_chat_completions_reasoning(&parsed),
        tool_calls,
        emitted_tool_call_ids: HashSet::new(),
        raw_response: response.clone(),
    })
}

fn is_chat_completions_event_stream(content_type: Option<&str>, text: &str) -> bool {
    content_type.is_some_and(|value| value.starts_with("text/event-stream"))
        || text.trim_start().starts_with("data:")
        || text.trim_start().starts_with("event:")
}

fn parse_chat_completions_sse_reader<R, G>(
    mut reader: R,
    on_event: &mut G,
) -> Result<ChatCompletionsStreamResult>
where
    R: BufRead,
    G: FnMut(TurnStreamEvent) + ?Sized,
{
    let mut state = ChatCompletionsSseState::default();
    let mut line = String::new();
    let mut data_lines = Vec::new();

    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            flush_chat_completions_sse_event(&data_lines, &mut state, on_event)?;
            break;
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            if flush_chat_completions_sse_event(&data_lines, &mut state, on_event)? {
                data_lines.clear();
                break;
            }
            data_lines.clear();
            continue;
        }

        if let Some(data) = trimmed.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_string());
        }
    }

    if !state.terminal {
        bail!("stream closed before Chat Completions [DONE]");
    }
    Ok(state.into_result())
}

fn flush_chat_completions_sse_event<G>(
    data_lines: &[String],
    state: &mut ChatCompletionsSseState,
    on_event: &mut G,
) -> Result<bool>
where
    G: FnMut(TurnStreamEvent) + ?Sized,
{
    let data = data_lines.join("\n");
    if data.is_empty() {
        return Ok(false);
    }
    if data == "[DONE]" {
        state.emit_complete_tool_calls(true, on_event);
        state.terminal = true;
        return Ok(true);
    }

    let event: Value = serde_json::from_str(&data)
        .with_context(|| format!("invalid Chat Completions SSE payload: {data}"))?;
    state.process_event(&event, on_event)?;
    Ok(false)
}

#[derive(Default)]
struct ChatCompletionsSseState {
    id: Option<String>,
    finish_reason: Option<String>,
    terminal: bool,
    assistant_text: String,
    reasoning_chain: String,
    tool_call_deltas: BTreeMap<usize, ChatCompletionsToolCallDelta>,
    emitted_tool_call_ids: HashSet<String>,
}

impl ChatCompletionsSseState {
    fn process_event<G>(&mut self, event: &Value, on_event: &mut G) -> Result<()>
    where
        G: FnMut(TurnStreamEvent) + ?Sized,
    {
        if let Some(error) = event.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| event.get("message").and_then(Value::as_str))
                .unwrap_or("Chat Completions stream failed");
            bail!("{message}");
        }

        if self.id.is_none() {
            if let Some(id) = event.get("id").and_then(Value::as_str) {
                self.id = Some(id.to_string());
            }
        }

        for choice in event
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(delta) = choice.get("delta") {
                self.process_delta(delta, on_event)?;
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                if !reason.is_empty() {
                    self.finish_reason = Some(reason.to_string());
                    if reason == "tool_calls" {
                        self.emit_complete_tool_calls(true, on_event);
                    }
                }
            }
        }

        Ok(())
    }

    fn process_delta<G>(&mut self, delta: &Value, on_event: &mut G) -> Result<()>
    where
        G: FnMut(TurnStreamEvent) + ?Sized,
    {
        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            self.assistant_text.push_str(content);
            on_event(TurnStreamEvent::TextDelta(content.to_string()));
        }

        for key in ["reasoning_content", "reasoning"] {
            if let Some(reasoning) = delta.get(key).and_then(Value::as_str) {
                self.reasoning_chain.push_str(reasoning);
                on_event(TurnStreamEvent::ThinkingDelta(reasoning.to_string()));
            }
        }

        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for (position, tool_call) in tool_calls.iter().enumerate() {
                let index = tool_call
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize)
                    .unwrap_or(position);
                {
                    let entry = self.tool_call_deltas.entry(index).or_default();
                    if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                        if !id.is_empty() {
                            entry.call_id = id.to_string();
                        }
                    }
                    if let Some(kind) = tool_call.get("type").and_then(Value::as_str) {
                        if !kind.is_empty() {
                            entry.kind = kind.to_string();
                        }
                    }
                    if let Some(function) = tool_call.get("function") {
                        if let Some(name) = function.get("name").and_then(Value::as_str) {
                            if !name.is_empty() {
                                entry.name = name.to_string();
                            }
                        }
                        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                            entry.arguments.push_str(arguments);
                        }
                    }
                }
                self.maybe_emit_tool_call(index, false, on_event);
            }
        }

        Ok(())
    }

    fn maybe_emit_tool_call<G>(&mut self, index: usize, allow_raw: bool, on_event: &mut G)
    where
        G: FnMut(TurnStreamEvent) + ?Sized,
    {
        if self
            .tool_call_deltas
            .get(&index)
            .map(|entry| entry.emitted)
            .unwrap_or(true)
        {
            return;
        }
        let Some((tool_call, raw_arguments)) = self.completed_tool_call(index, allow_raw) else {
            return;
        };
        on_event(TurnStreamEvent::ToolCallsRequested(vec![ToolCallRequest {
            call_id: tool_call.call_id.clone(),
            tool_id: tool_call.name.clone(),
            input: raw_arguments,
        }]));
        self.emitted_tool_call_ids.insert(tool_call.call_id.clone());
        if let Some(entry) = self.tool_call_deltas.get_mut(&index) {
            entry.emitted = true;
        }
    }

    fn emit_complete_tool_calls<G>(&mut self, allow_raw: bool, on_event: &mut G)
    where
        G: FnMut(TurnStreamEvent) + ?Sized,
    {
        let indexes: Vec<usize> = self.tool_call_deltas.keys().copied().collect();
        for index in indexes {
            self.maybe_emit_tool_call(index, allow_raw, on_event);
        }
    }

    fn completed_tool_call(
        &self,
        index: usize,
        allow_raw: bool,
    ) -> Option<(OpenAIResponseToolCall, String)> {
        let entry = self.tool_call_deltas.get(&index)?;
        if entry.call_id.is_empty() || entry.name.is_empty() {
            return None;
        }
        let parsed_arguments = match serde_json::from_str::<Value>(&entry.arguments) {
            Ok(value) => value,
            Err(_) if allow_raw => Value::String(entry.arguments.clone()),
            Err(_) => return None,
        };
        Some((
            OpenAIResponseToolCall {
                item_id: None,
                status: None,
                call_id: entry.call_id.clone(),
                name: entry.name.clone(),
                arguments: parsed_arguments,
            },
            entry.arguments.clone(),
        ))
    }

    fn into_result(mut self) -> ChatCompletionsStreamResult {
        let mut noop = |_| {};
        self.emit_complete_tool_calls(true, &mut noop);
        let tool_calls = self
            .tool_call_deltas
            .keys()
            .filter_map(|index| self.completed_tool_call(*index, true).map(|(call, _)| call))
            .collect::<Vec<_>>();
        let raw_response = self.build_raw_response();
        ChatCompletionsStreamResult {
            assistant_text: self.assistant_text,
            reasoning_chain: (!self.reasoning_chain.is_empty()).then_some(self.reasoning_chain),
            tool_calls,
            emitted_tool_call_ids: self.emitted_tool_call_ids,
            raw_response,
        }
    }

    fn build_raw_response(&self) -> Value {
        let tool_calls = self
            .tool_call_deltas
            .values()
            .filter(|entry| !entry.call_id.is_empty() && !entry.name.is_empty())
            .map(|entry| {
                json!({
                    "id": entry.call_id,
                    "type": if entry.kind.is_empty() { "function" } else { entry.kind.as_str() },
                    "function": {
                        "name": entry.name,
                        "arguments": entry.arguments,
                    }
                })
            })
            .collect::<Vec<_>>();

        let mut message = json!({
            "role": "assistant",
            "content": if self.assistant_text.is_empty() {
                Value::Null
            } else {
                Value::String(self.assistant_text.clone())
            },
        });
        if !self.reasoning_chain.is_empty() {
            message["reasoning_content"] = Value::String(self.reasoning_chain.clone());
        }
        if !tool_calls.is_empty() {
            message["tool_calls"] = Value::Array(tool_calls);
        }

        json!({
            "id": self.id.clone().unwrap_or_default(),
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": message,
                "finish_reason": self.finish_reason.clone().unwrap_or_else(|| "stop".to_string()),
            }],
        })
    }
}

#[derive(Default)]
struct ChatCompletionsToolCallDelta {
    call_id: String,
    kind: String,
    name: String,
    arguments: String,
    emitted: bool,
}

#[cfg(test)]
fn parse_chat_completions_sse_response_for_tests<G>(
    stream: &str,
    on_event: &mut G,
) -> Result<ChatCompletionsStreamResult>
where
    G: FnMut(TurnStreamEvent),
{
    parse_chat_completions_sse_reader(std::io::BufReader::new(stream.as_bytes()), on_event)
}

pub(super) fn setup_completions_session(
    state: &mut AppState,
    resources: &LoadedResources,
    provider: &ProviderDescriptor,
    model_id: String,
    auth_store: &mut AuthStore,
    options: &TurnRequestOptions<'_>,
    use_native: bool,
) -> Result<OpenAICompletionsTurnSession> {
    let execution = super::resolve_openai_execution_config(state, auth_store, provider)?;
    let registry =
        super::super::mcp_discovery::registry_with_mcp_tools(resources, state.tool_runner.as_ref());
    let permission_context = load_runtime_permission_context_with_inputs(
        &state.cwd,
        resources,
        state,
        RuntimePermissionInputs {
            request_tool_filter: options.tool_filter.cloned(),
        },
    )?;
    let response_format = openai_chat_response_format(options.structured_output, use_native);
    let mut tools = openai_chat_completion_tools_for_request(
        &registry,
        options.structured_output,
        use_native,
        Some(&permission_context),
    )?;
    if !options.excluded_tools.is_empty() {
        tools.retain(|tool| {
            options
                .excluded_tools
                .iter()
                .all(|ex| ex.as_str() != tool.function.name.as_str())
        });
    }
    if super::super::state_has_image_attachments(state) {
        tools.retain(|tool| tool.function.name != "VisionAnalyze");
    }
    let (system_prompt, managed_system_prompt_1, plan_mode_context, system_reminder) =
        if options.lightweight_context {
            (
                "Reply directly and concisely.".to_string(),
                None,
                None,
                String::new(),
            )
        } else {
            (
                render_runtime_system_prompt(
                    state,
                    resources,
                    &model_id,
                    &tools
                        .iter()
                        .map(|tool| tool.function.name.clone())
                        .collect::<std::collections::BTreeSet<_>>(),
                )?,
                managed_system_prompt_1_from_env(),
                crate::plan_mode::take_plan_mode_context_message(state, resources)?,
                build_system_reminder(state, &crate::runtime::git_status_context()),
            )
        };

    let model_descriptor = provider.models.iter().find(|m| m.id == model_id);
    let model_supports_reasoning = model_descriptor
        .map(|m| m.supports_reasoning)
        .unwrap_or(false);
    let compat = model_descriptor
        .and_then(|m| m.compat.as_ref())
        .and_then(|c| c.as_openai_completions())
        .cloned()
        .or_else(|| inferred_completions_compat(provider));

    Ok(OpenAICompletionsTurnSession {
        execution,
        tools,
        response_format,
        system_prompt,
        managed_system_prompt_1,
        plan_mode_context,
        system_reminder,
        structured_output: options.structured_output.cloned(),
        model_id,
        compat,
        model_supports_reasoning,
    })
}

fn inferred_completions_compat(provider: &ProviderDescriptor) -> Option<OpenAiCompletionsCompat> {
    let provider_id = provider.id.trim().to_ascii_lowercase();
    let base_url = provider.base_url.to_ascii_lowercase();
    if provider_id == "openrouter" || base_url.contains("openrouter.ai") {
        return Some(OpenAiCompletionsCompat {
            thinking_format: Some(ThinkingFormat::Openrouter),
            ..OpenAiCompletionsCompat::default()
        });
    }
    None
}

/// Resolved reasoning fields for one Chat Completions request.
struct ReasoningFields {
    reasoning_effort: Option<String>,
    reasoning: Option<Value>,
    thinking: Option<Value>,
    enable_thinking: Option<bool>,
    chat_template_kwargs: Option<Value>,
    requires_reasoning_content_on_assistant_messages: bool,
}

/// Maps puffer's effort_level + the model's compat → the wire-format
/// fields used by the active thinking_format. Pi-mono parity:
/// `pi-mono/packages/ai/src/providers/openai-completions.ts:1071`.
///
/// When `model.supports_reasoning` is false OR
/// `compat.supports_reasoning_effort = Some(false)`, returns "no
/// reasoning fields" so non-reasoning models keep their cheap path.
fn resolve_reasoning_fields(
    compat: Option<&puffer_provider_registry::OpenAiCompletionsCompat>,
    model_supports_reasoning: bool,
    effort_level: &str,
) -> ReasoningFields {
    let mut fields = ReasoningFields {
        reasoning_effort: None,
        reasoning: None,
        thinking: None,
        enable_thinking: None,
        chat_template_kwargs: None,
        requires_reasoning_content_on_assistant_messages: compat
            .and_then(|c| c.requires_reasoning_content_on_assistant_messages)
            .unwrap_or(false),
    };

    if !model_supports_reasoning {
        return fields;
    }
    if compat
        .and_then(|c| c.supports_reasoning_effort)
        .map(|v| !v)
        .unwrap_or(false)
    {
        return fields;
    }

    // Skip reasoning fields when the user explicitly opted out via /effort low+.
    if matches!(effort_level, "off" | "none") {
        return fields;
    }

    // Resolve the puffer-effort → vendor-string name. The `auto`
    // synonyms collapse to "medium" so we always send a known value.
    let resolved_level = match effort_level {
        "auto" | "unset" | "default" | "" => "medium",
        "max" => "high",
        other => other,
    };
    let vendor_level = compat
        .and_then(|c| c.reasoning_effort_map.as_ref())
        .and_then(|map| map.get(resolved_level))
        .cloned()
        .unwrap_or_else(|| resolved_level.to_string());

    let format = compat
        .and_then(|c| c.thinking_format)
        .unwrap_or(ThinkingFormat::Openai);

    match format {
        ThinkingFormat::Openai => {
            fields.reasoning_effort = Some(vendor_level);
        }
        ThinkingFormat::Openrouter => {
            fields.reasoning = Some(json!({ "effort": vendor_level }));
        }
        ThinkingFormat::Deepseek => {
            fields.thinking = Some(json!({ "type": "enabled" }));
            fields.reasoning_effort = Some(vendor_level);
        }
        ThinkingFormat::Zai | ThinkingFormat::Qwen => {
            fields.enable_thinking = Some(true);
        }
        ThinkingFormat::QwenChatTemplate => {
            fields.chat_template_kwargs = Some(json!({ "enable_thinking": true }));
        }
    }

    fields
}

#[cfg(test)]
mod reasoning_fields_tests {
    use super::*;
    use indexmap::IndexMap;
    use puffer_provider_registry::OpenAiCompletionsCompat;

    #[test]
    fn non_reasoning_model_emits_no_fields() {
        let f = resolve_reasoning_fields(None, false, "high");
        assert!(f.reasoning_effort.is_none());
        assert!(f.thinking.is_none());
        assert!(f.reasoning.is_none());
        assert!(f.enable_thinking.is_none());
    }

    #[test]
    fn default_format_uses_top_level_reasoning_effort() {
        let f = resolve_reasoning_fields(None, true, "high");
        assert_eq!(f.reasoning_effort.as_deref(), Some("high"));
        assert!(f.reasoning.is_none());
    }

    #[test]
    fn openrouter_uses_nested_reasoning_object() {
        let compat = OpenAiCompletionsCompat {
            thinking_format: Some(ThinkingFormat::Openrouter),
            ..Default::default()
        };
        let f = resolve_reasoning_fields(Some(&compat), true, "high");
        assert!(f.reasoning_effort.is_none());
        assert_eq!(f.reasoning, Some(json!({ "effort": "high" })));
    }

    #[test]
    fn openrouter_provider_infers_nested_reasoning_object() {
        let provider = ProviderDescriptor {
            id: "openrouter".to_string(),
            display_name: "OpenRouter".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            default_api: "openai-completions".to_string(),
            auth_modes: Vec::new(),
            headers: Default::default(),
            query_params: Default::default(),
            discovery: None,
            media: None,
            models: Vec::new(),
            chat_completions_path: None,
        };
        let compat = inferred_completions_compat(&provider).expect("openrouter compat");
        let f = resolve_reasoning_fields(Some(&compat), true, "high");
        assert!(f.reasoning_effort.is_none());
        assert_eq!(f.reasoning, Some(json!({ "effort": "high" })));
    }

    #[test]
    fn deepseek_emits_both_thinking_and_reasoning_effort() {
        let mut map = IndexMap::new();
        map.insert("xhigh".to_string(), "max".to_string());
        let compat = OpenAiCompletionsCompat {
            thinking_format: Some(ThinkingFormat::Deepseek),
            reasoning_effort_map: Some(map),
            ..Default::default()
        };
        let f = resolve_reasoning_fields(Some(&compat), true, "xhigh");
        assert_eq!(f.thinking, Some(json!({ "type": "enabled" })));
        assert_eq!(f.reasoning_effort.as_deref(), Some("max"));
    }

    #[test]
    fn zai_uses_top_level_enable_thinking() {
        let compat = OpenAiCompletionsCompat {
            thinking_format: Some(ThinkingFormat::Zai),
            ..Default::default()
        };
        let f = resolve_reasoning_fields(Some(&compat), true, "high");
        assert_eq!(f.enable_thinking, Some(true));
        assert!(f.reasoning_effort.is_none());
    }

    #[test]
    fn qwen_chat_template_uses_chat_template_kwargs() {
        let compat = OpenAiCompletionsCompat {
            thinking_format: Some(ThinkingFormat::QwenChatTemplate),
            ..Default::default()
        };
        let f = resolve_reasoning_fields(Some(&compat), true, "high");
        assert_eq!(
            f.chat_template_kwargs,
            Some(json!({ "enable_thinking": true }))
        );
    }

    #[test]
    fn explicit_supports_reasoning_effort_false_disables_field() {
        let compat = OpenAiCompletionsCompat {
            supports_reasoning_effort: Some(false),
            ..Default::default()
        };
        let f = resolve_reasoning_fields(Some(&compat), true, "high");
        assert!(f.reasoning_effort.is_none());
        assert!(f.thinking.is_none());
    }

    #[test]
    fn requires_reasoning_content_flag_propagates() {
        let compat = OpenAiCompletionsCompat {
            requires_reasoning_content_on_assistant_messages: Some(true),
            ..Default::default()
        };
        let f = resolve_reasoning_fields(Some(&compat), true, "high");
        assert!(f.requires_reasoning_content_on_assistant_messages);
    }

    #[test]
    fn auto_effort_collapses_to_medium() {
        let f = resolve_reasoning_fields(None, true, "auto");
        assert_eq!(f.reasoning_effort.as_deref(), Some("medium"));
    }

    #[test]
    fn off_effort_emits_no_reasoning_fields() {
        let f = resolve_reasoning_fields(None, true, "off");
        assert!(f.reasoning_effort.is_none());
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;
    use crate::runtime::TurnStreamEvent;

    #[test]
    fn parses_gemini_complete_tool_call_chunk() {
        let stream = concat!(
            "data: {\"id\":\"chatcmpl-gemini\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_gemini_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"Cargo.toml\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-gemini\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut events = Vec::new();
        let parsed =
            parse_chat_completions_sse_response_for_tests(stream, &mut |event| events.push(event))
                .unwrap();

        assert_eq!(parsed.assistant_text, "");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].call_id, "call_gemini_1");
        assert_eq!(parsed.tool_calls[0].name, "read_file");
        assert_eq!(
            parsed.tool_calls[0].arguments,
            json!({ "path": "Cargo.toml" })
        );
        assert!(events.iter().any(|event| matches!(
            event,
            TurnStreamEvent::ToolCallsRequested(calls)
                if calls.len() == 1
                    && calls[0].call_id == "call_gemini_1"
                    && calls[0].input == "{\"path\":\"Cargo.toml\"}"
        )));
    }

    #[test]
    fn parses_openai_fragmented_tool_call_deltas() {
        let stream = concat!(
            "data: {\"id\":\"chatcmpl-openai\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"I'll check. \"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-openai\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_openai_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"pa\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-openai\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"th\\\":\\\"Cargo\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-openai\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\".toml\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut text_deltas = Vec::new();
        let parsed = parse_chat_completions_sse_response_for_tests(stream, &mut |event| {
            if let TurnStreamEvent::TextDelta(delta) = event {
                text_deltas.push(delta);
            }
        })
        .unwrap();

        assert_eq!(text_deltas, vec!["I'll check. ".to_string()]);
        assert_eq!(parsed.assistant_text, "I'll check. ");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].call_id, "call_openai_1");
        assert_eq!(parsed.tool_calls[0].name, "read_file");
        assert_eq!(
            parsed.tool_calls[0].arguments,
            json!({ "path": "Cargo.toml" })
        );
    }
}
