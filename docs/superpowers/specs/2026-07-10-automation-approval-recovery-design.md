# Automation Approval And Terminal Settlement Design

Date: 2026-07-10

## Context And Decision

PR #505 introduces Automation activation, run history, and human-reviewed
connector actions. The current implementation splits one run across history and
suspension files, embeds Automation provenance inside connector input, trusts
client-editable side-effect flags, and loses run settlement after the unified
outbound path sends an action.

The first design proposed durable mid-flow continuation replay. Independent
state-machine, YAGNI, and repository-fit reviews found that this would create a
partial workflow engine without sufficient per-step checkpoint or effect
semantics. Replaying provider, AgentEnv, filesystem, shell, or agent work is not
safe merely because connector sends are gated.

This revision therefore supports human approval only for a terminal top-level
connector action. It keeps the safety mechanisms required for drafting,
delivery uncertainty, terminal settlement, and crash recovery while deferring
continuation replay.

Compatibility with the unreleased Automation review schemas is not required.
Monitor RPC aliases named by the repository's monitor guardrails remain intact.

## Goals

- Give each Automation run one durable lifecycle record.
- Make side-effect and editability policy server-owned.
- Ensure every outward connector effect is either explicitly ungated by trusted
  catalog policy or protected by human approval.
- Never report delivery failure after a sent receipt is committed.
- Recover missing drafts and incomplete terminal run settlement without
  resending.
- Represent ambiguous delivery honestly and require explicit risk
  acknowledgement before retry or abandonment.
- Expose and activate only runtime combinations Puffer can execute end to end.
- Serialize local runtime lifecycle/config mutations and protect generated
  secrets.
- Keep the implementation bounded to the existing single-daemon, local-JSON
  architecture.

## Non-Goals

- Resuming steps after a human-gated action.
- Approval gates with successors, joins, loop nesting, or agent nesting.
- AgentEnv schedule/webhook ingress into Puffer-owned boundaries.
- Side-effecting tools inside first-class Agent or legacy `puffer_agent` steps.
- Arbitrary JSON payload editing during review.
- A false exactly-once guarantee for connector delivery.
- SQLite, a durable general-purpose queue, an artifact deployment journal, or a
  new runtime-management daemon.
- Migration or compatibility parsing for the superseded Automation run,
  suspension, origin, and approval schemas.

## Superseded Contracts

This design and the new component update specs supersede:

- `specs/puffer-automation/02.md` where it permits non-terminal suspensions,
  `approved_input`, or client-controlled review policy.
- `specs/puffer-cli/249.md` where it advertises schedule/webhook/transform
  capabilities, persists continuation suspensions, or uses Automation-specific
  execute/reject mutations.
- `specs/puffer-desktop/792.md` where review detail exposes raw input or Desktop
  sends `approved_input`, `connector_action_execute`, or
  `automation_pending_action_reject`.

## Core Invariants

1. `AutomationRunStore` is the source of truth for Automation lifecycle state.
2. `OutboundStore` is the source of truth for review decisions and connector
   delivery state.
3. A generated opaque action ID is persisted in the run before draft
   materialization.
4. Internal provenance and routing metadata never enter connector input.
5. Client-authored flags may tighten review but can never weaken the trusted
   connector permission floor.
6. A sent action remains sent even if terminal run settlement fails.
7. All mutations of one action use the same per-action coordination lock.
8. An interrupted `Running` run is never automatically replayed.
9. Recovery performs local store reconciliation only; it never calls a
   connector, provider, AgentEnv, or Docker.
10. An Automation is Active only after fresh preflight and successful live
    binding enablement.

## Server-Owned Policy

### Side-Effect Classification

Automation save/compile resolves the connector, connection, and action through
the installed connector catalog. It derives the effective permission through
`outbound_gate::effective_action_permission`, including the builtin permission
floor. Product Automation does not use the standing-approval semantics of
`SendOrigin::RuleAutomation`.

`draft_only` and node-level `human_approval_required` may require review for an
otherwise ungated action. They cannot make a catalog-gated action synchronous.
The Automation-level review setting is a default for outward effects, not a
request to gate read-only actions, and `false` cannot disable mandatory review.

Unknown connectors/actions and unknown effect classifications fail closed.
First-class Agent and legacy `puffer_agent` steps may declare only connector
tools that trusted catalog policy permits without a review draft. A tool that
requires review cannot run inside an agent step and is rejected before
activation. This does not introduce a general idempotency taxonomy.

### Approval Metadata

`ConnectorActionDefinition` gains optional server-owned review metadata. The
default policy is exact approval:

