# Automation Design Handoff

## Purpose

The desktop Automation tab is a prompt-first place for users to create and
manage simple automations without a canvas. The current implementation is UI
only. It uses local Svelte state to model creation, saved automations, editable
settings, and run history so that the product shape can be reviewed before
backend contracts are added.

The design intent is:

- Keep automation creation linear and reviewable.
- Match Puffer's compact desktop visual style.
- Use user-facing automation language.
- Avoid node graphs, infinite canvas controls, and internal status copy.

## Current Entry Points

### Sidebar

Automation is available as a sidebar destination in the desktop shell. The
screen title is `Automation`.

### Home Prompt

The home screen starts with `Create an automation` and reuses Puffer's composer
structure:

- Attachment button.
- Model picker.
- Fast toggle.
- Thinking selector.
- Permissions selector.
- Send button.

The placeholder asks users to describe what to automate in natural language.
Submitting the prompt opens the full-page builder and pre-fills configuration
when the prompt matches known patterns.

### Library Area

Below the prompt, the library is a segmented control:

- `Your automations`
- `Template Library`

`Your automations` starts empty. The empty state says
`创建你的第一个automation，处理重复的工作流` and has a `create automation`
action. The toolbar action is `new`.

`Template Library` shows starter cards that open the builder with predefined
name, instructions, and trigger data.

## Current Creation Path

The builder is a full page, not a modal or side panel.

Top bar:

- Breadcrumb back to `Automations`.
- `Create New` label.
- `Cancel`.
- `Save`.

Body sections:

- `Name`
- `Triggers`
- `Instructions`
- `Tools`
- `Cloud Agent Environment`

Saving creates a local automation card and returns to the home screen. Cancel
returns to the home screen without creating a card.

### Natural-Language Prefill

The prompt parser currently recognizes broad keyword groups:

- Pull request prompts become `PR review draft`.
- Calendar, invite, RSVP, or meeting prompts become `Calendar RSVP`.
- Gmail or email prompts become `Email reply draft`.
- Slack, message, or reply prompts become `Reply draft`.
- Daily, weekday, morning, digest, or every prompts become `Morning digest`.

The prefill is intentionally local and heuristic. It is only used to make the UI
review path feel real.

### Template Starters

Current templates:

- `Review PRs`
- `Reply drafts`
- `Calendar RSVP`
- `Morning digest`

Each template maps to a name, instructions, icon, and initial trigger.

## Current Trigger Model

Triggers are shown as compact sentence rows. The current trigger options are:

- `Every day at` `09:00`
- `Custom schedule` `Cron`
- `PR opened in` `Select repos` `by` `Anyone`
- `Draft opened in` `Select repos`
- `Comment added in` `Select repos`
- `Label changes in` `Select repos`

Added triggers can be edited through the trigger picker or removed from the row.
The trigger picker closes when users click outside the picker.

Current limitations:

- Only one trigger is represented in state at a time, even though the UI says
  `Add Trigger`.
- Trigger labels and targets are mock copy. They still need to be replaced with
  real trigger names, source apps, event types, and required inputs from the
  existing connector catalog.

## Current Tool And MCP Model

Tools are selected at the app API capability level. One app can contribute
multiple selectable items, and each capability becomes its own row.

Current apps and capabilities:

- GitHub: `Watch Pull Requests`, `Comment on Pull Request`, `Update Commit Status`
- Slack: `Read Slack Channels`, `Send to Slack`, `Reply in Slack Thread`
- Gmail: `Read Gmail Threads`, `Create Gmail Draft`, `Apply Gmail Label`
- Google Calendar: `Read Calendar Events`, `Check Availability`, `Draft RSVP`
- Linear: `Read Linear Issues`, `Create Linear Issue`, `Comment on Linear Issue`
- Notion: `Search Notion`, `Create Notion Page`, `Update Notion Page`

Capabilities with a destination or mode show an inline target chip, such as
`Send to Slack` `to` `#teams`. Target chips cycle through local options.

Selected tools can be edited or removed. The tool picker closes when users click
outside the picker.

`Memories` is always shown as a built-in context tool.

Current limitation: app names and API capability labels are mock copy. They
still need to be replaced with the real actions exposed by existing connectors
and MCP servers, including required inputs, optional targets, permission
requirements, and connection readiness states.

## Current Detail Page

Clicking a saved automation opens a full-page detail view.

Top bar:

- Breadcrumb back to `Automations`.
- `Test Run`.
- `Save`.
- Overflow menu with `Delete`.

Identity area:

- Editable automation name.
- Active toggle.
- Owner text, currently `You`.

Tabs:

- `Settings`
- `Run History`

### Settings Tab

Settings reuses the builder controls:

- Trigger row and trigger picker.
- Instructions box.
- Tool rows and tool picker.

Changes are local until the user clicks `Save`. Save updates the local card,
including title, description, status, trigger summary, selected tools, enabled
state, and icon.

### Run History Tab

Before a run, the tab shows `No runs yet`.

Clicking `Test Run` creates a local history row:

