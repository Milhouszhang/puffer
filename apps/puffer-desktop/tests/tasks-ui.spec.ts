import { expect, test, type Page } from "@playwright/test";
import { FakeDaemon } from "./support/fakeDaemon";

async function openTasks(page: Page) {
  await page.locator(".pf-sidebar").getByRole("button", { name: "Tasks" }).click();
}

function monitorRulesSnapshot(overrides: Record<string, unknown> = {}) {
  return {
    workflows: [],
    runs: [],
    connections: [
      {
        slug: "telegram-user",
        connector_slug: "telegram-login",
        description: "Personal Telegram",
        state: "active",
        has_consumer: true,
        auth_failure_notified: false,
        can_trigger_workflow: true,
        connect_command: "/connect telegram-login telegram-user",
        monitor_command: "/monitor telegram-user"
      }
    ],
    workflow_bindings: [
      {
        slug: "monitor-telegram-user",
        description: "Monitor telegram-user for actionable tasks",
        connection_slug: "telegram-user",
        connector_slug: "telegram-login",
        status: "enabled",
        enabled: true,
        action_type: "triage_agent",
        monitor: true,
        monitor_memory_path: "/tmp/telegram-user.md",
        contact_ids: [],
        ...overrides
      }
    ],
    monitor_memories: [
      {
        connection_slug: "telegram-user",
        path: "/tmp/telegram-user.md",
        content: "# Monitor Memory: telegram-user\n",
        truncated: false
      }
    ],
    monitor_tasks: []
  };
}

async function openRulesDialog(page: Page, daemon: FakeDaemon) {
  await daemon.install(page);
  await daemon.open(page);
  await openTasks(page);
  await page.getByRole("button", { name: "Configure" }).click();
  await daemon.waitForRequest("contacts_list");
  const dialog = page.getByRole("dialog", { name: "Task settings" });
  await dialog.getByRole("button", { name: "Rules and memory" }).click();
  return dialog;
}

test("tasks history shows received monitor messages and agent outcomes", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openTasks(page);
  await page.getByRole("button", { name: "History" }).click();
  await daemon.waitForRequest("task_monitor_history_list");

  const dialog = page.getByRole("dialog", { name: "Task history" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByLabel("Received messages").getByRole("button", {
    name: /Telegram from Alice: deployment status/
  })).toBeVisible();
  await expect(dialog.getByLabel("Received messages").getByRole("button", {
    name: /Telegram from Alice: thanks/
  })).toBeVisible();
  await expect(dialog.getByLabel("Agent history")).toContainText("Shared digest outcome for message 1 of 2");
  await expect(dialog.getByLabel("Agent history")).toContainText("Digest triage");
  await expect(dialog.getByLabel("Agent history")).toContainText("Created monitor task monitor-1.");
  await expect(dialog.getByLabel("Agent history")).toContainText("38 tokens");
  await expect(dialog.getByLabel("Agent history")).toContainText("No ignored task or ignore analysis");
});

test("tasks list lazily renders large snapshots", async ({ page }) => {
  const daemon = new FakeDaemon();
  const now = Date.now();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: [],
    connections: [],
    connectors: [],
    workflow_bindings: [],
    monitor_tasks: Array.from({ length: 65 }, (_, index) => ({
      task_id: `monitor-lazy-${index}`,
      subject: `Lazy task ${index}`,
      description: `Task ${index} should not render until its batch is loaded.`,
      status: "pending",
      source: "monitor",
      monitor_connection: "telegram-user",
      monitor_connector: "telegram-login",
      started_at_ms: now - index,
      updated_at_ms: now - index
    })),
    monitor_task_error: null
  });
  await daemon.install(page);
  await daemon.open(page);

  await openTasks(page);
  await daemon.waitForRequest("workflow_list");

  const list = page.getByLabel("Task list");
  await expect(list.locator(".pf-task-row")).toHaveCount(40);
  await expect(list).not.toContainText("Lazy task 64");
  await list.getByRole("button", { name: "Load 25 more tasks" }).click();
  await expect(list.locator(".pf-task-row")).toHaveCount(65);
  await expect(list).toContainText("Lazy task 64");
});

