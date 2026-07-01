# Automation Design Roadmap

## Purpose

Automation should help users turn repeated coordination work into reviewable
drafts and clear follow-up actions. The UI should feel like a calm operations
workspace inside Puffer, not a node editor or infinite canvas.

This roadmap is a design guide for the desktop Automation tab. It intentionally
focuses on user-visible automations and leaves backend execution details for later
contracts.

## North Star

The Automation tab becomes the place where a user can:

- See what needs attention now.
- Create an automation from a familiar template.
- Review the context and draft before any outward action happens.
- Understand what is running, paused, failed, or recently completed.
- Adjust repeated work without thinking in graphs, nodes, or canvas mechanics.

## Design Principles

### Linear First

Every automation should be explainable as a short sequence:

1. Something happens.
2. Puffer gathers context.
3. Puffer prepares a draft or recommendation.
4. The user reviews and decides.
5. The result is recorded.

The UI can show branches or conditions later, but the primary representation
should remain list, detail, and timeline based.

### User-Facing Language

Visible copy should describe what the user can do, not implementation status.
Use phrases like `Review inbox`, `Drafts stay editable`, `You approve actions`,
and `Last run`. Avoid exposing internal terms such as preview-only, missing
backend wiring, trigger plumbing, storage state, or implementation guardrails.

### Approval Is A Product Experience

Approval should feel like a normal review step, not a safety disclaimer. Users
should see:

- The proposed action.
- The context Puffer used.
- The draft content.
- The destination or audience.
- Clear choices to edit, approve, reject, or snooze.

### Compact, Puffer-Native Layout

Automation should use the same restrained desktop language as the rest of
Puffer: sidebar entry, top bar, prompt-first home, compact automation cards, and
predictable buttons. Avoid marketing-style heroes, decorative cards, dense
graph controls, and page-level navigation tabs.

## Information Architecture

### Primary Screen

The default Automation screen should follow a Langdock-like automation home inside
Puffer's desktop shell:

- Top bar with the page title.
- Centered prompt composer using the same element structure as Puffer's chat
  composer: attachment control, model picker, fast toggle, thinking selector,
  permissions selector, keyboard hint, and send button.
- The send button opens a full-page builder inside the Automation destination,
  replacing the home until the user goes back. For natural-language prompts,
  the builder should preserve the user's words in `Instructions` and pre-fill
  any matching name, trigger, and app rows for review.
- A library toolbar below the prompt with `Your automations` and
  `Template Library` as local tabs, plus a `New automation` action on the right.
- `Your automations` starts as an empty state until the user saves an automation.
- No separate `Automations` heading or explanatory subtitle above the library.
- No page-level secondary tab bar.
- No canvas, graph, or board surface.

The prompt is the primary creation entry. `Your automations` is an empty state before
the user creates anything, then becomes the re-entry point for saved user work. `Template
Library` cards start a new automation from a known pattern.

### Detail Sections

Each selected automation should use the same plain sections:

`When`
: The event or condition that makes the automation useful.

`Then`
: What Puffer prepares for the user.

`Review`
: How the user checks, edits, approves, rejects, or pauses the result.

`Recent activity`
: A short list of latest drafts, runs, setup hints, or status changes.

## Roadmap

### Phase 0: Static Shell

Status: shipped in desktop UI.

Goals:

- Add the sidebar destination.
- Establish the prompt-first home.
- Use product-facing copy.
- Keep the surface independent from backend execution.

Design notes:

- Default to the `Create an automation` prompt.
- Show representative automation cards below the prompt.
- Avoid secondary tabs.
- Avoid any canvas affordance.

### Phase 1: Approval Detail

Goal: make the most important user loop feel complete before adding creation.

Entry points:

- Click the `Review inbox` row.
- Click a waiting-draft indicator on any automation row.
- Open a notification or pending task deep link later.

Detail structure:

- Header: automation name, source, status, received time.
- Context: short source summary, relevant message or event excerpt, linked
project/session if available.
- Draft: editable proposed reply, review note, RSVP, issue update, or summary.
- Decision bar: `Approve`, `Edit`, `Reject`, `Snooze`.
- Activity: what Puffer checked, why it suggested the action, and previous
decisions from the same source.

Key states:

- Ready to review.
- Edited locally.
- Approved.
- Rejected.
- Snoozed.
- Needs more context.

Acceptance criteria:

- A user can understand the proposed action without opening another screen.
- Approving is visually distinct from editing.
- Rejecting asks for a short reason only when useful.
- The UI never implies that an action has been sent before confirmation.

### Phase 2: Template-To-Automation Creation

Goal: let users create automations without designing graphs.

Entry points:

- Send from the home composer.
- Template cards in the library.
- `New automation` in the library toolbar.

Recommended templates:

- PR review assistant.
- Issue triage.
- Telegram reply draft.
- Calendar RSVP assistant.
- Morning digest.
- Release watch.

Configuration pattern:

1. Open a dedicated creation page from the home composer or a template card.
2. Preserve the home composer text as the initial instructions.
3. Name the automation in the first field.
4. Add or review triggers in a compact row list.
5. Review or edit the instructions in one large prompt field.
6. Choose the model, tools, and environment controls below the prompt.
7. Save the automation or cancel back to the home.

Layout:

- Full-page builder, not a modal, side pane, or canvas.
- Top: breadcrumb back to `Automations`, current creation label, `Cancel`, and
  `Save`.
- Main: name field, `Triggers`, `Instructions`, `Tools`, and
  `Cloud Agent Environment`, in that order. Tool rows should mirror trigger
  rows by showing each selected API capability as its own sentence-like item.
  Capabilities that need a destination or mode should show an inline target
  chip, such as `Send to Slack` `to` `#teams`. The `Add Tool or MCP` picker
  should expose each app's API capabilities as separate selectable items inside
  the picker.
  Added trigger and tool rows must remain editable and removable before save.

Acceptance criteria:

- The user never needs to think about nodes or edges.
- Creation does not appear inside the selected detail pane.
- Every field has a plain-language label.
- Creation follows the Cursor single-page automation pattern while using
  Puffer's existing visual system.

### Phase 3: Active Management

Goal: make enabled automations understandable and controllable.

Automation list rows should show:

- Name.
- Source.
- State: running, paused, needs attention, failing.
- Last run.
- Next expected run when applicable.
- Waiting drafts when applicable.

Detail panel should include:

- Toggle enabled or paused.
- Last run result.
- Recent failures.
- Linked template or configuration.
- Quick actions: edit, duplicate, archive.

Acceptance criteria:

- A user can answer "what is this doing?" in under five seconds.
- Pause and resume are reversible and obvious.
- Failure copy is actionable and does not expose raw backend errors unless
expanded.

### Phase 4: Run History And Trace

Goal: explain what happened after the fact.

History row should show:

- Automation name.
- Outcome.
- Source event.
- Time.
- User decision when applicable.

Run detail should show a vertical timeline:

1. Source event received.
2. Context gathered.
3. Draft prepared.
4. User decision.
5. Final result.

Acceptance criteria:

- History can be filtered by automation, source, outcome, and time.
- Failed runs show the recovery path.
- Approved actions show who approved them and when.

### Phase 5: Test Run And Preview

Goal: help users trust an automation before enabling it.

Path:

1. Select or paste a sample event.
2. Run a dry preview.
3. Inspect the proposed draft and context.
4. Adjust configuration.
5. Save or enable.

Acceptance criteria:

- The test run clearly reads as a preview.
- The generated draft can be edited or copied.
- The user can return to the configuration without losing input.

### Phase 6: Settings And Governance

Goal: support team and workspace policies once the core loop is stable.

Future areas:

- Default approval requirements by connector.
- Allowed destinations.
- Quiet hours.
- Escalation contacts.
- Audit export.
- Workspace-level template presets.

Acceptance criteria:

- Policy language remains user-facing.
- Workspace rules explain what users can do, not only what is blocked.

## Cross-Automation Components

### Automation Row

Reusable row for review, active, starter, and history-backed automations. It
should support:

- Icon.
- Title.
- Source.
- Status.
- Time metadata.
- Optional count or warning badge.

### Detail Header

Consistent header for detail panes:

- Title.
- Source and time.
- Status badge.
- Primary action when appropriate.

### Draft Review Panel

The approval detail needs a reusable draft panel that supports:

- Plain text editing.
- Destination preview.
- Context side notes.
- Save state.
- Approve or reject actions.

### Run Timeline

History and approval details both need a vertical event timeline. It should be
compact, scannable, and able to hide technical detail behind disclosure rows.

## Empty States

`Saved list`
: "No saved automations yet" with a `New` action.

`Review inbox row`
: "Nothing needs review" with a secondary prompt to create an automation.

`Starter rows`
: "No starters found" with refresh or custom automation later.

`Recent activity`
: "No runs yet" with explanation that history appears after automations run.

## Copy Guidelines

Prefer:

- `Review inbox`
- `Drafts stay editable`
- `You approve actions`
- `Last run`
- `Needs review`
- `Ready to enable`
- `Paused`
- `Try with sample`

Avoid in visible UI:

- `UI preview only`
- `Backend not wired`
- `Not connected to storage`
- `Human-gated`
- `Execution pipeline`
- `Trigger envelope`
- `Monitor guardrail`

## Non-Goals

- Infinite canvas editing.
- Exposing automation graph internals.
- Direct send or post actions without a user decision.
- Full backend automation storage in the first UI pass.
- Replacing the existing Automations screen.

## Suggested Implementation Order

1. Approval detail automation.
2. Template-to-automation creation.
3. Active management details.
4. History drilldown.
5. Test run preview.
6. Empty and error states across all sections.
7. Backend contracts and persistence.

This order starts with the part users will trust or reject first: whether
Puffer can show a useful draft, explain it, and let the user make the final
decision.
