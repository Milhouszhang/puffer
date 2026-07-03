import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { pufferBinary } from "./support/daemonFixture.mjs";

test("agent-app.mjs prints a reachable URL and cleans up on SIGTERM", async () => {
  test.skip(!existsSync(pufferBinary), `Build puffer first; missing ${pufferBinary}`);
  test.setTimeout(90_000);

  const child = spawn(process.execPath, ["scripts/agent-app.mjs"], { cwd: process.cwd() });
  let out = "";
  child.stdout.on("data", (c) => (out += String(c)));

  const line = (prefix: string) =>
    out.split(/\r?\n/).find((l) => l.startsWith(prefix))?.slice(prefix.length);
  await expect
    .poll(() => line("AGENT_APP_URL="), { timeout: 60_000 })
    .toBeTruthy();
  const url = line("AGENT_APP_URL=")!;
  const root = line("AGENT_APP_ROOT=")!;
  expect(root).toBeTruthy();

  const res = await fetch(url); // Playwright webServer already guarantees Vite on 1420
  expect(res.status).toBe(200);
  expect(url).toContain("corbinaBackend=");
  expect(url).toContain("corbinaToken=");

  child.kill("SIGTERM");
  await new Promise<void>((r) => child.once("exit", () => r()));
  await expect.poll(() => existsSync(root), { timeout: 5_000 }).toBe(false);
});
