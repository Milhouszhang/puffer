# Automation Terminal Approval Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make PR #505 safe to launch with durable, server-authorized, terminal-only Automation approvals, truthful delivery uncertainty, local crash reconciliation, and honest runtime activation.

**Architecture:** `AutomationRunStore` owns bounded Automation lifecycle records, while the existing `OutboundStore` owns review and connector delivery. Mandatory-review connector actions are accepted only as the terminal top-level step; approval sends once and then performs local terminal settlement without a continuation engine. Server-owned connector metadata controls side effects and editable text, and daemon-owned coordination locks serialize action and Automation lifecycle mutations.

**Tech Stack:** Rust 2021, Serde/serde_json, UUID, SHA-256, daemon JSON-RPC, Svelte 5, TypeScript, Vitest, Playwright.

## Global Constraints

- Do not preserve the unreleased Automation run/suspension/approval schema.
- Retain `monitor_action_execute` and `task_monitor_action_execute` because the Monitor runtime contract names them.
- Do not add SQLite, a general durable queue, continuation replay, capability fingerprints, or a new runtime manager daemon.
- Every mandatory-review connector action must be the terminal top-level step.
- Client node flags may tighten review but never weaken `effective_action_permission`.
- Never place origin, routing, trigger context, or authorization metadata in connector input.
- Once `Sent` is committed, no later error may be reported as delivery failure.
- Treat every connector error after invocation begins as `Uncertain` unless a future connector contract proves no dispatch.
- Add no third-party dependencies. Use ASCII. Follow TDD and Conventional Commits.
- Keep edits in existing large files narrow; run `scripts/report-large-files.sh` rather than performing unrelated splits.

## File Structure

**Create:**

- `crates/puffer-automation/src/run.rs` - run states and persisted action intent.
- `crates/puffer-automation/src/run_store.rs` - bounded, private, atomic run storage.
- `crates/puffer-cli/src/automation_action_policy.rs` - installed-catalog policy resolution.
- `crates/puffer-cli/src/daemon_automation_runs.rs` - history projection and local reconciliation.
- `crates/puffer-cli/src/daemon_coordination.rs` - reusable keyed in-process locks.

**Modify narrowly:**

- `crates/puffer-subscriptions/src/{catalog.rs,outbound_gate.rs,outbound_store.rs,lib.rs}`
- `crates/puffer-automation/src/{lib.rs,spec.rs,validation.rs}`
- `crates/puffer-core/runtime/claude_tools/workflow/connector_tools.rs`
- `crates/puffer-cli/src/{main.rs,daemon.rs,daemon_automations.rs,daemon_automation_runtime.rs}`
- `crates/puffer-cli/src/{daemon_workflows.rs,daemon_workflows/outbound_action.rs}`
- `crates/puffer-cli/src/{daemon_workflow_runtime.rs,workflow_local_runtime.rs,workflow_local_runtime_bootstrap.rs,workflow_local_runtime_tests.rs}`
- `crates/puffer-cli/tests/automation_real_e2e.rs`
- `apps/puffer-desktop/src/lib/{api/desktop.ts,api/desktop.workflow-daemon.test.ts,types.ts}`
- `apps/puffer-desktop/src/lib/screens/Automation.svelte`
- `apps/puffer-desktop/src/lib/screens/agent/{ToolCard.svelte,connectorDraftStatus.ts,connectorDraftStatus.test.ts}`
- `apps/puffer-desktop/tests/{support/fakeDaemon.ts,automation-ui.spec.ts,outbound-gate-matrix.spec.ts}`

Do not split `Automation.svelte` or convert `outbound_action.rs` into a directory in this change.

---

### Task 1: Make Review Policy Server-Owned And Enforce Terminal Gates

**Files:**

- Modify: `crates/puffer-subscriptions/src/catalog.rs`
- Modify: `crates/puffer-subscriptions/src/outbound_gate.rs`
- Modify: `crates/puffer-subscriptions/src/lib.rs`
- Modify: `crates/puffer-automation/src/validation.rs`
- Modify: `crates/puffer-automation/src/lib.rs`
- Create: `crates/puffer-cli/src/automation_action_policy.rs`
- Modify: `crates/puffer-cli/src/main.rs`
- Modify: `crates/puffer-cli/src/daemon_automations.rs`
- Modify: `crates/puffer-cli/src/daemon_automation_runtime.rs`

**Interfaces:**

- Produces `ConnectorActionReviewDefinition`, `ConnectorEditableTextDefinition`, and resolved `OutboundApprovalPolicy`.
- Produces `ResolvedAutomationActionPolicy` and `resolve_automation_action_policy(...)`.
- Produces `validate_terminal_review_topology(...)` for save and activation.

- [ ] **Step 1: Add failing connector metadata tests**

```rust
#[test]
fn send_message_declares_editable_text_aliases() {
    let template = builtin_connector_template("telegram-login").unwrap();
    let action = template.actions.get("send_message").unwrap();
    let editable = action.review.editable_text.as_ref().unwrap();
    assert_eq!(editable.allowed_input_fields, ["message", "text", "caption"]);
    assert!(!editable.allow_empty);
    assert!(editable.max_bytes <= MAX_APPROVED_TEXT_BYTES);
}

#[test]
fn read_action_defaults_to_exact_review() {
    let template = builtin_connector_template("slack-browser").unwrap();
    assert!(template.actions["read_history"].review.editable_text.is_none());
}
```

- [ ] **Step 2: Run the catalog tests and confirm RED**

```bash
cargo test -p puffer-subscriptions catalog -- --nocapture
```

Expected: compilation fails because the review metadata does not exist.

- [ ] **Step 3: Implement connector review metadata**

```rust
pub const MAX_APPROVED_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectorActionReviewDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editable_text: Option<ConnectorEditableTextDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectorEditableTextDefinition {
    pub allowed_input_fields: Vec<String>,
    pub max_bytes: usize,
    #[serde(default)]
    pub allow_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutboundApprovalPolicy {
    Exact,
    EditableText { input_field: String, max_bytes: usize, allow_empty: bool },
}
```

Add `review` to `ConnectorActionDefinition`, defaulting to exact. Declare aliases only for editable send/draft actions. At minimum cover generic/browser `send_message` and email `draft_reply`, `draft_forward`, and `send_email`; leave mark/read/delete/reaction/RSVP-style actions exact.

