# Automation Approval And Recovery Design

Date: 2026-07-10

## Context

PR #505 introduces Automation activation, run history, human-reviewed connector
actions, and local/AgentEnv runtime integration. The current implementation
splits one Automation run across run-history and suspension files, embeds
Automation provenance inside connector input, and does not settle or resume a
run after the unified outbound action path sends its draft. It also advertises
runtime combinations that cannot execute end to end.

This design replaces those boundaries rather than preserving the unreleased
schema and RPC compatibility. The priorities are long-term correctness,
recoverability, bounded performance, and a small implementation surface.

## Goals

- Make each Automation run have one durable source of truth.
- Preserve the human gate for every connector operation with an outward side
  effect.
- Never report that delivery failed after a sent receipt has been committed.
- Recover safely from crashes at every boundary between run state, draft state,
  connector delivery, and continuation execution.
- Prevent action provenance, routing metadata, and arbitrary edited payloads
  from crossing the connector boundary.
- Expose and activate only runtime combinations that Puffer can execute end to
  end.
- Serialize local runtime lifecycle mutations and keep in-memory config in sync
  with persisted config.
- Keep storage and recovery bounded without adding a database, event bus, or
  general job framework.

## Non-Goals

- Bridging AgentEnv schedule or webhook triggers back into Puffer-owned agent or
  connector boundaries.
- Human-gated side effects inside loop bodies or inside a first-class agent
  step.
- Arbitrary JSON payload editing during approval.
- A false exactly-once guarantee for external delivery.
- SQLite, a durable general-purpose queue, or a new runtime management daemon.
- Migration or compatibility parsing for the unreleased run, suspension,
  outbound, or RPC schemas replaced by this change.

## Core Invariants

1. `AutomationRunStore` is the source of truth for Automation execution state.
2. `OutboundStore` is the source of truth for human decisions and connector
   delivery state.
3. Internal origin and routing data never appear in connector input.
4. A run resumes from the immutable definition snapshot captured for that run,
   not from the current editable Automation record.
5. A sent action is never made unsent by a later Automation failure.
6. All mutations of one outbound action use the same per-action coordination
   lock.
7. All continuation execution for one run uses the same per-run lock.
8. File locks are held only for short read-modify-write operations, never for
   connector, provider, Docker, or AgentEnv calls.
9. Unsupported runtime ownership combinations fail closed before deployment.
10. An Automation is Active only after every required artifact and binding is
    deployed and enabled.

## Domain Model

### Automation Run Aggregate

The separate suspension file is removed. The run store contains bounded run
records, and run history is a projection of those records.

```text
AutomationRunRecord
  id
  automation_id
  version
  definition
    automation_revision
    spec_hash
    normalized_spec?       # retained only while non-terminal
  state
  started_at_ms
  updated_at_ms
  summary
  result?
```

The persisted state is a tagged Rust enum:

```text
Running
AwaitingApproval {
  action_intent,
  continuation_checkpoint?
}
ResumePending {
  action_id,
  receipt,
  continuation_checkpoint?,
  attempts,
  last_error?
}
Completed
Rejected { reason }
Failed { error_code, message }
```

There is no durable `Resuming` state. A per-run process lock represents an
in-flight continuation. The durable `ResumePending` state remains recoverable
if the process exits before, during, or after the continuation call.

`ContinuationCheckpoint` contains the next top-level step index, trigger input,
root output, and previous output. Terminal gated actions have no checkpoint.

The run captures a normalized Automation spec, revision, and spec hash before
execution. Resume compiles from that snapshot. Credentials and secret values
are resolved at execution time and are never copied into the snapshot. Terminal
runs discard the normalized spec and checkpoint while retaining revision, hash,
result, and audit summary.

### Pending Action Intent

When execution reaches a gate, it builds a typed action intent containing the
connector, connection, action, connector-owned input, recipient identity,
display information, and approval policy. The action ID is derived from
`run_id`, `step_id`, and a stable gate sequence.

The run is first atomically changed to `AwaitingApproval`, including this
intent and any checkpoint. `OutboundStore::ensure_draft` then materializes the
action idempotently. A matching existing action is returned. An existing action
with different content or origin is treated as corruption and fails closed.

This ordering lets startup recovery recreate a missing draft without placing
continuation state inside the outbound record.

### Typed Outbound Origin

The generic optional origin fields and `input.__automation` convention are
replaced by a tagged enum:

```rust
enum OutboundOrigin {
    Session(SessionOrigin),
    Monitor(MonitorOrigin),
    Automation(AutomationActionOrigin),
}
```

`AutomationActionOrigin` contains `automation_id`, `run_id`, `step_id`, and
`spec_hash`. Constructors require the appropriate typed origin. Queue filtering
and run settlement never infer origin from connector input or user-controlled
metadata.

### Approval Policy

Every outbound action stores one server-selected policy:

