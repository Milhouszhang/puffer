import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { createInterface } from "node:readline";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * @typedef {Object} DaemonHandshake
 * @property {string} url
 * @property {string} token
 * @property {string} workspaceRoot
 * @property {string} protocolVersion
 */

/**
 * @typedef {Object} DaemonFixtureOptions
 * @property {string} [openaiBaseUrl]
 * @property {string} [anthropicBaseUrl]
 * @property {string} [openaiApiKey]  // defaults to "sk-test"
 * @property {string} defaultProvider
 * @property {string} defaultModel
 */

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");
const defaultPufferBinary = path.join(repoRoot, "target", "debug", "puffer");
const pufferBinary = process.env.PUFFER_DESKTOP_TEST_DAEMON ?? defaultPufferBinary;

class DaemonFixture {
  /**
   * @param {DaemonHandshake} handshake
   * @param {import("node:child_process").ChildProcessWithoutNullStreams} child
   * @param {string} root
   */
  constructor(handshake, child, root) {
    this.handshake = handshake;
    this.child = child;
    this.root = root;
    this.stderr = "";
  }

  /**
   * @param {DaemonFixtureOptions} options
   * @returns {Promise<DaemonFixture>}
   */
  static async start(options) {
    const root = await mkdtemp(path.join(tmpdir(), "puffer-desktop-ui-"));
    const workspace = path.join(root, "workspace");
    const pufferHome = path.join(root, "home");
    const pufferConfig = path.join(pufferHome, ".puffer");
    const discoveryCache = path.join(root, "discovery.json");
    await mkdir(workspace, { recursive: true });
    await mkdir(pufferConfig, { recursive: true });
    if (options.anthropicBaseUrl) {
      const workspaceProviders = path.join(workspace, ".puffer", "resources", "providers");
      await mkdir(workspaceProviders, { recursive: true });
      await writeFile(
        path.join(workspaceProviders, "anthropic.yaml"),
        anthropicProviderYaml(options.anthropicBaseUrl)
      );
    }
    await writeFile(
      path.join(pufferConfig, "auth.json"),
      JSON.stringify({
        format_version: 1,
        providers: {
          ...(options.openaiBaseUrl
            ? { openai: { kind: "api_key", key: options.openaiApiKey ?? "sk-test" } }
            : {}),
          ...(options.anthropicBaseUrl ? { anthropic: { kind: "api_key", key: "sk-ant-test" } } : {})
        }
      })
    );
    await writeFile(discoveryCache, discoveryCacheJson());
    /** @type {Record<string, string | undefined>} */
    const env = {
      ...process.env,
      PUFFER_HOME: pufferHome,
      PUFFER_BUILTIN_RESOURCES_DIR: path.join(repoRoot, "resources"),
      PUFFER_DISCOVERY_CACHE_PATH: discoveryCache
    };
    if (options.openaiBaseUrl) {
      env.OPENAI_BASE_URL = options.openaiBaseUrl;
    }

    const child = spawn(
      pufferBinary,
      [
        "daemon",
        "--bind",
        "127.0.0.1:0",
        "--token",
        "desktop-ui-token",
        "--print-handshake",
        "--no-browser",
        "--disable-auto-title"
      ],
      {
        cwd: workspace,
        env
      }
    );
    let stderr = "";
    child.stderr.on("data", (chunk) => {
      stderr += String(chunk);
    });

    const handshake = await readHandshake(child, () => stderr);
    await daemonRpc(handshake, "update_config", {
      ...(options.openaiBaseUrl ? { openaiBaseUrl: options.openaiBaseUrl } : {}),
      defaultProvider: options.defaultProvider,
      defaultModel: options.defaultModel
    });
    const fixture = new DaemonFixture(handshake, child, root);
    fixture.stderr = stderr;
    child.stderr.on("data", (chunk) => {
      fixture.stderr += String(chunk);
    });
    return fixture;
  }

  async stop() {
    if (!this.child.killed) this.child.kill();
    await new Promise((resolve) => {
      this.child.once("exit", () => resolve(undefined));
      setTimeout(resolve, 1_000);
    });
    const unexpectedStderr = this.stderr
      .split(/\r?\n/)
      .filter((line) => line.trim() && !line.startsWith("puffer daemon listening on "))
      .join("\n");
    if (unexpectedStderr.trim()) {
      console.error(`puffer daemon stderr:\n${unexpectedStderr}`);
    }
    await rm(this.root, { recursive: true, force: true });
  }
}

