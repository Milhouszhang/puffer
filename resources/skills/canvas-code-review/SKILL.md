---
name: canvas-code-review
description: Recipe for rendering a code review as a Canvas — compose findings with evidence and actions into a scannable page instead of a long text answer.
allowed-tools:
  - Canvas
argument-hint: "[what to review]"
user-invocable: true
disable-model-invocation: false
---

A **recipe** for the `Canvas` tool: how to structure a code review as a Canvas
page. Canvas is a design system — the Canvas tool description documents the
component vocabulary and geometry (the repo's `resources/canvas/components.md`
holds the full catalog). This recipe is the task-level rule for *code review*
specifically.

## When to use
After reviewing a diff/PR/branch, when the result is easier to act on as a
scannable page than as prose. Call `Canvas` once with the composed tree.

## How to organize a code-review Canvas
1. **Header**: `title` = "Code review · <scope>"; `meta` = ["PR #…", "N files", "+x −y"].
2. **Summary first**: one short `summary` (what changed + overall verdict). Then a
   single `metrics` strip: total findings and the per-severity counts
   (color `high`/`crit`). Do **not** also add a decorative KPI grid.
3. **Findings, ranked by risk**: a `section{title:"Findings"}` containing `finding`
   nodes, highest severity first. Every finding **must**:
   - state `severity` and a precise `title`;
   - cite where: `locations:[{path,line}]`;
   - show the proof: `evidence` with the actual diff/code (use `-`/`+` lines),
     not a paraphrase;
   - expose what to do next: `actions` — a `fix` intent ("Suggest change") for
     fixable issues, `test` ("Add test") where coverage is missing, `explain`
     when the reader may need rationale.
4. Optional: a `callout` (at most one, near the top) for the single highest-
   priority thing to address before merge.

## Discipline (this is what separates Canvas from a pretty dashboard)
- **No claim without evidence.** A finding with no location and no diff is just
  an opinion — either attach evidence or drop it.
- **No finding without a next step** when one exists — attach an action.
- Rank by impact; don't pad with low-value notes to fill the page.
- Pick components by data shape (catalog), not by what looks nice.

## Minimal shape
```json
{
  "title": "Code review · feat/x",
  "meta": ["PR #123", "4 files", "+120 −18"],
  "summary": "Adds X. Solid overall; 2 issues before merge, 1 a security note.",
  "body": [
    { "type": "metrics", "items": [
      { "value": "2", "label": "findings" },
      { "value": "1", "label": "high", "tone": "high" } ] },
    { "type": "section", "title": "Findings", "children": [
      { "type": "finding", "severity": "high", "title": "...",
        "locations": [{ "path": "src/x.rs", "line": 42 }],
        "evidence": "- old line\n+ new line",
        "actions": [{ "id": "fix", "label": "Suggest change", "intent": "fix" }] }
    ]}
  ]
}
```

## Action round-trip
Action buttons are wired on both surfaces: clicking one bundles the finding
(title, severity, locations, diff evidence) plus an intent-specific instruction
into a message and continues the conversation as a new agent turn — inline in
Puffer Desktop, or through the daemon bridge from the HTML fallback (degrading
to clipboard copy when no daemon is reachable).