```text
Exact
EditableText { input_field }
```

The execute RPC accepts only optional `approved_text`. `Exact` rejects any
edit. `EditableText` maps the text to the stored field and leaves every other
input field unchanged. The server never falls back to inventing a `message`
field. Review detail responses contain sanitized display fields and editable
text, not the raw connector input.

Connector slug, connection slug, action name, origin, and authorization data
remain separate from connector-owned input. Connector execution receives them
through typed arguments rather than injected payload keys.

## State Transitions

### Outbound Action

```text
DraftReady or Failed -> Sending -> Sent
                       |
                       +-> Failed       only when no-send is definitive
                       +-> Uncertain    when delivery cannot be determined

DraftReady, Failed, or Uncertain -> Cancelled
```

Retrying `Uncertain` requires explicit duplicate-risk acknowledgement.
Connector execution returns a typed delivery outcome so pre-dispatch failures
can be distinguished from ambiguous transport or timeout failures.

### Automation Run

```text
Running -> Completed | Failed | AwaitingApproval
AwaitingApproval + sent action -> ResumePending
AwaitingApproval + cancelled action -> Rejected
ResumePending -> Completed | AwaitingApproval | ResumePending with error
```

Illegal transitions return stable error codes and do not mutate either store.

## Approval And Settlement Flow

The unified outbound execute handler receives `DaemonState` and uses one
origin-aware service.

1. Acquire the action coordination lock.
2. Load and validate action ID, version, status, origin, and approval policy.
3. For Automation origin, confirm the run is waiting for the same action and
   spec hash.
4. Apply the optional approved text to the daemon-selected input field.
5. Persist `Sending`, request ID, attempt ID, approved text, and approved
   content hash.
6. Execute the connector.
7. Persist `Sent` and the receipt before any origin settlement.
8. Move the Automation run from `AwaitingApproval` to `ResumePending`.
9. Release the action lock.
10. Under the run lock, settle `ResumePending` using the pinned definition and
    checkpoint.

After step 7 the RPC response must report delivery as sent even if steps 8-10
fail. It may report the run as `resume_pending`, allowing the UI to distinguish
delivery success from continuation progress.

An idempotent replay with the same client request ID returns the existing sent
action and still invokes origin settlement. It must not return early before
settlement.

## Rejection And Cancellation

Execute, Automation reject, and generic cancel use the same action lock.

1. Persist the action as `Cancelled`.
2. Settle an Automation-origin cancellation by moving the matching run to
   `Rejected` with the review reason.
3. If run settlement fails after cancellation commits, return cancellation
   success with recovery pending and let reconciliation finish the run.

The first terminal action state wins an approve/reject race. The losing request
receives a state-conflict error. A run is never marked rejected before the
action has become non-sendable.

## Crash Recovery

Recovery runs once during daemon startup and can also be invoked explicitly for
a run. It scans only non-terminal runs.

- Any outbound action left in `Sending` by the previous daemon process becomes
  `Uncertain`; recovery never guesses whether that connector call delivered.
- `AwaitingApproval`, missing action: call `ensure_draft` from the stored intent.
- `AwaitingApproval`, matching sent action: move to `ResumePending` and settle.
- `AwaitingApproval`, matching cancelled action: move to `Rejected`.
- `AwaitingApproval`, mismatched origin/content: mark recovery required and do
  not send.
- `ResumePending`: verify the linked action is sent and execute the pinned
  continuation.

The normal send path attempts settlement immediately. There is no polling loop.
If immediate and startup recovery both fail, the run remains visibly
`ResumePending` with a stable error code and supports explicit retry.

Continuation execution has at-least-once semantics. This is safe only because
all outward connector effects are gated. Until a durable mid-agent checkpoint
exists, first-class agent steps may use only read-only or idempotent tools.
Side-effecting agent tools and loop-body approval gates are rejected at save or
activation time.

## Concurrency And Storage

`AutomationRunStore` and `OutboundStore` each use a process-wide lock keyed by
canonical path. Mutations re-read under the lock and atomically replace their
file. Temporary filenames must be unique per write rather than a shared
`.tmp` path.

The daemon singleton remains the cross-process ownership boundary. Per-action
and per-run lock registries use weak references or cleanup so completed IDs do
not leak memory. Different actions and different runs may progress concurrently.

No store mutex is held across network or Docker calls, and code does not hold
both store mutexes simultaneously. Cross-file consistency comes from durable
states and reconciliation rather than nested locks or an attempted two-file
transaction.

The run store keeps all active runs and at most 500 terminal records. Active
runs are never removed by history retention. Terminal records do not retain
large definition snapshots or checkpoints.

The replacement stores use explicit schema versions. An unexpected version
fails with an actionable reset error; the daemon neither mutates nor silently
deletes an incompatible file.

## Runtime Compatibility And Activation

