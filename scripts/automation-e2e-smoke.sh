#!/usr/bin/env bash
set -euo pipefail

mode="${1:---local}"

case "$mode" in
  --local)
    export PUFFER_AUTOMATION_E2E_MODE=local
    ;;
  --cloud)
    export PUFFER_AUTOMATION_E2E_MODE=cloud
    required=(
      PUFFER_WORKFLOW_API_URL
      PUFFER_WORKFLOW_WORKSPACE_ID
      PUFFER_WORKFLOW_API_TOKEN
    )
    missing=()
    for name in "${required[@]}"; do
      if [[ -z "${!name:-}" ]]; then
        missing+=("$name")
      fi
    done
    if (( ${#missing[@]} > 0 )); then
      printf 'missing required environment variable(s): %s\n' "${missing[*]}" >&2
      exit 1
    fi
    ;;
  *)
    printf 'usage: %s [--local|--cloud]\n' "$0" >&2
    exit 2
    ;;
esac

cargo test -p puffer-cli automation_real_e2e -- --ignored