- [ ] **Step 4: Add failing policy-floor and topology tests**

```rust
#[test]
fn client_false_cannot_ungate_builtin_send() {
    let mut spec = connector_action_spec("telegram-login", "send_message");
    set_node_bool(&mut spec, "human_approval_required", false);
    let policy = resolve_only_action_policy(&spec).unwrap();
    assert!(policy.requires_review);
}

#[test]
fn mandatory_review_action_with_successor_is_rejected() {
    let spec = flow_with_steps(vec![
        connector_step("send", "telegram-login", "send_message"),
        agentenv_step("after", "noop"),
    ]);
    let error = validate_terminal_review_topology(&spec, &builtin_classifier).unwrap_err();
    assert!(error.contains("terminal top-level"));
}

#[test]
fn mandatory_review_agent_tool_is_rejected() {
    let spec = agent_with_tool("telegram-login", "send_message");
    let error = validate_terminal_review_topology(&spec, &builtin_classifier).unwrap_err();
    assert!(error.contains("agent tool"));
}
```

- [ ] **Step 5: Run the topology tests and confirm RED**

```bash
cargo test -p puffer-automation terminal_review -- --nocapture
cargo test -p puffer-cli automation_action_policy -- --nocapture
```

Expected: tests fail because installed-catalog resolution and terminal topology validation are absent.

- [ ] **Step 6: Implement installed-catalog policy resolution**

Create:

```rust
pub(crate) struct ResolvedAutomationActionPolicy {
    pub connector_slug: String,
    pub connection_slug: String,
    pub action_slug: String,
    pub requires_review: bool,
    pub review: ConnectorActionReviewDefinition,
}

pub(crate) fn resolve_automation_action_policy(
    paths: &ConfigPaths,
    node: &AgentEnvNodeRef,
) -> Result<ResolvedAutomationActionPolicy>;
```

Resolve the connection through the subscription manager, require connector equality, load installed template with builtin fallback, and call `effective_action_permission`. Product Automation must not use `SendOrigin::RuleAutomation`.

Compute review as:

```rust
let requires_review = action_requires_human_review(...)
    || node.config_bool("draft_only")
    || node.config_bool("human_approval_required");
```

- [ ] **Step 7: Implement and wire terminal topology validation**

```rust
pub fn validate_terminal_review_topology(
    spec: &AutomationSpec,
    requires_review: &impl Fn(&AgentEnvNodeRef) -> Result<bool, String>,
) -> Result<(), String>;
```

Reject reviewed steps unless they are the final top-level step. Reject reviewed tools inside both first-class Agent and legacy `puffer_agent`. Wire the check into `automation_save` and compile/deploy.

Change `handle_automation_save` to receive `&DaemonState` so it can use `config_paths()`, the installed catalog, the Automation store, and later lifecycle coordination. Update the daemon dispatch in the same step.

- [ ] **Step 8: Run focused tests and commit**

```bash
cargo test -p puffer-subscriptions catalog
cargo test -p puffer-subscriptions outbound_gate
cargo test -p puffer-automation
cargo test -p puffer-cli daemon_automations
```

Expected: all pass.

```bash
git add crates/puffer-subscriptions crates/puffer-automation crates/puffer-cli/src/automation_action_policy.rs crates/puffer-cli/src/main.rs crates/puffer-cli/src/daemon_automations.rs crates/puffer-cli/src/daemon_automation_runtime.rs
git commit -m "fix(automation): enforce trusted terminal review policy"
```

---

### Task 2: Add The Durable Automation Run Aggregate

**Files:**

- Create: `crates/puffer-automation/src/run.rs`
- Create: `crates/puffer-automation/src/run_store.rs`
- Modify: `crates/puffer-automation/src/lib.rs`
- Modify: `crates/puffer-cli/src/daemon.rs`

**Interfaces:**

- Produces `AutomationRunRecord`, `AutomationRunState`, `AutomationActionIntent`, and `AutomationRunStore`.
- Produces `DaemonState::automation_run_store()`.
- `AutomationActionIntent` stores the resolved `OutboundApprovalPolicy` from Task 1.

- [ ] **Step 1: Write failing run-store transition tests**

```rust
#[test]
fn run_transitions_running_to_awaiting_to_completed() {
    let store = store();
    let run = store.start(new_run("automation-1")).unwrap();
    assert!(Uuid::parse_str(&run.id).is_ok());
    let waiting = store.await_approval(
        &run.id,
        "action-1".into(),
        "send".into(),
        intent(),
    ).unwrap();
    assert!(matches!(waiting.state, AutomationRunState::AwaitingApproval { .. }));
    let completed = store.complete_from_action(&run.id, "action-1", json!({"ok": true})).unwrap();
    assert!(matches!(completed.state, AutomationRunState::Completed { .. }));
}

#[test]
fn startup_fails_interrupted_running_runs() {
    let store = store();
    let run = store.start(new_run("automation-1")).unwrap();
    store.fail_interrupted_runs().unwrap();
    assert_eq!(store.get(&run.id).unwrap().error_code(), Some("automation_run_interrupted"));
}
```

- [ ] **Step 2: Run the tests and confirm RED**

```bash
cargo test -p puffer-automation run_store -- --nocapture
```

Expected: compilation fails because the run model/store do not exist.

- [ ] **Step 3: Implement the run model**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AutomationRunState {
    Running,
    AwaitingApproval {
        action_id: String,
        step_id: String,
        action_intent: AutomationActionIntent,
        base_intent_hash: String,
    },
    Completed { result: Option<Value> },
    Rejected { reason: String },
    Failed {
        error_code: String,
        message: String,
        delivery_may_have_occurred: bool,
    },
}

pub struct NewAutomationRun {
    pub automation_id: String,
    pub automation_revision: u64,
    pub spec_hash: String,
    pub title: String,
    pub source_event: String,
}
```

Store run/Automation IDs, title/source, revision, spec hash, timestamps, summary, and state. Do not add continuation data.

- [ ] **Step 4: Implement bounded private storage**

```rust
pub struct AutomationRunStore {
    path: PathBuf,
    inner: Mutex<AutomationRunFile>,
}