Compilation produces an execution plan that identifies trigger and step
ownership plus required runtime capabilities. One server-side compatibility
analyzer is reused by save diagnostics, preview, activation, and catalog
generation.

Supported ownership combinations are:

| Trigger owner | Flow ownership | Result |
| --- | --- | --- |
| Puffer connector event | Mixed Puffer and AgentEnv | Supported |
| AgentEnv schedule/webhook | AgentEnv only | Structurally supported |
| AgentEnv schedule/webhook | Any Puffer boundary | Rejected |

The current Desktop catalog does not expose schedule or webhook triggers.
There is no cross-runtime ingress bridge in this change.

The catalog publishes an AgentEnv node only when the selected runtime reports
and verifies the required execution capability. Listing a node definition is
not sufficient. The current local Compose stack does not provide the executor
sandbox required by `transform_js`, so the catalog hides it and activation of
an existing transform plan fails with a capability error.

Activation is phased:

1. Pure compile and validation.
2. Fresh runtime, auth, connector, ownership, and capability preflight.
3. Deploy inactive AgentEnv artifacts.
4. Deploy disabled Puffer bindings.
5. Enable the bindings.
6. Mark the Automation enabled.

Failure leaves the Automation paused with a runtime error and no enabled Puffer
binding. Newly created intermediate artifacts are cleaned up best effort. The
runtime state records compiled revision, spec hash, backend identity, and
capability fingerprint. A backend identity change makes the deployment stale
and requires reactivation.

Live trigger handling performs only cheap local identity, revision, and hash
checks. It does not run a remote health probe for every event.

## Local Runtime Lifecycle

`DaemonState` owns one `LocalRuntimeManager` with a lifecycle mutex. Start,
ensure-ready, test, repair, stop, and inspect all use this manager so Docker,
port allocation, bootstrap files, and runtime config cannot be mutated
concurrently.

Lifecycle helpers return status plus an updated config and do not silently save
user config. The RPC handler persists the config and then calls
`state.replace_config` before returning. A persistence failure leaves the
in-memory config unchanged. Config or lifecycle changes invalidate cached
capabilities and the backend identity used by deployments.

The runtime directory is mode `0700`. Secret-bearing `.env`, `seed.sql`, and
stored local config files are written atomically with mode `0600`, without
following symlinks. Logs and RPC responses never include generated secrets or
file contents.

## Deletion

Automation deletion receives `DaemonState` and checks the run store. Any
`Running`, `AwaitingApproval`, or `ResumePending` run blocks deletion. The user
must first resolve the run; deletion does not perform an implicit cross-store
cascade.

When no active run exists, live bindings are removed before the Automation
record. A binding-removal failure keeps the record. Terminal outbound actions
remain as audit history but are excluded from the review inbox.

## RPC And Desktop Contract

The canonical mutation RPCs are `outbound_action_execute` and
`outbound_action_cancel`. Automation compatibility wrappers and legacy aliases
are removed.

Execute parameters are `action_id`, `version`, optional `approved_text`,
`client_request_id`, and optional `duplicate_risk_ack`. Responses report the
action delivery result separately from optional Automation run settlement.

Stable subsystem error codes include:

- `outbound_action_version_mismatch`
- `outbound_action_terminal`
- `outbound_action_origin_mismatch`
- `outbound_action_edit_not_allowed`
- `outbound_delivery_uncertain`
- `automation_run_state_conflict`
- `automation_runtime_capability_missing`
- `automation_active_runs_prevent_delete`
- `automation_recovery_required`

The Desktop review UI renders the server approval policy. A sent action leaves
the inbox even when its run remains `ResumePending`; history displays
Continuing or Needs attention and provides an explicit retry. Uncertain actions
require a visible duplicate-risk acknowledgement. Active is shown only from a
successful final activation response.

## Test Strategy

Implementation follows TDD. Required coverage includes:

- Legal and illegal run/action state transitions.
- Concurrent run updates without lost records or temporary-file collisions.
- Typed origin validation and proof that internal metadata is not forwarded.
- Exact versus editable-text approval behavior and destination pinning.
- Terminal approval, mid-flow resume, next-gate suspension, and pinned-spec
  resume after the editable Automation changes.
- Sent-before-settlement, missing-draft, cancelled-before-run-settlement, and
  startup `ResumePending` recovery.
- Same-request replay without a duplicate connector call.
- Approve/reject races and per-action parallelism.
- Capability matrix, hidden unsupported catalog entries, and activation
  failure without an executor sandbox.
- Active-run deletion rejection.
- Local runtime lifecycle serialization, config replacement, cache invalidation,
  and Unix file modes.
- Restored Desktop review Playwright coverage for editable approval, exact
  approval, rejection, uncertain delivery, and resume-pending feedback.

Focused crate and Desktop tests run before workspace formatting, clippy, and
CI-equivalent gates.
