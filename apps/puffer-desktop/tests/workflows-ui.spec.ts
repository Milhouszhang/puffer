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

  const editor = page.getByLabel("Workflow editor page");
  await expect(editor).toContainText("Release workflow");
  await expect(editor.getByLabel("Workflow name")).toHaveValue("Release workflow");
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
  await expect(page.getByLabel("Workflow editor page")).toContainText("Release workflow");
});

test("workflow editor creates a runtime workflow with shared AgentEnv JSON", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: []
  });
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);

  const definition = {
    nodes: [
      {
        id: "telegram_trigger",
        type: "telegram_trigger",
        name: "Telegram Bot",
        config: { secretToken: "" },
        position: { x: 200, y: 150 }
      },
      {
        id: "telegram_message",
        type: "noop",
        name: "Send Message",
        config: {
          text: "{{ input.text }}",
          chat_id: "{{ input.chatId }}"
        },
        position: { x: 450, y: 150 }
      }
    ],
    edges: [{ source: "telegram_trigger", target: "telegram_message" }]
  };

  await page.locator(".pf-pipe-top-right").getByRole("button", { name: "Create Workflow" }).click();
  const dialog = page.getByRole("dialog", { name: "Create Workflow" });
  await expect(dialog).toBeVisible();
  await dialog.getByLabel("New workflow name").fill("Puffer editor workflow");
  await dialog.getByRole("button", { name: "Continue" }).click();

  await expect(page.getByLabel("Workflow editor page")).toContainText("New workflow draft");
  await page.getByRole("tab", { name: "JSON" }).click();
  await page.getByLabel("Workflow definition JSON").fill(JSON.stringify(definition, null, 2));
  await page.getByRole("button", { name: "Apply JSON" }).click();
  await page
    .locator(".pf-workflow-editor-page-actions")
    .getByRole("button", { name: "Create workflow", exact: true })
    .click();
  const created = await daemon.waitForRequest("workflow_create");
  expect(created.params.workflow).toMatchObject({
    name: "Puffer editor workflow",
    definition
  });
  expect(created.params.workflow).not.toHaveProperty("description");
  expect(created.params.workflow).not.toHaveProperty("id");
  expect(daemon.requests.some((request) => request.method === "workflow_open_ui")).toBe(false);
  await expect(page.getByRole("status")).toContainText("Created Puffer editor workflow");

  await page.getByRole("button", { name: "Back" }).click();
  const runtime = page.getByLabel("Runtime workflows");
  await expect(runtime).toContainText("Puffer editor workflow");
});