```text
Exact
EditableText {
  allowed_input_fields,
  max_bytes,
  allow_empty
}
```

The metadata names an ordered, non-empty set of accepted connector aliases. At
draft creation the daemon resolves exactly one field present in the input and
stores `EditableText { input_field }` on that action. Conflicting aliases fail
closed; there is no heuristic outside the declared metadata. The daemon
validates fields against the connector schema, applies a host maximum, and
treats all other input as immutable. Connection slug, connector slug, action,
recipient, origin, trigger context, and authorization data are never editable.

The daemon validates that the selected connection belongs to the connector and
derives the stable recipient from server-recognized action input. It forwards
only declared connector input. It does not inject connector/connection slugs,
action names, trigger/root/previous outputs, or `__automation` metadata.

## Automation Run Store

`DaemonState` owns one `AutomationRunStore`. The separate
`automation_suspensions.json` file is removed, and run history is a sanitized
projection of run records.

Run IDs are UUIDs, not timestamp-derived identifiers. Persisted states are:

```text
Running
AwaitingApproval {
  action_id,
  step_id,
  action_intent,
  base_intent_hash
}
Completed { result? }
Rejected { reason }
Failed {
  error_code,
  message,
  delivery_may_have_occurred
}
```

The Automation revision and canonical spec hash are stored for audit and UI
diagnostics. No normalized spec snapshot or continuation checkpoint is stored.

`action_intent` contains the immutable connector-owned input, recipient,
approval policy, and typed origin required to recreate the draft. It is never
returned by run-history RPCs or written to logs.

The store owns a bounded in-memory snapshot behind one mutex. A mutation builds
the candidate file, atomically persists it, and only then publishes it in
memory. The file uses an explicit schema version and mode `0600`. An unknown
schema fails with an actionable reset error and is never silently mutated.

All active runs and at most 500 terminal runs are retained. Terminal results
are sanitized and capped at 64 KiB serialized; summaries/errors are capped at
4 KiB. Oversized values are replaced by a truncation marker and digest.

## Terminal Gate Execution

A run is persisted as `Running` before its first provider, AgentEnv, or
connector-related runtime call.

Compiler validation requires every mandatory-review connector action to be the
terminal top-level step. It may not have a successor or appear inside a loop,
branch/join, first-class Agent, or legacy `puffer_agent` tool call.

When execution reaches the terminal gate:

1. Generate one opaque UUID action ID.
2. Build the typed action intent and canonical base-intent hash.
3. Persist `AwaitingApproval` with the action ID and intent.
4. Call `OutboundStore::ensure_draft(action_id, intent)`.
5. Return `awaiting_approval`.

`ensure_draft` returns a matching existing action. A matching ID with different
origin, base intent, connector, action, destination, or approval policy is a
corruption error and becomes non-sendable.

If execution completes without a gate, the run becomes `Completed`; a runtime
error becomes `Failed`. There is no post-approval continuation.

## Outbound State And Idempotency

Outbound states remain explicit:

```text
DraftReady or Failed -> Sending -> Sent
                       |
                       +-> Uncertain

DraftReady or Failed -> Cancelled
Uncertain -> Sending       only with duplicate-risk acknowledgement
Uncertain -> Cancelled     only with delivery-risk acknowledgement
DraftReady or Failed -> Expired
DraftReady, Failed, or Uncertain -> Quarantined   recovery mismatch only
```

All payload, policy, connection, recipient, schema, and run-link validation
occurs before `Sending`. Once the connector executor is invoked, any returned
error is conservatively `Uncertain` unless a future connector contract can
prove that dispatch did not occur. Pre-invocation validation errors are
`Failed` and remain reviewable.

`begin_send` records the client request ID and approved-content hash. Replaying
the same request ID is idempotent only when the approved content hash matches.
Reusing a request ID with changed text fails.

The outbound action file contains message bodies and connector input and must
also be mode `0600`. Generic terminal-action retention and audit rotation are
separate repository-wide follow-ups rather than part of this PR.

## Approval And Terminal Settlement

The origin-aware outbound execute handler receives `DaemonState`.

1. Acquire the action coordination lock and re-read the action.
2. Validate version, state, typed origin, run link, connection, schema, and
   approval policy.
3. Apply optional `approved_text` only to the declared editable field.
4. Persist `Sending`, request ID, attempt ID, and approved-content hash.
5. Invoke the connector.
6. Persist `Sent` and receipt before touching the run store.
7. Release the action coordination lock.
8. Best-effort transition the matching run from `AwaitingApproval` to
   `Completed`.

