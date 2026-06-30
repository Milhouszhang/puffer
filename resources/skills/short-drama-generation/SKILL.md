---
name: short-drama-generation
description: Use when the user asks to create a short drama from a prompt — e.g. "生成短剧", "制作微短剧", "make a short drama", "turn this script into a short drama", "ショートドラマを生成", "숏드라마를 만들어". Orchestrates script, storyboard, optional character images, per-shot video clips, and ffmpeg composition through the existing media tools.
allowed-tools:
  - Bash
  - Read
  - Write
  - Canvas
  - CanvasState
user-invocable: true
disable-model-invocation: false
requires-action: true
---

You orchestrate a short drama by driving the existing media tools yourself. There is
no single short-drama-generation tool. allowed-tools is guidance; media generation is enforced by
the internal tool permission path.

Trigger only on a request to CREATE/generate a short drama. Requests to analyze,
rewrite, summarize, or brainstorm a script do NOT trigger this skill unless the user
asks to produce the drama. Progress-only or promise-only replies are not completion:
after starting, either drive the pipeline or report the concrete blocker plainly.

## Gating mechanism (confirm each authored stage before spending credits)

Each stage that authors content (script, storyboard, character images, per-shot
results, final order) is gated through the existing `Canvas`/`CanvasState` tools so
the user reviews and edits a draft before you act on it. The mechanism uses no new
infrastructure — it is render → end-turn → re-entry → read-back:

- In a desktop inline environment, render the draft with `Canvas`, using
  `canvasId` = `canvas-drama-<slug>-stage<N>` (N = the stage number; `<slug>` is just a
  cosmetic label — the backend assigns the real canvasId). Then **end the
  current turn** — do NOT busy-wait or poll inside the skill.
- The user edits the Canvas and submits; submission automatically starts a new turn.
  That new turn **first** calls `CanvasState` with the same `canvasId` to read back the
  confirmed `values`, then writes artifacts and advances to the next stage.
- **Non-desktop environment** (no inline submit / no turn re-trigger): degrade to
  text-based per-stage confirmation, stating plainly that there is no visual gating in
  this environment.
- Never call `imagegen`/`videogen` and never advance on draft values before the
  confirmation for that stage has been read back.

## Pipeline (run in order; skip any stage whose inputs the prompt already supplies)

Form the drama `<id>` as `<slug>-<session8>`, where `<slug>` is a short kebab slug of
the drama title and `<session8>` is the first 8 characters of the session id (the
`sessionId` field returned by every `CanvasState` read-back — first available after the
Stage 0 read-back). The `<session8>` suffix is what makes each run land in a **fresh**
directory; without it, re-running a similar prompt collides with an earlier run's files
and the manifest write fails. Project files go under `.puffer/media/drama/<id>/`.

**Mint `<id>` exactly once** — when you first create `.puffer/media/drama/<id>/` (the
Stage 0 manifest write, which happens after the Stage 0 read-back) — then reuse that
**exact** `<id>` verbatim for every later stage, recovering it from your own earlier
writes in the conversation. Never re-derive a bare title slug, and never write into a
pre-existing drama directory. (Same session asked for a second drama → append `-2`,
`-3`, ….) Generated image/video artifacts are written by the tools to
`.puffer/media/images|videos/` — you only reference them, never relocate them.

0. **Models (gate before any credit-consuming stage).** Confirm the image and video
   provider/model up front. Both are mandatory.

   Render `Canvas` with `canvasId = canvas-drama-<slug>-stage0` whose body is a single
   `mediaModelSelect` node:

   ```json
   { "type": "Canvas", "canvasId": "canvas-drama-<slug>-stage0",
     "spec": { "title": "Models", "body": [ { "type": "mediaModelSelect" } ] } }
   ```

   The node is self-populating: it fetches the connected image/video capabilities itself,
   seeds each dropdown from the currently-saved global media defaults, renders an in-place
   "connect a provider in Settings" prompt for any kind with no connected provider, and on
   confirmation persists the choice back to the global media settings. Do **not** fetch
   capabilities, build options, or branch on empty lists yourself. Then **end the turn**.

   In the next turn read back with `CanvasState` (same canvasId): `values` carries
   `{imgProvider,imgModel,vidProvider,vidModel}`. **Validate**: if `imgModel` or
   `vidModel` is empty, stop and report that both image and video models must be selected (direct the
   user to Settings if a kind has no provider). Record these values in `manifest.json` for Stages 3/4 — this
   manifest write first creates `.puffer/media/drama/<id>/`, so this is where you mint
   `<id> = <slug>-<session8>` using the `sessionId` from this read-back.