test("workflow editor saves draft and executes current JSON in memory", async ({ page }) => {
  const daemon = new FakeDaemon();
  const definition = {
    nodes: [
      {
        id: "manual_webhook",
        type: "webhook",
        name: "Manual webhook",
        config: { path: "manual", methods: ["POST"], authentication: "none" },
        position: { x: 160, y: 120 }
      }
    ],
    edges: []
  };
  daemon.setWorkflowSnapshot({
    workflows: [
      {
        id: "wf-editor",
        name: "Editor workflow",
        description: "Draft",
        status: "draft",
        definition,
        updatedAt: "2026-06-18T00:00:00Z"
      }
    ],
    runs: []
  });
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);
  await page
    .getByLabel("Runtime workflows")
    .getByRole("button", { name: "Select workflow Editor workflow" })
    .click();
  await page.getByRole("tab", { name: "JSON" }).click();
  const editedDefinition = {
    ...definition,
    nodes: [
      {
        ...definition.nodes[0],
        config: { path: "edited", methods: ["POST"], authentication: "none" }
      }
    ]
  };
  await page.getByLabel("Workflow definition JSON").fill(JSON.stringify(editedDefinition, null, 2));
  await page.getByRole("button", { name: "Apply JSON" }).click();

  await page.getByRole("button", { name: "Test run" }).click();
  const inMemory = await daemon.waitForRequest("workflow_execute_in_memory");
  expect(inMemory.params.request).toMatchObject({
    definition: editedDefinition,
    input: { source: "puffer-desktop" }
  });
  await expect(page.getByRole("status")).toContainText("Test run completed");

  await page.getByRole("tab", { name: "Visual" }).click();
  const nodeCard = page.getByLabel("Select node Manual webhook");
  const box = await nodeCard.boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(box!.x + 24, box!.y + 24);
  await page.mouse.down();
  await page.mouse.move(box!.x + 84, box!.y + 54);
  await page.mouse.up();

  await page.locator(".pf-workflow-canvas-toolbar").getByRole("button", { name: "Add Node" }).click();
  const addNodeDialog = page.getByRole("dialog", { name: "Add Node" });
  await expect(addNodeDialog).toBeVisible();
  await expect(addNodeDialog.getByLabel("Node categories").getByRole("button", { name: /Trigger/ })).toBeVisible();
  await addNodeDialog.locator(".pf-workflow-node-picker-list").getByRole("button", { name: /Webhook/ }).click();
  const sourcePort = page.getByLabel("Start connection from Manual webhook");
  const targetPort = page.getByLabel("Finish connection into Webhook");
  const sourceBox = await sourcePort.boundingBox();
  const targetBox = await targetPort.boundingBox();
  expect(sourceBox).not.toBeNull();
  expect(targetBox).not.toBeNull();
  await page.mouse.move(sourceBox!.x + sourceBox!.width / 2, sourceBox!.y + sourceBox!.height / 2);
  await page.mouse.down();
  await expect(targetPort).toBeEnabled();
  await page.mouse.move(targetBox!.x + targetBox!.width / 2, targetBox!.y + targetBox!.height / 2);
  await page.mouse.up();

  const movedConnectedDefinition = {
    ...editedDefinition,
    nodes: [
      {
        ...editedDefinition.nodes[0],
        position: { x: 220, y: 150 }
      },
      {
        id: "webhook",
        type: "webhook",
        name: "Webhook",
        config: { path: "puffer_webhook", methods: ["POST"], authentication: "none" },
        trusted: false,
        position: { x: 380, y: 240 }
      }
    ],
    edges: [{ source: "manual_webhook", target: "webhook" }]
  };

  await page.getByRole("tab", { name: "JSON" }).click();
  await expect(page.getByLabel("Workflow definition JSON")).toHaveValue(
    JSON.stringify(movedConnectedDefinition, null, 2)
  );

  await page.getByRole("button", { name: "Save draft" }).click();
  const saved = await daemon.waitForRequest("workflow_update");
  expect(saved.params).toMatchObject({
    workflowId: "wf-editor",
    workflow: {
      name: "Editor workflow",
      description: "Draft",
      definition: movedConnectedDefinition
    }
  });
});

test("workflow add node groups executor definitions by service", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: []
  });
  daemon.setWorkflowNodeDefinitions([
    {
      type: "github_create_issue_execute_action",
      category: "executor",
      name: "Create Issue",
      description: "Create a GitHub issue.",
      trusted: false,
      isBuiltin: false
    },
    {
      type: "github_update_issue_execute_action",
      category: "executor",
      name: "Update Issue",
      description: "Update a GitHub issue.",
      trusted: false,
      isBuiltin: false
    }
  ]);
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);
  await page.locator(".pf-pipe-top-right").getByRole("button", { name: "Create Workflow" }).click();
  const dialog = page.getByRole("dialog", { name: "Create Workflow" });
  await dialog.getByLabel("New workflow name").fill("Grouped node workflow");
  await dialog.getByRole("button", { name: "Continue" }).click();

  await page.locator(".pf-workflow-canvas-toolbar").getByRole("button", { name: "Add Node" }).click();
  const addNodeDialog = page.getByRole("dialog", { name: "Add Node" });
  await expect(addNodeDialog).toContainText("Executor");
  await expect(addNodeDialog).toContainText("GitHub");
  await expect(addNodeDialog).toContainText("2 functions");
  await expect(addNodeDialog).not.toContainText("github_create_issue_execute_action");

  await addNodeDialog.getByRole("button", { name: /GitHub/ }).click();
  await addNodeDialog.getByRole("button", { name: /Create Issue/ }).click();

  await expect(page.getByLabel("Workflow canvas")).toContainText("Create Issue");
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
