import { existsSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { DaemonFixture, OpenAiMock, AnthropicMock, pufferBinary } from "./support/daemonFixture.mjs";

type DaemonFixtureOptions = Parameters<typeof DaemonFixture.start>[0];

test("real daemon UI can create an OpenAI-backed agent and render a reply", async ({ page }) => {
  test.skip(
    !existsSync(pufferBinary),
    `Build puffer first or set PUFFER_DESKTOP_TEST_DAEMON; missing ${pufferBinary}`
  );
  test.setTimeout(60_000);

  const mock = await OpenAiMock.start("Puffer smoke reply");
  const fixture = await DaemonFixture.start({
    openaiBaseUrl: mock.baseUrl,
    defaultProvider: "openai",
    defaultModel: "openai/gpt-5"
  });
  try {
    const params = new URLSearchParams({
      skipOnboarding: "1",
      corbinaBackend: fixture.handshake.url,
      corbinaToken: fixture.handshake.token
    });
    await page.goto(`/?${params.toString()}`);

    await expect(page.getByRole("heading", { name: "No sessions yet" })).toBeVisible();
    await page.getByRole("button", { name: "New agent in default workspace" }).click();
    const dialog = page.getByRole("dialog", { name: "New agent" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("radio", { name: /OpenAI|Codex/ })).toBeVisible();
    await dialog.getByRole("button", { name: "Start agent" }).click();

    const composer = page.locator(".pf-composer textarea");
    await expect(composer).toBeEnabled();
    await composer.fill("Say exactly: Puffer smoke reply");
    await page.getByRole("button", { name: "Send" }).click();

    await expect.poll(() => mock.responsesCalls, { timeout: 20_000 }).toBe(1);
    await expect(
      page.locator('.pf-msg[data-role="agent"]').filter({ hasText: "Puffer smoke reply" })
    ).toBeVisible();
    expect(mock.lastResponsesBody).toContain("Say exactly: Puffer smoke reply");
  } finally {
    await fixture.stop();
    await mock.stop();
  }
});

test("real daemon UI can create an Anthropic-backed agent and render a reply", async ({ page }) => {
  test.skip(
    !existsSync(pufferBinary),
    `Build puffer first or set PUFFER_DESKTOP_TEST_DAEMON; missing ${pufferBinary}`
  );
  test.setTimeout(60_000);

  const mock = await AnthropicMock.start("Claude smoke reply");
  const fixture = await DaemonFixture.start({
    anthropicBaseUrl: mock.baseUrl,
    defaultProvider: "anthropic",
    defaultModel: "anthropic/claude-sonnet-4-5"
  });
  try {
    const params = new URLSearchParams({
      skipOnboarding: "1",
      corbinaBackend: fixture.handshake.url,
      corbinaToken: fixture.handshake.token
    });
    await page.goto(`/?${params.toString()}`);

    await expect(page.getByRole("heading", { name: "No sessions yet" })).toBeVisible();
    await page.getByRole("button", { name: "New agent in default workspace" }).click();
    const dialog = page.getByRole("dialog", { name: "New agent" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("radio", { name: /Anthropic|Claude/ })).toBeVisible();
    await dialog.getByRole("button", { name: "Start agent" }).click();

    const composer = page.locator(".pf-composer textarea");
    await expect(composer).toBeEnabled();
    await composer.fill("Say exactly: Claude smoke reply");
    await page.getByRole("button", { name: "Send" }).click();

    await expect.poll(() => mock.messagesCalls, { timeout: 20_000 }).toBe(1);
    await expect(
      page.locator('.pf-msg[data-role="agent"]').filter({ hasText: "Claude smoke reply" })
    ).toBeVisible();
    expect(mock.lastMessagesBody).toContain("Say exactly: Claude smoke reply");
  } finally {
    await fixture.stop();
    await mock.stop();
  }
});

for (const scenario of [
  {
    label: "Codex alias",
    reply: "Codex alias smoke reply",
    expectedProvider: /OpenAI|Codex/,
    startMock: () => OpenAiMock.start("Codex alias smoke reply"),
    fixtureOptions: (baseUrl: string): DaemonFixtureOptions => ({
      openaiBaseUrl: baseUrl,
      defaultProvider: "codex",
      defaultModel: "codex/gpt-5"
    }),
    calls: (mock: OpenAiMock | AnthropicMock) => (mock as OpenAiMock).responsesCalls,
    lastBody: (mock: OpenAiMock | AnthropicMock) => (mock as OpenAiMock).lastResponsesBody
  },
  {
    label: "Claude alias",
    reply: "Claude alias smoke reply",
    expectedProvider: /Anthropic|Claude/,
    startMock: () => AnthropicMock.start("Claude alias smoke reply"),
    fixtureOptions: (baseUrl: string): DaemonFixtureOptions => ({
      anthropicBaseUrl: baseUrl,
      defaultProvider: "claude",
      defaultModel: "claude/claude-sonnet-4-5"
    }),
    calls: (mock: OpenAiMock | AnthropicMock) => (mock as AnthropicMock).messagesCalls,
    lastBody: (mock: OpenAiMock | AnthropicMock) => (mock as AnthropicMock).lastMessagesBody
  }
]) {
  test(`real daemon UI can create a ${scenario.label} agent and render a reply`, async ({
    page
  }) => {
    test.skip(
      !existsSync(pufferBinary),
      `Build puffer first or set PUFFER_DESKTOP_TEST_DAEMON; missing ${pufferBinary}`
    );
    test.setTimeout(60_000);

    const mock = await scenario.startMock();
    const fixture = await DaemonFixture.start(scenario.fixtureOptions(mock.baseUrl));
    try {
      const params = new URLSearchParams({
        skipOnboarding: "1",
        corbinaBackend: fixture.handshake.url,
        corbinaToken: fixture.handshake.token
      });
      await page.goto(`/?${params.toString()}`);

      await expect(page.getByRole("heading", { name: "No sessions yet" })).toBeVisible();
      await page.getByRole("button", { name: "New agent in default workspace" }).click();
      const dialog = page.getByRole("dialog", { name: "New agent" });
      await expect(dialog).toBeVisible();
      await expect(dialog.getByRole("radio", { name: scenario.expectedProvider })).toBeVisible();
      await dialog.getByRole("button", { name: "Start agent" }).click();

      const composer = page.locator(".pf-composer textarea");
      await expect(page.getByText(/Reconnect .* to continue this session\./)).toHaveCount(0);
      await expect(composer).toBeEnabled();
      await composer.fill(`Say exactly: ${scenario.reply}`);
      await page.getByRole("button", { name: "Send" }).click();

      await expect.poll(() => scenario.calls(mock), { timeout: 20_000 }).toBe(1);
      await expect(
        page.locator('.pf-msg[data-role="agent"]').filter({ hasText: scenario.reply })
      ).toBeVisible();
      expect(scenario.lastBody(mock)).toContain(`Say exactly: ${scenario.reply}`);
    } finally {
      await fixture.stop();
      await mock.stop();
    }
  });
}
