export type PipeNodeType = "trigger" | "agent" | "gate" | "sink";
export type PipeNode = {
  id: string;
  type: PipeNodeType;
  title?: string;
  name?: string;
  sub?: string;
  kind?: string;
  model?: string;
  tools?: string[];
  desc?: string;
};

export type PipeEdge = {
  from: string;
  to: string;
  label?: string;
};

export type RunState = "running" | "done" | "failed" | "blocked" | "skipped";
export type StepStatus = "done" | "running" | "error";

export type PipeStep = {
  at: string;
  node: string;
  kind: "trigger" | "tool" | "plan" | "gate" | "sink";
  title: string;
  arg?: string;
  status: StepStatus;
  agent?: string;
  by?: string;
};

export type PipeRun = {
  id: string;
  when: string;
  elapsed: string;
  label: string;
  title: string;
  author: string;
  current: string | null;
  state: RunState;
  path: string[];
  steps: PipeStep[];
};

export const PIPE_NODES: PipeNode[] = [
  { id: "trg", type: "trigger", kind: "github", title: "PR opened", sub: "stripe-api" },
  { id: "a1",  type: "agent",   name: "Triage",      model: "haiku",  tools: ["read", "grep"],  desc: "Classify PR, detect risk, find owners." },
  { id: "a2",  type: "agent",   name: "Reviewer",    model: "sonnet", tools: ["read", "diff"],  desc: "Line-by-line review on changed files." },
  { id: "g",   type: "gate",    title: "Human approval", sub: "if risk ≥ med" },
  { id: "a3",  type: "agent",   name: "Test-writer", model: "sonnet", tools: ["edit", "bash"],  desc: "Adds missing coverage." },
  { id: "s",   type: "sink",    kind: "pr", title: "Open PR", sub: "stripe-api" }
];

export const PIPE_EDGES: PipeEdge[] = [
  { from: "trg", to: "a1" },
  { from: "a1",  to: "a2", label: "risk ≥ low" },
  { from: "a2",  to: "g",  label: "needs tests" },
  { from: "g",   to: "a3", label: "approved" },
  { from: "a3",  to: "s",  label: "on pass" }
];

function passing(): PipeStep[] {
  return [
    { at: "+00:00", node: "trg", kind: "trigger", title: "PR opened", status: "done" },
    { at: "+00:03", node: "a1",  kind: "tool", title: "read_file", arg: "changed files", status: "done", agent: "Triage" },
    { at: "+00:09", node: "a1",  kind: "plan", title: "Classify: risk=low", status: "done", agent: "Triage" },
    { at: "+00:14", node: "a2",  kind: "tool", title: "diff_file", arg: "all hunks", status: "done", agent: "Reviewer" },
    { at: "+00:38", node: "a2",  kind: "plan", title: "No blockers · suggest tests", status: "done", agent: "Reviewer" },
    { at: "+00:40", node: "g",   kind: "gate", title: "Auto-approved", status: "done" },
    { at: "+00:44", node: "a3",  kind: "tool", title: "edit_file", arg: "spec.ts", status: "done", agent: "Test-writer" },
    { at: "+01:12", node: "a3",  kind: "tool", title: "bash", arg: "pnpm test", status: "done", agent: "Test-writer" },
    { at: "+01:50", node: "s",   kind: "sink", title: "PR updated · approved", status: "done" }
  ];
}

function lowRisk(): PipeStep[] {
  return [
    { at: "+00:00", node: "trg", kind: "trigger", title: "PR opened", status: "done" },
    { at: "+00:02", node: "a1",  kind: "plan", title: "Classify: risk=low · dep bump", status: "done", agent: "Triage" },
    { at: "+00:10", node: "a2",  kind: "plan", title: "Approved · no tests needed", status: "done", agent: "Reviewer" },
    { at: "+01:40", node: "s",   kind: "sink", title: "PR approved", status: "done" }
  ];
}

function blocked(): PipeStep[] {
  return [
    { at: "+00:00", node: "trg", kind: "trigger", title: "PR opened", status: "done" },
    { at: "+00:05", node: "a1",  kind: "plan", title: "Classify: risk=high · core rewrite", status: "done", agent: "Triage" },
    { at: "+00:22", node: "a2",  kind: "plan", title: "2 blockers: perf regression, missing migration", status: "done", agent: "Reviewer" },
    { at: "+00:22", node: "g",   kind: "gate", title: "Waiting on @harvey", status: "running" }
  ];
}

