# Agent-Driven Automated UI Testing Workflow — Design

Date: 2026-07-02
Branch: `feat/agent-ui-testing`
Status: Approved

## Background and Goals

Puffer desktop (`apps/puffer-desktop`, Svelte 5 + Tauri 2) already has a solid testing foundation:

- Playwright 1.59.1 + 30+ `tests/*-ui.spec.ts` files (a mock layer that stubs `__TAURI_INTERNALS__`)
- `DaemonFixture` embedded in `tests/real-daemon-ui.spec.ts`: spawns a real `target/debug/puffer` daemon (isolated temp HOME/workspace + mock providers) and injects the handshake into the frontend via URL query params (`corbinaBackend`/`corbinaToken`) — **frontend and backend talk over HTTP, so a plain browser can reach the real backend; no WKWebView driving required**
- Storybook (port 6006, with the a11y addon)
- `scripts/ci-gates.sh` regression gates

Goal: build on this foundation to integrate an **agent-driven automated testing workflow** that replaces manual testing with "agent explores → produces a report → hardens findings into regression specs", cutting manual testing time. Compounding growth of test assets (specs) is the fundamental time-saving mechanism.

Constraints: no backward compatibility concerns; optimize only for long-term value, stability, and performance; guard against over-engineering.

## Scenario Routing (Core Decision Table)

| Scenario | Tool | Deliverable | Manual work replaced |
|---|---|---|---|
| New-feature acceptance (flow exploration) | agent-browser + `agent-app.mjs` isolated environment | Exploration report + hardened `*-ui.spec.ts` | Manually clicking through every path of a new feature |
| UI/UX review (style changes, visual state sweep) | agent-browser screenshots + agent visual judgment; component-level via Storybook:6006 | Visual issue list + `toHaveScreenshot()` baseline specs | Eyeballing layout/dark mode/hover state by state |
| Bug deep-dive (daemon protocol, network, console) | playwright-mcp (network interception, trace, console) | Root cause analysis + minimal reproduction spec | Manually opening DevTools to capture and debug |
| Regression protection (every commit) | Repo's `@playwright/test` (no agent, no LLM) | ci-gates pass/fail | Manual regression testing |

Routing principles: use agent-browser for high-frequency iteration (~4x the token efficiency of the MCP option, same Playwright underneath, and role-based locators translate 1:1 into specs); use playwright-mcp for deep debugging (network/trace capabilities); CI never depends on an agent.

Tool selection rationale (researched 2026-07): agent-browser (official Vercel) and playwright-mcp (official Microsoft, 34k+ stars) are both mainstream, stable tools; a measured 10-step task consumed ~27k vs ~114k tokens, hence agent-browser as the exploration-layer default.

## Architecture and Data Flow

```
┌─ Exploration mode (local) ──────────────────────────────┐
│ scripts/agent-app.mjs                                   │
│   ├─ spawns an isolated daemon (temp HOME/workspace,    │
│   │  mock provider)                                     │
│   ├─ reuses a running Vite on :1420, or starts its own  │
│   └─ prints: http://127.0.0.1:1420/?skipOnboarding=1    │
│            &corbinaBackend=<url>&corbinaToken=<token>   │
│                          ↓                              │
│ agent-browser / playwright-mcp opens that URL           │
│ (tool-agnostic)                                         │
│   → real frontend + real Rust backend, fully isolated   │
│     from the user's dev data                            │
└─────────────────────────────────────────────────────────┘
┌─ Hardening mode (CI) ───────────────────────────────────┐
│ agent writes findings as tests/*-ui.spec.ts             │
│   → import tests/support/daemonFixture.mjs              │
│   → npm run test:desktop-ui → ci-gates                  │
└─────────────────────────────────────────────────────────┘
```

Key mechanism: the handshake is injected via URL query params, so a single Vite instance can serve the user's dev app and the agent's isolated instance simultaneously without interference — no second port needed.

## Components (4 change points, ~200 lines of new code, zero new runtime dependencies)

1. **`tests/support/daemonFixture.mjs`**
   Extract `DaemonFixture` and the provider mocks (OpenAI/Anthropic) from `real-daemon-ui.spec.ts`, rewritten as `.mjs` + JSDoc types; the original spec switches to importing it, with no compatibility layer left behind.
   `.mjs` over `.ts`: both the CLI script and TS specs can import it directly without introducing a tsx/ts-node dependency.

