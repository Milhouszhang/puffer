<script lang="ts">
  import "../design/workflow.css";

  import { onDestroy, onMount } from "svelte";
  import {
    deleteWorkflowBinding,
    deployRuntimeWorkflow,
    executeRuntimeWorkflow,
    listWorkflowExecutions,
    loadWorkflowSnapshot,
    toggleWorkflow
  } from "../api/desktop";
  import Icon from "../design/Icon.svelte";
  import type {
    WorkflowBinding,
    WorkflowConnection,
    WorkflowConnector,
    WorkflowExecutionRecord,
    WorkflowMonitorTask,
    WorkflowRuntimeRecord,
    WorkflowSnapshot
  } from "../types";

  type AutomationDraft = {
    name: string;
    description: string;
  };

  type WorkflowScreen = "overview" | "detail" | "draft";

  const EXECUTION_REFRESH_DELAY_MS = 1500;
  const EXECUTION_REFRESH_ATTEMPTS = 4;

  let loading = $state(false);
  let error = $state<string | null>(null);
  let workflowLoadError = $state<string | null>(null);
  let workflowQuery = $state("");
  let snapshot = $state<WorkflowSnapshot>({
    workflows: [],
    runs: [],
    connectors: [],
    connections: [],
    workflow_bindings: [],
    monitor_tasks: []
  });
  let togglingBindingSlug = $state<string | null>(null);
  let deletingBindingSlug = $state<string | null>(null);
  let selectedWorkflowKey = $state<string | null>(null);
  let runtimeActionBusy = $state<string | null>(null);
  let loadingExecutionsFor = $state<string | null>(null);
  let runtimeActionNotice = $state<string | null>(null);
  let workflowExecutions = $state<Record<string, WorkflowExecutionRecord[]>>({});
  let workflowExecutionErrors = $state<Record<string, string>>({});
  let workflowScreen = $state<WorkflowScreen>("overview");
  let createDialogOpen = $state(false);
  let createWorkflowName = $state("");
  let createWorkflowDescription = $state("");
  let createWorkflowNameError = $state<string | null>(null);
  let automationDraft = $state<AutomationDraft | null>(null);
  let executionRefreshTimers = new Map<string, number>();

  const workflows = $derived(snapshot.workflows ?? []);
  const bindings = $derived(snapshot.workflow_bindings ?? []);
  const connections = $derived(snapshot.connections ?? []);
  const connectors = $derived(snapshot.connectors ?? []);
  const monitorTasks = $derived(snapshot.monitor_tasks ?? []);
  const workflowError = $derived(snapshot.workflow_error ?? null);
  const queryTerms = $derived(
    workflowQuery
      .trim()
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean)
  );
  const filteredWorkflows = $derived(
    workflows.filter((workflow) => matchesTerms(runtimeWorkflowSearchText(workflow), queryTerms))
  );
  const runtimeUnavailable = $derived(workflowLoadError ?? workflowError ?? null);
  const selectedWorkflow = $derived(
    selectedWorkflowKey === null
      ? null
      : workflows.find((workflow, index) => workflowKey(workflow, index) === selectedWorkflowKey) ?? null
  );
  const activeBindings = $derived(bindings.filter((binding) => binding.enabled).length);
  const readyConnections = $derived(connections.filter((connection) => connection.can_trigger_workflow).length);
  const activeMonitorTasks = $derived(monitorTasks.filter((task) => !task.ignored).length);

  onMount(() => {
    void refresh();
  });

  onDestroy(() => {
    for (const timer of executionRefreshTimers.values()) {
      window.clearTimeout(timer);
    }
    executionRefreshTimers.clear();
  });

  async function refresh() {
    if (loading) return;
    loading = true;
    error = null;
    runtimeActionNotice = null;
    try {
      applyWorkflowSnapshot(await loadWorkflowSnapshot());
      workflowLoadError = null;
    } catch (err) {
      const message = workflowRequestErrorText(err, "loading workflows");
      error = message;
      workflowLoadError = message;
    } finally {
      loading = false;
    }
  }

  function applyWorkflowSnapshot(next: WorkflowSnapshot) {
    const normalized = {
      workflows: next.workflows ?? [],
      runs: next.runs ?? [],
      workflow_error: next.workflow_error ?? null,
      connectors: next.connectors ?? [],
      connections: next.connections ?? [],
      connector_error: next.connector_error ?? null,
      workflow_bindings: next.workflow_bindings ?? [],
      workflow_binding_error: next.workflow_binding_error ?? null,
      monitor_tasks: next.monitor_tasks ?? [],
      monitor_task_error: next.monitor_task_error ?? null
    };
    snapshot = normalized;
    syncExecutionCache(normalized.runs);
    if (
      selectedWorkflowKey !== null &&
      !normalized.workflows.some((workflow, index) => workflowKey(workflow, index) === selectedWorkflowKey)
    ) {
      selectedWorkflowKey = null;
    }
  }

  function openCreateWorkflowDialog() {
    if (runtimeActionBusy) return;
    createWorkflowName = "";
    createWorkflowDescription = "";
    createWorkflowNameError = null;
    createDialogOpen = true;
  }

  function closeCreateWorkflowDialog() {
    if (runtimeActionBusy) return;
    createDialogOpen = false;
    createWorkflowNameError = null;
  }

  function startAutomationDraftFromDialog() {
    const name = createWorkflowName.trim();
    if (!name) {
      createWorkflowNameError = "Name is required.";
      return;
    }
    if (runtimeActionBusy) return;
    error = null;
    runtimeActionNotice = null;
    automationDraft = {
      name,
      description: createWorkflowDescription.trim()
    };
    selectedWorkflowKey = null;
    workflowScreen = "draft";
    createDialogOpen = false;
    createWorkflowNameError = null;
  }

  async function toggleBinding(binding: WorkflowBinding) {
    if (togglingBindingSlug || deletingBindingSlug) return;
    togglingBindingSlug = binding.slug;
    error = null;
    try {
      applyWorkflowSnapshot(await toggleWorkflow(binding.slug, !binding.enabled));
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      togglingBindingSlug = null;
    }
  }

  async function deleteBinding(binding: WorkflowBinding) {
    if (togglingBindingSlug || deletingBindingSlug) return;
    deletingBindingSlug = binding.slug;
    error = null;
    try {
      applyWorkflowSnapshot(await deleteWorkflowBinding(binding.slug));
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      deletingBindingSlug = null;
    }
  }

  async function deployWorkflow(record: WorkflowRuntimeRecord, index: number) {
    const id = workflowApiId(record);
    if (!id || runtimeActionBusy) return;
    selectWorkflow(record, index);
    runtimeActionBusy = `deploy:${id}`;
    error = null;
    runtimeActionNotice = null;
    try {
      upsertRuntimeWorkflow(await deployRuntimeWorkflow(id));
      runtimeActionNotice = `Deployed ${workflowName(record)}.`;
    } catch (err) {
      error = workflowRequestErrorText(err, "deploying workflow");
    } finally {
      runtimeActionBusy = null;
    }
  }

  async function runWorkflow(record: WorkflowRuntimeRecord, index: number) {
    const id = workflowApiId(record);
    if (!id || runtimeActionBusy) return;
    selectWorkflow(record, index);
    runtimeActionBusy = `run:${id}`;
    error = null;
    runtimeActionNotice = null;
    try {
      const execution = await executeRuntimeWorkflow(id);
      addRuntimeExecution(id, execution);
      runtimeActionNotice = `Started ${workflowName(record)} run ${executionName(execution)}.`;
      await refreshWorkflowExecutions(id, "refreshing executions");
      scheduleExecutionRefresh(id, EXECUTION_REFRESH_ATTEMPTS);
    } catch (err) {
      error = workflowRequestErrorText(err, "running workflow");
    } finally {
      runtimeActionBusy = null;
    }
  }

  async function showExecutions(record: WorkflowRuntimeRecord, index: number) {
    const id = workflowApiId(record);
    if (!id || loadingExecutionsFor) return;
    selectWorkflow(record, index);
    loadingExecutionsFor = id;
    error = null;
    runtimeActionNotice = null;
    try {
      await refreshWorkflowExecutions(id, "loading executions");
    } catch (err) {
      error = workflowRequestErrorText(err, "loading executions");
    } finally {
      loadingExecutionsFor = null;
    }
  }

  function selectWorkflow(record: WorkflowRuntimeRecord, index: number) {
    workflowScreen = "detail";
    automationDraft = null;
    selectedWorkflowKey = workflowKey(record, index);
  }

  function returnToWorkflowOverview() {
    workflowScreen = "overview";
  }

  function upsertRuntimeWorkflow(record: WorkflowRuntimeRecord) {
    const id = workflowApiId(record);
    if (!id) return;
    const replaced = workflows.some((workflow) => workflowApiId(workflow) === id);
    snapshot = {
      ...snapshot,
      workflows: replaced
        ? workflows.map((workflow) => (workflowApiId(workflow) === id ? record : workflow))
        : [...workflows, record]
    };
  }

  function addRuntimeExecution(workflowId: string, execution: WorkflowExecutionRecord) {
    const localExecution = executionWorkflowId(execution)
      ? execution
      : { ...execution, workflowId, workflow_id: workflowId };
    const existing = workflowExecutions[workflowId] ?? executionsForWorkflowId(workflowId);
    const executionId = executionRecordId(localExecution);
    const nextExecutions = [
      localExecution,
      ...existing.filter((item) => executionRecordId(item) !== executionId)
    ];
    workflowExecutions = {
      ...workflowExecutions,
      [workflowId]: nextExecutions
    };
    snapshot = {
      ...snapshot,
      runs: [
        localExecution,
        ...snapshot.runs.filter((run) => executionRecordId(run) !== executionId)
      ]
    };
  }

  async function refreshWorkflowExecutions(
    workflowId: string,
    action: string
  ): Promise<WorkflowExecutionRecord[] | null> {
    workflowExecutionErrors = withoutRecordKey(workflowExecutionErrors, workflowId);
    try {
      const executions = await listWorkflowExecutions(workflowId);
      applyRuntimeExecutions(workflowId, executions);
      return executions;
    } catch (err) {
      workflowExecutionErrors = {
        ...workflowExecutionErrors,
        [workflowId]: workflowRequestErrorText(err, action)
      };
      return null;
    }
  }

  function applyRuntimeExecutions(workflowId: string, executions: WorkflowExecutionRecord[]) {
    workflowExecutions = {
      ...workflowExecutions,
      [workflowId]: executions
    };
    snapshot = {
      ...snapshot,
      runs: [
        ...executions,
        ...snapshot.runs.filter((run) => executionWorkflowId(run) !== workflowId)
      ]
    };
  }

  function scheduleExecutionRefresh(workflowId: string, remainingAttempts: number) {
    const previous = executionRefreshTimers.get(workflowId);
    if (previous !== undefined) window.clearTimeout(previous);
    if (remainingAttempts <= 0) {
      executionRefreshTimers.delete(workflowId);
      return;
    }
    const timer = window.setTimeout(() => {
      executionRefreshTimers.delete(workflowId);
      void refreshWorkflowExecutions(workflowId, "refreshing executions").then(() => {
        scheduleExecutionRefresh(workflowId, remainingAttempts - 1);
      });
    }, EXECUTION_REFRESH_DELAY_MS);
    executionRefreshTimers = new Map(executionRefreshTimers).set(workflowId, timer);
  }

  function workflowRequestErrorText(err: unknown, action: string): string {
    const message = err instanceof Error ? err.message : String(err);
    if (message.includes("timed out")) {
      return `Timed out ${action}. Check Settings > Workflows and confirm the configured runtime is running.`;
    }
    return message;
  }

  function syncExecutionCache(runs: WorkflowRuntimeRecord[]) {
    const next: Record<string, WorkflowExecutionRecord[]> = {};
    for (const run of runs) {
      const workflowId = executionWorkflowId(run);
      if (!workflowId) continue;
      next[workflowId] = [...(next[workflowId] ?? []), run];
    }
    workflowExecutions = {
      ...next,
      ...workflowExecutions
    };
  }

  function matchesTerms(searchText: string, terms: string[]): boolean {
    if (terms.length === 0) return true;
    const lower = searchText.toLowerCase();
    return terms.every((term) => lower.includes(term));
  }

  function runtimeWorkflowSearchText(record: WorkflowRuntimeRecord): string {
    return [
      recordString(record, ["id", "workflow_id", "workflowId", "slug"]),
      recordString(record, ["name", "display_name", "displayName", "title"]),
      recordString(record, ["status", "state", "deployment_status", "deploymentStatus"]),
      recordString(record, ["description", "summary"]),
      recordString(record, ["source", "workspaceId", "workspace_id"])
    ]
      .filter((value) => value !== "-")
      .join(" ");
  }

  function workflowKey(record: WorkflowRuntimeRecord, index: number): string {
    return workflowApiId(record) ?? `workflow-${index}`;
  }

  function workflowApiId(record: WorkflowRuntimeRecord): string | null {
    const id = recordString(record, ["id", "workflow_id", "workflowId", "slug"]);
    return id === "-" ? null : id;
  }

  function workflowName(record: WorkflowRuntimeRecord): string {
    const name = recordString(record, ["name", "display_name", "displayName", "title"]);
    if (name !== "-") return name;
    return recordString(record, ["id", "workflow_id", "workflowId", "slug"]);
  }

  function workflowDescription(record: WorkflowRuntimeRecord): string {
    const description = recordString(record, ["description", "summary"]);
    return description === "-" ? "" : description;
  }

  function workflowStatus(record: WorkflowRuntimeRecord): string {
    return recordString(record, [
      "status",
      "state",
      "deployment_status",
      "deploymentStatus",
      "lifecycle_state",
      "lifecycleState"
    ]);
  }

  function workflowMetadata(record: WorkflowRuntimeRecord): Array<{ label: string; value: string }> {
    return [
      { label: "ID", value: workflowApiId(record) ?? "-" },
      { label: "Name", value: workflowName(record) },
      { label: "Status", value: workflowStatus(record) },
      { label: "Deployment", value: recordString(record, ["deploymentStatus", "deployment_status"]) },
      { label: "Version", value: recordString(record, ["version", "revision"]) },
      { label: "Source", value: recordString(record, ["source", "provider", "runtime"]) }
    ].filter((item) => item.value !== "-");
  }

  function workflowRawJson(record: WorkflowRuntimeRecord): string {
    return JSON.stringify(record, null, 2);
  }

  function workflowStatusClass(record: WorkflowRuntimeRecord): string {
    const status = workflowStatus(record).toLowerCase();
    if (status.includes("fail") || status.includes("error")) return "failed";
    if (status.includes("run") || status.includes("active") || status.includes("deploy")) return "running";
    if (status.includes("done") || status.includes("complete") || status.includes("success")) return "completed";
    if (status.includes("draft") || status.includes("skip") || status.includes("pause")) return "skipped";
    return status.replace(/[^a-z0-9_-]/g, "") || "skipped";
  }

  function executionsForWorkflow(record: WorkflowRuntimeRecord): WorkflowExecutionRecord[] {
    const id = workflowApiId(record);
    if (!id) return [];
    return workflowExecutions[id] ?? executionsForWorkflowId(id);
  }

  function executionsForWorkflowId(workflowId: string): WorkflowExecutionRecord[] {
    return snapshot.runs.filter((run) => executionWorkflowId(run) === workflowId);
  }

  function executionWorkflowId(record: WorkflowRuntimeRecord): string | null {
    const id = recordString(record, ["workflowId", "workflow_id", "workflowSlug", "workflow_slug"]);
    return id === "-" ? null : id;
  }

  function executionRecordId(record: WorkflowRuntimeRecord): string | null {
    const id = recordString(record, ["id", "executionId", "execution_id", "runId", "run_id"]);
    return id === "-" ? null : id;
  }

  function executionKey(record: WorkflowExecutionRecord, index: number): string {
    return executionRecordId(record) ?? `execution-${index}`;
  }

  function executionName(record: WorkflowExecutionRecord): string {
    return executionRecordId(record) ?? "execution";
  }

  function executionStatus(record: WorkflowExecutionRecord): string {
    return recordString(record, ["status", "state", "result"]);
  }

  function executionStatusClass(record: WorkflowExecutionRecord): string {
    const status = executionStatus(record).toLowerCase();
    if (status.includes("fail") || status.includes("error")) return "failed";
    if (status.includes("run") || status.includes("pending")) return "running";
    if (status.includes("complete") || status.includes("success")) return "completed";
    return "skipped";
  }

  function executionErrorFor(record: WorkflowRuntimeRecord): string | null {
    const id = workflowApiId(record);
    if (!id) return null;
    return workflowExecutionErrors[id] ?? null;
  }

  function relatedBindings(record: WorkflowRuntimeRecord): WorkflowBinding[] {
    const id = workflowApiId(record)?.toLowerCase() ?? "";
    const name = workflowName(record).toLowerCase();
    return bindings.filter((binding) => {
      const text = [
        binding.slug,
        binding.description,
        binding.action_type,
        binding.action_path ?? "",
        binding.connection_slug,
        binding.connector_slug ?? ""
      ]
        .join(" ")
        .toLowerCase();
      return (id.length > 0 && text.includes(id)) || (name !== "-" && text.includes(name));
    });
  }

  function withoutRecordKey<T>(record: Record<string, T>, key: string): Record<string, T> {
    const { [key]: _removed, ...rest } = record;
    return rest;
  }

  function recordString(record: WorkflowRuntimeRecord, keys: string[]): string {
    for (const key of keys) {
      const value = record[key];
      const text = valueText(value);
      if (text) return text;
    }
    return "-";
  }

  function valueText(value: unknown): string | null {
    if (typeof value === "string") {
      const trimmed = value.trim();
      return trimmed ? trimmed : null;
    }
    if (typeof value === "number" || typeof value === "boolean") return String(value);
    return null;
  }

  function bindingAction(binding: WorkflowBinding): string {
    if (binding.action_type === "file_append") return binding.action_path ?? "file append";
    if (binding.action_type === "run_workflow") return binding.action_path ?? "workflow runtime";
    return binding.action_type;
  }

  function connectorLabel(connector: WorkflowConnector): string {
    return connector.description || connector.connector_slug;
  }

  function connectionLabel(connection: WorkflowConnection): string {
    return connection.description || connection.slug;
  }

  function taskLabel(task: WorkflowMonitorTask): string {
    return task.subject || task.task_id;
  }
