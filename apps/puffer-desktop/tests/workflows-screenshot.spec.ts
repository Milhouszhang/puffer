import { test, type Page } from "@playwright/test";
import { FakeDaemon } from "./support/fakeDaemon";

async function openWorkflows(page: Page) {
  await page.locator(".pf-sidebar").getByRole("button", { name: "Workflows" }).click();
}

async function openWorkflowDetail(page: Page) {
  await openWorkflows(page);
  await page
    .getByLabel("Runtime workflows")
    .getByRole("button", { name: /Agent review workflow/ })
    .click();
}

test("workflows overview screenshot", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);
  await page.waitForTimeout(500);
  await page.screenshot({ path: "test-results/workflows-overview.png", fullPage: true });
});

test("workflows detail screenshot (visual editor open)", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflowDetail(page);
  await page.waitForTimeout(500);
  await page.screenshot({ path: "test-results/workflows-detail-open.png", fullPage: true });
});

test("workflows detail screenshot (json editor open)", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflowDetail(page);
  await page.waitForTimeout(300);
  // The JSON editor tab became a collapsible Configuration JSON section.
  await page.getByText("Configuration JSON").click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: "test-results/workflows-detail-json.png", fullPage: true });
});

test("workflows detail screenshot (executions refreshed)", async ({ page }) => {
  const daemon = new FakeDaemon();
  await daemon.install(page);
  await daemon.open(page);

  await openWorkflows(page);
  await page.waitForTimeout(300);
  await page.getByLabel("Runtime workflows").getByRole("button", { name: "Executions" }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: "test-results/workflows-detail-runs-open.png", fullPage: true });
});
