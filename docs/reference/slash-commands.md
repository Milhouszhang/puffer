# Slash Commands

Generated from `crates/puffer-core/command/registry.rs`. When the registry
changes, update this file and run `scripts/check-slash-commands.sh`.

## Built-In Commands

- `/add-dir` - Add a new working directory.
- `/agents` - Manage agent configurations.
- `/telegram-relationships` - Rank Telegram contacts by recent chat frequency.
- `/autodream` - Consolidate durable project memory and suggest reusable traces.
- `/branch` - Create a branch of the current conversation.
- `/btw` - Ask a side question without interrupting the main conversation.
- `/buddy` - Show or interact with Clawd.
- `/clear` - Clear conversation history.
- `/color` - Set the prompt bar color.
- `/commit` - Create a git commit.
- `/compact` - Summarize the conversation to preserve context.
- `/config` - Open config.
- `/connect` - Configure a named connector connection.
- `/context` - Show current context usage.
- `/copy` - Copy the latest assistant output.
- `/cost` - Show cost and duration for the current session.
- `/diff` - View uncommitted changes and per-turn diffs.
- `/debug` - Show raw context sent to the model.
- `/doctor` - Diagnose the Puffer installation and settings.
- `/effort` - Set model effort.
- `/exit` - Exit the REPL.
- `/export` - Export the current conversation.
- `/files` - List files currently in context.
- `/fast` - Toggle fast mode.
- `/goal` - Set or view the long-running task goal.
- `/genskill` - Generate a reusable skill from the current conversation.
- `/help` - Show help and available commands.
- `/hooks` - View hook configurations.
- `/ide` - Manage IDE integrations and status.
- `/init` - Initialize project guidance and defaults.
- `/keybindings` - Open or create keybindings.
- `/login` - Sign in to a provider.
- `/loop` - Run a prompt repeatedly until stopped.
- `/logout` - Sign out from a provider.
- `/maximize` - Maximize a metric through repeated prompt execution.
- `/mcp` - Manage MCP servers.
- `/minimize` - Minimize a metric through repeated prompt execution.
- `/memory` - Edit memory files.
- `/recap` - Summarize the session transiently.
- `/model` - Select the active model.
- `/monitor` - Create connector monitors that turn messages into tasks.
- `/night` - Run autonomous overnight work.
- `/pentest` - Run an authorized web penetration test.
- `/permissions` - Manage tool permission rules.
- `/plan` - Enable plan mode or view the current session plan.
- `/plugin` - Manage Puffer plugins.
- `/pr-comments` - Get comments from a GitHub pull request.
- `/reflect` - Toggle runtime reflection.
- `/reload-plugins` - Activate pending plugin changes.
- `/remote-control` - Connect this terminal for remote-control sessions.
- `/remote-env` - Configure the remote environment.
- `/rename` - Rename the current conversation.
- `/resume` - Resume a previous conversation.
- `/rewind` - Restore code or conversation to a previous point.
- `/review` - Review the current worktree or pull request.
- `/sandbox` - Explain removed sandbox mode.
- `/security-review` - Complete a security review of pending changes.
- `/session` - Show remote session URL and QR code.
- `/skills` - List available skills.
- `/skill:<name>` - Run a loaded skill by slash-safe skill name.
- `/status` - Show current session configuration and status.
- `/statusline` - Set up status line UI.
- `/tag` - Toggle a searchable session tag.
- `/tasks` - List and manage background tasks.
- `/terminal-setup` - Terminal setup helper; it may be hidden depending on local state.
- `/theme` - Change the theme.
- `/ultrareview` - Multi-agent code review of a worktree or PR.
- `/usage` - Show plan usage limits.
- `/vim` - Toggle Vim editing mode.
- `/workflows` - Show workflow, connector, connection, task, and run status.

Aliases and loaded user-invocable skills are also exposed by the command surface.
The built-in alias list lives in the registry next to each command definition.