After step 6, the RPC always reports delivery as sent. If step 8 fails, the
response sets `runSettlementPending: true`; it never asks the user to resend.
Same-request replay and startup reconciliation retry the local settlement.

## Rejection, Expiry, And Uncertain Abandonment

Execute and cancel use the same action lock, so approve/reject races have one
winner.

- Cancelling `DraftReady` or `Failed` first commits `Cancelled`, releases the
  action lock, then settles the run as `Rejected`. Automation-origin
  cancellation requires a non-empty reason.
- `Expired` settles the run as `Failed` with
  `automation_approval_expired` and `delivery_may_have_occurred = false`.
- Cancelling `Uncertain` requires an explicit acknowledgement that delivery may
  already have occurred. The action records that it was cancelled from
  uncertain, and the run becomes `Failed` with
  `automation_delivery_uncertain_abandoned` and
  `delivery_may_have_occurred = true`. It must never appear as an ordinary
  rejection.

If local run settlement fails after an action terminal state commits, the
action result still succeeds and startup reconciliation completes the run.

## Startup Reconciliation

Recovery is local-only and bounded. It runs after stores are loaded and before
mutation RPCs are served; it performs no remote or Docker calls.

First, scan all non-terminal outbound actions, regardless of origin. Any
`Sending` left by the previous daemon process becomes `Uncertain`.

Then scan active Automation runs:

- `Running` -> `Failed(automation_run_interrupted)`.
- `AwaitingApproval`, action missing -> `ensure_draft` from the persisted
  intent.
- matching `Sent` -> `Completed` without resending.
- matching normal `Cancelled` -> `Rejected`.
- matching `Cancelled` from `Uncertain` -> uncertain-abandoned `Failed`.
- matching `Expired` -> approval-expired `Failed`.
- `DraftReady`, `Failed`, or `Uncertain` -> remain awaiting review.
- mismatched origin/content/policy -> quarantine the action as non-sendable and
  fail the run with `automation_recovery_required`.

No explicit run-retry RPC or background continuation worker is required for
terminal settlement.

## Concurrency

The existing daemon singleton remains the cross-process ownership boundary.
`OutboundStore` retains its path-keyed store mutex because drafts are created
from multiple crate layers. Automation run state is daemon-owned.

Execute and cancel share a daemon-owned per-action keyed lock. Lock entries must
not accumulate without bound. The handler re-reads after acquiring the lock and
never holds an action coordination lock together with a run-store mutex.
Different actions remain concurrent.

A separate per-Automation lifecycle lock coordinates save/pause, activation,
trigger admission, and deletion. It prevents a new run from starting between a
delete check and binding removal.

## Runtime Catalog And Activation

The current product surface exposes Puffer connector-event triggers and
server-validated connector actions. AgentEnv schedule/webhook triggers are not
listed and are rejected at activation. Local JavaScript Transform is not listed
or activatable until the selected runtime has an authoritative executable
capability; the current local Compose stack is treated as unsupported.

Catalog generation must not start Docker. Ownership validation is pure.
Activation performs a fresh runtime-target and executable-capability preflight.
The deployment stores a non-secret runtime-target key derived from mode,
normalized endpoint, and workspace ID; capability fingerprints are deferred
until AgentEnv exposes a stable capability-generation contract.

Activation guarantees observable invariants rather than pretending to provide
a multi-system transaction:

1. Compile and preflight before enabling live ingress.
2. Prepare required helper artifacts and paused Puffer bindings.
3. Enable the binding only after helpers are ready.
4. Mark the Automation enabled last.

Failure leaves no enabled Puffer binding and keeps a visible paused/error
record. Orphan helper cleanup is best effort. An AgentEnv helper that cannot be
prepared without live ingress is unsupported rather than called "inactive."

Live trigger admission performs only local revision, spec-hash, runtime-target,
and enabled-state checks. It does not probe a remote runtime for every event.

## Local Runtime Lifecycle And Secrets

`DaemonState` owns one local-runtime lifecycle mutex. Start, ensure-ready, test,
repair, stop, and inspect use the same coordinator; no method-for-method manager
class or separate daemon is introduced.

Persistent and transient local modes remain distinct:

- When Local is the selected backend, a helper returns a candidate config. The
  handler saves it, then calls `state.replace_config`, then reports success.
- When the global selection is Cloud and Puffer uses transient Local runtime
  state, only the stored transient-local config changes; global in-memory config
  is not replaced.

If secret creation succeeds but config persistence fails, the newly created
secret is deleted best effort and the previous in-memory config remains active.
All lifecycle functions use the supplied `ConfigPaths`; they do not rediscover
paths from cwd.

