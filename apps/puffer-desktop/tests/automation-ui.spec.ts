import { expect, test } from "@playwright/test";
import { FakeDaemon } from "./support/fakeDaemon";

async function openAutomation(page: import("@playwright/test").Page): Promise<void> {
  await page.locator(".pf-sidebar").getByRole("button", { name: "Automation" }).click();
}

async function backgroundLightnessGap(locator: import("@playwright/test").Locator): Promise<number> {
  return locator.evaluate((tablist) => {
    const selectedTab = tablist.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
    if (!selectedTab) return 0;

    const lightness = (color: string): number => {
      const oklch = color.match(/oklch\(([\d.]+)/);
      if (oklch) return Number(oklch[1]);

      const oklab = color.match(/oklab\(([\d.]+)%?/);
      if (oklab) {
        const value = Number(oklab[1]);
        return color.includes("%") ? value / 100 : value;
      }

      const rgb = color.match(/rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)/);
      if (rgb) {
        const [red, green, blue] = rgb.slice(1, 4).map(Number);
        return (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255;
      }

      const srgb = color.match(/color\(srgb\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)/);
      if (srgb) {
        const [red, green, blue] = srgb.slice(1, 4).map(Number);
        return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
      }

      return 0;
    };

    const containerLightness = lightness(getComputedStyle(tablist).backgroundColor);
    const selectedLightness = lightness(getComputedStyle(selectedTab).backgroundColor);
    return Math.abs(selectedLightness - containerLightness);
  });
}

const richAutomationSpec = {
  spec_version: 1,
  name: "Rich automation",
  description: "Preserve connector filters and loop flow.",
  source: { type: "template", template_id: "rich-template" },
  instructions: "Preserve this rich automation.",
  triggers: [
    {
      type: "puffer_connection",
      id: "incoming",
      connection_slug: "telegram-user",
      connector_slug: "telegram-login",
      filter: { pattern: "urgent" },
      ignore_filters: [{ pattern: "ignore" }],
      contact_ids: ["telegram-user-id@1"],
      summary: "Telegram incoming"
    }
  ],
  flow: {
    steps: [
      {
        type: "loop",
        id: "review-loop",
        loop: {
          mode: "repeat",
          input: { type: "trigger" },
          stop_when: { type: "output_equals", path: "done", value: true },
          max_iterations: 3
        },
        body: {
          steps: [
            {
              type: "agent_env_node",
              id: "rich-node",
              node: {
                node_type: "custom_node",
                name: "Custom node",
                trusted: true,
                config: { keep: "this" }
              },
              summary: "Custom loop body"
            }
          ]
        },
        summary: "Review until done"
      }
    ]
  },
  review: { human_approval_required: true }
};

test("automation opens as a prompt-first automation home", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);
  await daemon.waitForRequest("automation_list");

  await expect(page.locator(".pf-sidebar").getByRole("button", { name: "Automation" })).toHaveAttribute(
    "aria-current",
    "page"
  );
  await expect(page.locator(".pf-screen-top-title")).toHaveText("Automation");
  await expect(page.getByRole("heading", { name: "Create an automation" })).toBeVisible();
  await expect(page.getByText("Create an automation using natural language.")).toBeVisible();
  await expect(page.locator(".pf-automation-compose .pf-composer-wrap")).toBeVisible();
  await expect(page.locator(".pf-automation-compose .pf-composer")).toBeVisible();
  await expect(page.locator(".pf-automation-compose .pf-attachment-input")).toBeAttached();
  await expect(page.getByRole("button", { name: "Add content" })).toBeVisible();
  await expect(page.locator(".pf-automation-compose .picker .trigger")).toBeVisible();
  await expect(page.locator(".pf-automation-compose .picker .trigger")).toContainText("gpt-5.5");
  await expect(page.locator(".pf-automation-compose .picker .trigger")).toContainText("OpenAI");
  await expect(page.getByText("Fast", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Thinking level")).toBeVisible();
  await expect(page.getByLabel("Codex permissions")).toBeVisible();
  await expect(page.locator(".pf-automation-compose .pf-composer-hint")).toHaveText("⏎ to send · ⇧⏎ for newline");
  await expect(page.locator(".pf-automation-compose .pf-composer .pf-chip")).toHaveCount(0);
  await expect(page.locator(".pf-automation-compose .pf-composer textarea")).toHaveAttribute(
    "placeholder",
    "Tell Puffer what to automate, e.g. when a PR opens, prepare a review draft..."
  );
  await expect(page.locator(".pf-automation-compose").getByRole("button", { name: "Send", exact: true })).toBeVisible();
  await expect(page.locator(".pf-automation-compose > .pf-automation-chip-row")).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Automations", exact: true })).toHaveCount(0);
  await expect(page.getByText("Start from your automations or choose a template.")).toHaveCount(0);
  await expect(page.getByRole("tablist", { name: "Automation library" })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Your automations/ })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("tab", { name: /Template Library/ })).toHaveAttribute("aria-selected", "false");
  await expect(await backgroundLightnessGap(page.getByRole("tablist", { name: "Automation library" }))).toBeGreaterThan(0.02);
  await expect(page.getByRole("button", { name: "new", exact: true })).toBeVisible();
  await expect(page.getByLabel("Your automations empty state")).toBeVisible();
  await expect(page.getByText("No automations yet")).toBeVisible();
  await expect(page.getByText("创建你的第一个automation，处理重复的工作流")).toBeVisible();
  await expect(page.getByRole("button", { name: "create automation" })).toBeVisible();
  await expect(page.getByRole("list", { name: "Your automations" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: /Review inbox/ })).toHaveCount(0);
  await expect(page.getByRole("list", { name: "Template Library" })).toHaveCount(0);
  await page.getByRole("tab", { name: /Template Library/ }).click();
  await expect(page.getByRole("tab", { name: /Your automations/ })).toHaveAttribute("aria-selected", "false");
  await expect(page.getByRole("tab", { name: /Template Library/ })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("list", { name: "Your automations" })).toHaveCount(0);
  await expect(page.getByLabel("Your automations empty state")).toHaveCount(0);
  await expect(page.getByRole("list", { name: "Template Library" })).toBeVisible();
  await expect(page.getByRole("list", { name: "Template Library" }).getByRole("button", { name: /Review PRs/ })).toBeVisible();
  await expect(page.getByRole("list", { name: "Saved automations" })).toHaveCount(0);
  await expect(page.getByLabel("Selected automation details")).toHaveCount(0);
  await expect(page.getByText("Overview")).toHaveCount(0);
  await expect(page.getByText("Approvals")).toHaveCount(0);
  await expect(page.getByText("UI preview only")).toHaveCount(0);
  await expect(page.getByText("No infinite canvas")).toHaveCount(0);
  await expect(page.getByText("not connected to storage")).toHaveCount(0);
  await expect(page.getByText("human-gated")).toHaveCount(0);
  await expect(page.locator(".pf-automation-canvas")).toHaveCount(0);
});

