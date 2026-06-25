import { describe, it, expect } from "vitest";
import { filterDependentOptions, resolveDependentValue } from "./dependentSelect";

const opts = [
  { id: "seedream", label: "Seedream", group: "byteplus" },
  { id: "seededit", label: "Seededit", group: "byteplus" },
  { id: "imagen", label: "Imagen", group: "google" },
];

describe("dependentSelect helpers", () => {
  it("filters options to those whose group matches the parent value", () => {
    expect(filterDependentOptions(opts, "byteplus").map((o) => o.id)).toEqual([
      "seedream",
      "seededit",
    ]);
    expect(filterDependentOptions(opts, "google").map((o) => o.id)).toEqual(["imagen"]);
  });

  it("returns [] when the parent value matches no group", () => {
    expect(filterDependentOptions(opts, "missing")).toEqual([]);
    expect(filterDependentOptions(opts, undefined)).toEqual([]);
  });

  it("keeps the current value when it is still in the filtered set", () => {
    const filtered = filterDependentOptions(opts, "byteplus");
    expect(resolveDependentValue("seededit", filtered)).toBe("seededit");
  });

  it("resets to the first filtered option when the current value is out of range", () => {
    const filtered = filterDependentOptions(opts, "byteplus");
    expect(resolveDependentValue("imagen", filtered)).toBe("seedream");
  });

  it("returns empty string when no option is available", () => {
    expect(resolveDependentValue("seedream", [])).toBe("");
  });
});
