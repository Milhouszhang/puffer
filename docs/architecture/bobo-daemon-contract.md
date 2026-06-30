# Bobo Daemon Contract

Status: live architecture contract for the puffer daemon surface consumed by
Bobo.

Bobo is a local desktop shell. It starts `puffer daemon --print-handshake`, hands
the handshake to its frontend, and then depends on puffer for chat sessions,
provider auth/config, workflow snapshots, connector tasks, approval flows,
contacts, diagnostics, and browser/tool execution.

## Handshake

`puffer daemon` writes and optionally prints a handshake JSON object:

```json
{
  "url": "ws://127.0.0.1:<port>/ws",
  "token": "<daemon-token>",
  "protocolVersion": "1",
  "workspaceRoot": "/absolute/workspace"
}
```

Bobo passes `--handshake-file <BOBO_HOME>/projects/default/.puffer/daemon.handshake`
and expects the first stdout line from `--print-handshake` to be this JSON.

Compatibility rules:

- `url`, `token`, `protocolVersion`, and `workspaceRoot` are required.
- `workspaceRoot` must reflect the daemon process cwd.
- Additive fields are allowed.
- Removing or renaming existing fields requires a Bobo compatibility plan.

## Workspace And Data Home

Bobo starts the daemon with cwd `<BOBO_HOME>/projects/default`. Puffer must keep
that cwd distinct from its user config/data directory:

- cwd/workspace drives project-scoped resources, project ACL, daemon discovery,
  and browser CLI matching.
- user config/data, normally `~/.puffer`, stores sessions, auth, provider config,
  workflows, monitor state, connections, resources, and user memory.

Do not move user data under Bobo's project cwd unless Bobo explicitly changes
its storage contract.

## RPC Envelope

Bobo uses daemon websocket requests with a method string and JSON params. Methods
must be stable by name. New fields in response objects should be additive.

When a daemon method accepts aliases such as `monitor_reply_send` and
`task_monitor_reply_send`, keep both spellings until all desktop clients have
migrated. Prefer the `task_monitor_*` names for new Bobo code.

## Required Method Groups

Session/chat:

- `create_session`
- `list_grouped_sessions`
- `load_session_detail`
- `rename_session`
- `delete_session`
- `set_session_tags`
- `run_agent_turn`
- `cancel_turn`
- `resolve_permission`
- `resolve_user_question`
- `dispatch_slash_command`

Provider/config:

- `login_with_api_key`
- `login_with_oauth`
- `logout_provider`
- `update_config`
- `load_settings_snapshot`

Contacts/connectors/workflows/tasks:

- `workflow_list`
- `workflow_save`
- `workflow_toggle`
- `workflow_binding_create`
- `workflow_binding_delete`
- `workflow_connection_delete`
- `workflow_runs_list`
- `workflow_run_show`
- `contacts_list`
- `contacts_search`
- `contacts_refresh`
- `task_monitor_create`
- `task_monitor_ignore`
- `task_monitor_complete`
- `task_monitor_reply_send`
- `task_monitor_action_execute`
- `task_monitor_memory_save`
- `task_monitor_history_list`
- `task_monitor_trace_list`
- `telegram_diagnostics_export`

Tool/file support:

- `read_file`
- `write_file`
- `list_dir`
- pty/browser/lsp/mcp methods surfaced by the shared chat UI.

## Provider Contract

Bobo's WorldRouter login maps to puffer's built-in OpenAI-compatible provider:

- `login_with_api_key` with `providerId: "openai"`
- `update_config` with `openaiBaseUrl`, `defaultProvider: "openai"`, and
  `defaultModel`

There is no `puffer` provider id. Do not add one as a compatibility shim unless
the provider registry owns that new meaning across all clients.

## `load_session_detail.activeTurnId`

`activeTurnId` is intentionally tri-state:

- string: the daemon in-memory turn registry has an active turn for the session.
- `null`: the daemon knows there is no active turn.
- absent: legacy transcript without enough boundary data.

Bobo relies on this distinction to avoid treating old `TurnBoundary` rows as
active turns after reload.

## `workflow_list` Home Contract

Bobo Home reads `workflow_list.monitor_tasks[]` for active monitor/user tasks.
Task history and trace endpoints may include terminal historical records. Keep
active Home cards and historical trace rows distinct; historical task creation
metadata is not by itself an active task.

## Error Compatibility

Turn and transport failures should preserve raw diagnostic details while exposing
stable categories where available. Bobo has UI handling for classes such as:

- `model_gateway_unavailable`
- `provider_stream_closed`
- `runner_unreachable`

Do not collapse provider stream closure, runner availability, and model gateway
transport errors into a single generic string; Bobo needs enough structure to
show useful recovery text.

## Change Checklist

Before changing daemon behavior used by Bobo:

1. Search Bobo for the method/field.
2. Preserve old method aliases or response fields where possible.
3. Add daemon tests for the response shape.
4. Document the Bobo version floor if the change cannot be backward compatible.
5. Coordinate monitor/task policy changes with the monitor-pipeline architecture
   docs.
