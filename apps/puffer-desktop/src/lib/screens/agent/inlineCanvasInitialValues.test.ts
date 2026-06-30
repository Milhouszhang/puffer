import { describe, it, expect } from "vitest";
import { initialValues } from "./inlineCanvasInitialValues";

describe("initialValues new primitives", () => {
  it("collects textarea/editableTable/mediaPicker", () => {
    const v = initialValues({ body: [
      { type: "textarea", id: "script" },
      { type: "editableTable", id: "sb", rows: [["a","b"]] },
      { type: "mediaPicker", id: "one" },
      { type: "mediaPicker", id: "many", multi: true },
    ]});
    expect(v).toEqual({ script: "", sb: [["a","b"]], one: null, many: [] });
  });
  it("keeps existing primitives unchanged", () => {
    const v = initialValues({ body: [{ type: "toggle", id: "t" }] });
    expect(v).toEqual({ t: false });
  });
  it("falls back to [] for editableTable without rows", () => {
    const v = initialValues({ body: [{ type: "editableTable", id: "sb" }] });
    expect(v).toEqual({ sb: [] });
  });
  it("copies editableTable rows so the seed never aliases the spec", () => {
    const rows = [["a", "b"]];
    const v = initialValues({ body: [{ type: "editableTable", id: "sb", rows }] });
    expect(v.sb).toEqual(rows);
    expect(v.sb).not.toBe(rows);
    expect((v.sb as unknown[][])[0]).not.toBe(rows[0]);
  });
  it("seeds the four fixed keys for a mediaModelSelect node", () => {
    const v = initialValues({ body: [{ type: "mediaModelSelect" }] });
    expect(v).toEqual({ imgProvider: "", imgModel: "", vidProvider: "", vidModel: "" });
  });
  it("seeds mediaPicker selection from value (default-checked characters)", () => {
    const v = initialValues({ body: [
      { type: "mediaPicker", id: "pick", multi: true, value: ["a", "b", "c"],
        items: [{ id: "a" }, { id: "b" }, { id: "c" }] },
    ]});
    expect(v).toEqual({ pick: ["a", "b", "c"] });
  });
  it("seeds dependentSelect to its first option id", () => {
    const v = initialValues({ body: [
      { type: "singleSelect", id: "p", options: [{ id: "byteplus", label: "BytePlus" }] },
      { type: "dependentSelect", id: "m", dependsOn: "p", options: [
        { id: "seedream", label: "Seedream", group: "byteplus" },
        { id: "other", label: "Other", group: "elsewhere" },
      ] },
    ]});
    expect(v).toEqual({ p: "byteplus", m: "seedream" });
  });
});
