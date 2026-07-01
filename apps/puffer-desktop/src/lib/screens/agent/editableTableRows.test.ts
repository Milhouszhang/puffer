import { describe, it, expect } from "vitest";
import { appendRow, nextAutoId, isAutoIdColumn } from "./editableTableRows";

describe("editableTable add-row id generation", () => {
  const cols = ["shotId", "subject", "action", "duration", "characters"];

  it("auto-fills the next shot-NNN id when the first column is id-like", () => {
    const rows = [
      ["shot-001", "a", "", "", ""],
      ["shot-002", "b", "", "", ""],
    ];
    const next = appendRow(cols, rows);
    expect(next).toHaveLength(3);
    expect(next[2][0]).toBe("shot-003");
    expect(next[2].slice(1)).toEqual(["", "", "", ""]);
  });

  it("starts at shot-001 for an empty table", () => {
    expect(appendRow(cols, [])[0][0]).toBe("shot-001");
  });

  it("uses the max existing numeric suffix, not the row count", () => {
    const rows = [["shot-005", "", "", "", ""]];
    expect(appendRow(cols, rows)[1][0]).toBe("shot-006");
  });

  it("derives the id base from the column name", () => {
    expect(nextAutoId("sceneId", [])).toBe("scene-001");
  });

  it("leaves the first cell blank when the first column is not id-like", () => {
    const next = appendRow(["name", "value"], [["x", "1"]]);
    expect(next[1]).toEqual(["", ""]);
  });

  it("does not mutate the input rows", () => {
    const rows = [["shot-001", "a", "", "", ""]];
    appendRow(cols, rows);
    expect(rows).toHaveLength(1);
  });

  it("recognises id-like columns case-insensitively", () => {
    expect(isAutoIdColumn("shotID")).toBe(true);
    expect(isAutoIdColumn("subject")).toBe(false);
  });
});