function failed(): PipeStep[] {
  return [
    { at: "+00:00", node: "trg", kind: "trigger", title: "PR opened", status: "done" },
    { at: "+00:02", node: "a1",  kind: "tool", title: "read_file", arg: "changed files", status: "error", agent: "Triage" },
    { at: "+00:02", node: "a1",  kind: "plan", title: "Abort: repo size exceeds 200MB cap", status: "error", agent: "Triage" }
  ];
}

function skipped(): PipeStep[] {
  return [
    { at: "+00:00", node: "trg", kind: "trigger", title: "PR opened", status: "done" },
    { at: "+00:01", node: "a1",  kind: "plan", title: "Skip: docs-only change", status: "done", agent: "Triage" }
  ];
}

export const RUNS: PipeRun[] = [
  {
    id: "r8", when: "just now", elapsed: "47s", label: "#4281",
    title: "Fix proration for trialing subs", author: "@harvey",
    current: "a2", state: "running", path: ["trg", "a1", "a2"],
    steps: [
      { at: "+00:00", node: "trg", kind: "trigger", title: "PR #4281 opened", by: "@harvey", status: "done" },
      { at: "+00:02", node: "a1",  kind: "tool",    title: "read_file",  arg: "subscription.ts", status: "done",  agent: "Triage" },
      { at: "+00:04", node: "a1",  kind: "tool",    title: "grep",       arg: "'proration'",     status: "done",  agent: "Triage" },
      { at: "+00:08", node: "a1",  kind: "plan",    title: "Classify: risk=medium · billing path", status: "done",  agent: "Triage" },
      { at: "+00:12", node: "a2",  kind: "tool",    title: "diff_file",  arg: "subscription.ts", status: "done",  agent: "Reviewer" },
      { at: "+00:16", node: "a2",  kind: "tool",    title: "diff_file",  arg: "invoice.ts",       status: "done",  agent: "Reviewer" },
      { at: "+00:23", node: "a2",  kind: "plan",    title: "Flag: missing coverage on unused_time branch", status: "done",  agent: "Reviewer" },
      { at: "+00:41", node: "a2",  kind: "tool",    title: "post_review_comment", arg: "line 47", status: "running", agent: "Reviewer" }
    ]
  },
  { id: "r7", when: "8 min ago",    elapsed: "3m 12s", label: "#4280", title: "Add idempotency to invoice.create", author: "@lin",          current: null, state: "done",    path: ["trg", "a1", "a2", "g", "a3", "s"], steps: passing() },
  { id: "r6", when: "27 min ago",   elapsed: "1m 44s", label: "#4279", title: "Chore: bump stripe sdk to 15.3.0",  author: "@renovate-bot", current: null, state: "done",    path: ["trg", "a1", "a2", "s"],            steps: lowRisk() },
  { id: "r5", when: "1 h ago",      elapsed: "2m 08s", label: "#4278", title: "Refactor customer sync into workers", author: "@harvey",     current: null, state: "blocked", path: ["trg", "a1", "a2", "g"],            steps: blocked() },
  { id: "r4", when: "2 h ago",      elapsed: "4m 31s", label: "#4277", title: "Feature: coupon stacking",            author: "@mika",       current: null, state: "done",    path: ["trg", "a1", "a2", "g", "a3", "s"], steps: passing() },
  { id: "r3", when: "4 h ago",      elapsed: "58s",    label: "#4276", title: "Revert accidental dep bump",          author: "@harvey",     current: null, state: "failed",  path: ["trg", "a1"],                       steps: failed() },
  { id: "r2", when: "yesterday",    elapsed: "2m 15s", label: "#4275", title: "Typo fix in README",                  author: "@mika",       current: null, state: "skipped", path: ["trg", "a1"],                       steps: skipped() },
  { id: "r1", when: "yesterday",    elapsed: "3m 41s", label: "#4274", title: "Add webhook retry logic",             author: "@lin",        current: null, state: "done",    path: ["trg", "a1", "a2", "g", "a3", "s"], steps: passing() }
];

export const TRIGGER_ICONS: Record<string, string> = { github: "git", cron: "clock", sentry: "flame", webhook: "globe" };
export const SINK_ICONS: Record<string, string> = { pr: "git", slack: "globe" };
