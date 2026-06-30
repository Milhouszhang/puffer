import { expect, test, type Page } from "@playwright/test";
import { FakeDaemon } from "./support/fakeDaemon";

async function openWorkflows(page: Page) {
  await page.locator(".pf-sidebar").getByRole("button", { name: "Workflows" }).click();
}

test("workflow overview lists and searches runtime workflow records", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [
      {
        id: "wf-release",
        name: "Release workflow",
        description: "Ships runtime releases",
        status: "active",
        deploymentStatus: "deployed",
        version: 7,
        source: "agentenv",
        updatedAt: "2026-06-18T00:00:00Z"
      },
      {
        id: "wf-digest",
        name: "Daily digest",
        status: "draft",
        updatedAt: "2026-06-17T00:00:00Z"
      }
    ],
    runs: [
      {
        id: "exec-release-1",
        workflowId: "wf-release",
        workflow_id: "wf-release",
        status: "completed",
        completedAt: "2026-06-18T00:05:00Z",
        input: { release: "1.2.3" },
        output: { ok: true }
      }
    ],
    workflow_bindings: [
      {
        slug: "release-on-telegram",
        description: "Run release workflow from Telegram",
        connection_slug: "telegram-user",
        connector_slug: "telegram-login",
        status: "enabled",
        enabled: true,
        action_type: "run_workflow",
        action_path: "wf-release"
      }
    ]
  });
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);

  const runtime = page.getByLabel("Runtime workflows");
  await expect(runtime).toContainText("Release workflow");
  await expect(runtime).toContainText("Daily digest");

  await runtime.getByRole("button", { name: "Select workflow Release workflow" }).click();

  const detail = page.getByLabel("Workflow detail page");
  await expect(detail).toContainText("Release workflow");
  await expect(detail.getByLabel("Workflow details")).toContainText("Ships runtime releases");
  await expect(page.getByRole("button", { name: "Back" })).toBeVisible();

  await page.getByRole("button", { name: "Back" }).click();

  await page.getByLabel("Search workflows").fill("release active");

  await expect(runtime).toContainText("Release workflow");
  await expect(runtime).not.toContainText("Daily digest");
});

test("workflow runtime rows call AgentEnv runtime actions", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [
      {
        id: "wf-release",
        name: "Release workflow",
        status: "draft",
        updatedAt: "2026-06-18T00:00:00Z"
      }
    ],
    runs: [
      {
        id: "exec-release-seeded",
        workflowId: "wf-release",
        status: "completed",
        completedAt: "2026-06-18T00:01:00Z"
      }
    ]
  });
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);

  const runtime = page.getByLabel("Runtime workflows");
  const row = runtime.locator(".pf-workflow-runtime-row", { hasText: "Release workflow" });

  await row.getByRole("button", { name: "Deploy" }).click();
  const deployed = await daemon.waitForRequest("workflow_deploy");
  expect(deployed.params).toMatchObject({ workflowId: "wf-release" });
  await expect(page.getByRole("status")).toContainText("Deployed Release workflow");
  await page.getByRole("button", { name: "Back" }).click();
  await expect(row).toContainText("active");

  await row.getByRole("button", { name: "Run" }).click();
  const executed = await daemon.waitForRequest("workflow_execute");
  expect(executed.params).toMatchObject({ workflowId: "wf-release" });
  const autoListed = await daemon.waitForRequest("workflow_list_executions");
  expect(autoListed.params).toMatchObject({ workflowId: "wf-release" });
  await expect(page.getByRole("status")).toContainText("Started Release workflow run exec-wf-release-2");
  await page.getByRole("button", { name: "Back" }).click();

  await row.getByRole("button", { name: "Executions" }).click();
  const listed = await daemon.waitForRequest("workflow_list_executions");
  expect(listed.params).toMatchObject({ workflowId: "wf-release" });
  await expect(page.getByLabel("Workflow detail page")).toContainText("Release workflow");
});

test("workflow create flow starts a local automation draft", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: []
  });
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);

  await page.locator(".pf-pipe-top-right").getByRole("button", { name: "Create Automation" }).click();
  const dialog = page.getByRole("dialog", { name: "Create Automation" });
  await expect(dialog).toBeVisible();
  await dialog.getByLabel("New automation name").fill("Puffer automation");
  await dialog.getByLabel("New automation description").fill("Linear builder placeholder");
  await dialog.getByRole("button", { name: "Start draft", exact: true }).click();

  const draft = page.getByLabel("Automation draft page");
  await expect(draft).toContainText("Puffer automation");
  await expect(draft.getByLabel("Automation draft details")).toContainText("Not created");
  await expect(draft.getByLabel("Automation draft details")).toContainText("Pending builder");
  expect(daemon.requests.some((request) => request.method === "workflow_create")).toBe(false);
  expect(daemon.requests.some((request) => request.method === "workflow_open_ui")).toBe(false);
  expect(daemon.requests.some((request) => request.method === "workflow_node_definitions")).toBe(false);

  await page.getByRole("button", { name: "Back" }).click();
  const runtime = page.getByLabel("Runtime workflows");
  await expect(runtime).not.toContainText("Puffer automation");
});

test("workflow runtime unavailable renders an explicit error", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: [],
    workflow_error: "workflow methods require the Puffer daemon and an AgentEnv workflow runtime"
  });
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);

  const runtime = page.getByLabel("Runtime workflows");
  await expect(runtime).toContainText("Workflow runtime unavailable");
  await expect(runtime).toContainText("AgentEnv workflow runtime");
  await expect(runtime).not.toContainText("No matching workflows");
  await expect(page.locator(".pf-pipe-top-id")).not.toContainText("Workflows 0");
});