1. **Script.** If the prompt already contains a script (or names a script file), use it
   directly (no gate needed). Otherwise draft one, then gate it: render
   `Canvas` with `canvasId = canvas-drama-<slug>-stage1` and spec
   `{title:"Script draft",body:[{type:"textarea",id:"script",rows:14,value:"<draft>"}]}`
   The spec is exactly this — the canvas title is the only heading. Do **not** add a
   `summary`, do **not** wrap the textarea in a `card`, and do **not** set `regenerable`;
   the script draft textarea shows directly with only a Submit action. Then end the turn.
   In the next turn read it back with `CanvasState` (same canvasId)
   and save `values.script` to `.puffer/media/drama/<id>/script.md`.

2. **Storyboard.** If the prompt already contains a shot breakdown, use it directly.
   Otherwise break the script into ordered shots (aim for a handful; one beat per shot).
   Give each shot a stable lowercase id (`shot-001`, `shot-002`, …) and record: subject,
   action, scene, lighting, camera, style, target duration (seconds), which characters
   appear, and any stability constraints. These fields become the video prompt — richer
   shots yield better clips.

   Gate the draft: render `Canvas` with `canvasId = canvas-drama-<slug>-stage2` and spec
   `{title:"Storyboard",body:[{type:"editableTable",id:"storyboard",layout:"cards",columns:["shotId","subject","action","duration","characters"],rows:<draft shots>}]}`
   (`layout:"cards"` renders one card per shot with column 0 = shotId as the card
   title and the rest as labeled wrapping fields — the editableTable sits directly in
   `body`). Do **not** wrap it in a `card` and do **not** set `regenerable`. Then end
   the turn. In the next
   turn read it back with `CanvasState`: `values` for the editableTable id
   `"storyboard"` is the confirmed 2D array. In one shot, write
   `.puffer/media/drama/<id>/storyboard.md` (a markdown table of the confirmed rows) and
   seed `manifest.json`'s `shots[]` — column 0 is the `shotId`, the remaining columns
   become the shot's prompt fields.

3. **Character images (reference for video).** Scan the prompt for image references that
   are `https://` or `asset://` URLs.
   - If present, use those URLs directly as `--image-reference` in stage 4. Do NOT
     generate images. Note: `asset://` references only resolve on upload-capable video
     providers (e.g. WorldRouter, which uploads the reference itself). A direct-URL provider
     (e.g. BytePlus) sends the reference verbatim to the model and cannot fetch an `asset://`
     handle — if the chosen video provider is direct-URL and the prompt supplied an `asset://`
     reference, ask the user for a public `https://` URL instead of passing it through.
   - If absent and the user wants character-consistent shots, generate **one image per character in parallel** — never fold the cast into a grouped call. Identity is bound by which call you issued — each `imagegen` call is 1:1 with one character, so the returned artifact IS that character's reference; never bind by returned image order (providers do not guarantee it) and never identify a character by inspecting pixels. The call-to-character mapping is known at issue time and is the only binding used.

     **Square is carried by the prompt — never pass `--aspect`.** The reference image
     must read as square, but different image models support different ratio knobs and some
     reject any explicit ratio. Do **not** pass `--aspect`; squareness is carried entirely by
     the mandatory square clause in the per-character prompt below.

     **Style anchor (compose once, reuse verbatim).** Before generating, write a single shared
     style phrase describing the drama's overall look (medium, rendering, palette,
     line/lighting), derived from the storyboard's `style` field — e.g.
     `"flat 2D anime illustration, soft cel shading, muted warm palette, clean outlines"`. It is
     the anchor that keeps the whole cast in one consistent style; every per-character call reuses
     it **verbatim** and never varies it per character. This shared anchor — not a grouped call —
     is what makes the separately-generated cast cohere.

     Collect the distinct character names from the confirmed storyboard's `characters` column and
     emit the per-character `imagegen` calls **together in a single turn** so the backend runs them
     concurrently (one approval unblocks the whole batch). Cap each turn at **5** `imagegen` calls;
     more than 5 characters → send successive turns of ≤5 (e.g. 7 characters → 5 then 2). One
     `imagegen` per character, never folding two into one call — **N characters → N calls → N images**. For each character build:
     `<style anchor>, square 1:1 composition with equal width and height, full-body head-to-toe
     front view of <character + appearance>, standing, centered, plain pure-white background,
     even studio lighting, no text, no letters, no watermark, no logo, no captions` — then run
     `imagegen --prompt "<that prompt>" --count 1 --provider <imgProvider> --model <imgModel>`.
     One call → one character → one image; the N (≤5 per turn) calls go out together as one parallel
     batch, and you read every result back after the batch returns. Never add `--aspect`.

     Make each character stylized / non-photorealistic (cartoon, 3D render, illustration):
     image-to-video providers (e.g. BytePlus) reject photoreal real-person images on moderation.
     **Never combine multiple characters into one image.** For each returned image read the tool
     result's `remoteSourceUrl` (same key the video tool uses):
       - If `remoteSourceUrl` is present, record it under that character in `manifest.json`
         `characterRefs` (`{ "<character>": "<url>" }`) and use it as that character's
         `--image-reference` in stage 4.
       - If a character's image failed or its `remoteSourceUrl` is absent **while other
         characters got one**, that single character has no usable reference: record no
         `characterRefs` entry for it and let it fall back to text-to-video for the shots it
         appears in (Stage 4). The rest proceed normally — one missing image never aborts the cast.
       - If **no** character in the whole cast produced a `remoteSourceUrl`, that is the
         configured image provider not producing referenceable URLs at all — stop and report
         that image-to-video is unavailable. Do NOT silently degrade the entire cast to
         text-to-video; the user chose an image model on purpose.
   - If absent and consistency is not required, run text-to-video in stage 4.

   When you have generated the per-character images for **all** chunks (not after each
   chunk), gate the choice once: render `Canvas` with
   `canvasId = canvas-drama-<slug>-stage3` and `title:"Character image"`, whose `body` is a
   single `mediaPicker` with no wrapping card: `{type:"mediaPicker", id:"pick", multi:true,
   value:[<every item id>], items:[{id,url,label,description}, …]}` — one item per character.
   Set `url` to that character's `remoteSourceUrl` (or its asset url on desktop), `label` to
   the character name only, and `description` to that character's sheet description. `value`
   lists every item id, so all characters are checked by default. Then end the turn. In the
   next turn read it back with `CanvasState`: `pick` is the array of checked item ids; map
   each back to its character via `characterRefs`. For each checked character, its
   **`characterRefs` url (the remote `remoteSourceUrl`)** is the stage 4 `--image-reference`
   value — the picker's `url` is the thumbnail only (it may be a desktop `asset://` url) and is
   never used as the reference. Any unchecked character falls back to text-to-video for the
   shots it appears in. There is no Regenerate toggle — to redo a character, generate it again and
   re-render this canvas.