</script>

<div class="pf-pipe pf-pipe-editor">
  <div class="pf-pipe-top">
    <div class="pf-pipe-top-id">
      <strong>Workflows</strong>
      {#if !runtimeUnavailable}
        <span class="pf-pipe-hash">{workflows.length} runtime</span>
      {/if}
      {#if workflowQuery.trim()}
        <span class="pf-pipe-hash">{filteredWorkflows.length} shown</span>
      {/if}
      <span class="pf-pipe-save-note">Runtime workflows are managed by the configured backend.</span>
    </div>
    <div class="pf-pipe-top-right">
      <label class="pf-workflow-top-search">
        <span class="pf-connector-searchbox">
          <Icon name="search" size={12} />
          <input
            aria-label="Search workflows"
            value={workflowQuery}
            placeholder="Search workflows"
            oninput={(event) => (workflowQuery = event.currentTarget.value)}
          />
        </span>
      </label>
      <button
        type="button"
        class="sc-btn"
        data-variant="ghost"
        data-size="sm"
        disabled={loading}
        onclick={() => void refresh()}
      >
        <Icon name="refresh" size={12} />Refresh
      </button>
      <button
        type="button"
        class="sc-btn"
        data-variant="default"
        data-size="sm"
        disabled={runtimeActionBusy !== null}
        onclick={openCreateWorkflowDialog}
      >
        <Icon name="plus" size={12} />Create Automation
      </button>
    </div>
  </div>

  {#if createDialogOpen}
    <div class="pf-workflow-modal-backdrop" role="presentation">
      <div
        class="pf-workflow-create-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="pf-workflow-create-title"
        aria-describedby="pf-workflow-create-description"
      >
        <form
          onsubmit={(event) => {
            event.preventDefault();
            startAutomationDraftFromDialog();
          }}
        >
          <div class="pf-workflow-modal-head">
            <div>
              <h2 id="pf-workflow-create-title">Create Automation</h2>
              <p id="pf-workflow-create-description">
                Start an automation draft. The draft will be written to the runtime after the builder compiles an AgentEnv definition.
              </p>
            </div>
            <button
              type="button"
              class="sc-icon-btn"
              aria-label="Close create automation dialog"
              onclick={closeCreateWorkflowDialog}
            >
              <Icon name="x" size={14} />
            </button>
          </div>
          <div class="pf-workflow-create-fields">
            <label>
              <span>Name</span>
              <input
                aria-label="New automation name"
                value={createWorkflowName}
                placeholder="e.g., Daily report"
                oninput={(event) => {
                  createWorkflowName = event.currentTarget.value;
                  createWorkflowNameError = null;
                }}
              />
            </label>
            {#if createWorkflowNameError}
              <div class="pf-workflow-inline-error" role="alert">{createWorkflowNameError}</div>
            {/if}
            <label>
              <span>Description <em>optional</em></span>
              <input
                aria-label="New automation description"
                value={createWorkflowDescription}
                placeholder="What should this workflow do?"
                oninput={(event) => (createWorkflowDescription = event.currentTarget.value)}
              />
            </label>
          </div>
          <div class="pf-workflow-modal-actions">
            <button
              type="button"
              class="sc-btn"
              data-variant="ghost"
              data-size="sm"
              onclick={closeCreateWorkflowDialog}
            >
              Cancel
            </button>
            <button
              type="submit"
              class="sc-btn"
              data-variant="default"
              data-size="sm"
              disabled={runtimeActionBusy !== null || !createWorkflowName.trim()}
            >
              Start draft
            </button>
          </div>
        </form>
      </div>
    </div>
  {/if}

  {#if error}
    <div class="pf-workflow-alert" role="alert">{error}</div>
  {/if}
  {#if runtimeActionNotice}
    <div class="pf-workflow-alert pf-workflow-notice" role="status">{runtimeActionNotice}</div>
  {/if}

  {#if workflowScreen === "draft" && automationDraft}
    <div class="pf-workflow-detail-page" aria-label="Automation draft page">
      <div class="pf-workflow-detail-top">
        <button
          type="button"
          class="sc-btn"
          data-variant="ghost"
          data-size="sm"
          onclick={returnToWorkflowOverview}
        >
          <Icon name="chevL" size={13} />Back
        </button>
        <div class="pf-workflow-detail-title">
          <strong>{automationDraft.name}</strong>
          <small>local automation draft</small>
        </div>
      </div>

      <div class="pf-workflow-detail-grid">
        <section class="pf-workflow-panel pf-workflow-detail-panel" aria-label="Automation draft details">
          <div class="pf-workflow-panel-head">
            <div>
              <strong>Details</strong>
              <small>{automationDraft.description || "No description"}</small>
            </div>
          </div>
          <dl class="pf-workflow-metadata">
            <div>
              <dt>Name</dt>
              <dd>{automationDraft.name}</dd>
            </div>
            <div>
              <dt>Runtime</dt>
              <dd>Not created</dd>
            </div>
            <div>
              <dt>Definition</dt>
              <dd>Pending builder</dd>
            </div>
          </dl>
        </section>
      </div>
    </div>
  {:else if workflowScreen === "detail" && selectedWorkflow}
    <div class="pf-workflow-detail-page" aria-label="Workflow detail page">
      <div class="pf-workflow-detail-top">
        <button
          type="button"
          class="sc-btn"
          data-variant="ghost"
          data-size="sm"
          onclick={returnToWorkflowOverview}
        >
          <Icon name="chevL" size={13} />Back
        </button>
        <div class="pf-workflow-detail-title">
          <strong>{workflowName(selectedWorkflow)}</strong>
          <small>{workflowApiId(selectedWorkflow) ?? "runtime workflow"}</small>
        </div>
        <div class="pf-workflow-detail-actions">
          <button
            type="button"
            class="sc-btn"
            data-variant="ghost"
            data-size="sm"
            disabled={runtimeActionBusy !== null || !workflowApiId(selectedWorkflow)}
            aria-busy={runtimeActionBusy === `deploy:${workflowApiId(selectedWorkflow)}`}
            onclick={() => void deployWorkflow(selectedWorkflow, workflows.indexOf(selectedWorkflow))}
          >
            <Icon name="rocket" size={12} />Deploy
          </button>
          <button
            type="button"
            class="sc-btn"
            data-variant="ghost"
            data-size="sm"
            disabled={runtimeActionBusy !== null || !workflowApiId(selectedWorkflow)}
            aria-busy={runtimeActionBusy === `run:${workflowApiId(selectedWorkflow)}`}
            onclick={() => void runWorkflow(selectedWorkflow, workflows.indexOf(selectedWorkflow))}
          >
            <Icon name="play" size={12} />Run
          </button>
          <button
            type="button"
            class="sc-btn"
            data-variant="ghost"
            data-size="sm"
            disabled={loadingExecutionsFor !== null || !workflowApiId(selectedWorkflow)}
            aria-busy={loadingExecutionsFor === workflowApiId(selectedWorkflow)}
            onclick={() => void showExecutions(selectedWorkflow, workflows.indexOf(selectedWorkflow))}
          >
            <Icon name="logs" size={12} />Executions
          </button>
        </div>
      </div>

      <div class="pf-workflow-detail-grid">
        <section class="pf-workflow-panel pf-workflow-detail-panel" aria-label="Workflow details">
          <div class="pf-workflow-panel-head">
            <div>
              <strong>Details</strong>
              <small>{workflowDescription(selectedWorkflow) || "No description"}</small>
            </div>
          </div>
          <dl class="pf-workflow-metadata">
            {#each workflowMetadata(selectedWorkflow) as item (item.label)}
              <div>
                <dt>{item.label}</dt>
                <dd>{item.value}</dd>
              </div>
            {/each}
          </dl>
        </section>

        <section class="pf-workflow-panel pf-workflow-detail-panel" aria-label="Recent executions">
          <div class="pf-workflow-panel-head">
            <div>
              <strong>Recent Executions</strong>
              <small>{executionsForWorkflow(selectedWorkflow).length} loaded</small>
            </div>
          </div>
          <div class="pf-workflow-table">
            {#if executionErrorFor(selectedWorkflow)}
              <div class="pf-workflow-empty pf-workflow-runtime-error" role="alert">
                {executionErrorFor(selectedWorkflow)}
              </div>
            {:else if executionsForWorkflow(selectedWorkflow).length === 0}
              <div class="pf-workflow-empty">No executions loaded.</div>
            {:else}
              {#each executionsForWorkflow(selectedWorkflow) as execution, index (executionKey(execution, index))}
                <div class="pf-workflow-row pf-workflow-execution-row">
                  <span class="pf-run-pip {executionStatusClass(execution)}"></span>
                  <span class="pf-workflow-row-main">
                    <strong>{executionName(execution)}</strong>
                    <small>{executionStatus(execution)}</small>
                  </span>
                </div>
              {/each}
            {/if}
          </div>
        </section>

        <section class="pf-workflow-panel pf-workflow-detail-panel" aria-label="Related bindings">
          <div class="pf-workflow-panel-head">
            <div>
              <strong>Related Bindings</strong>
              <small>{relatedBindings(selectedWorkflow).length} matching</small>
            </div>
          </div>
          <div class="pf-workflow-table">
            {#if relatedBindings(selectedWorkflow).length === 0}
              <div class="pf-workflow-empty">No event bindings reference this workflow.</div>
            {:else}
              {#each relatedBindings(selectedWorkflow) as binding (binding.slug)}
                <div class="pf-workflow-row">
                  <span class="pf-run-pip {binding.enabled ? 'completed' : 'skipped'}"></span>
                  <span class="pf-workflow-row-main">
                    <strong>{binding.slug}</strong>
                    <small>{binding.connection_slug} -> {bindingAction(binding)}</small>
                  </span>
                  <span class="pf-workflow-row-state" data-enabled={binding.enabled}>{binding.status}</span>
                </div>
              {/each}
            {/if}
          </div>
        </section>

        <section class="pf-workflow-panel pf-workflow-detail-panel" aria-label="Runtime configuration">
          <details class="pf-workflow-disclosure">
            <summary>Configuration JSON</summary>
            <pre class="pf-workflow-json">{workflowRawJson(selectedWorkflow)}</pre>
          </details>
        </section>
      </div>
    </div>
  {:else}
  <div class="pf-workflow-overview" aria-label="Workflow overview">
    <section class="pf-workflow-panel pf-workflow-list-panel" aria-label="Runtime workflows">
      <div class="pf-workflow-panel-head">
        <div>
          <strong>Runtime Workflows</strong>
          {#if runtimeUnavailable}
            <small>Runtime unavailable</small>
          {:else}
            <small>{workflows.length} records</small>
          {/if}
        </div>
      </div>
      <div class="pf-workflow-table">
        {#if loading}
          <div class="pf-pipe-empty">Loading workflows...</div>
        {:else if runtimeUnavailable && workflows.length === 0}
          <div class="pf-workflow-empty pf-workflow-runtime-error" role="alert">
            Workflow runtime unavailable: {runtimeUnavailable}
          </div>
        {:else if filteredWorkflows.length === 0}
          <div class="pf-workflow-empty">
            {workflowQuery.trim() ? "No matching workflows." : "No runtime workflows returned by AgentEnv."}
          </div>
        {:else}
          {#each filteredWorkflows as item, index (workflowKey(item, index))}
            <div class="pf-workflow-row pf-workflow-runtime-row" data-selected={selectedWorkflowKey === workflowKey(item, index)}>
              <span class="pf-run-pip {workflowStatusClass(item)}"></span>
              <button
                type="button"
                class="pf-workflow-row-main pf-workflow-row-select"
                aria-label={`Select workflow ${workflowName(item)}`}
                aria-pressed={selectedWorkflowKey === workflowKey(item, index)}
                onclick={() => selectWorkflow(item, index)}
              >
                <strong>{workflowName(item)}</strong>
                <small>{recordString(item, ["id", "workflow_id", "workflowId", "slug"])}</small>
              </button>
              <span class="pf-workflow-row-stats" aria-label="Workflow metadata">
                <span class="pf-workflow-stat-inline">
                  <strong>{workflowStatus(item)}</strong>
                  <small>status</small>
                </span>
              </span>
              <span class="pf-workflow-row-actions">
                <button
                  type="button"
                  class="sc-btn"
                  data-variant="ghost"
                  data-size="sm"
                  disabled={runtimeActionBusy !== null || !workflowApiId(item)}
                  aria-busy={runtimeActionBusy === `deploy:${workflowApiId(item)}`}
                  onclick={() => void deployWorkflow(item, index)}
                >
                  <Icon name="rocket" size={12} />Deploy
                </button>
                <button
                  type="button"
                  class="sc-btn"
                  data-variant="ghost"
                  data-size="sm"
                  disabled={runtimeActionBusy !== null || !workflowApiId(item)}
                  aria-busy={runtimeActionBusy === `run:${workflowApiId(item)}`}
                  onclick={() => void runWorkflow(item, index)}
                >
                  <Icon name="play" size={12} />Run
                </button>
                <button
                  type="button"
                  class="sc-btn"
                  data-variant="ghost"
                  data-size="sm"
                  disabled={loadingExecutionsFor !== null || !workflowApiId(item)}
                  aria-busy={loadingExecutionsFor === workflowApiId(item)}
                  onclick={() => void showExecutions(item, index)}
                >
                  <Icon name="logs" size={12} />Executions
                </button>
              </span>
            </div>
          {/each}
        {/if}
      </div>
    </section>

    <section class="pf-workflow-panel" aria-label="Workflow bindings">
      <div class="pf-workflow-panel-head">
        <div>
          <strong>Puffer Event Bindings</strong>
          <small>{activeBindings}/{bindings.length} enabled</small>
        </div>
      </div>
      <div class="pf-workflow-table">
        {#if bindings.length === 0}
          <div class="pf-workflow-empty">No workflow bindings.</div>
        {:else}
          {#each bindings as binding (binding.slug)}
            <div class="pf-workflow-row">
              <span class="pf-run-pip {binding.enabled ? 'completed' : 'skipped'}"></span>
              <span class="pf-workflow-row-main">
                <strong>{binding.slug}</strong>
                <small>{binding.connection_slug} -> {bindingAction(binding)}</small>
              </span>
              <span class="pf-workflow-row-state" data-enabled={binding.enabled}>{binding.status}</span>
              <button
                type="button"
                class="sc-btn"
                data-variant="ghost"
                data-size="sm"
                disabled={togglingBindingSlug !== null || deletingBindingSlug !== null}
                aria-busy={togglingBindingSlug === binding.slug}
                onclick={() => void toggleBinding(binding)}
              >
                <Icon name={binding.enabled ? "pause2" : "play"} size={12} />{binding.enabled ? "Pause" : "Resume"}
              </button>
              <button
                type="button"
                class="sc-btn"
                data-variant="ghost"
                data-size="sm"
                disabled={togglingBindingSlug !== null || deletingBindingSlug !== null}
                aria-busy={deletingBindingSlug === binding.slug}
                onclick={() => void deleteBinding(binding)}
              >
                <Icon name="trash" size={12} />Delete
              </button>
            </div>
          {/each}
        {/if}
      </div>
    </section>

    <section class="pf-workflow-panel" aria-label="Connections and connectors">
      <div class="pf-workflow-panel-head">
        <div>
          <strong>Connections</strong>
          <small>{readyConnections}/{connections.length} trigger-ready</small>
        </div>
        <div>
          <strong>Connectors</strong>
          <small>{connectors.length} available</small>
        </div>
      </div>
      <div class="pf-workflow-table">
        {#if connections.length === 0}
          <div class="pf-workflow-empty">No connections configured.</div>
        {:else}
          {#each connections.slice(0, 8) as connection (connection.slug)}
            <div class="pf-workflow-row">
              <span class="pf-run-pip {connection.can_trigger_workflow ? 'completed' : 'skipped'}"></span>
              <span class="pf-workflow-row-main">
                <strong>{connection.slug}</strong>
                <small>{connectionLabel(connection)}</small>
              </span>
              <span class="pf-workflow-row-state" data-enabled={connection.can_trigger_workflow}>{connection.state}</span>
            </div>
          {/each}
        {/if}
      </div>
      {#if connectors.length > 0}
        <div class="pf-workflow-connector-summary" aria-label="Connector catalog">
          {#each connectors.slice(0, 12) as connector (connector.connector_slug)}
            <span class="pf-workflow-connector-pill" title={connectorLabel(connector)}>
              {connector.connector_slug}
            </span>
          {/each}
        </div>
      {/if}
    </section>

    {#if monitorTasks.length > 0}
      <section class="pf-workflow-panel" aria-label="Monitor tasks">
        <div class="pf-workflow-panel-head">
          <div>
            <strong>Monitor Tasks</strong>
            <small>{activeMonitorTasks}/{monitorTasks.length} active</small>
          </div>
        </div>
        <div class="pf-workflow-table">
          {#each monitorTasks.slice(0, 8) as task (task.task_id)}
            <div class="pf-workflow-row">
              <span class="pf-run-pip {task.ignored ? 'skipped' : 'running'}"></span>
              <span class="pf-workflow-row-main">
                <strong>{taskLabel(task)}</strong>
                <small>{task.monitor_connection ?? task.monitor_connector ?? task.task_id}</small>
              </span>
              <span class="pf-workflow-row-state" data-enabled={!task.ignored}>{task.status}</span>
            </div>
          {/each}
        </div>
      </section>
    {/if}
  </div>
  {/if}
</div>
