import { describe, it, expect } from "vitest";
import { actionIntent, actionLabel, actionMessage } from "./canvasActions";

const finding = {
  type: "finding",
  severity: "high",
  title: "Retry loop double-charges on timeout",
  body: "The retry wrapper re-invokes charge() without an idempotency key.",
  evidence: "- charge(order)\n+ charge_idempotent(order, key)",
  locations: [{ path: "src/pay.rs", line: 210 }, "src/retry.rs:31"],
};

describe("actionIntent / actionLabel", () => {
  it("prefers intent, falls back to kind, then empty", () => {
    expect(actionIntent({ intent: "fix", kind: "test" })).toBe("fix");
    expect(actionIntent({ kind: "explain" })).toBe("explain");
    expect(actionIntent({})).toBe("");
  });
  it("labels from label, then intent, then a generic fallback", () => {
    expect(actionLabel({ label: "Suggest change", intent: "fix" })).toBe("Suggest change");
    expect(actionLabel({ intent: "test" })).toBe("test");
    expect(actionLabel({})).toBe("Action");
  });
});

describe("actionMessage", () => {
  it("bundles verb, canvas id, finding context, and the intent tail", () => {
    const msg = actionMessage(finding, { label: "Suggest change", intent: "fix" }, "canvas-42");
    expect(msg).toContain('Fix this issue (from Canvas "canvas-42").');
    expect(msg).toContain("Finding: Retry loop double-charges on timeout  [high]");
    expect(msg).toContain("Location: src/pay.rs:210, src/retry.rs:31");
    expect(msg).toContain("Context: The retry wrapper");
    expect(msg).toContain("Evidence:\n- charge(order)");
    expect(msg).toContain("Propose the concrete change and apply it.");
  });
  it("uses the action label as verb for unknown intents and adds no tail", () => {
    const msg = actionMessage(finding, { label: "Compare options" }, "canvas-1");
    expect(msg.startsWith('Compare options (from Canvas "canvas-1").')).toBe(true);
    expect(msg).not.toContain("Propose the concrete change");
  });
  it("omits sections whose data is missing", () => {
    const msg = actionMessage({ title: "Bare" }, { intent: "explain" }, "canvas-7");
    expect(msg).toContain("Finding: Bare");
    expect(msg).not.toContain("Location:");
    expect(msg).not.toContain("Context:");
    expect(msg).not.toContain("Evidence:");
  });
  it("tolerates malformed locations", () => {
    const msg = actionMessage(
      { title: "T", locations: [null, 5, { line: 3 }, { file: "a.ts" }] },
      { intent: "fix" },
      "canvas-9",
    );
    expect(msg).toContain("Location: a.ts");
  });
});
