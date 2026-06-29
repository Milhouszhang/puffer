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

test("automation opens as a prompt-first automation home", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);

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
  await expect(page.getByRole("heading", { name: "Cloud Agent Environment" })).toBeVisible();
  await expect(page.getByText("Use Configured Environment")).toBeVisible();
  await expect(page.getByLabel("Use Configured Environment")).toBeChecked();
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
});

test("save creates a local automation card", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openAutomation(page);
  await page.getByRole("button", { name: "new", exact: true }).click();
  await page.getByLabel("Name").fill("Daily issue triage");
  await page.getByLabel("Instructions").fill("Every morning, summarize new issues and prepare a triage note.");
  await page.getByRole("button", { name: "Add Trigger" }).click();
  await page.getByRole("menuitem", { name: /Every/ }).click();
  await page.getByRole("button", { name: "Save" }).click();

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
  await expect(page.getByRole("list", { name: "Run history" }).getByText("Test run")).toBeVisible();
  await expect(page.getByRole("list", { name: "Run history" }).getByText("Waiting for review")).toBeVisible();
});