class OpenAiMock {
  /**
   * @param {import("node:http").Server} server
   * @param {string} baseUrl
   * @param {string} reply
   */
  constructor(server, baseUrl, reply) {
    this.server = server;
    this.baseUrl = baseUrl;
    this.reply = reply;
    this.responsesCalls = 0;
    this.lastResponsesBody = "";
  }

  /**
   * @param {string} reply
   * @returns {Promise<OpenAiMock>}
   */
  static async start(reply) {
    /** @type {OpenAiMock | null} */
    let mock = null;
    const server = createServer((request, response) => {
      if (mock) {
        void mock.handle(request, response);
      } else {
        response.writeHead(503, { "content-type": "text/plain" });
        response.end("mock not ready");
      }
    });
    await new Promise((resolve) => {
      server.listen(0, "127.0.0.1", () => resolve(undefined));
    });
    const address = server.address();
    if (address === null || typeof address === "string") {
      throw new Error("mock server did not bind a TCP address");
    }
    const ready = new OpenAiMock(server, `http://127.0.0.1:${address.port}`, reply);
    mock = ready;
    return ready;
  }

  async stop() {
    await new Promise((resolve, reject) => {
      this.server.close((error) => (error ? reject(error) : resolve(undefined)));
    });
  }

  /**
   * @param {import("node:http").IncomingMessage} request
   * @param {import("node:http").ServerResponse} response
   */
  async handle(request, response) {
    if (request.url === "/v1/models") {
      writeJson(response, {
        data: [{ id: "gpt-5", name: "GPT 5 smoke" }]
      });
      return;
    }
    if (request.url === "/v1/responses") {
      this.responsesCalls += 1;
      this.lastResponsesBody = await readRequestBody(request);
      writeJson(response, {
        id: "resp_desktop_ui_smoke",
        status: "completed",
        output_text: this.reply,
        output: [
          {
            type: "message",
            role: "assistant",
            content: [{ type: "output_text", text: this.reply }]
          }
        ],
        usage: {
          input_tokens: 10,
          output_tokens: 4,
          input_tokens_details: { cached_tokens: 0 }
        }
      });
      return;
    }
    response.writeHead(404, { "content-type": "text/plain" });
    response.end("not found");
  }
}

class AnthropicMock {
  /**
   * @param {import("node:http").Server} server
   * @param {string} baseUrl
   * @param {string} reply
   */
  constructor(server, baseUrl, reply) {
    this.server = server;
    this.baseUrl = baseUrl;
    this.reply = reply;
    this.messagesCalls = 0;
    this.lastMessagesBody = "";
  }

  /**
   * @param {string} reply
   * @returns {Promise<AnthropicMock>}
   */
  static async start(reply) {
    /** @type {AnthropicMock | null} */
    let mock = null;
    const server = createServer((request, response) => {
      if (mock) {
        void mock.handle(request, response);
      } else {
        response.writeHead(503, { "content-type": "text/plain" });
        response.end("mock not ready");
      }
    });
    await new Promise((resolve) => {
      server.listen(0, "127.0.0.1", () => resolve(undefined));
    });
    const address = server.address();
    if (address === null || typeof address === "string") {
      throw new Error("mock server did not bind a TCP address");
    }
    const ready = new AnthropicMock(server, `http://127.0.0.1:${address.port}`, reply);
    mock = ready;
    return ready;
  }

  async stop() {
    await new Promise((resolve, reject) => {
      this.server.close((error) => (error ? reject(error) : resolve(undefined)));
    });
  }

  /**
   * @param {import("node:http").IncomingMessage} request
   * @param {import("node:http").ServerResponse} response
   */
  async handle(request, response) {
    if (request.url === "/v1/models") {
      writeJson(response, {
        data: [{ id: "claude-sonnet-4-5", display_name: "Claude Sonnet 4.5" }]
      });
      return;
    }
    if (request.url?.startsWith("/v1/messages")) {
      this.messagesCalls += 1;
      this.lastMessagesBody = await readRequestBody(request);
      writeSse(response, anthropicTextStream(this.reply));
      return;
    }
    response.writeHead(404, { "content-type": "text/plain" });
    response.end("not found");
  }
}

/**
 * @param {import("node:child_process").ChildProcessWithoutNullStreams} child
 * @param {() => string} stderr
 * @returns {Promise<DaemonHandshake>}
 */
async function readHandshake(child, stderr) {
  const lines = createInterface({ input: child.stdout });
  const linePromise = new Promise((resolve, reject) => {
    lines.once("line", resolve);
    child.once("exit", (code, signal) => {
      reject(new Error(`daemon exited before handshake code=${code} signal=${signal}\n${stderr()}`));
    });
  });
  const timeout = new Promise((_, reject) => {
    setTimeout(() => reject(new Error(`daemon handshake timed out\n${stderr()}`)), 10_000);
  });
  const line = await Promise.race([linePromise, timeout]);
  lines.close();
  return JSON.parse(String(line));
}