test("tasks history links ignored task analysis by monitor envelope", async ({ page }) => {
  const daemon = new FakeDaemon();
  const now = Date.now();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: [],
    connections: [],
    connectors: [],
    workflow_bindings: [],
    monitor_tasks: [
      {
        task_id: "monitor-ignored",
        subject: "Ignore noisy sender",
        description: "This sender is not actionable.",
        status: "completed",
        source: "monitor",
        monitor_connection: "telegram-user",
        monitor_connector: "telegram-login",
        monitor_envelope_id: "env-ignore",
        ignored: true,
        ignore_reason: "duplicate bot alert",
        ignore_analysis_started: true,
        ignore_analysis_status: "completed",
        ignore_analysis_result: "The filter is scoped to the room and sender.",
        ignore_analysis_usage: {
          input_tokens: 20,
          output_tokens: 5,
          cache_read_tokens: 4,
          spent_tokens: 21
        },
        started_at_ms: now - 20_000,
        updated_at_ms: now - 10_000
      }
    ],
    monitor_task_error: null
  });
  daemon.setMonitorHistory({
    messages: [
      {
        idx: 7,
        run_id: "run-ignore",
        workflow_slug: "monitor-telegram-user",
        connection_slug: "telegram-user",
        connector_slug: "telegram-login",
        envelope_id: "env-ignore",
        received_at_ms: now - 21_000,
        topic: "telegram-user",
        kind: "message",
        summary: "Telegram from bot: duplicate alert",
        text: "duplicate alert",
        payload: { chat_title: "Ops", sender_username: "bot", message: "duplicate alert" },
        action_log: [
          {
            action: "monitor_ignore_filter",
            status: "completed",
            summary: "Matched an installed monitor ignore filter before triage.",
            started_at_ms: now - 21_000,
            ended_at_ms: now - 21_000,
            usage: null
          }
        ],
        status: "completed",
        started_at_ms: now - 21_000,
        ended_at_ms: now - 21_000
      }
    ]
  });
  await daemon.install(page);
  await daemon.open(page);

  await openTasks(page);
  await page.getByRole("button", { name: "History" }).click();

  const dialog = page.getByRole("dialog", { name: "Task history" });
  await expect(dialog).toContainText("Ignore filter");
  await expect(dialog).toContainText("duplicate bot alert");
  await expect(dialog).toContainText("The filter is scoped to the room and sender.");
  await expect(dialog).toContainText("21 tokens");
});

test("tasks history shows monitor messages while triage is still processing", async ({ page }) => {
  const daemon = new FakeDaemon();
  const now = Date.now();
  daemon.setMonitorHistory({
    messages: [
      {
        idx: 12,
        run_id: "run-processing",
        workflow_slug: "monitor-telegram-user",
        connection_slug: "telegram-user",
        connector_slug: "telegram-login",
        envelope_id: "env-processing",
        received_at_ms: now - 2_000,
        topic: "telegram-user",
        kind: "message",
        summary: "Telegram from Alice: can you check this?",
        text: "can you check this?",
        payload: { chat_title: "Support", sender_username: "alice", message: "can you check this?" },
        action_log: [
          {
            action: "triage_agent",
            status: "running",
            summary: "triage agent is processing this message.",
            started_at_ms: now - 1_000,
            ended_at_ms: now - 1_000,
            usage: null
          }
        ],
        status: "running",
        started_at_ms: now - 1_000,
        ended_at_ms: now - 1_000
      }
    ]
  });
  await daemon.install(page);
  await daemon.open(page);

  await openTasks(page);
  await page.getByRole("button", { name: "History" }).click();

  const dialog = page.getByRole("dialog", { name: "Task history" });
  await expect(dialog.getByLabel("Received messages")).toContainText("Telegram from Alice");
  await expect(dialog.getByLabel("Agent history")).toContainText("processing · tokens n/a");
  await expect(dialog.getByLabel("Agent history")).toContainText("triage agent is processing this message.");
});