impl AutomationRunStore {
    pub fn load(path: PathBuf) -> Result<Self, AutomationRunStoreError>;
    pub fn get(&self, run_id: &str) -> Result<AutomationRunRecord, AutomationRunStoreError>;
    pub fn start(&self, input: NewAutomationRun) -> Result<AutomationRunRecord, AutomationRunStoreError>;
    pub fn await_approval(&self, run_id: &str, action_id: String, step_id: String, intent: AutomationActionIntent) -> Result<AutomationRunRecord, AutomationRunStoreError>;
    pub fn complete(&self, run_id: &str, result: Value) -> Result<AutomationRunRecord, AutomationRunStoreError>;
    pub fn fail(&self, run_id: &str, code: &str, message: &str) -> Result<AutomationRunRecord, AutomationRunStoreError>;
    pub fn complete_from_action(&self, run_id: &str, action_id: &str, result: Value) -> Result<AutomationRunRecord, AutomationRunStoreError>;
    pub fn reject_from_action(&self, run_id: &str, action_id: &str, reason: &str) -> Result<AutomationRunRecord, AutomationRunStoreError>;
    pub fn fail_from_action(&self, run_id: &str, action_id: &str, code: &str, message: &str, possible_delivery: bool) -> Result<AutomationRunRecord, AutomationRunStoreError>;
    pub fn fail_interrupted_runs(&self) -> Result<usize, AutomationRunStoreError>;
    pub fn active_for_automation(&self, automation_id: &str) -> Vec<AutomationRunRecord>;
    pub fn list_for_automation(&self, automation_id: &str) -> Vec<AutomationRunRecord>;
}
```

Persist the candidate before swapping memory. Keep all active plus 500 terminal runs. Cap result JSON at 64 KiB and text at 4 KiB. Use file mode `0600` on Unix and reject unknown schema versions.

Use a typed `AutomationRunStoreError` with stable `code()` values for unsupported schema, not found, state conflict, action mismatch, serialization, and I/O. Do not parse error strings in callers.

- [ ] **Step 5: Add retention, permission, and concurrency tests**

Create 501 terminal runs plus one active run and assert the active run remains. Spawn multiple threads against one store and assert no lost updates. Under `#[cfg(unix)]`, inspect mode `0600`.

- [ ] **Step 6: Wire the store into `DaemonState`**

```rust
automation_runs: Arc<AutomationRunStore>,

pub(crate) fn automation_run_store(&self) -> &AutomationRunStore {
    self.automation_runs.as_ref()
}
```

Load `user_config_dir/automation_runs.json`. Task 4 will migrate runtime behavior.

- [ ] **Step 7: Run tests and commit**

```bash
cargo test -p puffer-automation run_store
cargo test -p puffer-cli daemon_state
```

Expected: all pass.

```bash
git add crates/puffer-automation/src/run.rs crates/puffer-automation/src/run_store.rs crates/puffer-automation/src/lib.rs crates/puffer-cli/src/daemon.rs
git commit -m "feat(automation): add durable run aggregate"
```

---

### Task 3: Type And Harden The Outbound Action Store

**Files:**

- Modify: `crates/puffer-subscriptions/src/outbound_store.rs`
- Modify: `crates/puffer-subscriptions/src/lib.rs`
- Modify: `crates/puffer-core/runtime/claude_tools/workflow/connector_tools.rs`
- Modify: `crates/puffer-cli/src/daemon_workflows.rs`
- Modify: `crates/puffer-cli/src/daemon_workflows/outbound_action.rs`
- Modify: `crates/puffer-cli/src/daemon_workflows/monitor_snapshot_tests.rs`
- Modify: `crates/puffer-cli/src/daemon_workflows/task_snapshot.rs`

**Interfaces:**

- Produces typed `OutboundOrigin` and `OutboundActionStatus`; consumes `OutboundApprovalPolicy` from Task 1.
- Produces `ensure_draft`, content-checked `begin_send`, `mark_send_uncertain`, and `recover_stale_sending`.

- [ ] **Step 1: Write failing typed-origin and policy tests**

```rust
#[test]
fn automation_origin_is_not_part_of_connector_input() {
    let action = store().ensure_draft("oa-fixed", automation_draft()).unwrap();
    assert!(matches!(action.origin, OutboundOrigin::Automation { .. }));
    assert!(action.input.get("__automation").is_none());
}

#[test]
fn exact_action_rejects_approved_text() {
    let action = store().ensure_draft("oa-fixed", exact_draft()).unwrap();
    let error = store().begin_send(&action.id, action.version, Some("edit"), "req-1", false).unwrap_err();
    assert_eq!(error.code(), "outbound_action_edit_not_allowed");
}

#[test]
fn editable_action_changes_only_the_resolved_field() {
    let action = store().ensure_draft("oa-fixed", editable_draft("body")).unwrap();
    let started = store().begin_send(&action.id, action.version, Some("edited"), "req-1", false).unwrap();
    assert_eq!(started.input["body"], "edited");
    assert_eq!(started.input["to"], "alice");
}
```

- [ ] **Step 2: Run tests and confirm RED**

```bash
cargo test -p puffer-subscriptions outbound_store -- --nocapture
```

Expected: compilation fails because typed origin/status/policy and new methods do not exist.

