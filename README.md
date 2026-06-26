# Puffer Code

Puffer Code is a Rust agent runtime and CLI. It provides an interactive TUI, a
websocket daemon used by desktop shells such as Bobo, connector/subscriber
workflows, permissions, skills, provider transports, tools, and session storage.

## Quick Start

```bash
cargo build -p puffer-cli
cargo run -p puffer-cli -- --help
```

Common local validation:

```bash
cargo test --workspace
cargo fmt --check
bash scripts/check-doc-links.sh
bash scripts/check-slash-commands.sh
bash scripts/report-large-files.sh
```

`scripts/report-large-files.sh` is informational by default. Use it to make
large-file risk visible when a change touches a known oversized module.

## Repo Map

`Cargo.toml` is the source of truth for workspace membership. Main areas:

- `crates/puffer-cli` - CLI, daemon, desktop-facing RPC handlers, auth, browser
  daemon support, workflow/task/contact endpoints.
- `crates/puffer-core` - runtime state, provider-neutral agent loop,
  permissions, command registry, slash-command dispatch, prompts, tool
  execution integration.
- `crates/puffer-tools` and `resources/tools/` - tool definitions and built-in
  execution backends.
- `crates/puffer-resources` and `resources/` - bundled/user/workspace prompts,
  tools, skills, plugins, MCP servers, connectors, and manifests.
- `crates/puffer-session-store` - transcript/session storage and load/list/fork
  support.
- `crates/puffer-workflow`, `crates/puffer-subscriptions`,
  `crates/puffer-subscriber-*`, and `crates/puffer-connector-*` - workflows,
  connector monitors, subscribers, action dispatch, and connection integrations.
- `crates/puffer-provider-*` and `crates/puffer-transport-anthropic` - provider
  descriptors, auth, and transport-specific compatibility.
- `crates/puffer-runner-*` and `crates/puffer-tool-runner` - runner APIs and
  local/grpc tool execution.
- `apps/puffer-desktop` - desktop app surface and fuzz/test harnesses.
- `benchmark/` and `vendor/` - benchmarks and vendored third-party components.

## Live Docs

- `AGENTS.md` - agent-facing repo guide and contribution guardrails.
- `docs/README.md` - docs index and status taxonomy.
- `docs/architecture/agent-loop.md` - provider-neutral agent loop architecture.
- `docs/architecture/bobo-daemon-contract.md` - Bobo-facing daemon compatibility
  contract.
- `docs/architecture/permissions-and-skills.md` - permissions, ACL, skill, and
  Lambda Skill boundaries.
- `docs/reference/slash-commands.md` - generated built-in slash-command surface.
- `docs/observability/langfuse-design.md` - observability design.

Numbered files under `specs/` are change records. Treat them as historical
unless a live architecture doc explicitly references them as current behavior.