test("task monitor configuration uses the design primary add button", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: [],
    connections: [
      {
        slug: "lark-user",
        connector_slug: "lark-login",
        description: "Lark connection backed by user login",
        state: "active",
        has_consumer: true,
        auth_failure_notified: false,
        can_trigger_workflow: true,
        connect_command: "/connect lark-login lark-user",
        monitor_command: null
      }
    ],
    workflow_bindings: [],
    monitor_tasks: []
  });
  await daemon.install(page);
  await daemon.open(page);

  await openTasks(page);
  await page.getByRole("button", { name: "Configure" }).click();
  await daemon.waitForRequest("contacts_list");

  const dialog = page.getByRole("dialog", { name: "Task settings" });
  const addButtonMetrics = await dialog.getByRole("button", { name: "Add" }).evaluate((element) => {
    const styles = getComputedStyle(element);
    const box = element.getBoundingClientRect();
    return {
      width: Math.round(box.width),
      height: Math.round(box.height),
      borderRadius: styles.borderRadius,
      backgroundColor: styles.backgroundColor,
      color: styles.color
    };
  });
  expect(addButtonMetrics).toEqual({
    width: 48,
    height: 30,
    borderRadius: "6px",
    backgroundColor: "rgb(17, 17, 17)",
    color: "rgb(255, 255, 255)"
  });
  const addButton = dialog.getByRole("button", { name: "Add" });
  await expect(addButton).toBeEnabled();
  await addButton.click();
  const request = await daemon.waitForRequest("task_monitor_create");
  expect(request.params).toEqual({
    connection_slug: "lark-user",
    model: null,
    contact_ids: []
  });
});

test("task monitor configuration reconnects a degraded selected account", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: [],
    connections: [
      {
        slug: "telegram-user",
        connector_slug: "telegram-login",
        description: "Personal Telegram",
        state: "degraded",
        has_consumer: false,
        auth_failure_notified: true,
        can_trigger_workflow: true,
        connect_command: "/connect telegram-login telegram-user",
        monitor_command: "/monitor telegram-user"
      }
    ],
    workflow_bindings: [],
    monitor_tasks: []
  });
  await daemon.install(page);
  await daemon.open(page);

  await openTasks(page);
  await page.getByRole("button", { name: "Configure" }).click();
  await daemon.waitForRequest("contacts_list");

  const dialog = page.getByRole("dialog", { name: "Task settings" });
  await expect(dialog).toContainText("telegram-user needs auth repair");
  const reconnectButton = dialog.getByRole("button", { name: "Reconnect" });
  await expect(reconnectButton).toBeEnabled();
  await reconnectButton.click();

  const request = await daemon.waitForRequest("run_agent_turn");
  expect(request.params.message).toBe("/connect telegram-login telegram-user");
  expect(daemon.requests.some((item) => item.method === "task_monitor_create")).toBe(false);
});

test("task monitor rules renders condition groups and saves memory", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot({
    workflows: [],
    runs: [],
    connections: [
      {
        slug: "telegram-user",
        connector_slug: "telegram-login",
        description: "Personal Telegram",
        state: "active",
        has_consumer: true,
        auth_failure_notified: false,
        can_trigger_workflow: true,
        connect_command: "/connect telegram-login telegram-user",
        monitor_command: "/monitor telegram-user"
      }
    ],
    workflow_bindings: [
      {
        slug: "monitor-telegram-user",
        description: "Monitor telegram-user for actionable tasks",
        connection_slug: "telegram-user",
        connector_slug: "telegram-login",
        status: "enabled",
        enabled: true,
        action_type: "triage_agent",
        monitor: true,
        monitor_memory_path: "/tmp/telegram-user.md",
        contact_ids: []
      }
    ],
    monitor_memories: [
      {
        connection_slug: "telegram-user",
        path: "/tmp/telegram-user.md",
        content: "# Monitor Memory: telegram-user\n",
        truncated: false
      }
    ],
    monitor_tasks: []
  });
  await daemon.install(page);
  await daemon.open(page);

  await openTasks(page);
  await page.getByRole("button", { name: "Configure" }).click();
  await daemon.waitForRequest("contacts_list");

  const dialog = page.getByRole("dialog", { name: "Task settings" });
  await dialog.getByRole("button", { name: "Rules and memory" }).click();

  const includeGroup = dialog.getByRole("group", { name: "Only create tasks when" });
  const excludeGroup = dialog.getByRole("group", { name: "Skip tasks when" });
  await expect(includeGroup.getByText("No required conditions")).toBeVisible();
  await expect(excludeGroup.getByText("No skip conditions")).toBeVisible();
  await expect(includeGroup.getByRole("button", { name: "Add task condition" })).toBeEnabled();
  await expect(excludeGroup.getByRole("button", { name: "Add skip condition" })).toBeEnabled();

  const saveMemoryButton = dialog.getByRole("button", { name: "Save memory" });
  await expect(saveMemoryButton).toBeEnabled();

  await saveMemoryButton.click();
  const request = await daemon.waitForRequest("task_monitor_memory_save");
  expect(request.params).toEqual({
    connection_slug: "telegram-user",
    content: "# Monitor Memory: telegram-user\n"
  });
});

