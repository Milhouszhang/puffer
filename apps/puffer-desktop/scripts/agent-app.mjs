#!/usr/bin/env node
// Start a Puffer instance fully isolated from the user's dev data and print a directly openable URL.
// Usage: node scripts/agent-app.mjs [--provider mock|real]
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { existsSync } from "node:fs";
import { DaemonFixture, OpenAiMock, pufferBinary } from "../tests/support/daemonFixture.mjs";

const appDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
if (!existsSync(pufferBinary)) {
  console.error(`agent-app: missing ${pufferBinary} — run: cargo build -p puffer-cli`);
  process.exit(1);
}
const VITE_URL = "http://127.0.0.1:1420";
const provider = process.argv.includes("--provider")
  ? process.argv[process.argv.indexOf("--provider") + 1]
  : "mock";

function fail(msg) {
  console.error(`agent-app: ${msg}`);
  process.exit(1);
}

async function viteRunning() {
  try {
    const res = await fetch(VITE_URL, { signal: AbortSignal.timeout(1_000) });
    return res.ok;
  } catch {
    return false;
  }
}

async function ensureVite() {
  if (await viteRunning()) return null; // reuse; leave it alone on exit
  const vite = spawn(
    process.execPath,
    ["./node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "1420"],
    { cwd: appDir, stdio: ["ignore", "ignore", "inherit"] }
  );
  for (let i = 0; i < 60; i++) {
    if (await viteRunning()) return vite;
    await new Promise((r) => setTimeout(r, 1_000));
  }
  vite.kill();
  fail("Vite failed to become ready on :1420 within 60s");
}

let mock = null;
let openaiBaseUrl;
let openaiApiKey = "sk-test";
let defaultModel = "openai/gpt-5";
if (provider === "mock") {
  mock = await OpenAiMock.start("Puffer agent-app canned reply");
  openaiBaseUrl = mock.baseUrl;
} else if (provider === "real") {
  openaiBaseUrl = process.env.RELAYDANCE_BASE_URL ?? fail("RELAYDANCE_BASE_URL not set");
  openaiApiKey = process.env.RELAYDANCE_API_KEY ?? fail("RELAYDANCE_API_KEY not set");
} else {
  fail(`unknown --provider ${provider} (use mock|real)`);
}

const vite = await ensureVite();
const fixture = await DaemonFixture.start({
  openaiBaseUrl,
  openaiApiKey,
  defaultProvider: "openai",
  defaultModel
});

const params = new URLSearchParams({
  skipOnboarding: "1",
  corbinaBackend: fixture.handshake.url,
  corbinaToken: fixture.handshake.token
});
console.log(`AGENT_APP_ROOT=${fixture.root}`);
console.log(`AGENT_APP_URL=${VITE_URL}/?${params.toString()}`);
console.log("agent-app: ready. Ctrl-C to stop and clean up.");

async function shutdown() {
  await fixture.stop(); // kill daemon + remove temp dir
  if (mock) await mock.stop();
  if (vite) vite.kill(); // only kill a self-started Vite
  process.exit(0);
}
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
