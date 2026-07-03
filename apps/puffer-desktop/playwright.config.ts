import { defineConfig } from "@playwright/test";

const nodeExecutable = JSON.stringify(process.execPath);
const shouldReuseExistingServer = !process.env.CI && !process.env.CODEX_CI;
const serverPort = Number(process.env.PUFFER_DESKTOP_TEST_PORT ?? "1420");
const baseURL = `http://127.0.0.1:${serverPort}`;
const browserChannel =
  process.env.PUFFER_DESKTOP_USE_SYSTEM_CHROME === "1" ? "chrome" : undefined;

export default defineConfig({
  testDir: "tests",
  timeout: 120_000,
  expect: {
    timeout: 10_000
  },
  // Deliberately zero, including in CI: a red desktop-ui job means a real
  // contract broke — diagnose it from the uploaded artifacts and fix the
  // root cause (see AGENTS.md). Retries would mask exactly the slow rot
  // that killed 86 specs before the suite was gated.
  retries: 0,
  // Playwright's CI default is a single worker, which made the suite the
  // pipeline's long pole (8m vs 2m locally). GitHub's 4-vCPU runners handle
  // two fine — specs are isolated per page/FakeDaemon. Locally keep the
  // default (50% of cores).
  workers: process.env.CI ? 2 : undefined,
  webServer: {
    command: `${nodeExecutable} ./node_modules/vite/bin/vite.js --host 127.0.0.1 --port ${serverPort}`,
    url: `${baseURL}/?skipOnboarding`,
    reuseExistingServer: shouldReuseExistingServer,
    timeout: 120_000
  },
  use: {
    baseURL,
    ...(browserChannel ? { channel: browserChannel } : {}),
    headless: true,
    // In CI keep a replayable trace + screenshot for every failure so a
    // CI-only red can be root-caused locally (npx playwright show-trace)
    // instead of being rerun until green. Off locally: the live run is
    // already reproducible there and traces add per-test overhead.
    trace: process.env.CI ? "retain-on-failure" : "off",
    screenshot: process.env.CI ? "only-on-failure" : "off"
  }
});