/**
 * @param {DaemonHandshake} handshake
 * @param {string} method
 * @param {Record<string, unknown>} params
 * @returns {Promise<unknown>}
 */
async function daemonRpc(handshake, method, params) {
  const url = new URL(handshake.url);
  url.searchParams.set("token", handshake.token);
  const socket = new WebSocket(url);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", () => resolve(undefined), { once: true });
    socket.addEventListener("error", () => reject(new Error(`daemon websocket failed for ${method}`)), {
      once: true
    });
  });
  try {
    const id = "setup-1";
    const result = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error(`${method} timed out`));
      }, 10_000);
      socket.addEventListener("message", (event) => {
        const message = JSON.parse(String(event.data));
        if (message.id !== id) return;
        clearTimeout(timeout);
        if (message.error) {
          reject(new Error(`${method} failed: ${JSON.stringify(message.error)}`));
        } else {
          resolve(message.result);
        }
      });
    });
    socket.send(JSON.stringify({ id, method, params }));
    return await result;
  } finally {
    socket.close();
  }
}

/**
 * @param {import("node:http").IncomingMessage} request
 * @returns {Promise<string>}
 */
async function readRequestBody(request) {
  const chunks = [];
  for await (const chunk of request) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  }
  return Buffer.concat(chunks).toString("utf8");
}

/**
 * @param {import("node:http").ServerResponse} response
 * @param {unknown} value
 */
function writeJson(response, value) {
  const body = JSON.stringify(value);
  response.writeHead(200, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(body)
  });
  response.end(body);
}

/**
 * @param {import("node:http").ServerResponse} response
 * @param {string} events
 */
function writeSse(response, events) {
  response.writeHead(200, {
    "content-type": "text/event-stream",
    "cache-control": "no-cache",
    connection: "keep-alive"
  });
  response.end(events);
}

/**
 * @param {string} reply
 * @returns {string}
 */
function anthropicTextStream(reply) {
  return [
    sseEvent("message_start", {
      type: "message_start",
      message: {
        id: "msg_desktop_ui_smoke",
        type: "message",
        role: "assistant",
        model: "claude-sonnet-4-5",
        content: [],
        usage: {
          input_tokens: 10,
          cache_read_input_tokens: 0,
          cache_creation_input_tokens: 0,
          output_tokens: 1
        }
      }
    }),
    sseEvent("content_block_start", {
      type: "content_block_start",
      index: 0,
      content_block: { type: "text", text: "" }
    }),
    sseEvent("content_block_delta", {
      type: "content_block_delta",
      index: 0,
      delta: { type: "text_delta", text: reply }
    }),
    sseEvent("content_block_stop", { type: "content_block_stop", index: 0 }),
    sseEvent("message_delta", {
      type: "message_delta",
      delta: { stop_reason: "end_turn" },
      usage: { output_tokens: 4 }
    }),
    sseEvent("message_stop", { type: "message_stop" })
  ].join("");
}

/**
 * @param {string} event
 * @param {unknown} data
 * @returns {string}
 */
function sseEvent(event, data) {
  return `event:${event}\ndata:${JSON.stringify(data)}\n\n`;
}

function discoveryCacheJson() {
  const now = 1_700_000_000_000;
  return JSON.stringify({
    entries: {
      "llama-cpp": { models: [], cached_at_ms: now },
      lmstudio: { models: [], cached_at_ms: now },
      ollama: { models: [], cached_at_ms: now },
      vllm: { models: [], cached_at_ms: now }
    }
  });
}

/**
 * @param {string} baseUrl
 * @returns {string}
 */
function anthropicProviderYaml(baseUrl) {
  return `id: anthropic
display_name: Anthropic
base_url: "${baseUrl}"
default_api: anthropic-messages
auth_modes:
  - api_key
  - oauth
discovery:
  path: /v1/models
  response: anthropic_models
  api: anthropic-messages
  context_window: 200000
  max_output_tokens: 8192
  supports_reasoning: true
models:
  - id: claude-sonnet-4-5
    display_name: Claude Sonnet 4.5
    provider: anthropic
    api: anthropic-messages
    context_window: 200000
    max_output_tokens: 8192
    supports_reasoning: true
`;
}

export { DaemonFixture, OpenAiMock, AnthropicMock, pufferBinary, daemonRpc };