test("task monitor rules adds a skip condition from the builder", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot(monitorRulesSnapshot());

  const dialog = await openRulesDialog(page, daemon);
  const excludeGroup = dialog.getByRole("group", { name: "Skip tasks when" });
  await excludeGroup.getByRole("button", { name: "Add skip condition" }).click();

  await excludeGroup.getByLabel("Value").fill("作业");
  await excludeGroup.getByRole("button", { name: "Add condition" }).click();

  const request = await daemon.waitForRequest("task_monitor_rule_add");
  expect(request.params).toEqual({
    connection_slug: "telegram-user",
    mode: "exclude",
    kind: "keyword",
    keywords: ["作业"],
    operator: "contains",
    case_insensitive: true
  });
  await expect(
    excludeGroup.getByRole("button", { name: "Remove skip condition Message text contains 作业" })
  ).toBeVisible();
});

test("task monitor rules adds a task condition from the builder", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot(monitorRulesSnapshot());

  const dialog = await openRulesDialog(page, daemon);
  const includeGroup = dialog.getByRole("group", { name: "Only create tasks when" });
  await includeGroup.getByRole("button", { name: "Add task condition" }).click();

  await includeGroup.getByLabel("Value").fill("urgent");
  await includeGroup.getByRole("button", { name: "Add condition" }).click();

  const request = await daemon.waitForRequest("task_monitor_rule_add");
  expect(request.params).toEqual({
    connection_slug: "telegram-user",
    mode: "include",
    kind: "keyword",
    keywords: ["urgent"],
    operator: "contains",
    case_insensitive: true
  });
  await expect(
    includeGroup.getByRole("button", { name: "Remove task condition Message text contains urgent" })
  ).toBeVisible();
});

test("task monitor rules exposes exactly two condition groups", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot(monitorRulesSnapshot());

  const dialog = await openRulesDialog(page, daemon);
  await expect(dialog.locator(".pf-monitor-rule-group")).toHaveCount(2);
  await expect(dialog.getByRole("group", { name: "Only create tasks when" })).toBeVisible();
  await expect(dialog.getByRole("group", { name: "Skip tasks when" })).toBeVisible();
});

test("task monitor rules renders installed include and exclude condition chips", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot(monitorRulesSnapshot({
    include_filters: [
      { type: "regex", pattern: "作业", case_insensitive: true }
    ],
    ignore_filters: [
      { type: "regex", pattern: "广告|通知", case_insensitive: true }
    ]
  }));

  const dialog = await openRulesDialog(page, daemon);
  const includeGroup = dialog.getByRole("group", { name: "Only create tasks when" });
  const excludeGroup = dialog.getByRole("group", { name: "Skip tasks when" });
  await expect(includeGroup.getByRole("button", { name: "Remove task condition Message text contains 作业" })).toBeVisible();
  await expect(excludeGroup.getByRole("button", { name: "Remove skip condition Message text contains 广告, 通知" })).toBeVisible();
});

test("task monitor rules hides regex case-insensitive wrappers in field chips", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot(monitorRulesSnapshot({
    ignore_filters: [
      { type: "jq", expression: ".sender_name | test(\"(?i:john)\")" }
    ],
    monitor_rule_schema: {
      version: 1,
      event_source: "telegram-user",
      text_fields: [],
      fields: [
        {
          path: "sender_name",
          label: "Sender name",
          type: "string",
          operators: ["contains", "equals", "matches"]
        }
      ]
    }
  }));

  const dialog = await openRulesDialog(page, daemon);
  const excludeRow = dialog.getByRole("group", { name: "Skip tasks when" });
  await expect(excludeRow.getByRole("button", { name: "Remove skip condition Sender name contains john" })).toBeVisible();
  await expect(excludeRow).not.toContainText("(?i:");
});

