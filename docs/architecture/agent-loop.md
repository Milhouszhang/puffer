# Agent Loop Architecture

This document explains how Puffer drives a single user prompt through one
or more LLM turns, executes any requested tool calls, and returns a
final assistant message.

## Layered design

```
┌─────────────────────────────────────────────────────────────┐
│  user input                                                 │
│      │                                                      │
│      ▼                                                      │
│  runtime::execute_user_prompt[_with_*]      (runtime.rs)    │
│      │  resolves provider + model, picks adapter            │
│      ▼                                                      │
│  ProviderAdapter::execute_turn[_streaming]                  │
│      │  vendor-specific retry logic (e.g. native ↔          │
│      │  fallback structured output) lives here              │
│      ▼                                                      │
│  agent_loop::run_{streaming,blocking}_loop                  │
│      │  provider-agnostic turn driver:                      │
│      │    • drain_completed_shell_tasks                     │
│      │    • compact_conversation_with                       │
│      │    • session.one_turn_*                              │
│      │    • execute_tool_batch                              │
│      │    • FunctionCallOutput synthesis                    │
│      │    • reflection observation                          │
│      │    • run_turn_hooks (end-of-turn)                    │
│      ▼                                                      │
│  TurnSession::one_turn_{streaming,blocking}                 │
│      │  per-prompt session owned by the provider:           │
│      │    • build wire body (vendor-specific JSON)          │
│      │    • HTTP / SSE I/O                                  │
│      │    • parse response into pre_tool_items              │
│      │    • extract pending tool calls                      │
│      ▼                                                      │
│  vendor SDK / wire                                          │
└─────────────────────────────────────────────────────────────┘
```

The agent loop lives in `crates/puffer-core/runtime/agent_loop.rs`.
Each provider lives in its own module:

| API id | Adapter | Session | File |
|---|---|---|---|
| `anthropic-messages` | `AnthropicAdapter` | `AnthropicTurnSession` | `runtime/anthropic.rs` |
| `openai-responses` (incl. `azure-` and `openai-codex-`) | `OpenAIResponsesAdapter` | `OpenAIResponsesTurnSession` | `runtime/openai/responses_session.rs` |
| `openai-completions` | `OpenAICompletionsAdapter` | `OpenAICompletionsTurnSession` | `runtime/openai/completions_session.rs` |

Dispatch is a `match` in `runtime/provider_adapter.rs::adapter_for_api`.

## Why this shape

The starting point was:

- `runtime.rs` directly imported `puffer_transport_anthropic::*` and
  `puffer_provider_openai::*` types and dispatched by string
  comparison on the model's API id.
- Each provider's `execute_*` function owned a turn-by-turn loop that
  called the same helpers (compaction, reflection, tool execution)
  in slightly different orders.
- Adding a new provider meant editing `runtime.rs` in 4+ places and
  duplicating ~300 lines of loop scaffolding.

