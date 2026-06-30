import { describe, it, expect } from "vitest";
import { regeneratePrompt, canRegenerate } from "./canvasRegenerate";

describe("regeneratePrompt", () => {
  it("names the canvasId and forbids questions", () => {
    const p = regeneratePrompt("canvas-drama-7-stage1");
    expect(p).toContain("canvas-drama-7-stage1");
    expect(p.toLowerCase()).toContain("do not ask");
    expect(p.toLowerCase()).toContain("regenerate");
  });
});

describe("canRegenerate", () => {
  const base = {
    regenerable: true,
    canSubmit: true,
    regenerating: false,
    submitState: "idle" as const,
  };
  it("is true when regenerable, submittable, idle", () => {
    expect(canRegenerate(base)).toBe(true);
  });
  it("is false when the canvas is not regenerable", () => {
    expect(canRegenerate({ ...base, regenerable: false })).toBe(false);
  });
  it("is false when the canvas cannot submit (no session/handler)", () => {
    expect(canRegenerate({ ...base, canSubmit: false })).toBe(false);
  });
  it("is false while a regenerate is in flight", () => {
    expect(canRegenerate({ ...base, regenerating: true })).toBe(false);
  });
  it("is false while submitting and after submitted", () => {
    expect(canRegenerate({ ...base, submitState: "submitting" })).toBe(false);
    expect(canRegenerate({ ...base, submitState: "submitted" })).toBe(false);
  });
});
