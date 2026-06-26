import { describe, expect, it } from "vitest";
import { normalizeCanvasSpec } from "./canvasSpec";
import { initialValues } from "./inlineCanvasInitialValues";

describe("normalizeCanvasSpec", () => {
  it("coerces a stringified JSON array body into an array", () => {
    const spec = { title: "Script draft", body: '[{"type":"textarea","id":"script","value":"hello"}]' };
    const out = normalizeCanvasSpec(spec);
    expect(Array.isArray(out?.body)).toBe(true);
    expect((out?.body as unknown[]).length).toBe(1);
  });

  it("leaves a real array body unchanged (same reference, no copy)", () => {
    const body = [{ type: "text", value: "hi" }];
    const out = normalizeCanvasSpec({ title: "T", body });
    expect(out?.body).toBe(body);
  });

  it("leaves a spec without a body unchanged", () => {
    const spec = { title: "T" };
    expect(normalizeCanvasSpec(spec)).toBe(spec);
  });

  it("leaves a non-JSON string body unchanged (not a canvas)", () => {
    const out = normalizeCanvasSpec({ title: "T", body: "hello" });
    expect(out?.body).toBe("hello");
  });

  it("leaves a stringified non-array body unchanged (not a canvas)", () => {
    expect(normalizeCanvasSpec({ body: '"hello"' })?.body).toBe('"hello"');
    expect(normalizeCanvasSpec({ body: "42" })?.body).toBe("42");
  });

  it("returns null for null input without throwing", () => {
    expect(normalizeCanvasSpec(null)).toBeNull();
  });

  it("seeds interactive values from a coerced textarea spec", () => {
    const spec = { title: "Script draft", body: '[{"type":"textarea","id":"script","value":"hello"}]' };
    const values = initialValues(normalizeCanvasSpec(spec));
    expect(values.script).toBe("hello");
  });
});
