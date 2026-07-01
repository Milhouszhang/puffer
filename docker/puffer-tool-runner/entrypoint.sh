#!/usr/bin/env bash
set -euo pipefail

HOME_DIR="${HOME:-/root}"
USER_DIR="${MANAGED_AGENT_USER_PATH:-/user}"
WORKSPACE_DIR="${MANAGED_AGENT_WORKSPACE_PATH:-/workspace}"
RUNNER_PORT="${MANAGED_AGENT_PUFFER_RUNNER_PORT:-50051}"
RUNNER_BIND="${MANAGED_AGENT_PUFFER_RUNNER_BIND:-0.0.0.0:${RUNNER_PORT}}"
RUNNER_TOKEN="${MANAGED_AGENT_PUFFER_RUNNER_TOKEN:-${PUFFER_RUNNER_TOKEN:-}}"
RESOURCES_DIR="${PUFFER_BUILTIN_RESOURCES_DIR:-/opt/puffer/resources}"

export HOME="$HOME_DIR"
export PUFFER_HOME="${PUFFER_HOME:-$HOME_DIR}"
export PUFFER_BUILTIN_RESOURCES_DIR="$RESOURCES_DIR"

if [ "$#" -gt 0 ]; then
  exec "$@"
fi

mkdir -p "$USER_DIR" "$WORKSPACE_DIR" "$PUFFER_HOME/.puffer"
cd "$USER_DIR"

args=(--bind "$RUNNER_BIND" --cwd "$USER_DIR")
if [ -n "$RUNNER_TOKEN" ]; then
  args+=(--token "$RUNNER_TOKEN")
elif [ "${PUFFER_RUNNER_ALLOW_UNAUTHENTICATED:-}" = "1" ]; then
  args+=(--allow-unauthenticated)
fi

exec /usr/local/bin/puffer-tool-runner "${args[@]}"