- [ ] **Step 3: Replace string states and optional origin fields**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutboundOrigin {
    Session { session_id: String, turn_id: Option<String> },
    Monitor { session_id: String, turn_id: Option<String>, task_id: String },
    Automation { automation_id: String, run_id: String, step_id: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutboundActionStatus {
    DraftReady,
    Sending,
    Sent,
    Failed,
    Uncertain,
    Cancelled,
    Expired,
    Quarantined,
}

```

Add origin helper methods `task_id()`, `automation_id()`, `run_id()`, and `step_id()` and update all current constructors.

- [ ] **Step 4: Implement idempotent draft materialization and content checks**

```rust
pub fn ensure_draft(&self, action_id: &str, draft: NewOutboundDraft) -> Result<OutboundAction>;

pub struct BeginSendOutcome {
    pub action: OutboundAction,
    pub input: Value,
    pub already_sent: bool,
}
```

`ensure_draft` compares origin, connector, connection, action, recipient, base content hash, and policy. `begin_send` applies only the stored editable field, enforces text rules, records approved-content hash, and accepts same-request replay only for the same hash.

- [ ] **Step 5: Add failing uncertainty and replay tests**

```rust
#[test]
fn stale_sending_becomes_uncertain() {
    let action = sending_action();
    assert_eq!(store().recover_stale_sending().unwrap(), 1);
    assert_eq!(store().get(&action.id).unwrap().unwrap().status, OutboundActionStatus::Uncertain);
}

#[test]
fn same_request_with_changed_content_is_rejected() {
    let sent = sent_editable_action("req-1", "first");
    let error = store().begin_send(&sent.id, sent.version, Some("changed"), "req-1", false).unwrap_err();
    assert_eq!(error.code(), "outbound_action_request_content_mismatch");
}

#[test]
fn uncertain_cancel_requires_delivery_ack() {
    let action = uncertain_action();
    let error = store().cancel(&action.id, action.version, Some("stop"), false).unwrap_err();
    assert_eq!(error.code(), "outbound_uncertain_ack_required");
}
```

- [ ] **Step 6: Implement uncertainty, expiry, private writes, and cancel provenance**

Add `mark_send_uncertain`, `recover_stale_sending`, `cancel(..., uncertain_delivery_ack)`, and a recovery-only `quarantine(...)`. Record the pre-cancel state. Preserve TTL expiry through `OutboundActionStatus::Expired`. Quarantined actions are terminal and never executable. Use file mode `0600` on Unix.

Extend the existing outbound error type with stable `code()` values for version mismatch, terminal state, edit not allowed, request-content mismatch, duplicate-risk acknowledgement, uncertain-cancel acknowledgement, attempt mismatch, and unsupported schema. Callers must branch on codes/variants, never substring matches.

- [ ] **Step 7: Run affected tests and commit**

```bash
cargo test -p puffer-subscriptions outbound_store
cargo test -p puffer-core connector_action_draft
cargo test -p puffer-cli outbound_action
cargo test -p puffer-cli monitor_snapshot
```

Expected: all pass.

```bash
git add crates/puffer-subscriptions crates/puffer-core/runtime/claude_tools/workflow/connector_tools.rs crates/puffer-cli/src/daemon_workflows.rs crates/puffer-cli/src/daemon_workflows
git commit -m "fix(outbound): type origins and delivery uncertainty"
```

---

### Task 4: Replace Suspension History With Terminal Run Execution And Recovery

**Files:**

- Create: `crates/puffer-cli/src/daemon_automation_runs.rs`
- Modify: `crates/puffer-cli/src/main.rs`
- Modify: `crates/puffer-cli/src/daemon.rs`
- Modify: `crates/puffer-cli/src/daemon_automation_runtime.rs`
- Modify: `crates/puffer-cli/src/daemon_workflows/outbound_action.rs`
- Modify: `crates/puffer-cli/tests/automation_real_e2e.rs`

**Interfaces:**

- Consumes the run store, policy resolver, and hardened outbound store.
- Produces startup reconciliation and sanitized run-history DTOs.
- Removes all continuation suspension/resume code.

- [ ] **Step 1: Replace mid-flow tests with terminal-only failing tests**

```rust
#[test]
fn reviewed_action_must_be_terminal() {
    let spec = connector_action_then_transform_spec();
    let error = compile_and_deploy(&spec).unwrap_err();
    assert!(error.to_string().contains("terminal top-level"));
}

#[test]
fn run_is_persisted_before_runtime_execution() {
    let result = run_with_runtime(&failing_runtime());
    assert!(result.is_err());
    assert!(matches!(only_run().state, AutomationRunState::Failed { .. }));
}

#[test]
fn terminal_gate_persists_run_before_materializing_draft() {
    let output = run_terminal_gated_automation();
    assert_eq!(output.status, "awaiting_approval");
    let run = only_run();
    let action = outbound_store().get(run.awaiting_action_id().unwrap()).unwrap().unwrap();
    assert_eq!(action.origin.run_id(), Some(run.id.as_str()));
}
```

- [ ] **Step 2: Run runtime tests and confirm RED**

```bash
cargo test -p puffer-cli daemon_automation_runtime -- --nocapture
```

Expected: old suspension behavior remains and run creation occurs too late.

- [ ] **Step 3: Add history projection and reconciliation module**

```rust
pub(crate) fn automation_run_history_value(
    state: &DaemonState,
    automation_id: &str,
) -> Result<Value>;

pub(crate) fn reconcile_automation_stores(
    runs: &AutomationRunStore,
    outbound: &OutboundStore,
) -> Result<AutomationReconcileSummary>;
```

History exposes ID, Automation ID, status, timestamps, summary, sanitized result, error code, and `delivery_may_have_occurred`. It never exposes action intent/input.

- [ ] **Step 4: Persist runs before execution and remove suspensions**

Start every preview/live run before its first runtime call:

```rust
let run = state.automation_run_store().start(NewAutomationRun {
    automation_id: record.id.clone(),
    automation_revision: record.revision,
    spec_hash: automation_spec_hash(&record.spec)?,
    title,
    source_event,
})?;
```

On error call `fail_run`; on ordinary completion call `complete`. Remove `AutomationSuspensionFile`, `AutomationRunSuspension`, `upsert_suspension`, `remove_suspensions`, `resume_automation_run`, and `mark_automation_run_approved` plus their tests.

- [ ] **Step 5: Materialize only terminal drafts**

At the terminal gate:

1. Resolve installed action policy.
2. Resolve exactly one allowed editable alias; conflicting aliases fail.
3. Generate an action UUID.
4. Build `AutomationActionIntent` from connector-owned input only.
5. Persist `AwaitingApproval`.
6. Call `ensure_draft` with typed Automation origin.

Do not inject connector/action/routing/trigger/Automation metadata into input.

Update `automation_pending_action_list/get` to select Automation rows from typed origin. Detail returns resolved approval policy, editable text, and sanitized destination/display fields; remove raw `input`, `message_field`, and heuristic body-key discovery.

- [ ] **Step 6: Implement local startup reconciliation**

During `DaemonState::load`, after both stores load:

```rust
outbound.recover_stale_sending()?;
runs.fail_interrupted_runs()?;
reconcile_automation_stores(&runs, &outbound)?;
```

Recreate missing drafts, settle sent/cancelled/expired actions, preserve draft/failed/uncertain review, and quarantine mismatches. Perform no remote calls.

- [ ] **Step 7: Add recovery tests**

Cover missing draft recreation, sent-before-run settlement, normal cancel, expiry, uncertain-cancelled action, stale Running, and mismatched origin/content.

- [ ] **Step 8: Run tests and commit**

```bash
cargo test -p puffer-cli daemon_automation_runtime
cargo test -p puffer-cli --test automation_real_e2e
cargo test -p puffer-automation
```

Expected: all pass and no orphaned resume/suspension warning remains.

```bash
git add crates/puffer-cli/src/daemon_automation_runs.rs crates/puffer-cli/src/main.rs crates/puffer-cli/src/daemon.rs crates/puffer-cli/src/daemon_automation_runtime.rs crates/puffer-cli/src/daemon_workflows/outbound_action.rs crates/puffer-cli/tests/automation_real_e2e.rs
git commit -m "fix(automation): persist terminal approval runs"
```

---

### Task 5: Make Execute And Cancel Origin-Aware And Race-Safe

**Files:**

- Create: `crates/puffer-cli/src/daemon_coordination.rs`
- Modify: `crates/puffer-cli/src/main.rs`
- Modify: `crates/puffer-cli/src/daemon.rs`
- Modify: `crates/puffer-cli/src/daemon_workflows.rs`
- Modify: `crates/puffer-cli/src/daemon_workflows/outbound_action.rs`
- Modify: `crates/puffer-cli/src/daemon_automation_runs.rs`

**Interfaces:**

- Produces `KeyedLocks` for action and Automation lifecycle coordination.
- Changes outbound mutation handlers to receive `&DaemonState`.
- Produces terminal Automation settlement after send/cancel/expiry.

- [ ] **Step 1: Add failing execute/settlement tests**

```rust
#[test]
fn sent_action_completes_terminal_run() {
    let fixture = automation_fixture();
    let result = execute(&fixture, "req-1", Some("edited")).unwrap();
    assert_eq!(result["status"], "sent");
    assert_eq!(fixture.run().status(), "completed");
    assert_eq!(fixture.executor.calls(), 1);
}

#[test]
fn same_request_reconciles_without_resending() {
    let fixture = sent_unsettled_fixture();
    execute(&fixture, "req-1", Some("approved")).unwrap();
    assert_eq!(fixture.executor.calls(), 0);
    assert_eq!(fixture.run().status(), "completed");
}

#[test]
fn executor_error_after_invocation_is_uncertain() {
    let fixture = failing_executor_fixture();
    let error = execute(&fixture, "req-1", None).unwrap_err();
    assert_eq!(error.code(), "outbound_delivery_uncertain");
    assert_eq!(fixture.action().status, OutboundActionStatus::Uncertain);
}
```

- [ ] **Step 2: Add failing approve/cancel race tests**

Use barriers so execute and cancel start against one version. Assert one terminal action transition wins. Add an uncertain-cancel case that requires `uncertain_delivery_ack` and fails the run with possible delivery.

- [ ] **Step 3: Run tests and confirm RED**

```bash
cargo test -p puffer-cli outbound_action -- --nocapture
```

Expected: settlement is missing, executor errors become Failed, or the race creates inconsistent states.

- [ ] **Step 4: Implement daemon-owned keyed coordination**

```rust
pub(crate) struct KeyedLocks {
    entries: Mutex<HashMap<String, Weak<Mutex<()>>>>,
}

impl KeyedLocks {
    pub(crate) fn get(&self, key: impl Into<String>) -> Arc<Mutex<()>>;
}
```

Add registries to `DaemonState` and use `action:{id}` / `automation:{id}` keys. Weak entries avoid a cleanup service.

- [ ] **Step 5: Refactor execute to commit send before settlement**

```rust
pub(crate) fn handle_outbound_action_execute(
    state: &DaemonState,
    params: &Value,
) -> Result<Value>;
```

Under the action lock: re-read, validate, begin send, invoke, and commit Sent/Uncertain. Release the action lock before changing the run. If Sent is already committed, return success even when run settlement fails and include `runSettlementPending: true`.

If `begin_send` expires an Automation action, release the lock and fail the linked run with `automation_approval_expired`; do not leave it AwaitingApproval.

- [ ] **Step 6: Refactor cancel and remove Automation-only mutation aliases**

Use `outbound_action_cancel` for Automation rejection. Require reason for Automation origin and `uncertain_delivery_ack` for Uncertain. Release the lock before run settlement.

Remove `automation_pending_action_reject` and `connector_action_execute`. Retain `monitor_action_execute`, `task_monitor_action_execute`, `outbound_action_execute`, and `outbound_action_cancel`.

- [ ] **Step 7: Run tests and commit**

```bash
cargo test -p puffer-cli outbound_action
cargo test -p puffer-cli daemon_automation_runtime
cargo test -p puffer-cli monitor_action
```

Expected: all pass, including races.

```bash
git add crates/puffer-cli/src/daemon_coordination.rs crates/puffer-cli/src/main.rs crates/puffer-cli/src/daemon.rs crates/puffer-cli/src/daemon_workflows.rs crates/puffer-cli/src/daemon_workflows/outbound_action.rs crates/puffer-cli/src/daemon_automation_runs.rs
git commit -m "fix(outbound): settle automation actions safely"
```

---

### Task 6: Make Catalog, Activation, And Deletion Honest

**Files:**

- Modify: `crates/puffer-automation/src/spec.rs`
- Modify: `crates/puffer-automation/src/validation.rs`
- Modify: `crates/puffer-cli/src/daemon.rs`
- Modify: `crates/puffer-cli/src/daemon_automations.rs`
- Modify: `crates/puffer-cli/src/daemon_automation_runtime.rs`
- Modify: `crates/puffer-cli/src/daemon_automation_runs.rs`

**Interfaces:**

- Adds `runtime_target_key` to `AutomationRuntimeState`.
- Uses Automation lifecycle keyed locks from Task 5.
- Produces catalog and activation behavior consumed by Desktop.

- [ ] **Step 1: Add failing catalog and activation tests**

```rust
#[test]
fn catalog_omits_unexecutable_agentenv_entries() {
    let catalog = automation_catalog(local_paths());
    assert!(catalog_trigger(&catalog, "webhook").is_none());
    assert!(catalog_trigger(&catalog, "schedule:daily").is_none());
    assert!(catalog_action(&catalog, "agentenv:transform_js:local-transform").is_none());
}

#[test]
fn activation_rejects_agentenv_owned_trigger() {
    let error = activate(webhook_spec()).unwrap_err();
    assert_eq!(error.code(), "automation_runtime_capability_missing");
}

#[test]
fn runtime_target_change_requires_reactivation() {
    let record = deployed_record("target-a");
    let error = ensure_live_with_target(&record, "target-b").unwrap_err();
    assert!(error.to_string().contains("reactivate"));
}
```

- [ ] **Step 2: Run tests and confirm RED**

```bash
cargo test -p puffer-cli automation_catalog -- --nocapture
cargo test -p puffer-cli daemon_automation_runtime -- --nocapture
```

Expected: unsupported entries remain advertised and no target key is checked.

- [ ] **Step 3: Remove unsupported catalog entries and add target identity**

Stop hardcoding webhook, schedule, and local transform. Catalog generation must not call `ensure_ready` or start Docker.

Hash canonical JSON containing run location, backend mode, normalized base URL, and workspace ID. Never include token or secret IDs.

- [ ] **Step 4: Enforce enabled-last activation**

Implement only these observable phases:

1. Compile and validate ownership/policy.
2. Freshly validate runtime target and executable nodes.
3. Prepare helper artifacts and paused Puffer bindings.
4. Enable the binding after helpers are ready.
5. Mark the record Enabled last.

If a helper cannot be prepared without live ingress, reject it. Failure disables/removes newly enabled bindings best effort and persists a paused/error record.

- [ ] **Step 5: Add failing trigger-admission/delete race test**

Use a barrier. Assert deletion either sees the new Running run and leaves the record paused, or completes before admission and causes admission to fail. It must never delete while a run starts successfully.

- [ ] **Step 6: Coordinate save, activation, admission, and deletion**

Acquire `automation:{id}` for save/pause, compile/deploy, live admission through creation of Running, and delete. Deletion disables admission/binding, rechecks active runs, leaves a visible paused record when blocked, and otherwise removes bindings before the record.

- [ ] **Step 7: Run tests and commit**

```bash
cargo test -p puffer-automation
cargo test -p puffer-cli daemon_automations
cargo test -p puffer-cli daemon_automation_runtime
cargo test -p puffer-cli --test automation_real_e2e
```

Expected: all pass.

```bash
git add crates/puffer-automation/src/spec.rs crates/puffer-automation/src/validation.rs crates/puffer-cli/src/daemon.rs crates/puffer-cli/src/daemon_automations.rs crates/puffer-cli/src/daemon_automation_runtime.rs crates/puffer-cli/src/daemon_automation_runs.rs
git commit -m "fix(automation): gate activation and deletion"
```

---

### Task 7: Serialize And Secure The Local Runtime Lifecycle

**Files:**

- Modify: `crates/puffer-cli/src/daemon.rs`
- Modify: `crates/puffer-cli/src/daemon_workflow_runtime.rs`
- Modify: `crates/puffer-cli/src/workflow_local_runtime.rs`
- Modify: `crates/puffer-cli/src/workflow_local_runtime_bootstrap.rs`
- Modify: `crates/puffer-cli/src/workflow_local_runtime_tests.rs`

**Interfaces:**

- Adds one daemon-owned `local_runtime_lifecycle: Arc<Mutex<()>>`.
- Lifecycle helpers return candidate config and created-secret metadata instead of silently saving selected user config.

- [ ] **Step 1: Add failing serialization and config ownership tests**

```rust
#[test]
fn selected_local_persists_then_replaces_memory() {
    let state = state_with_local_backend();
    let result = repair_local(&state).unwrap();
    assert!(result.success);
    assert_eq!(
        state.config_snapshot().workflow_backend.api_base_url,
        persisted_config().workflow_backend.api_base_url,
    );
}

#[test]
fn transient_local_does_not_replace_cloud_selection() {
    let state = state_with_cloud_backend();
    ensure_transient_local(&state).unwrap();
    assert_eq!(state.config_snapshot().workflow_backend.mode, WorkflowBackendMode::Cloud);
}
```

Use a blocking fake runner to prove concurrent start/repair cannot enter the lifecycle body together.

- [ ] **Step 2: Add failing secure-file and rollback tests**

On Unix assert runtime root mode `0700` and `.env`, `seed.sql`, and stored config mode `0600`. Inject config-save failure after secret creation and assert `SecretVault::delete` removes the new secret while memory remains unchanged.

- [ ] **Step 3: Run tests and confirm RED**

```bash
cargo test -p puffer-cli workflow_local_runtime -- --nocapture
```

Expected: lifecycle calls overlap, repair leaves config stale, or generated files use default modes.

- [ ] **Step 4: Add the lifecycle mutex and candidate-config result**

```rust
pub(crate) struct LocalRuntimeConfigUpdate {
    pub candidate_config: PufferConfig,
    pub created_secret_id: Option<String>,
    pub scope: LocalRuntimeConfigScope,
}

pub(crate) enum LocalRuntimeConfigScope {
    PersistentSelectedBackend,
    TransientLocal,
}
```

Return this update alongside the existing status/repair result instead of introducing a manager abstraction. Acquire the same mutex in start, ensure-ready, test, repair, stop, and inspect handlers. For persistent scope, save, clean up a new secret on failure, then call `replace_config`. Transient scope updates only the transient local file.

- [ ] **Step 5: Implement private atomic writes and supplied-path behavior**

Create the runtime root with `0700`. Write private files through a same-directory temp file opened with `create_new`, mode `0600`, `write_all`, `sync_all`, and atomic rename. Reject a destination symlink. Pass `ConfigPaths` to stop/log/inspect instead of rediscovering cwd.

- [ ] **Step 6: Run tests and commit**

```bash
cargo test -p puffer-cli workflow_local_runtime
cargo test -p puffer-cli daemon_workflow_runtime
```

Expected: all pass.

```bash
git add crates/puffer-cli/src/daemon.rs crates/puffer-cli/src/daemon_workflow_runtime.rs crates/puffer-cli/src/workflow_local_runtime.rs crates/puffer-cli/src/workflow_local_runtime_bootstrap.rs crates/puffer-cli/src/workflow_local_runtime_tests.rs
git commit -m "fix(runtime): serialize and secure local lifecycle"
```

---

### Task 8: Migrate The Desktop Review Contract And Restore E2E Coverage

**Files:**

- Modify: `apps/puffer-desktop/src/lib/api/desktop.ts`
- Modify: `apps/puffer-desktop/src/lib/api/desktop.workflow-daemon.test.ts`
- Modify: `apps/puffer-desktop/src/lib/types.ts`
- Modify: `apps/puffer-desktop/src/lib/screens/Automation.svelte`
- Modify: `apps/puffer-desktop/src/lib/screens/agent/ToolCard.svelte`
- Modify: `apps/puffer-desktop/src/lib/screens/agent/connectorDraftStatus.ts`
- Modify: `apps/puffer-desktop/src/lib/screens/agent/connectorDraftStatus.test.ts`
- Modify: `apps/puffer-desktop/tests/support/fakeDaemon.ts`
- Modify: `apps/puffer-desktop/tests/automation-ui.spec.ts`
- Modify: `apps/puffer-desktop/tests/outbound-gate-matrix.spec.ts`

**Interfaces:**

- Consumes canonical outbound execute/cancel and sanitized review DTOs.
- Removes Automation compatibility wrappers and raw-input editing.

- [ ] **Step 1: Add failing API tests**

```ts
await executeOutboundAction({
  actionId: "oa-1",
  version: 2,
  approvedText: "edited",
  clientRequestId: "req-1"
});

expect(request).toHaveBeenCalledWith("outbound_action_execute", {
  action_id: "oa-1",
  version: 2,
  approved_text: "edited",
  client_request_id: "req-1"
});

await cancelOutboundAction({
  actionId: "oa-1",
  version: 2,
  reason: "Reject",
  uncertainDeliveryAck: true
});
```

Assert `executeConnectorActionDraft` and `rejectAutomationPendingAction` are no longer exported.

- [ ] **Step 2: Run Vitest and confirm RED**

From `apps/puffer-desktop`:

```bash
npx vitest run src/lib/api/desktop.workflow-daemon.test.ts
```

Expected: old wrappers and `approved_input`/`approved_message` remain.

- [ ] **Step 3: Migrate API and types**

```ts
export type AutomationApprovalPolicy =
  | { kind: "exact" }
  | { kind: "editable_text"; input_field: string; max_bytes: number; allow_empty: boolean };

export type AutomationPendingActionDetail = AutomationPendingActionListItem & {
  approval_policy: AutomationApprovalPolicy;
  editable_text: string | null;
  destination_metadata: Record<string, unknown>;
  error?: unknown;
};
```

Remove `input`, `message_field`, and `approvedInput`. Update `ToolCard.svelte` to send `approvedText` and to require a separate acknowledgement before cancelling Uncertain. Do not change Monitor alias route names.

- [ ] **Step 4: Add failing Playwright scenarios**

Restore the review-inbox `test.fixme` to `test`. Cover editable approval, exact approval, normal rejection, sent plus `runSettlementPending`, uncertain retry acknowledgement, uncertain abandon acknowledgement, expiry, hidden unsupported catalog entries, and active-run delete failure.

- [ ] **Step 5: Update UI and strict fake daemon**

Render from `approval_policy`; never retain raw input. Keep Snooze local-only. Permit at most one mandatory-review action and place it last. Exclude mandatory-review connector actions from `puffer_agent`'s tool list so the explicit terminal step is the only outward effect.

Model DraftReady, Failed, Uncertain, Sent, Cancelled, Expired, and Quarantined in `FakeDaemon`; continue rejecting unknown RPCs. Update `connectorDraftStatus.ts` and the outbound gate matrix for the new stable error codes and uncertain-abandon confirmation.

- [ ] **Step 6: Run Desktop checks and commit**

```bash
npx vitest run src/lib/api/desktop.workflow-daemon.test.ts
npx vitest run src/lib/screens/agent/connectorDraftStatus.test.ts
npm run check
npm run test:desktop-ui -- tests/automation-ui.spec.ts
npm run test:desktop-ui -- tests/outbound-gate-matrix.spec.ts
```

Expected: all pass with no skipped/fixme review-inbox test.

```bash
git add apps/puffer-desktop/src/lib/api/desktop.ts apps/puffer-desktop/src/lib/api/desktop.workflow-daemon.test.ts apps/puffer-desktop/src/lib/types.ts apps/puffer-desktop/src/lib/screens/Automation.svelte apps/puffer-desktop/src/lib/screens/agent/ToolCard.svelte apps/puffer-desktop/src/lib/screens/agent/connectorDraftStatus.ts apps/puffer-desktop/src/lib/screens/agent/connectorDraftStatus.test.ts apps/puffer-desktop/tests/support/fakeDaemon.ts apps/puffer-desktop/tests/automation-ui.spec.ts apps/puffer-desktop/tests/outbound-gate-matrix.spec.ts
git commit -m "fix(desktop): migrate automation terminal review"
```

---

### Task 9: Run Full Verification And Independent Review

**Files:** Modify only files required by verified failures or reviewer findings.

- [ ] **Step 1: Run formatting and focused Rust suites**

```bash
cargo fmt --all --check
cargo test -p puffer-subscriptions
cargo test -p puffer-automation
cargo test -p puffer-cli daemon_automation_runtime
cargo test -p puffer-cli outbound_action
cargo test -p puffer-cli workflow_local_runtime
cargo test -p puffer-cli --test automation_real_e2e
```

Expected: all exit 0.

- [ ] **Step 2: Run clippy and workspace tests**

```bash
cargo clippy --workspace --all-targets -- -D clippy::correctness -D clippy::suspicious
cargo test --workspace
```

Expected: both exit 0.

- [ ] **Step 3: Run Desktop and repository gates**

From `apps/puffer-desktop`:

```bash
npm run check
npx vitest run src/lib/api/desktop.workflow-daemon.test.ts
npm run test:desktop-ui
```

From repository root:

```bash
scripts/check-doc-links.sh
scripts/report-large-files.sh
scripts/ci-gates.sh
```

Expected: all required gates exit 0. Report touched large files instead of splitting them.

- [ ] **Step 4: Request independent code review**

Use `superpowers:requesting-code-review` with base `2edfdccf208b88ceae8ee18eb380668fb01258ac`, current head, and the updated design/specs. Fix every Critical or Important finding and repeat the smallest relevant verification.

- [ ] **Step 5: Review final diff and commit verified fixes**

```bash
git diff upstream/master...HEAD --check
git status --short
git diff upstream/master...HEAD
```

If review produced changes, stage only reviewed files and commit:

```bash
git add -A
git commit -m "fix(automation): address final review findings"
```

Expected: clean worktree after the final commit.

---

### Task 10: Update PR #505, Merge, Then File Deferred Issues

**Prerequisite:** `gh auth status` must show a valid account authorized for `berabuddies/puffer` and `agentenv/monorepo`. Review found the current tokens invalid, so authenticate before this task.

- [ ] **Step 1: Confirm PR head and push reviewed commits**

```bash
gh pr view 505 --repo berabuddies/puffer --json headRefName,headRepositoryOwner,headRefOid,mergeable,statusCheckRollup
git log --oneline 2edfdccf..HEAD
git diff 2edfdccf...HEAD --check
```

Push with a normal fast-forward to the exact owner/branch returned by `gh pr view`; do not overwrite unrelated history.

The current public PR metadata resolves to `gloriazhang99/puffer:feat/automation-implementation`, so the expected push command is:

```bash
git push git@fuzzland.github.com:gloriazhang99/puffer.git HEAD:feat/automation-implementation
```

If GitHub denies the push, stop and restore maintainer-edit authentication; do not force-push or merge an unfixed head.

- [ ] **Step 2: Wait for checks and merge**

```bash
gh pr checks 505 --repo berabuddies/puffer --watch
gh pr view 505 --repo berabuddies/puffer --json mergeable,reviewDecision,statusCheckRollup
gh pr merge 505 --repo berabuddies/puffer --merge --delete-branch=false
```

Expected: checks pass and PR #505 becomes merged.

- [ ] **Step 3: File deferred issue - durable mid-flow continuation**

Repository: `agentenv/monorepo`

Title:

```text
Support durable mid-flow Puffer Automation approval continuations
```

Body:

```markdown
## Context

Puffer PR berabuddies/puffer#505 intentionally launches with human-gated connector actions restricted to the terminal top-level step.

## Required design

- Persist per-step execution checkpoints rather than only a next-step index.
- Define server-owned effect/idempotency metadata for every replayed Puffer tool and AgentEnv node.
- Prevent duplicate provider, filesystem, shell, AgentEnv, tool, and connector effects after crashes.
- Keep committed connector delivery distinct from continuation settlement.
- Define retry, abandonment, privacy, and storage bounds.

## Acceptance

A non-terminal approval survives a process exit before and after every continuation step without duplicate outward effects or ambiguous run state. Crash tests cover each boundary.
```

Save that body as `/tmp/puffer-mid-flow-approval.md`, then run:

```bash
gh issue create --repo agentenv/monorepo --title "Support durable mid-flow Puffer Automation approval continuations" --body-file /tmp/puffer-mid-flow-approval.md
```

- [ ] **Step 4: File deferred issue - AgentEnv trigger bridge**

Repository: `agentenv/monorepo`

Title:

```text
Bridge AgentEnv schedule and webhook triggers into Puffer-owned Automation steps
```

Body:

```markdown
## Context

Puffer rejects AgentEnv-owned schedule/webhook triggers when a flow contains `puffer_agent` or Puffer connector actions because no supported ingress returns the event to Puffer.

## Required design

- Authenticated trigger ingress carrying a stable event envelope into Puffer.
- Deduplication and provenance across AgentEnv and Puffer.
- Activation, pause, update, delete, and partial-deployment semantics.
- No Active state before the mixed flow is executable end to end.

## Acceptance

Schedule/webhook -> puffer_agent -> reviewed connector action passes an end-to-end test, including duplicate delivery and partial-deployment recovery.
```

Save that body as `/tmp/puffer-trigger-bridge.md`, then run:

```bash
gh issue create --repo agentenv/monorepo --title "Bridge AgentEnv schedule and webhook triggers into Puffer-owned Automation steps" --body-file /tmp/puffer-trigger-bridge.md
```

- [ ] **Step 5: File deferred issue - executable local capabilities**

Repository: `agentenv/monorepo`

Title:

```text
Expose executable AgentEnv capabilities and provide the local transform sandbox
```

Body:

```markdown
## Context

The local Compose runtime can list `transform_js` while execution fails because its executor sandbox is absent. Node discovery is not an executable-capability guarantee.

## Required design

- Start and health-check the executor/sandbox required by `transform_js`.
- Expose executable capability separately from node discovery.
- Include non-secret runtime/version identity for activation preflight.
- Add a no-side-effect execution probe.

## Acceptance

Puffer can prove before activation that `transform_js` is executable, and a local Compose end-to-end transform run passes.
```

Save that body as `/tmp/puffer-local-executor.md`, then run:

```bash
gh issue create --repo agentenv/monorepo --title "Expose executable AgentEnv capabilities and provide the local transform sandbox" --body-file /tmp/puffer-local-executor.md
```

- [ ] **Step 6: File deferred issue - outbound retention**

Repository: `agentenv/monorepo`

Title:

```text
Bound Puffer outbound action and audit retention
```

Body:

```markdown
## Context

Automation run history is bounded, but generic outbound terminal actions and NDJSON audit entries can grow indefinitely and may contain sensitive metadata.

## Required design

- Retain pending and uncertain actions while bounding old terminal records.
- Preserve records referenced by active workflows or audit requirements.
- Rotate audit files without losing decision provenance.
- Keep private file permissions and metadata-only diagnostics.

## Acceptance

Sustained outbound use remains within a documented storage bound without deleting pending actions or required audit evidence.
```

Save that body as `/tmp/puffer-outbound-retention.md`, then run:

```bash
gh issue create --repo agentenv/monorepo --title "Bound Puffer outbound action and audit retention" --body-file /tmp/puffer-outbound-retention.md
```

- [ ] **Step 7: Record final outcome**

Return the merged PR URL, merge commit SHA, issue URLs, verification commands, and any residual risk.
