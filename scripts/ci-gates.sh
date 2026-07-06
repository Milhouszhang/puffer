#!/usr/bin/env bash
# ci-gates.sh — run every gate CI enforces, locally, in parallel lanes.
#
# Mirrors .github/workflows/ci.yml. Use before pushing (especially after a
# merge/rebase, where the lefthook pre-commit/pre-push hooks intentionally
# skip) to catch failures in one pass instead of one push-attempt at a time.
#
#   scripts/ci-gates.sh          # all gates (~5 min, bounded by root nextest)
#   scripts/ci-gates.sh --quick  # compile + lint gates only, skip test suites
#
# The four lanes are independent build graphs, so they genuinely overlap:
#   A: root workspace    — fmt, clippy, nextest, doc tests
#   B: src-tauri         — fmt, clippy, nextest   (separate Cargo workspace)
#   C: desktop frontend  — svelte-check, node tests
#   D: desktop UI        — full Playwright suite (skipped in --quick)
# Within a lane, cargo steps stay sequential (they share the target/ lock).
set -u

cd "$(git rev-parse --show-toplevel)"

QUICK=0
[ "${1:-}" = "--quick" ] && QUICK=1

# tmux TUI tests are terminal-/timing-fragile; CI skips them the same way.
export PUFFER_SKIP_TMUX_TESTS=1

LOGDIR="$(mktemp -d "${TMPDIR:-/tmp}/ci-gates.XXXXXX")"

lane_root() {
  cargo fmt --all --check || return 1
  cargo clippy --workspace --all-targets -- \
    -D clippy::correctness -D clippy::suspicious || return 1
  [ "$QUICK" = 1 ] && return 0
  cargo nextest run --workspace --locked --profile ci || return 1
  cargo test --workspace --doc --locked || return 1
}

lane_tauri() {
  cd apps/puffer-desktop/src-tauri || return 1
  cargo fmt --all --check || return 1
  cargo clippy --all-targets -- \
    -D clippy::correctness -D clippy::suspicious || return 1
  [ "$QUICK" = 1 ] && return 0
  cargo nextest run --locked --profile ci || return 1
}

lane_frontend() {
  cd apps/puffer-desktop || return 1
  npm run check || return 1
  [ "$QUICK" = 1 ] && return 0
  npm run test:tauri-config || return 1
  npm run test:sidebar-css || return 1
  npm run test:provider-visuals || return 1
}

# Full Playwright UI suite (~2 min). Playwright manages its own vite webServer
# (reuses a dev server already on 1420). pipefail so a pipeline can never mask
# playwright's exit code.
lane_desktop_ui() {
  set -o pipefail
  [ "$QUICK" = 1 ] && return 0
  cd apps/puffer-desktop || return 1
  npx playwright test || return 1
}

lane_root       > "$LOGDIR/root.log"       2>&1 & PID_ROOT=$!
lane_tauri      > "$LOGDIR/tauri.log"      2>&1 & PID_TAURI=$!
lane_frontend   > "$LOGDIR/frontend.log"   2>&1 & PID_FRONTEND=$!
lane_desktop_ui > "$LOGDIR/desktop-ui.log" 2>&1 & PID_DESKTOP_UI=$!

FAILED=0
report() { # name pid logfile
  if wait "$2"; then
    echo "✔ $1"
  else
    FAILED=1
    echo "✘ $1 — last 40 lines of $3:"
    tail -40 "$3" | sed 's/^/    /'
  fi
}

report "root workspace (fmt + clippy + tests)" "$PID_ROOT" "$LOGDIR/root.log"
report "src-tauri (fmt + clippy + tests)"      "$PID_TAURI" "$LOGDIR/tauri.log"
report "desktop frontend (svelte-check + node tests)" "$PID_FRONTEND" "$LOGDIR/frontend.log"
report "desktop UI (playwright suite)"         "$PID_DESKTOP_UI" "$LOGDIR/desktop-ui.log"

if [ "$FAILED" = 0 ]; then
  echo "All CI gates green. Full logs: $LOGDIR"
else
  echo "Gate(s) failed. Full logs: $LOGDIR"
fi
exit "$FAILED"