4. **Per-shot video.** Generate the shots **in parallel**: emit a chunk's `videogen` calls
   **together in a single turn** so the backend runs them concurrently (one approval unblocks
   the batch). Cap each turn at **5** `videogen` calls; more than 5 shots → successive turns
   of ≤5 (e.g. 12 shots → 5, 5, 2). Generation order does not matter here — the final play
   order is confirmed in Stage 5. Build one `videogen` command per shot:
   - `videogen --prompt "<@Image bindings><shot visual + action>" --provider <vidProvider> --model <vidModel>`
   - Add `--image-reference <url>` for each character in that shot's `characters` column that
     has a checked entry, taking the url from `characterRefs[<character>]` (the remote
     `remoteSourceUrl` recorded in Stage 3 — not the picker's display url), in stable order.
     **Bind every reference in the prompt** — the provider does not
     map image→character by upload order, so a multi-character shot mis-assigns faces without
     explicit tags. Prefix the prompt with one binding line per reference, numbered to match
     the `--image-reference` flags exactly (`@Image1` = the first `--image-reference`,
     `@Image2` = the second, …):
     `@Image1 = <character + one-line appearance>, keep this character's face, hair, and outfit
     consistent; @Image2 = <next character + appearance>, …` — THEN the shot's visual + action.
     A shot with one reference still gets its single `@Image1 = …` line. A shot whose
     characters are all unchecked or unavailable runs text-to-video (no `@Image` bindings).
   - Each `videogen` call polls its clip to completion in its own parallel worker, so a chunk
     finishes in roughly the slowest single clip's time, not the sum. Set an explicit long
     Bash timeout within the current Bash cap on **each** call, sized for the slowest single
     clip — never for the whole drama. One call → one finished clip.
   - Read `path` and `artifactId` from the tool result and record both into the
     manifest as `videoPath` and `videoArtifactId` (see below).
   - After **all** shot chunks have finished (not after each chunk), gate the keep/drop
     selection once (mirroring stage 3):
     render `Canvas` with `canvasId = canvas-drama-<slug>-stage4` and
     `title:"Per-shot video"`, whose `body` is a single `mediaPicker` with no wrapping
     card: `{type:"mediaPicker", id:"shots", multi:true, value:[<every succeeded shotId>],
     items:[{id,kind:"video",artifactId,label,description}, …]}` — one item per SUCCEEDED shot.
     Set `id` and `label` to the shotId, `kind` to `"video"`, `artifactId` to that clip's
     `videoArtifactId` from the manifest, and `description` to the shot's prompt summary.
     The picker renders each tile from its artifact's first-frame poster, so the item
     needs no `path`.
     `value` lists every succeeded shotId, so all clips are checked by default. Then end
     the turn. In the next turn read it back with `CanvasState`: `shots` is the array of
     checked shotIds — these are the clips kept for composition; unchecked shots are
     dropped. There is no retry — to redo a shot, re-run its `videogen` and re-render this
     canvas (same as stage 3's redo note). A shot whose `videogen` failed is not added as a
     tile; report failed shots plainly in turn text (see Failure contracts).

5. **Compose.** Before composing, gate the final order and mux mode: render `Canvas` with
   `canvasId = canvas-drama-<slug>-stage5`, a `card` containing an `editableTable`
   (`id:"order"`, `columns: ["shotId"]`, `rows` = the stage-4-kept clips in current order —
   the user confirms/reorders) and a `singleSelect` (`id:"mux"`, options `copy` /
   `re-encode`), then end the turn. In the next turn read it back with `CanvasState`:
   compose in the confirmed `values.order`, preferring stream-copy unless `values.mux` is
   `re-encode`.

   Stitch the successful shot clips in the confirmed order with ffmpeg. First
   probe ffmpeg: `command -v ffmpeg`. If missing, stop and report — do not fake a file.
   Include only the stage-4-kept clips (the confirmed `values.order`); if none were
   kept, skip composition and report. Build the concat list with single-quote escaping (each clip line is
   `file '<path>'`, with any `'` in the path written as `'\''`). Prefer stream-copy
   (clips from the same provider share codec/params); only if concat-copy fails with a
   codec/params mismatch, retry with a re-encode:

   ```bash
   : > .puffer/media/drama/<id>/concat.txt
   # append one line per SUCCEEDED clip, in order (escape single quotes):
   printf "file '%s'\n" "<clip path, ' -> '\\''>" >> .puffer/media/drama/<id>/concat.txt
   # primary: fast, no re-encode
   ffmpeg -f concat -safe 0 -i .puffer/media/drama/<id>/concat.txt \
     -c copy .puffer/media/drama/<id>/final.mp4
   # fallback only if the copy fails on mismatched streams:
   ffmpeg -f concat -safe 0 -i .puffer/media/drama/<id>/concat.txt \
     -c:v libx264 -pix_fmt yuv420p .puffer/media/drama/<id>/final.mp4
   ```

   If some shots failed but others composed, report it as a partial drama and list the
   missing shot ids.

## Manifest (your working ledger — keep it simple)

Maintain `.puffer/media/drama/<id>/manifest.json` as you go. It is a plain ordered list,
not a schema'd artifact:

```json
{
  "id": "<id>",
  "shots": [
    { "shotId": "shot-001", "status": "succeeded", "prompt": "...", "imageReferences": ["https://..."], "videoArtifactId": "...", "videoPath": ".puffer/media/videos/<aid>/..." }
  ],
  "final": ".puffer/media/drama/<id>/final.mp4"
}
```

## Failure contracts (never paper over)

- If a kind has no connected provider, the Stage 0 node shows an in-place "connect a provider in
  Settings" prompt and that model stays empty; on read-back, stop and tell the user to connect a
  provider for that kind in Settings — never fall back to text-to-video or to config defaults.
- If `imgModel` or `vidModel` is empty on read-back, stop and report that both image and video models
  must be selected before continuing.
- Never pass `--aspect` to `imagegen`; squareness comes only from the prompt's square
  clause. This sidesteps "axis ratio value not allowed" / "ratio not mapped" rejections on
  models that only support their own default ratio. If an image still comes back slightly
  non-square, that is acceptable for a reference frame — do not retry with a ratio flag.
- A parallel media batch (Stage 3 images or Stage 4 videos) raises the media approval
  **once** for the whole turn. Choose **Always allow** when first prompted so later chunks
  run with no further prompt; "Approve once" only covers the current turn, so each chunk
  re-prompts. A trailing chunk of a single call uses the normal single-command path and may
  prompt on its own unless Always allow was chosen — this is expected, not an error.
- Do not advance any gated stage on draft values — wait for the stage's confirmation to
  be read back first.
- If `CanvasState` returns no value for a gated stage (the user did not submit), report
  "no confirmation received" and stop; do not fall back to the draft.
- In a non-desktop environment (no Canvas / no inline submit), degrade to text-based
  per-stage confirmation — including provider/model — and say so plainly; do not skip the
  confirmation.
- If a chosen video provider is Relaydance (prompt-only) and the user wants image
  references, report that the configured provider does not support image references.
- If ffmpeg is unavailable or composition fails, report it plainly and keep the
  per-shot clips; do not claim a composed drama was produced.
- Report final-video success only when `final.mp4` actually exists; a missing final
  video can still leave useful per-shot clips — say so rather than implying success.
- Do not hand-author placeholder media (SVG, stills, stub mp4) and present it as
  generated output.