- Title: `Test run`
- Status: `Waiting for review`
- Started: `Just now`
- Duration: `-`
- Summary: `Puffer is checking the current configuration.`

The button also switches the detail page to `Run History`.

### Delete

The overflow menu opens a compact action menu. `Delete` removes the selected
local automation and returns to the home screen.

## State Boundaries

Current implementation lives in `apps/puffer-desktop/src/lib/screens/Automation.svelte`.

Important local state:

- `screenMode`: `home`, `new`, or `detail`.
- `savedAutomations`: local saved user automations.
- `selectedAutomationId`: selected detail automation.
- `automationName`, `automationPrompt`, `automationTrigger`, `selectedTools`,
  and `automationEnabled`: active draft/detail edit state.
- `activeAutomationLibraryTab`: home library tab.
- `activeAutomationDetailTab`: detail tab.
- `triggerMenuOpen`, `toolMenuOpen`, and `automationActionMenuOpen`: popup state.

No backend persistence, daemon RPC, connector execution, or real scheduling is
connected yet.

## Interaction Coverage Added

Implemented interactions:

- Open Automation from the sidebar.
- Create from the home prompt.
- Create from `new`.
- Create from a template card.
- Save a local automation.
- Cancel creation.
- Open saved automation detail.
- Rename automation in detail.
- Edit instructions in detail.
- Toggle active state in detail.
- Save detail edits.
- Add, edit, and remove trigger rows.
- Add, edit, remove, and retarget tool rows.
- Select app API capabilities inside the tool picker.
- Close trigger and tool pickers by clicking outside.
- Switch between `Settings` and `Run History`.
- Create a local test-run history item.
- Open overflow menu and delete a local automation.
- Keep automation terminology out of visible UI where this screen owns the copy.

## Interactions Not Yet Added

### Creation And Editing

- Multiple triggers in one automation.
- Replace mock trigger copy with real connector-backed trigger options,
  including source app, event name, required inputs, and configuration state.
- Replace mock tool and MCP copy with real connector actions and MCP tools,
  including capability names, required inputs, optional targets, and permission
  requirements.
- Trigger-specific configuration panels, such as repo picker, cron editor,
  contact picker, calendar picker, and label picker.
- Manual editing for trigger target chips.
- Dedicated model picker inside the builder and detail page.
- Environment details beyond the static `Use Configured Environment` row.
- Dirty state, unsaved-change warning, and save confirmation feedback.
- Keyboard support for closing popups with Escape.
- Keyboard navigation inside trigger and tool menus beyond native button focus.
- Click-outside handling for the overflow action menu.
- Search filtering for triggers.
- Empty result state for trigger search.
- More explicit distinction between adding a new tool and editing an existing
  tool when the picker is open.
- Duplicate automation action.
- Archive or pause-from-card action.

### Home And Library

- Search or filter across saved automations and templates.
- Sorting saved automations by recent update, name, status, or source.
- Status chips for saved cards beyond local text.
- Card-level quick actions.
- Template categories.
- Template detail preview before opening the builder.
- Import or paste existing automation configuration.

### Detail Page

- Real run history rows with outcome, source event, duration, and approval
  metadata.
- Run history filters.
- Run history detail drawer or timeline.
- Test run input, such as selecting a sample event or past message.
- Test run result preview with generated draft, context, and errors.
- Active toggle save behavior and pending state.
- Delete confirmation.
- Disabled state for destructive or unavailable actions.
- Owner selector or sharing metadata.
- Last saved timestamp.

### Review And Approval

- Review inbox view.
- Pending draft review detail.
- Editable proposed action or draft output.
- Approve, reject, snooze, and edit decision controls.
- Destination preview for outward actions.
- Reason capture for rejected actions.
- A clear audit trail showing who approved what and when.

### Backend And Contracts

- Durable automation storage.
- Daemon RPCs for create, update, delete, test run, run history, and enable or
  pause.
- Connector-backed trigger discovery.
- Connector-backed tool capability discovery.
- Permission and credential readiness states.
- Validation errors from backend contracts.
- Real execution scheduling.
- Real dry-run execution.
- Workspace or team policy constraints.

## Suggested Next Design Steps

1. Add dirty-state and save feedback to creation and detail pages.
2. Add trigger-specific configuration controls for GitHub repos and schedules.
3. Add a test-run preview path with sample input and generated output.
4. Design the review inbox and approval detail page.
5. Define backend contracts for saved automations, trigger configs, tool configs,
   test runs, and run history.
6. Add delete confirmation and duplicate/archive actions.

## Verification Assets

Current UI coverage is in `apps/puffer-desktop/tests/automation-ui.spec.ts`.

The test suite covers:

- Prompt-first home.
- Empty `Your automations` state.
- Template library.
- Builder layout and controls.
- Trigger and tool picker behavior.
- Capability-level tool selection.
- Saved-card creation.
- Detail page settings.
- Run history empty and test-run state.
- Overflow delete menu visibility.
- Segmented-control background contrast.
