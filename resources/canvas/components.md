# Canvas component catalog

Machine-readable usage knowledge for the `Canvas` tool's design system. This is
the layer that makes Canvas a *design system*, not a schema: it tells the agent
not just that a component exists, but **when to use it, when not to, and how**.
Compose a tree of these into the `body` of a Canvas spec. Prefer information
density and evidence over decoration.

> Discipline (anti-slop): every claim a node makes — a metric, a finding, a risk
> — should be backed by **evidence** (a file location, a diff, a command output)
> and, where the user could act, an **action**. A node is useful because it lets
> the reader inspect or act, not because it looks nice. Do not emit decorative
> cards/metrics/charts that aren't tied to evidence.

---

## Layout

### section
- purpose: a titled group of related nodes; the main structural unit.
- use_when: you have a labeled part of the page ("Findings", "Coverage").
- props: `title`. children: any nodes.

### grid
- purpose: equal cards laid out responsively.
- use_when: several peer items of similar weight (e.g. metric cards, file cards).
- avoid_when: a single item, or items with a natural order/ranking → use a list or table.
- props: `min` (px, min column width). children: usually `card`.

### columns
- purpose: a few side-by-side blocks (e.g. summary | details).
- props: none. children: 2–3 nodes.

### card
- purpose: a small bordered container for one coherent sub-unit.
- avoid_when: framing a single sentence — just use `text`. (slop signal)
- props: `title?`. children: any.

### divider
- purpose: a thin rule between unrelated blocks. Use sparingly.

## Text
### heading `{level:1|2|3, text}` — a title within content.
### text `{value}` — a paragraph; plain text, preserves newlines.
### badge `{text, tone?}` — a small status label; tone: crit|high|ok|info.

## Data
### metrics
- purpose: an inline summary strip of counts ("3 findings · 1 high · 8 files").
- use_when: a handful of headline numbers at the top of a result.
- avoid_when: a single number (use text), or values needing comparison (use bar).
- props: `items:[{value,label,tone?}]`. tone colors the number (crit|high|ok).

### kv `{rows:[[key,value]]}` — compact key/value facts (metadata, config).
### table `{columns:[..], rows:[[..]]}` — tabular records; the default for lists of structured rows. Use for precise values and ranking.

## Viz
### bar
- purpose: compare a small set of categories by magnitude.
- use_when: "energy transition speed by region", relative scores.
- avoid_when: time series (not supported), or part-of-whole percentages.
- props: `items:[{label,value,max?,tone?}]`.

### heatmap
- purpose: intensity across two axes (region × risk dimension).
- use_when: a small matrix where relative intensity matters more than exact value.
- props: `columns:[..]`, `rows:[{label, cells:[0..4]}]` (0 none → 4 critical).

## Input
Interactive nodes collect typed user choices that the agent can later read with
`CanvasState`. Every interactive node MUST have a stable `id`; that id becomes
the key in `CanvasState.values`.

### toggle `{id,label,value?,help?}`
- purpose: a binary yes/no or enabled/disabled choice.
- use_when: the user should include/exclude one behavior.

### singleSelect `{id,label,options:[{id,label}],value?,help?}`
- purpose: one choice from a short option set.
- use_when: the options are mutually exclusive.

### dependentSelect `{id,label?,dependsOn,options:[{id,label,group}],value?,help?}`
- purpose: a single-choice dropdown whose options are filtered by another select's value.
- use_when: a second choice depends on a first (e.g. model depends on provider). Pair it with a
  `singleSelect` whose `id` equals this node's `dependsOn`; each option's `group` is the parent value
  it belongs to.
- avoid_when: the choice is independent → use `singleSelect`.
- writes back: the chosen option `id` (a string). When the parent changes, the value auto-resets to
  the first option in the new group.

### multiSelect `{id,label,options:[{id,label}],value?:string[],help?}`
- purpose: several independent choices from a short option set.
- use_when: the user may select more than one area, file, filter, or action.