The redesign borrows from
[pi-mono](https://github.com/mariozechner/pi-mono), specifically
`packages/agent/src/agent-loop.ts` (the loop) and
`packages/ai/src/api-registry.ts` (the registry). In pi-mono:

- The loop is provider-agnostic. It calls
  `streamSimple(model, context, options)` and consumes neutral
  `AssistantMessageEvent`s.
- Tool execution is in the loop. Providers don't know about a tool
  registry.
- Each provider implements
  `(model, context, options) → AssistantMessageEventStream` and
  nothing else.

Puffer adopts the same separation in Rust:

- `agent_loop::TurnSession` is the seam — each provider's session
  exposes only `one_turn_*`, `generate_summary`, and
  `tool_execution_backend`. The loop never imports vendor types.
- `agent_loop::run_*_loop` owns the turn-by-turn driver. It calls
  back into the session once per iteration.
- Adapters are thin shims that build a session and hand it to
  `agent_loop`. Adapters keep any vendor-level retry logic that lives
  *outside* the per-prompt loop (e.g. OpenAI's native↔fallback
  structured-output retry restarts the whole loop with a different
  session).

## TurnSession contract

```rust
pub(crate) trait TurnSession {
    fn one_turn_streaming(
        &mut self,
        state: &mut AppState,
        auth_store: &mut AuthStore,
        items: &mut Vec<ConversationItem>,
        on_event: &mut dyn FnMut(TurnStreamEvent),
    ) -> Result<AssistantTurn>;

    fn one_turn_blocking(
        &mut self,
        state: &mut AppState,
        auth_store: &mut AuthStore,
        items: &mut Vec<ConversationItem>,
    ) -> Result<AssistantTurn> { /* default delegates to streaming */ }

    fn generate_summary(&self, old_context: &str, model_id: &str) -> Option<String>;
    fn tool_execution_backend(&self) -> ToolExecutionBackend<'_>;

    fn pre_loop_inject(&mut self, _items: &mut Vec<ConversationItem>) {}
    fn notify_compacted(&mut self) {}
}
```

A session is built once per user prompt by the adapter's
`setup_*_session` function. It captures all "constant per prompt"
state (request URL, headers, auth, serialized tools, system prompt,
plan-mode context, supports-reasoning flag, threading flag, etc.).
Per-iteration mutable state (e.g. `previous_response_id`,
`continuation_start` for OpenAI Responses) lives in the session as
`&mut self` fields, updated inside `one_turn_*`.

`AssistantTurn` is the neutral output of one round trip:

```rust
pub(crate) struct AssistantTurn {
    pub pre_tool_items: Vec<ConversationItem>,        // asst text + reasoning + FunctionCall
    pub tool_calls: Vec<ToolCallRequest>,             // pending calls
    pub assistant_text: String,                       // final text (only when no tool_calls)
    pub input_tokens_hint: Option<usize>,             // for compaction sizing
    pub emitted_tool_call_ids: HashSet<String>,       // already emitted via streaming
}
```

The loop:
1. appends `pre_tool_items` to the transcript,
2. emits `TurnStreamEvent::ToolCallsRequested` for ids the session
   did not already surface (the dedupe set),
3. runs each `ToolCallRequest` through `execute_tool_call` with
   `session.tool_execution_backend()`,
4. appends a `FunctionCallOutput` item per invocation,
5. invokes the optional `ReflectionTracker`,
6. runs compaction (calling `session.generate_summary` for the
   summary fn),
7. on a successful compaction, calls `session.notify_compacted` so
   the session can invalidate any threading state.

When `tool_calls` is empty the loop emits `run_turn_hooks` and
returns `TurnExecution`.

## Hooks the session can opt into

- `pre_loop_inject` — called once after `transcript_to_items` and
  before the first iteration. OpenAI Responses uses this to pin a
  `currentDate + gitStatus` context reminder at index 0; Anthropic
  does not need it because the same reminder rides as a system block.
- `notify_compacted` — called when the loop performs a compaction.
  OpenAI Responses uses it to clear `previous_response_id` and
  `continuation_start` because the API's server-side cached state no
  longer matches the local transcript.

## Threading + continuation (OpenAI Responses)

When `openai_supports_response_threading(provider, base_url)` returns
true, each request body carries `previous_response_id` from the
previous turn's response, and the wire `input` array contains only
items the API has not seen. `OpenAIResponsesTurnSession` tracks two
fields:

- `previous_response_id: Option<String>` — set after each non-empty
  response if the model exposes `id` in the wire payload.
- `continuation_start: Option<usize>` — set at the END of
  `one_turn_*` to `items_len_at_request + established_count`, where
  `established_count` is the number of "established" items
  (`assistant_message + reasoning`). The session pushes
  `FunctionCall` blocks into `pre_tool_items` *after* recording
  `continuation_start`, so the next iteration's wire input begins at
  the FunctionCall position and includes both the FunctionCall and
  the agent_loop-appended FunctionCallOutput.

Compaction invalidates this state. `notify_compacted` resets both
fields to `None`.

## What's preserved end-to-end

Migration acceptance criteria:

- 9-round multi-turn tool loop (Anthropic, both blocking + streaming) — `tests/iteration_behavior.rs`.
- 2-round tool → text (OpenAI Responses, blocking + SSE) — `tests/agent_loop_e2e.rs`.
- 2-round tool → text (OpenAI Chat Completions) — `tests/agent_loop_e2e.rs`.
- Cross-provider equivalence: same prompt + tool fixture, same final
  `TurnExecution` (Anthropic vs OpenAI Responses) — `tests/agent_loop_e2e.rs`.
- Existing `execute_anthropic_tool_calls_*` (5 tests) and
  `execute_openai_tool_calls_*` (8 tests) — fake response payloads
  through the test helpers in each module.
- Compaction's `compact_conversation_with` is still called pre-loop
  and post-iteration with the session's `generate_summary` as the
  Phase 2 summary fn.
- `ReflectionTracker::observe_batch_with_judge` is invoked inside
  the loop after each tool batch, with full `(state, resources,
  providers, auth_store)` context.
- `drain_completed_shell_tasks` injects background-task notices at
  the top of each iteration.
- `run_turn_hooks` runs once at end-of-turn with the final text and
  invocation count.
- `prompt_too_long` (413) recovery for Anthropic blocking is inside
  `AnthropicTurnSession::one_turn_blocking` — drains oldest items in
  place and retries until the request fits.
- OpenAI OAuth refresh on 401 is inside
  `send_openai_request_with_refresh{,_streaming}` — the session
  forwards `&mut auth_store` so credentials can rotate mid-turn.
- OpenAI native ↔ fallback structured-output retry stays at the
  adapter level (it reruns the entire agent_loop with a new session).

## Remaining gaps vs pi-mono (deferred)

| | pi-mono | puffer | Status |
|---|---|---|---|
| Δ1 Tool execution location | agent loop | agent_loop | ✅ |
| Δ2 Provider shape | stateless `(model, ctx, opts) → events` | per-prompt session with 4 methods | ✅ |
| Δ3 Stream event richness | `start / text_start / text_end / thinking_* / toolcall_* / done / error` with cumulative `partial: AssistantMessage` | `TextDelta / ThinkingDelta / ToolCallsRequested / ToolInvocations / Usage / RetryAttempt / ReflectionTrace / ReflectionCheckpoint` (deltas only, no cumulative partial) | deferred |
| Δ4 Error surface | encoded in stream | `Result<TurnExecution>` | deferred |
| Δ5 App↔neutral mapping hook | `convertToLlm` config hook | `transcript_to_items` is fixed | deferred |
| Δ6 Declarative compat | `Model.compat?: …` | string sniffing on `provider.id` and `base_url` | deferred |
| Δ7 Tools in neutral context | `Context.tools: Tool[]` | session builds its own vendor-shape tool list | deferred |
| Δ8 Lazy provider load | dynamic import per provider | static (Rust) | n/a |

The first two — the structural changes that make the agent loop
provider-agnostic — are the load-bearing improvements. Δ3, Δ4, Δ6
are quality-of-life follow-ups; none of them blocks adding a new
provider, only how clean the result looks.

## Adding a new provider

1. Implement `TurnSession` for a new struct in a sibling module
   (e.g. `runtime/gemini.rs`).
2. Provide a `setup_*_session` factory.
3. Implement `ProviderAdapter` for an empty unit struct that calls
   `agent_loop::run_{blocking,streaming}_loop`.
4. Add one match arm in `runtime/provider_adapter.rs::adapter_for_api`.

`runtime.rs` does not change.

## File map

```
crates/puffer-core/runtime/
├── agent_loop.rs              ← driver + TurnSession trait
├── provider_adapter.rs        ← ProviderAdapter trait + adapter_for_api
├── anthropic.rs               ← AnthropicTurnSession + AnthropicAdapter
├── anthropic_sse.rs           ← Anthropic SSE parser
├── openai.rs                  ← OpenAIResponsesAdapter, OpenAICompletionsAdapter
│                                + retained `execute_openai_streaming` for the
│                                  websocket fallback
├── openai_sse.rs              ← OpenAI Responses SSE parser
├── openai_ws.rs               ← OpenAI Responses websocket transport
└── openai/
    ├── conversation.rs        ← `ConversationItem` and per-vendor wire converters
    │                            (file name is historical; the type is neutral)
    ├── responses_session.rs   ← OpenAIResponsesTurnSession
    ├── completions_session.rs ← OpenAICompletionsTurnSession
    ├── support.rs             ← OpenAI execution config, capability sniffing
    └── websocket.rs           ← OpenAIResponsesAdapter websocket path
                                 (currently bypasses agent_loop — separate
                                  migration)
```

## References

- PR #64: <https://github.com/berabuddies/puffer/pull/64>
- Original comparative design source: pi-mono's `packages/agent/src/agent-loop.ts`
  and `packages/ai/src/api-registry.ts`.