The runtime root directory is mode `0700`. Secret-bearing `.env`, `seed.sql`,
and stored local config files use atomic, non-symlink-following writes with mode
`0600`. Logs and RPC responses exclude tokens, peppers, JWT secrets, seed
contents, and message bodies.

## Deletion

Deletion acquires the per-Automation lifecycle lock, stops new admission and
disables the live binding, then rechecks the run store.

- `Running` or `AwaitingApproval` blocks deletion.
- A blocked deletion leaves the Automation paused and visible.
- With no active run, bindings are removed before the Automation record.
- Deletion never implicitly sends, rejects, or abandons an uncertain action.

## RPC And Desktop Contract

The Automation review queries remain:

- `automation_pending_action_list`
- `automation_pending_action_get`

Mutations use:

- `outbound_action_execute`
- `outbound_action_cancel`

Automation-only `connector_action_execute`,
`automation_pending_action_reject`, and Desktop compatibility wrappers are
removed. `monitor_action_execute` and `task_monitor_action_execute` remain as
required Monitor aliases and continue using their existing response contract.
No new dual-case aliases are added for Automation fields.

Execute accepts `action_id`, `version`, optional `approved_text`,
`client_request_id`, and optional `duplicate_risk_ack`. Cancel accepts
`action_id`, `version`, reason, and optional `uncertain_delivery_ack`.

Review detail returns the server-selected approval policy, editable text, and
sanitized display/destination fields. It never returns raw connector input.

Desktop behavior is:

- `EditableText` shows one editor; `Exact` shows read-only sanitized fields.
- The builder permits at most one mandatory-review action and places it last;
  server validation remains authoritative.
- A sent action leaves the inbox immediately.
- `runSettlementPending` may show a transient "Sent; updating run history"
  notice, but no resend or continuation retry control.
- `Uncertain` requires duplicate-risk acknowledgement before retry and
  delivery-risk acknowledgement before abandonment.
- Expired and uncertain-abandoned runs display distinct, truthful outcomes.
- Active is shown only after final activation success.
- Snooze remains a local-only UI action.

## Stable Errors

- `outbound_action_version_mismatch`
- `outbound_action_terminal`
- `outbound_action_origin_mismatch`
- `outbound_action_edit_not_allowed`
- `outbound_action_request_content_mismatch`
- `outbound_delivery_uncertain`
- `outbound_uncertain_ack_required`
- `automation_run_state_conflict`
- `automation_run_interrupted`
- `automation_approval_expired`
- `automation_delivery_uncertain_abandoned`
- `automation_runtime_capability_missing`
- `automation_active_runs_prevent_delete`
- `automation_store_version_unsupported`
- `automation_recovery_required`

## Test Strategy

Implementation follows TDD. Required coverage includes:

- Server-owned permission floors cannot be weakened by Automation flags.
- Unknown or mandatory-review Agent/`puffer_agent` tools fail closed.
- Mandatory-review actions are rejected unless terminal and top-level.
- Run UUIDs, legal transitions, interrupted-Running recovery, retention,
  `0600` permissions, concurrent updates, and incompatible schema handling.
- Typed origin cannot be spoofed through input and is never forwarded.
- Exact/editable policy, host/schema limits, empty-text rules, immutable
  destination, and connection-to-connector validation.
- Same request/content replay succeeds without duplicate execution; changed
  content with the same request ID fails.
- Sent-before-run-settlement recovery, missing-draft recreation, normal reject,
  expiry, stale `Sending -> Uncertain`, uncertain retry, and uncertain
  abandonment.
- Approve/cancel races and parallel execution of different actions.
- Activation rejection for schedule/webhook and unsupported transform, plus
  enabled-last behavior and runtime-target mismatch.
- Trigger-admission versus deletion race.
- Local lifecycle serialization, persistent versus transient config behavior,
  config replacement, orphan-secret cleanup, supplied paths, and secure modes.
- Restored Desktop Playwright coverage for editable approval, exact approval,
  normal rejection, uncertain acknowledgement, delayed terminal settlement,
  expiry, hidden unsupported catalog entries, and active-run deletion errors.

## Deferred Follow-Ups

After the terminal-only launch is stable, separate issues may cover:

- Durable mid-flow continuation with per-step effect/idempotency contracts.
- AgentEnv schedule/webhook-to-Puffer ingress.
- A supported local AgentEnv executor sandbox and executable-capability API.
- General outbound terminal retention and audit-log rotation.
- Richer cross-origin review UI and operational recovery dashboards.