### barSelect `{id,label,options:[{id,label}],value?,help?}`
- purpose: one choice that benefits from being displayed near comparative bars.
- use_when: the user is choosing a ranked category rather than entering data.

### slider `{id,label,min?,max?,step?,value?,help?}`
- purpose: a numeric preference or threshold.
- use_when: the exact number can be approximate and adjusted visually.

### textInput `{id,label,placeholder?,value?,help?}`
- purpose: a short free-form value.
- avoid_when: collecting secrets or long text; use dedicated secret/user input
  tools instead.

### textarea `{id,label?,placeholder?,rows?,value?}`
- purpose: multiline free-form text editing (a script draft, a long prompt).
- use_when: the value spans multiple lines and the user should review/edit it inline.
- avoid_when: a one-line value → use `textInput`.
- writes back: a string (the edited text).

### editableTable `{id,columns,rows,label?}`
- purpose: an editable grid the user can edit cell-by-cell plus add, remove, and
  reorder rows.
- use_when: the user confirms/edits a list of structured rows (e.g. a storyboard
  of shots) before the agent acts on it.
- avoid_when: read-only records → use `table`.
- writes back: a 2D array (`rows`) reflecting the final edits and order.

### mediaPicker `{id,items:[{id,kind?,url?,artifactId?,label?,description?}],multi?,value?}`
- purpose: pick from a grid of images or videos.
- use_when: the user selects one (or, with `multi:true`, several) generated
  candidate images or video clips.
- item kind: `kind` is `"image"` (default) or `"video"` — explicit, never
  inferred from the URL/extension. Image items carry a directly-loadable `url`;
  video items carry the generated clip's `artifactId` (from the `videogen` tool
  result), through which the frontend resolves both the first-frame poster
  thumbnail and the on-demand streaming preview URL.
- preview: on desktop, clicking an item opens a preview. Image items show the
  enlarged image; video items play the clip with controls. The grid thumbnail of
  a video item is its first-frame poster image (resolved from `artifactId`); a
  clip whose poster is unavailable still opens to play. Each preview also shows
  that item's `description` (centered).
- writes back: single → the chosen item `id`; multi → an array of selected ids.

### mediaModelSelect `{}`
- purpose: pick the image-gen and video-gen provider/model for a run.
- use_when: gating the media model choice before any credit-consuming generation (short-drama Stage 0).
- self-populating: takes no `id` and no `options`. It fetches the connected image/video capabilities
  itself, seeds each dropdown from the saved global media defaults, shows an in-place "connect a
  provider in Settings" prompt for any kind with no connected provider, and persists the confirmed
  choice back to the global media settings.
- writes back: four fixed keys — `imgProvider`, `imgModel`, `vidProvider`, `vidModel`. Submit is
  gated until both `imgModel` and `vidModel` are chosen.

## Evidence + rich (carry the proof)
### finding
- purpose: one issue/observation that the reader may need to act on. The core
  unit of a code review / audit.
- MUST carry: `severity` (critical|high|medium|low|info) and `title`.
- SHOULD carry evidence: `locations:[{path,line}]` and/or `evidence` (diff/code text).
- SHOULD carry `actions` when the user can do something (fix/test/explain).
- props: `{severity,title,status?,locations?,body?,evidence?,actions?:[{id,label,intent?}]}`
  intent: fix|test|explain.
- avoid_when: a neutral fact with no severity → use text/kv. Don't inflate notes
  into findings.

### diff `{text}` / code `{text}` — monospace block; `+`/`-` lines are tinted as a diff. Use to show the actual change/evidence, not to paraphrase it.
### callout `{tone, title, body}` — ONE highlighted takeaway (the single most important thing). tone: crit|high|ok|info. Use at most once near the top. Overuse is a slop signal.

---

## Composition notes
- Lead with a `metrics` strip or a single `callout`, not both.
- Rank findings by severity; the highest-impact item first.
- Keep evidence next to the claim (diff inside the finding, not in a separate box).
- Pick the component that fits the data: ranking → table/bar, part-of-whole →
  (not bar — say so in text), matrix → heatmap, issue-to-act-on → finding.