test("new automation button opens the full-page builder", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);
  await page.getByRole("button", { name: "new", exact: true }).click();

  await expect(page.getByLabel("New automation page")).toBeVisible();
  await expect(page.getByLabel("Name")).toBeVisible();
  await expect(page.getByLabel("Name")).toHaveValue("Untitled automation");
  await expect(page.getByRole("button", { name: "Cancel" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Save" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Create", exact: true })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Triggers" })).toBeVisible();
  await expect(page.getByRole("button", { name: /PR pushed/ })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Add Trigger" })).toBeVisible();
  await expect(page.getByRole("list", { name: "Your automations" })).toHaveCount(0);
  await expect(page.getByRole("tablist", { name: "Automation library" })).toHaveCount(0);

  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("tablist", { name: "Automation library" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Untitled automation/ })).toHaveCount(0);
});

test("new opens a full-page automation builder", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);
  await page.locator(".pf-automation-compose .pf-composer textarea").fill("When a PR opens, prepare a review draft.");
  await page.locator(".pf-automation-compose").getByRole("button", { name: "Send", exact: true }).click();

  await expect(page.getByLabel("New automation page")).toBeVisible();
  await expect(page.getByRole("heading", { name: "New automation" })).toHaveCount(1);
  await expect(page.getByText("Automations")).toBeVisible();
  await expect(page.getByText("Create New")).toBeVisible();
  await expect(page.getByLabel("Name")).toBeVisible();
  await expect(page.getByLabel("Name")).toHaveValue("PR review draft");
  await expect(page.getByRole("heading", { name: "Triggers" })).toBeVisible();
  await expect(page.getByRole("button", { name: /PR opened/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Select repos/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Add Trigger" })).toBeVisible();
  await page.getByRole("button", { name: "Add Trigger" }).click();
  await expect(page.getByRole("menu", { name: "Add trigger" })).toBeVisible();
  await expect(page.getByPlaceholder("Search triggers...")).toBeVisible();
  await expect(page.getByRole("menuitem", { name: /Every/ })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: /Pull request/ })).toBeVisible();
  await page.locator(".pf-automation-builder-main").click({ position: { x: 6, y: 6 } });
  await expect(page.getByRole("menu", { name: "Add trigger" })).toHaveCount(0);
  await page.getByRole("button", { name: "Add Trigger" }).click();
  await expect(page.getByRole("menu", { name: "Add trigger" })).toBeVisible();
  await page.getByRole("menuitem", { name: /Pull request/ }).click();
  await expect(page.getByRole("button", { name: /PR opened/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Edit trigger" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Remove trigger" })).toBeVisible();
  await page.getByRole("button", { name: "Edit trigger" }).click();
  await expect(page.getByRole("menu", { name: "Add trigger" })).toBeVisible();
  await page.getByRole("menuitem", { name: /Every/ }).click();
  await expect(page.getByRole("button", { name: /Every day at/ })).toBeVisible();
  await page.getByRole("button", { name: "Remove trigger" }).click();
  await expect(page.getByRole("button", { name: /Every day at/ })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Add Trigger" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Instructions" })).toBeVisible();
  await expect(page.getByLabel("Instructions")).toHaveValue("When a PR opens, prepare a review draft.");
  await expect(page.getByRole("button", { name: /Codex 5.3 High/ })).toBeVisible();
  await expect(page.getByText("Some tools might not be configured yet")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Tools" })).toBeVisible();
  await expect(page.getByText("Memories")).toBeVisible();
  await expect(page.getByRole("button", { name: "Comment on Pull Request tool", exact: true })).toContainText("Comment on Pull Request");
  await expect(page.getByRole("button", { name: "Comment on Pull Request target" })).toContainText("Allow PR Approval");
  const commentToolRow = page.locator(".pf-automation-tool-config-row").filter({
    has: page.getByRole("button", { name: "Comment on Pull Request tool", exact: true })
  });
  const commentToolTextBox = await commentToolRow.locator(".pf-automation-tool-main").boundingBox();
  const commentToolTargetBox = await commentToolRow.locator(".pf-automation-tool-target").boundingBox();
  expect(commentToolTextBox).not.toBeNull();
  expect(commentToolTargetBox).not.toBeNull();
  expect(commentToolTargetBox!.x - (commentToolTextBox!.x + commentToolTextBox!.width)).toBeLessThan(18);
  await expect(page.getByRole("button", { name: "Select GitHub APIs" })).toHaveCount(0);
  await page.getByRole("button", { name: "Add Tool or MCP" }).click();
  await expect(page.getByRole("menu", { name: "Common apps" })).toBeVisible();
  await expect(page.getByPlaceholder("Search tools and APIs...")).toBeVisible();
  await page.locator(".pf-automation-builder-main").click({ position: { x: 6, y: 6 } });
  await expect(page.getByRole("menu", { name: "Common apps" })).toHaveCount(0);
  await page.getByRole("button", { name: "Add Tool or MCP" }).click();
  await expect(page.getByRole("menu", { name: "Common apps" })).toBeVisible();
  await expect(page.getByRole("group", { name: "AgentEnv API capabilities" })).toHaveCount(0);
  await expect(page.getByRole("menuitemcheckbox", { name: /Raw AgentEnv Node/ })).toHaveCount(0);
  await expect(page.getByText("list AgentEnv node definitions")).toHaveCount(0);
  const githubCapabilities = page.getByRole("group", { name: "GitHub API capabilities" });
  const slackCapabilities = page.getByRole("group", { name: "Slack API capabilities" });
  await expect(githubCapabilities).toBeVisible();
  await expect(githubCapabilities.getByRole("menuitemcheckbox", { name: /Comment on Pull Request/ })).toHaveAttribute("aria-checked", "true");
  await expect(githubCapabilities.getByRole("menuitemcheckbox", { name: /Watch Pull Requests/ })).toHaveAttribute("aria-checked", "false");
  await expect(slackCapabilities).toBeVisible();
  await expect(slackCapabilities.getByRole("menuitemcheckbox", { name: /Send to Slack/ })).toHaveAttribute("aria-checked", "false");
  await slackCapabilities.getByRole("menuitemcheckbox", { name: /Send to Slack/ }).click();
  await expect(page.getByRole("button", { name: "Send to Slack tool", exact: true })).toContainText("Send to Slack");
  await expect(page.getByRole("button", { name: "Send to Slack target" })).toContainText("#teams");
  await githubCapabilities.getByRole("menuitemcheckbox", { name: /Comment on Pull Request/ }).click();
  await expect(page.getByRole("button", { name: "Comment on Pull Request tool", exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Edit Send to Slack tool" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Remove Send to Slack tool" })).toBeVisible();
  await page.getByRole("button", { name: "Edit Send to Slack tool" }).click();
  await expect(page.getByRole("menu", { name: "Common apps" })).toBeVisible();
  await page.getByRole("menuitemcheckbox", { name: /Create Gmail Draft/ }).click();
  await expect(page.getByRole("button", { name: "Create Gmail Draft tool", exact: true })).toContainText("Create Gmail Draft");
  await expect(page.getByRole("button", { name: "Send to Slack tool", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Remove Create Gmail Draft tool" }).click();
  await expect(page.getByRole("button", { name: "Create Gmail Draft tool", exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Add Tool or MCP" })).toBeVisible();
  await page.getByRole("button", { name: "Add Tool or MCP" }).click();
  await expect(page.getByRole("menu", { name: "Common apps" })).toBeVisible();
  await expect(page.getByRole("group", { name: "GitHub API capabilities" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Slack API capabilities" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Gmail API capabilities" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Google Calendar API capabilities" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Linear API capabilities" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Notion API capabilities" })).toBeVisible();
  await page.getByRole("menuitemcheckbox", { name: /Create Gmail Draft/ }).click();
  await expect(page.getByRole("button", { name: "Create Gmail Draft tool", exact: true })).toContainText("Create Gmail Draft");
  await expect(page.getByRole("button", { name: "Create Gmail Draft target" })).toContainText("Primary inbox");
  await expect(page.getByRole("button", { name: "Select Gmail APIs" })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Run location" })).toBeVisible();
  await expect(page.getByRole("radio", { name: /Local/ })).toBeChecked();
  await expect(page.getByRole("radio", { name: /AgentEnv Cloud/ })).not.toBeChecked();
  await expect(page.getByRole("heading", { name: "Cloud Agent Environment" })).toHaveCount(0);
  await expect(page.getByPlaceholder("Follow up...")).toHaveCount(0);
  await expect(page.getByRole("list", { name: "Your automations" })).toHaveCount(0);
  await expect(page.getByRole("list", { name: "Template Library" })).toHaveCount(0);
  await expect(page.getByLabel("Selected automation details")).toHaveCount(0);
  await expect(page.getByRole("tab")).toHaveCount(0);
  await expect(page.locator(".pf-automation-canvas")).toHaveCount(0);

  await page.getByRole("button", { name: "Back to automations" }).click();
  await expect(page.getByLabel("Your automations empty state")).toBeVisible();
  await expect(page.getByRole("tablist", { name: "Automation library" })).toBeVisible();
});

test("template cards open the full-page automation builder", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);
  await page.getByRole("tab", { name: /Template Library/ }).click();
  await page.getByRole("list", { name: "Template Library" }).getByRole("button", { name: /Calendar RSVP/ }).click();

  await expect(page.getByLabel("New automation page")).toBeVisible();
  await expect(page.getByLabel("Name")).toHaveValue("Calendar RSVP");
  await expect(page.getByRole("button", { name: /Invite arrives on/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Calendar/ })).toBeVisible();
  await expect(page.getByLabel("Instructions")).toHaveValue(
    "When a calendar invite arrives, check conflicts and prepare an RSVP suggestion."
  );

  await page.getByRole("button", { name: "Save" }).click();
  const saved = await daemon.waitForRequest("automation_save");
  expect(saved.params.spec).toMatchObject({
    source: { type: "template", template_id: "calendar-rsvp" }
  });
});

test("new automations default to configured automation runtime", async ({ page }) => {
  const daemon = new FakeDaemon();
  daemon.setWorkflowBackend({
    mode: "agent_env_cloud",
    apiUrl: "https://api.agentenv.io",
    uiUrl: "https://agentenv.io",
    workspaceId: "workspace-cloud",
    hasToken: true
  });
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);
  await daemon.waitForRequest("automation_list");
  await page.getByRole("button", { name: "new", exact: true }).click();
  await expect(page.getByLabel("New automation page").getByRole("radio", { name: /AgentEnv Cloud/ })).toBeChecked();

  await page.getByLabel("Name").fill("Cloud triage");
  await page.getByLabel("Instructions").fill("Run this preview in the cloud runtime.");
  await page.getByRole("button", { name: "Save" }).click();

  const created = await daemon.waitForRequest("automation_save");
  expect(created.params.spec).toMatchObject({
    name: "Cloud triage",
    run_location: "agent_env_cloud"
  });
});

test("automation builder links to automation runtime settings", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);
  await page.getByRole("button", { name: "new", exact: true }).click();
  await page.getByRole("button", { name: "Configure Runtime" }).click();

  const pane = page.locator(".pf-settings-pane");
  await expect(pane.getByRole("heading", { name: "Automation Runtime" })).toBeVisible();
  await expect(pane.getByRole("radiogroup", { name: "Automation runtime mode" })).toBeVisible();
});

test("save persists an automation through daemon RPCs", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);
  await daemon.waitForRequest("automation_list");
  await page.getByRole("button", { name: "new", exact: true }).click();
  await page.getByLabel("Name").fill("Daily issue triage");
  await page.getByLabel("Instructions").fill("Every morning, summarize new issues and prepare a triage note.");
  await page.getByRole("button", { name: "Add Trigger" }).click();
  await page.getByRole("menuitem", { name: /Every/ }).click();
  await page.getByRole("button", { name: "Save" }).click();
  const created = await daemon.waitForRequest("automation_save");
  expect(created.params).toMatchObject({
    status: "enabled",
    spec: {
      spec_version: 1,
      name: "Daily issue triage",
      source: { type: "blank" },
      instructions: "Every morning, summarize new issues and prepare a triage note.",
      run_location: "local",
      triggers: [
        {
          type: "agent_env_node",
          node: {
            node_type: "schedule",
            config: { target: "09:00" }
          }
        }
      ],
      flow: {
        steps: [
          {
            type: "agent_env_node",
            id: "agent",
            node: { node_type: "puffer_agent" }
          }
        ]
      }
    }
  });
  expect(created.params).not.toHaveProperty("expectedRevision");

  await expect(page.getByRole("tablist", { name: "Automation library" })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Your automations/ })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("list", { name: "Your automations" }).getByRole("button", { name: /Daily issue triage/ })).toBeVisible();

  await page.getByRole("list", { name: "Your automations" }).getByRole("button", { name: /Daily issue triage/ }).click();

  await expect(page.getByLabel("Automation detail page")).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Automation path" })).toContainText("Automations");
  await expect(page.getByLabel("Automation name")).toHaveValue("Daily issue triage");
  await expect(page.getByRole("button", { name: "Test Run" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Save" })).toBeVisible();
  await expect(page.getByRole("button", { name: "More automation actions" })).toBeVisible();
  await page.getByRole("button", { name: "More automation actions" }).click();
  await expect(page.getByRole("menu", { name: "Automation actions" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Delete" })).toBeVisible();
  await expect(page.getByLabel("Active")).toBeChecked();
  await expect(page.getByText("Active | You")).toBeVisible();
  await expect(page.getByRole("tablist", { name: "Automation detail" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Settings" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("tab", { name: "Run History" })).toHaveAttribute("aria-selected", "false");
  await expect(page.getByRole("heading", { name: "Triggers" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Every day at/ })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Instructions" })).toBeVisible();
  await expect(page.getByLabel("Instructions")).toHaveValue("Every morning, summarize new issues and prepare a triage note.");
  await expect(page.getByRole("heading", { name: "Tools" })).toBeVisible();
  await expect(page.getByText("Memories")).toBeVisible();

  await page.getByLabel("Automation name").fill("Daily issue review");
  await page.getByLabel("Instructions").fill("Every morning, summarize new issues and assign next steps.");
  await page.getByRole("button", { name: "Save" }).click();
  const updated = await daemon.waitForRequest(
    "automation_save",
    (request) =>
      Boolean(request.params.spec) &&
      JSON.stringify(request.params.spec).includes("Daily issue review")
  );
  expect(updated.params.expectedRevision).toBe(1);
  expect(updated.params).toMatchObject({
    status: "enabled",
    spec: {
      name: "Daily issue review",
      instructions: "Every morning, summarize new issues and assign next steps."
    }
  });
  await page.getByLabel("Back to automations").click();

  await expect(page.getByRole("list", { name: "Your automations" }).getByRole("button", { name: /Daily issue review/ })).toBeVisible();
  await page.getByRole("list", { name: "Your automations" }).getByRole("button", { name: /Daily issue review/ }).click();
  await expect(page.getByLabel("Automation name")).toHaveValue("Daily issue review");
  await expect(page.getByLabel("Instructions")).toHaveValue("Every morning, summarize new issues and assign next steps.");

  await page.getByRole("tab", { name: "Run History" }).click();
  await expect(page.getByRole("tab", { name: "Settings" })).toHaveAttribute("aria-selected", "false");
  await expect(page.getByRole("tab", { name: "Run History" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("tabpanel", { name: "Run History" })).toBeVisible();
  await expect(page.getByText("No runs yet")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Triggers" })).toHaveCount(0);

  await page.getByRole("button", { name: "Test Run" }).click();
  const previewSave = await daemon.waitForRequest(
    "automation_save",
    (request) =>
      request.params.id === updated.params.id &&
      request.params.expectedRevision === 2 &&
      Boolean(request.params.spec) &&
      JSON.stringify(request.params.spec).includes("Daily issue review")
  );
  const sync = await daemon.waitForRequest("automation_sync_preview");
  const preview = await daemon.waitForRequest("automation_run_preview");
  expect(sync.params).toMatchObject({
    id: updated.params.id,
    expectedRevision: 2
  });
  expect(preview.params.id).toBe(updated.params.id);
  expect(daemon.requests.indexOf(previewSave)).toBeLessThan(daemon.requests.indexOf(sync));
  expect(daemon.requests.indexOf(sync)).toBeLessThan(daemon.requests.indexOf(preview));
  expect(daemon.requests.some((request) => request.method === "automation_compile_deploy")).toBe(false);
  await expect(page.getByRole("list", { name: "Run history" }).getByText("Test run")).toBeVisible();
  await expect(page.getByRole("list", { name: "Run history" }).getByText("Waiting for review")).toBeVisible();

  await page.getByRole("button", { name: "More automation actions" }).click();
  await page.getByRole("menuitem", { name: "Delete" }).click();
  const deleted = await daemon.waitForRequest("automation_delete");
  expect(deleted.params.id).toBe(updated.params.id);
  await expect(page.getByLabel("Your automations empty state")).toBeVisible();
});

test("prompt-created save records natural language source", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);
  await daemon.waitForRequest("automation_list");
  await page.locator(".pf-automation-compose .pf-composer textarea").fill("When a PR opens, prepare a review draft.");
  await page.locator(".pf-automation-compose").getByRole("button", { name: "Send", exact: true }).click();
  await page.getByRole("button", { name: "Save" }).click();

  const saved = await daemon.waitForRequest("automation_save");
  expect(saved.params.spec).toMatchObject({
    source: {
      type: "natural_language",
      prompt: "When a PR opens, prepare a review draft."
    }
  });
});

test("detail save preserves unsupported rich Automation spec fields", async ({ page }) => {
  const daemon = new FakeDaemon({
    automations: [
      {
        id: "rich-automation",
        status: "enabled",
        revision: 7,
        spec: richAutomationSpec,
        runtime: {
          status: "not_compiled",
          spec_hash: null,
          compiled_revision: null,
          agentenv_workflow_count: 0,
          puffer_binding_count: 0,
          last_error: null
        },
        created_at_ms: Date.now() - 10_000,
        updated_at_ms: Date.now() - 5_000
      }
    ]
  });
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);
  await daemon.waitForRequest("automation_list");
  await page.getByRole("list", { name: "Your automations" }).getByRole("button", { name: /Rich automation/ }).click();
  await page.getByLabel("Automation name").fill("Rich automation updated");
  await page.getByLabel("Instructions").fill("Keep the hidden fields intact.");
  await page.getByRole("button", { name: "Save" }).click();

  const saved = await daemon.waitForRequest(
    "automation_save",
    (request) =>
      Boolean(request.params.spec) &&
      JSON.stringify(request.params.spec).includes("Rich automation updated")
  );
  expect(saved.params.expectedRevision).toBe(7);
  expect(saved.params.spec).toMatchObject({
    name: "Rich automation updated",
    description: "Keep the hidden fields intact.",
    source: { type: "template", template_id: "rich-template" },
    triggers: [
      {
        type: "puffer_connection",
        id: "incoming",
        filter: { pattern: "urgent" },
        ignore_filters: [{ pattern: "ignore" }],
        contact_ids: ["telegram-user-id@1"]
      }
    ],
    flow: {
      steps: [
        {
          type: "loop",
          id: "review-loop",
          body: {
            steps: [
              {
                type: "agent_env_node",
                id: "rich-node",
                node: {
                  node_type: "custom_node",
                  config: { keep: "this" }
                }
              }
            ]
          }
        }
      ]
    }
  });
});