test("task monitor rules removes one saved condition chip without deleting siblings", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowSnapshot(monitorRulesSnapshot({
    ignore_filters: [
      { type: "regex", pattern: "作业", case_insensitive: true },
      { type: "regex", pattern: "广告", case_insensitive: true }
    ]
  }));

  const dialog = await openRulesDialog(page, daemon);
  const excludeGroup = dialog.getByRole("group", { name: "Skip tasks when" });
  await excludeGroup.getByRole("button", { name: "Remove skip condition Message text contains 广告" }).click();

  const deleteRequest = await daemon.waitForRequest("task_monitor_rule_delete");
  expect(deleteRequest.params).toEqual({
    connection_slug: "telegram-user",
    mode: "exclude",
    rule: { type: "regex", pattern: "广告", case_insensitive: true }
  });
  await expect(excludeGroup.getByRole("button", { name: "Remove skip condition Message text contains 作业" })).toBeVisible();
  await expect(excludeGroup.getByRole("button", { name: "Remove skip condition Message text contains 广告" })).toBeHidden();
});

test("task monitor configuration scopes subscriptions to selected contacts", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openTasks(page);
  await page.getByRole("button", { name: "Configure" }).click();
  await daemon.waitForRequest("contacts_list");

  const dialog = page.getByRole("dialog", { name: "Task settings" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Contacts" }).click();
  await dialog.getByRole("checkbox", { name: /Alice/ }).check();

  const aliceCheckboxMetrics = await dialog.getByRole("checkbox", { name: /Alice/ }).evaluate((element) => {
    const styles = getComputedStyle(element);
    return {
      width: styles.width,
      height: styles.height,
      borderRadius: styles.borderRadius,
      backgroundColor: styles.backgroundColor
    };
  });
  expect(aliceCheckboxMetrics).toEqual({
    width: "16px",
    height: "16px",
    borderRadius: "4px",
    backgroundColor: "rgb(22, 22, 22)"
  });

  const editButtonMetrics = await dialog.getByRole("button", { name: "Edit Alice" }).evaluate((element) => {
    const styles = getComputedStyle(element);
    const box = element.getBoundingClientRect();
    return {
      width: Math.round(box.width),
      height: Math.round(box.height),
      borderRadius: styles.borderRadius,
      backgroundColor: styles.backgroundColor
    };
  });
  expect(editButtonMetrics).toEqual({
    width: 30,
    height: 30,
    borderRadius: "999px",
    backgroundColor: "rgb(244, 244, 244)"
  });

  await dialog.getByRole("button", { name: "Monitor" }).click();
  await dialog.getByRole("button", { name: /^Update$/ }).click();

  const request = await daemon.waitForRequest("task_monitor_create");
  expect(request.params.contact_ids).toEqual(["google@alice@example.com", "telegram@alice"]);
  await expect(page.locator(".pf-tasks-title")).toContainText("2 contact ids", { timeout: 5_000 });
});

test("task monitor configuration keeps bottom content reachable when it overflows", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setContactsSnapshot({
    contacts: Array.from({ length: 40 }, (_, index) => ({
      id: `contact-${index}`,
      name: `Contact ${index + 1}`,
      description: `Fixture contact ${index + 1}`,
      avatar: null,
      contact_ids: [`telegram@contact_${index}`]
    })),
    candidates: []
  });
  await page.setViewportSize({ width: 900, height: 560 });
  await daemon.install(page);
  await daemon.open(page);

  await openTasks(page);
  await page.getByRole("button", { name: "Configure" }).click();
  await daemon.waitForRequest("contacts_list");

  const dialog = page.getByRole("dialog", { name: "Task settings" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Contacts" }).click();

  const pane = page.locator(".pf-task-settings-pane");
  const metrics = await pane.evaluate((element) => ({
    clientHeight: element.clientHeight,
    scrollHeight: element.scrollHeight,
    overflowY: getComputedStyle(element).overflowY
  }));
  expect(metrics.overflowY).toBe("auto");
  expect(metrics.scrollHeight).toBeGreaterThan(metrics.clientHeight);

  await pane.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });

  const bottomReachable = await pane.evaluate((element) => {
    const rows = element.querySelectorAll(".pf-task-contact-table-row");
    const lastRow = rows[rows.length - 1];
    if (!lastRow) return false;
    const paneBox = element.getBoundingClientRect();
    const rowBox = lastRow.getBoundingClientRect();
    return rowBox.bottom <= paneBox.bottom + 1;
  });
  expect(bottomReachable).toBe(true);
  await expect(dialog).toContainText("Contact 40");
});
