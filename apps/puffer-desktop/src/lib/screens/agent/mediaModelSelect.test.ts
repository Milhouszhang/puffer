import { describe, it, expect } from "vitest";
import {
  providerOptions,
  modelOptions,
  seedSelection,
  selectionComplete,
  hasMediaModelSelect,
  buildMediaWriteBack,
} from "./mediaModelSelect";
import type {
  MediaCapabilityInfo,
  MediaGenerationSettings,
  MediaSettings,
} from "../../types";

function cap(
  providerId: string,
  modelId: string,
  extra: Partial<MediaCapabilityInfo> = {},
): MediaCapabilityInfo {
  return {
    providerId,
    providerDisplayName: `${providerId} name`,
    modelId,
    modelDisplayName: `${modelId} name`,
    kind: "image",
    operation: "generate",
    axes: [],
    status: "available",
    source: "test",
    reason: null,
    checkedAtMs: 0,
    ...extra,
  };
}

function saved(providerId: string, logicalModelId: string): MediaGenerationSettings {
  return { providerId, logicalModelId, selections: {} };
}

describe("providerOptions", () => {
  it("returns distinct providers in first-seen order", () => {
    const caps = [cap("ark", "seedream"), cap("ark", "seedream-pro"), cap("relay", "flux")];
    expect(providerOptions(caps)).toEqual([
      { id: "ark", label: "ark name" },
      { id: "relay", label: "relay name" },
    ]);
  });

  it("returns [] for no capabilities", () => {
    expect(providerOptions([])).toEqual([]);
  });
});

describe("modelOptions", () => {
  it("maps each capability to an option grouped by provider", () => {
    const caps = [cap("ark", "seedream"), cap("relay", "flux")];
    expect(modelOptions(caps)).toEqual([
      { id: "seedream", label: "seedream name", group: "ark" },
      { id: "flux", label: "flux name", group: "relay" },
    ]);
  });
});

describe("seedSelection", () => {
  it("returns empty selection when no capabilities are connected", () => {
    expect(seedSelection([], saved("ark", "seedream"))).toEqual({ provider: "", model: "" });
  });

  it("uses the saved provider and model when both are still available", () => {
    const caps = [cap("ark", "seedream"), cap("relay", "flux")];
    expect(seedSelection(caps, saved("relay", "flux"))).toEqual({ provider: "relay", model: "flux" });
  });

  it("falls back to the first available when there is no saved selection", () => {
    const caps = [cap("ark", "seedream"), cap("relay", "flux")];
    expect(seedSelection(caps, null)).toEqual({ provider: "ark", model: "seedream" });
  });

  it("keeps the saved provider but resets the model when the saved model is gone", () => {
    const caps = [cap("ark", "seedream"), cap("ark", "seedream-pro")];
    expect(seedSelection(caps, saved("ark", "removed-model"))).toEqual({
      provider: "ark",
      model: "seedream",
    });
  });

  it("falls back to the first available when the saved provider is gone", () => {
    const caps = [cap("relay", "flux")];
    expect(seedSelection(caps, saved("ark", "seedream"))).toEqual({ provider: "relay", model: "flux" });
  });
});

describe("selectionComplete", () => {
  it("is true only when both image and video models are chosen", () => {
    expect(selectionComplete({ imgModel: "a", vidModel: "b" })).toBe(true);
    expect(selectionComplete({ imgModel: "a", vidModel: "" })).toBe(false);
    expect(selectionComplete({ imgModel: "", vidModel: "b" })).toBe(false);
    expect(selectionComplete({})).toBe(false);
  });
});

describe("hasMediaModelSelect", () => {
  it("finds a mediaModelSelect node at the top level of a body", () => {
    expect(hasMediaModelSelect({ body: [{ type: "mediaModelSelect" }] })).toBe(true);
  });

  it("finds a mediaModelSelect node nested under children", () => {
    const spec = { body: [{ type: "group", children: [{ type: "mediaModelSelect" }] }] };
    expect(hasMediaModelSelect(spec)).toBe(true);
  });

  it("is false when no mediaModelSelect node is present", () => {
    expect(hasMediaModelSelect({ body: [{ type: "toggle", id: "t" }] })).toBe(false);
  });
});

describe("buildMediaWriteBack", () => {
  const loaded: MediaSettings = {
    image: { providerId: "ark", logicalModelId: "seedream", selections: { ratio: "16:9" } },
    video: { providerId: "relay", logicalModelId: "kling", selections: { dur: "5" } },
  };

  it("preserves a kind's selections when its provider and model are unchanged", () => {
    const out = buildMediaWriteBack(
      { imgProvider: "ark", imgModel: "seedream", vidProvider: "relay", vidModel: "kling" },
      loaded,
    );
    expect(out).toEqual({
      image: { providerId: "ark", logicalModelId: "seedream", selections: { ratio: "16:9" } },
      video: { providerId: "relay", logicalModelId: "kling", selections: { dur: "5" } },
    });
  });

  it("resets selections when the model changes", () => {
    const out = buildMediaWriteBack(
      { imgProvider: "ark", imgModel: "seedream-pro", vidProvider: "relay", vidModel: "kling" },
      loaded,
    );
    expect(out.image).toEqual({ providerId: "ark", logicalModelId: "seedream-pro", selections: {} });
    expect(out.video).toEqual({ providerId: "relay", logicalModelId: "kling", selections: { dur: "5" } });
  });

  it("resets selections when the provider changes", () => {
    const out = buildMediaWriteBack(
      { imgProvider: "byte", imgModel: "flux", vidProvider: "relay", vidModel: "kling" },
      loaded,
    );
    expect(out.image).toEqual({ providerId: "byte", logicalModelId: "flux", selections: {} });
  });

  it("keeps the baseline (including null) when the current model is empty", () => {
    const loadedNull: MediaSettings = {
      image: null,
      video: { providerId: "relay", logicalModelId: "kling", selections: {} },
    };
    const out = buildMediaWriteBack(
      { imgProvider: "", imgModel: "", vidProvider: "relay", vidModel: "kling" },
      loadedNull,
    );
    expect(out.image).toBeNull();
    expect(out.video).toEqual({ providerId: "relay", logicalModelId: "kling", selections: {} });
  });

  it("never nulls the other kind when one kind is empty", () => {
    const out = buildMediaWriteBack(
      { imgProvider: "ark", imgModel: "seedream", vidProvider: "", vidModel: "" },
      loaded,
    );
    expect(out.image).toEqual({ providerId: "ark", logicalModelId: "seedream", selections: { ratio: "16:9" } });
    expect(out.video).toEqual({ providerId: "relay", logicalModelId: "kling", selections: { dur: "5" } });
  });
});