test("workflow overview toggles and deletes Puffer event bindings", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: [],
    workflow_bindings: [
      {
        slug: "support-alerts",
        description: "Append support alerts",
        connection_slug: "telegram-user",
        connector_slug: "telegram-login",
        status: "enabled",
        enabled: true,
        action_type: "file_append",
        action_path: "/tmp/support-alerts.log"
      }
    ]
  });
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);

  const bindings = page.getByLabel("Workflow bindings");
  await expect(bindings).toContainText("support-alerts");
  await bindings.getByRole("button", { name: "Pause" }).click();

  const toggle = await daemon.waitForRequest("workflow_toggle");
  expect(toggle.params).toMatchObject({ slug: "support-alerts", enabled: false });
  await expect(bindings).toContainText("paused");

  await bindings.getByRole("button", { name: "Delete" }).click();
  const deleted = await daemon.waitForRequest("workflow_binding_delete");
  expect(deleted.params).toMatchObject({ slug: "support-alerts" });
  await expect(bindings).not.toContainText("support-alerts");
});

test("workflow overview keeps connector and monitor context", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: [],
    connections: [
      {
        slug: "telegram-user",
        connector_slug: "telegram-login",
        description: "Personal Telegram account",
        state: "active",
        has_consumer: true,
        can_trigger_workflow: true
      }
    ],
    connectors: [
      {
        connector_slug: "telegram-login",
        description: "Telegram personal account",
        skill: "telegram",
        requires_auth: true,
        can_subscribe: true,
        can_proxy_agent: false,
        can_trigger_workflow: true,
        action_slugs: ["send_message"]
      }
    ],
    monitor_tasks: [
      {
        task_id: "monitor-1",
        subject: "Reply to support ping",
        description: "Customer asked for status.",
        status: "pending",
        monitor_connection: "telegram-user"
      }
    ]
  });
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);

  await expect(page.getByLabel("Connections and connectors")).toContainText("telegram-user");
  await expect(page.getByLabel("Connections and connectors")).toContainText("telegram-login");
  await expect(page.getByLabel("Monitor tasks")).toContainText("Reply to support ping");
});

test("fake daemon supports workflow runtime RPC records", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  const result = await page.evaluate(async (url) => {
    const socket = new WebSocket(url);
    await new Promise<void>((resolve, reject) => {
      socket.onopen = () => resolve();
      socket.onerror = () => reject(new Error("fake daemon websocket failed"));
    });

    let nextId = 1;
    async function rpc(method: string, params: Record<string, unknown>) {
      const id = String(nextId);
      nextId += 1;
      return new Promise<Record<string, unknown> | Record<string, unknown>[]>((resolve, reject) => {
        const onMessage = (event: MessageEvent<string>) => {
          const message = JSON.parse(event.data) as {
            id?: string | number;
            ok?: boolean;
            result?: Record<string, unknown> | Record<string, unknown>[];
            error?: string;
          };
          if (String(message.id) !== id) return;
          socket.removeEventListener("message", onMessage);
          if (message.ok === false) {
            reject(new Error(message.error ?? "fake daemon rpc failed"));
            return;
          }
          resolve(message.result ?? {});
        };
        socket.addEventListener("message", onMessage);
        socket.send(JSON.stringify({ type: "request", id, method, params }));
      });
    }

    const created = await rpc("workflow_create", {
      workflow: { id: "wf-runtime", name: "Runtime API", definition: { nodes: [], edges: [] } }
    });
    const updated = await rpc("workflow_update", {
      workflowId: "wf-runtime",
      workflow: { name: "Runtime API edited", definition: { nodes: [], edges: [] } }
    });
    const nodeDefinitions = await rpc("workflow_node_definitions", {});
    const nodeDefinition = await rpc("workflow_node_definition", { type: "webhook" });
    const deployed = await rpc("workflow_deploy", { workflowId: "wf-runtime" });
    const undeployed = await rpc("workflow_undeploy", { workflowId: "wf-runtime" });
    const executed = await rpc("workflow_execute", {
      workflowId: "wf-runtime",
      request: { input: { ok: true } }
    });
    const inMemory = await rpc("workflow_execute_in_memory", {
      request: { definition: { nodes: [], edges: [] }, input: { ok: true } }
    });
    const executions = await rpc("workflow_list_executions", { workflowId: "wf-runtime" });
    const executionId = String((executed as Record<string, unknown>).id ?? "");
    const fetched = await rpc("workflow_get_execution", {
      workflowId: "wf-runtime",
      executionId
    });
    const opened = await rpc("workflow_open_ui", {});
    socket.close();

    return {
      created,
      updated,
      nodeDefinitions,
      nodeDefinition,
      deployed,
      undeployed,
      executed,
      inMemory,
      executions,
      fetched,
      opened
    };
  }, daemon.url);

  expect(result.created).toMatchObject({ id: "wf-runtime", name: "Runtime API", status: "draft" });
  expect(result.updated).toMatchObject({ id: "wf-runtime", name: "Runtime API edited" });
  expect(result.nodeDefinitions).toEqual(
    expect.arrayContaining([expect.objectContaining({ type: "webhook" })])
  );
  expect(result.nodeDefinition).toMatchObject({ type: "webhook", schemas: { config: expect.any(Object) } });
  expect(result.deployed).toMatchObject({ id: "wf-runtime", status: "active" });
  expect(result.undeployed).toMatchObject({ id: "wf-runtime", status: "draft" });
  expect(result.executed).toMatchObject({ workflowId: "wf-runtime", status: "completed" });
  expect(result.inMemory).toMatchObject({ status: "completed", nodeOutputs: {} });
  expect(result.executions).toHaveLength(1);
  expect(result.fetched).toMatchObject({ workflowId: "wf-runtime", status: "completed" });
  expect(result.opened).toMatchObject({ url: "http://localhost:5173/workflows", opened: true });
});