2. **`scripts/agent-app.mjs`** (~80 lines)
   - `--provider mock|real`: defaults to mock; `real` reads the key from the `RELAYDANCE_API_KEY` environment variable and points at the relaydance gateway, failing fast if the key is missing
   - The mock provider returns a fixed canned reply (sufficient for UI flow testing; no scripted reply configuration — YAGNI)
   - Vite lifecycle: if a Vite is already running on 1420, reuse it and leave it alone on exit; otherwise start one and take responsibility for killing it on exit
   - Finally prints the full URL with handshake params (humans and any agent tool can open it directly)
   - On SIGINT/SIGTERM: kill the daemon (and any self-started Vite), clean up the temp directory

3. **`.mcp.json`** (new; the repo currently has no such file)
   Add playwright-mcp (`npx @playwright/mcp@latest`), scoped to the bug deep-dive scenario; MCP tools load lazily, costing no tokens when unused.

4. **`AGENTS.md`**
   New section: scenario routing table + explore→harden workflow conventions (specs live in `tests/`, role-based selector style, must pass `npm run test:desktop-ui` to count as hardened, component-level UI review prefers Storybook).

5. **Visual baseline platform boundary (review addendum)**
   `toHaveScreenshot()` baselines are platform-dependent: CI runs on `ubuntu-latest` while baselines are generated on macOS, so font/anti-aliasing differences would make CI fail spuriously. Visual specs therefore live separately (`tests/visual/*.spec.ts`) with their own script `test:desktop-visual`, and **run on macOS locally only** (developer/agent pre-commit verification), staying out of GitHub CI; functional specs go into CI as usual. Whether rebuilding Linux baselines inside a CI container is worthwhile is deferred until visual specs accumulate enough to cause maintenance pain.

## Error Handling

- `target/debug/puffer` missing → error out with a hint to run `cargo build -p puffer-cli`
- Handshake timeout (15s) → dump daemon stderr, then exit
- Vite failed to start → explicit error, no silent retry
- `--provider real` with `RELAYDANCE_API_KEY` missing → fail fast
- Fail fast across the board, no automatic recovery (this is a dev tool; failures must be loud)

## Testing Strategy

- Correctness of the extraction refactor: guaranteed by the existing `real-daemon-ui.spec.ts` continuing to pass (it is the extracted module's first consumer)
- `agent-app.mjs`: one minimal smoke spec — start the script, parse the printed URL, confirm the daemon process is alive and the URL is reachable (HTTP 200), send SIGTERM and confirm process exit plus temp directory cleanup
- Visual issues found during UI/UX exploration are hardened into `toHaveScreenshot()` baseline specs under `tests/visual/`, verified locally on macOS via `test:desktop-visual` (see component 5 for the platform boundary)

## Explicitly Out of Scope (over-engineering guardrails)

- In-process instrumentation (Victauri-style homegrown tooling): Puffer's frontend and backend talk over an HTTP handshake, so a browser can already reach the real backend — that class of solution addresses a problem Puffer doesn't have
- Agent exploration in CI: CI only runs hardened specs, never depends on an LLM
- Claude Code skill packaging: re-evaluate against real pain points once the workflow has been running smoothly (~two weeks)
- Visual regression services beyond screenshot baselines
- A second Vite port for isolation
- Native shell automation (Dock badge, windows, menus): no mainstream solution exists; keep OS-level manual verification

## Decision Log

| Decision | Conclusion | Rationale |
|---|---|---|
| Primary use case | Explore + harden loop | Test assets compound; highest long-term value |
| LLM provider | mock by default, real (relaydance) optional | Deterministic, free, fast; full-chain verification on demand |
| CI scope | Hardened specs only | Maximum CI stability, no agent/LLM dependency |
| Entry point form | Standalone CLI script | Works for humans and agents alike, tool-agnostic |
| Default exploration tool | agent-browser | ~4x token efficiency, already installed with zero config, hardening mapping equivalent to playwright-mcp |
| Deep-dive tool | playwright-mcp | Network interception/trace/console capabilities |
