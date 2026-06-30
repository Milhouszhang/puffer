import { describe, expect, it } from "vitest";
import type { TimelineItem } from "../../types";
import { isAlreadyPersistedTool, persistedToolIdSet } from "./timelineToolDedup";

function toolItem(id: string, toolName = "Canvas"): TimelineItem {
  return {
    id,
    kind: "tool",
    title: toolName,
    summary: "",
    body: "",
    meta: [],
    toolName,
    status: "success",
    input: "{}",
    output: "",
    inputJson: null
  };
}

function assistantItem(id: string): TimelineItem {
  return {
    id,
    kind: "assistant",
    title: "assistant",
    summary: "",
    body: "text",
    meta: []
  };
}

describe("timelineToolDedup", () => {
  it("recognizes a transient tool already present in persisted (normal order)", () => {
    const persisted = [toolItem("tool-skill", "Skill"), toolItem("tool-canvas", "Canvas")];
    const ids = persistedToolIdSet(persisted);

    const transientCanvas = toolItem("tool-canvas", "Canvas");
    expect(isAlreadyPersistedTool(transientCanvas, ids)).toBe(true);
  });

  it("recognizes the canvas as already persisted regardless of order (the forward-search miss)", () => {
    // Canvas appears EARLIER in persisted, but the transient stream has an
    // assistant-like item ahead of the canvas. A forward search from the
    // assistant's match position would skip past the canvas; the id-set lookup
    // must not.
    const persisted = [toolItem("tool-canvas", "Canvas"), assistantItem("tool-later-message")];
    const ids = persistedToolIdSet(persisted);

    const transientAssistantFirst = assistantItem("tool-later-message");
    const transientCanvas = toolItem("tool-canvas", "Canvas");

    // The assistant is not a tool → not handled by the id-set fast path.
    expect(isAlreadyPersistedTool(transientAssistantFirst, ids)).toBe(false);
    // The canvas is still recognized as already persisted.
    expect(isAlreadyPersistedTool(transientCanvas, ids)).toBe(true);
  });

  it("only tracks tool-kind ids, ignoring non-tool persisted items", () => {
    const persisted = [assistantItem("tool-assistant"), toolItem("tool-canvas", "Canvas")];
    const ids = persistedToolIdSet(persisted);

    expect(ids.has("tool-canvas")).toBe(true);
    expect(ids.has("tool-assistant")).toBe(false);
  });

  it("returns false for non-tool items and for unknown tool ids", () => {
    const persisted = [toolItem("tool-canvas", "Canvas")];
    const ids = persistedToolIdSet(persisted);

    // Non-tool item even if its id collides with a persisted tool id.
    expect(isAlreadyPersistedTool(assistantItem("tool-canvas"), ids)).toBe(false);
    // Tool item whose id is not persisted.
    expect(isAlreadyPersistedTool(toolItem("tool-other", "Read"), ids)).toBe(false);
  });
});
