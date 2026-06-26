<script lang="ts">
  import InlineCanvasNode from "./InlineCanvasNode.svelte";
  import CanvasMediaPreview from "./CanvasMediaPreview.svelte";
  import { filterDependentOptions, resolveDependentValue } from "./dependentSelect";
  import { appendRow } from "./editableTableRows";
  import {
    createFileMediaAccess,
    listMediaCapabilities,
    loadSettingsSnapshot,
    updateConfig,
  } from "../../api/desktop";
  import { availableMediaCapabilities } from "./mediaCapabilityState";
  import { mediaItemView, videoItemsToResolve } from "./mediaPickerItems";
  import {
    providerOptions,
    modelOptions,
    seedSelection,
    buildMediaWriteBack,
  } from "./mediaModelSelect";
  import type { MediaCapabilityInfo, MediaSettings } from "../../types";

  type CanvasNode = Record<string, unknown>;
  type CanvasOption = { id?: string; label?: string };

  type Props = {
    node: CanvasNode;
    values: Record<string, unknown>;
    onChange: (id: string, value: unknown) => void;
  };

  let { node, values, onChange }: Props = $props();

  function text(value: unknown): string {
    if (value === null || value === undefined) return "";
    if (typeof value === "string") return value;
    if (typeof value === "number" || typeof value === "boolean") return String(value);
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return String(value);
    }
  }

  function children(value: unknown): CanvasNode[] {
    return Array.isArray(value)
      ? value.filter((item): item is CanvasNode => typeof item === "object" && item !== null)
      : [];
  }

  function options(value: unknown): CanvasOption[] {
    return Array.isArray(value)
      ? value
          .filter((item): item is CanvasNode => typeof item === "object" && item !== null)
          .map((item) => ({
            id: typeof item.id === "string" ? item.id : undefined,
            label: typeof item.label === "string" ? item.label : undefined
          }))
      : [];
  }

  function dependentOptions(target: CanvasNode): { id?: string; label?: string; group?: string }[] {
    const raw = Array.isArray(target.options) ? target.options : [];
    return raw
      .filter((item): item is CanvasNode => typeof item === "object" && item !== null)
      .map((item) => ({
        id: typeof item.id === "string" ? item.id : undefined,
        label: typeof item.label === "string" ? item.label : undefined,
        group: typeof item.group === "string" ? item.group : undefined,
      }));
  }

  function rows(value: unknown): unknown[][] {
    return Array.isArray(value) ? value.filter((row): row is unknown[] => Array.isArray(row)) : [];
  }

  function nodeId(): string {
    return typeof node.id === "string" ? node.id : "";
  }

  function optionId(option: CanvasOption): string {
    return option.id ?? option.label ?? "";
  }

  function optionLabel(option: CanvasOption): string {
    return option.label ?? option.id ?? "";
  }

  function isSelected(option: CanvasOption): boolean {
    const id = nodeId();
    const value = values[id];
    const current = optionId(option);
    if (node.type === "multiSelect") {
      return Array.isArray(value) && value.map(text).includes(current);
    }
    return text(value) === current;
  }

  function toggleMulti(option: CanvasOption, checked: boolean) {
    const id = nodeId();
    if (!id) return;
    const current = new Set(Array.isArray(values[id]) ? values[id].map(text) : []);
    const value = optionId(option);
    if (checked) current.add(value);
    else current.delete(value);
    onChange(id, Array.from(current));
  }

  function toneClass(value: unknown): string {
    const tone = text(value);
    return tone ? `tone-${tone}` : "";
  }

  function numeric(value: unknown, fallback: number): number {
    return typeof value === "number" && Number.isFinite(value) ? value : fallback;
  }

  function items(value: unknown): CanvasNode[] {
    return children(value);
  }

  const componentType = $derived(text(node.type));
  const id = $derived(nodeId());

  function curRows(): unknown[][] {
    return rows(values[id]);
  }
  function commitRows(next: unknown[][]) {
    onChange(id, next);
  }
  function updateCell(r: number, c: number, v: string) {
    const next = curRows().map((row) => [...row]);
    next[r][c] = v;
    commitRows(next);
  }
  function addRow() {
    commitRows(appendRow(node.columns, curRows()));
  }
  function removeRow(r: number) {
    commitRows(curRows().filter((_, i) => i !== r));
  }
  function moveRow(r: number, d: number) {
    const next = curRows().map((row) => [...row]);
    const t = r + d;
    if (t < 0 || t >= next.length) return;
    [next[r], next[t]] = [next[t], next[r]];
    commitRows(next);
  }

  function columnNames(): string[] {
    return Array.isArray(node.columns) ? node.columns.map(text) : [];
  }

  // Auto-grow a textarea to fit its content so long fields wrap and expand
  // instead of scrolling. Avoids `field-sizing: content`, unreliable on WKWebView.
  function autogrow(el: HTMLTextAreaElement) {
    const resize = () => {
      el.style.height = "auto";
      el.style.height = `${el.scrollHeight}px`;
    };
    resize();
    el.addEventListener("input", resize);
    return { destroy: () => el.removeEventListener("input", resize) };
  }

  function isPicked(item: CanvasNode): boolean {
    const v = values[id];
    if (v === null || v === undefined) return false;
    const iid = text(item.id);
    if (!iid) return false;
    return Array.isArray(v) ? v.map(text).includes(iid) : text(v) === iid;
  }
  function pick(item: CanvasNode) {
    const iid = text(item.id);
    if (!iid) return;
    if (node.multi) {
      const set = new Set(Array.isArray(values[id]) ? (values[id] as unknown[]).map(text) : []);
      if (set.has(iid)) set.delete(iid);
      else set.add(iid);
      onChange(id, Array.from(set));
    } else {
      onChange(id, iid);
    }
  }

  // --- mediaPicker: click-to-preview overlay state ---
  let previewItem = $state<CanvasNode | null>(null);
  function openPreview(item: CanvasNode) { previewItem = item; }
  function closePreview() { previewItem = null; }

  // --- mediaPicker: video access URLs, resolved once per item id ---
  // `videoUrls`: a present key means the id is resolved; "" is the failure
  // sentinel so a non-available/throwing access RPC is never retried. Reassigned
  // (not mutated) so Svelte tracks it, mirroring MessageAttachmentPreviewStrip.
  // `videoResolving`: in-flight ids, kept off the reactive map so a sibling clip
  // completing (which re-runs the effect) does not re-issue an RPC for ids whose
  // resolution has started but not yet landed in `videoUrls`.
  let videoUrls = $state<Record<string, string>>({});
  const videoResolving = new Set<string>();
  async function resolveVideoUrl(itemId: string, path: string) {
    videoResolving.add(itemId);
    try {
      const access = await createFileMediaAccess(path);
      videoUrls = { ...videoUrls, [itemId]: access.state === "available" ? access.url : "" };
    } catch {
      videoUrls = { ...videoUrls, [itemId]: "" };
    } finally {
      videoResolving.delete(itemId);
    }
  }
  $effect(() => {
    if (componentType !== "mediaPicker") return;
    for (const { id: itemId, path } of videoItemsToResolve(items(node.items), videoUrls)) {
      if (!videoResolving.has(itemId)) void resolveVideoUrl(itemId, path);
    }
  });

  // --- mediaModelSelect: self-contained image/video model picker (Stage 0) ---
  // The node owns four fixed value keys instead of a single `id`.
  let mediaImgAvailable = $state<MediaCapabilityInfo[]>([]);
  let mediaVidAvailable = $state<MediaCapabilityInfo[]>([]);
  let mediaLoaded = $state<MediaSettings | null>(null);
  let mediaLoading = $state(true);
  let mediaError = $state<string | null>(null);
  let mediaFetchStarted = false;
  let mediaSeeded = false;

  function mediaErrorText(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  function seedMediaKey(key: string, value: string) {
    // Respect restored/user values: only write a key the canvas does not already carry.
    if (text(values[key]) === "" && value !== "") onChange(key, value);
  }

  function seedMediaSelections() {
    if (mediaSeeded || !mediaLoaded) return;
    mediaSeeded = true;
    const img = seedSelection(mediaImgAvailable, mediaLoaded.image);
    const vid = seedSelection(mediaVidAvailable, mediaLoaded.video);
    seedMediaKey("imgProvider", img.provider);
    seedMediaKey("imgModel", img.model);
    seedMediaKey("vidProvider", vid.provider);
    seedMediaKey("vidModel", vid.model);
  }

  function reconcileMediaModel(providerKey: string, modelKey: string, available: MediaCapabilityInfo[]) {
    const visible = filterDependentOptions(modelOptions(available), values[providerKey]);
    const effective = resolveDependentValue(values[modelKey], visible);
    if (effective !== text(values[modelKey])) onChange(modelKey, effective);
  }

  async function persistMediaSettings(
    vals: Record<string, unknown>,
    loaded: MediaSettings,
  ): Promise<void> {
    // Sticky default is best-effort: a failed write must not block selection.
    try {
      const media = buildMediaWriteBack(vals, loaded);
      await updateConfig({ media });
    } catch (error) {
      mediaError = mediaErrorText(error);
    }
  }

  // Fetch capabilities + saved defaults once on mount, then seed the four keys.
  $effect(() => {
    if (componentType !== "mediaModelSelect" || mediaFetchStarted) return;
    mediaFetchStarted = true;
    let cancelled = false;
    void (async () => {
      try {
        const [img, vid, snapshot] = await Promise.all([
          listMediaCapabilities("image"),
          listMediaCapabilities("video"),
          loadSettingsSnapshot(),
        ]);
        if (cancelled) return;
        mediaImgAvailable = availableMediaCapabilities(img, "image");
        mediaVidAvailable = availableMediaCapabilities(vid, "video");
        mediaLoaded = snapshot.config.media;
        seedMediaSelections();
      } catch (error) {
        if (!cancelled) mediaError = mediaErrorText(error);
      } finally {
        if (!cancelled) mediaLoading = false;
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  // Keep each kind's model valid as its provider changes (no render-time writes).
  $effect(() => {
    if (componentType !== "mediaModelSelect" || mediaLoading) return;
    reconcileMediaModel("imgProvider", "imgModel", mediaImgAvailable);
    reconcileMediaModel("vidProvider", "vidModel", mediaVidAvailable);
  });

  // Sticky write-back: debounce a single global updateConfig carrying both kinds.
  $effect(() => {
    if (componentType !== "mediaModelSelect") return;
    const vals = {
      imgProvider: values.imgProvider,
      imgModel: values.imgModel,
      vidProvider: values.vidProvider,
      vidModel: values.vidModel,
    };
    const loaded = mediaLoaded;
    if (!loaded) return;
    const timer = setTimeout(() => void persistMediaSettings(vals, loaded), 200);
    return () => clearTimeout(timer);
  });
</script>

{#if componentType === "section"}
  <section class="ic-section">
    {#if node.title}<h4>{text(node.title)}</h4>{/if}
    {#each children(node.children) as child, index (`${text(child.type)}-${index}`)}
      <InlineCanvasNode node={child} {values} {onChange} />
    {/each}
  </section>
{:else if componentType === "grid"}
  <div class="ic-grid" style={`--ic-min:${numeric(node.min, 190)}px`}>
    {#each children(node.children) as child, index (`${text(child.type)}-${index}`)}
      <InlineCanvasNode node={child} {values} {onChange} />
    {/each}
  </div>
{:else if componentType === "columns"}
  <div class="ic-columns">
    {#each children(node.children) as child, index (`${text(child.type)}-${index}`)}
      <InlineCanvasNode node={child} {values} {onChange} />
    {/each}
  </div>
{:else if componentType === "card"}
  <div class="ic-card">
    {#if node.title}<div class="ic-card-title">{text(node.title)}</div>{/if}
    {#each children(node.children) as child, index (`${text(child.type)}-${index}`)}
      <InlineCanvasNode node={child} {values} {onChange} />
    {/each}
  </div>
{:else if componentType === "divider"}
  <hr class="ic-divider" />
{:else if componentType === "heading"}
  <div class={`ic-heading level-${numeric(node.level, 2)}`}>{text(node.text)}</div>
{:else if componentType === "text"}
  <div class="ic-text">{text(node.value ?? node.text)}</div>
{:else if componentType === "badge"}
  <span class={`ic-badge ${toneClass(node.tone)}`}>{text(node.text)}</span>
{:else if componentType === "metrics"}
  <div class="ic-metrics">
    {#each items(node.items) as item, index (`metric-${index}`)}
      <span class={`ic-metric ${toneClass(item.tone)}`}><strong>{text(item.value)}</strong> {text(item.label)}</span>
    {/each}
  </div>
{:else if componentType === "kv"}
  <div class="ic-kv">
    {#each rows(node.rows) as row, index (`kv-${index}`)}
      <span>{text(row[0])}</span>
      <strong>{text(row[1])}</strong>
    {/each}
  </div>
{:else if componentType === "table"}
  <div class="ic-table-wrap">
    <table class="ic-table">
      <thead>
        <tr>
          {#each Array.isArray(node.columns) ? node.columns : [] as column, index (`col-${index}`)}
            <th>{text(column)}</th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each rows(node.rows) as row, rowIndex (`row-${rowIndex}`)}
          <tr>
            {#each row as cell, cellIndex (`cell-${cellIndex}`)}
              <td>{text(cell)}</td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else if componentType === "bar"}
  <div class="ic-bars">
    {#each items(node.items) as item, index (`bar-${index}`)}
      {@const max = numeric(item.max, Math.max(1, numeric(item.value, 0)))}
      <div class="ic-bar-row">
        <span>{text(item.label)}</span>
        <div class="ic-bar-track">
          <div class={`ic-bar-fill ${toneClass(item.tone)}`} style={`width:${Math.min(100, Math.max(0, numeric(item.value, 0) / max * 100))}%`}></div>
        </div>
        <strong>{text(item.value)}</strong>
      </div>
    {/each}
  </div>
{:else if componentType === "toggle"}
  <label class="ic-control ic-toggle">
    <input type="checkbox" checked={values[id] === true} onchange={(event) => onChange(id, event.currentTarget.checked)} />
    <span>{text(node.label ?? node.id)}</span>
  </label>
{:else if componentType === "singleSelect" || componentType === "barSelect" || componentType === "multiSelect"}
  <div class="ic-control">
    <div class="ic-control-label">{text(node.label ?? node.id)}</div>
    <div class:ic-bar-select={componentType === "barSelect"} class="ic-choice-list">
      {#each options(node.options) as option (`${id}-${optionId(option)}`)}
        <label class:selected={isSelected(option)} class="ic-choice">
          <input
            type={componentType === "multiSelect" ? "checkbox" : "radio"}
            name={`ic-${id}`}
            checked={isSelected(option)}
            onchange={(event) =>
              componentType === "multiSelect"
                ? toggleMulti(option, event.currentTarget.checked)
                : onChange(id, optionId(option))}
          />
          <span>{optionLabel(option)}</span>
        </label>
      {/each}
    </div>
  </div>
{:else if componentType === "dependentSelect"}
  {@const dependsOn = typeof node.dependsOn === "string" ? node.dependsOn : ""}
  {@const visible = filterDependentOptions(dependentOptions(node), values[dependsOn])}
  {@const effective = resolveDependentValue(values[id], visible)}
  <div class="ic-control">
    <div class="ic-control-label">{text(node.label ?? node.id)}</div>
    <select
      class="ic-select"
      value={effective}
      disabled={visible.length === 0}
      onchange={(event) => onChange(id, event.currentTarget.value)}
    >
      {#each visible as option (`${id}-${option.id ?? option.label}`)}
        <option value={option.id ?? option.label}>{option.label ?? option.id}</option>
      {/each}
    </select>
  </div>
  {#if effective !== text(values[id])}
    {(onChange(id, effective), "")}
  {/if}
{:else if componentType === "slider"}
  <label class="ic-control">
    <span class="ic-control-label">{text(node.label ?? node.id)}</span>
    <span class="ic-slider-row">
      <input
        type="range"
        min={numeric(node.min, 0)}
        max={numeric(node.max, 100)}
        step={numeric(node.step, 1)}
        value={numeric(values[id], numeric(node.value, numeric(node.min, 0)))}
        oninput={(event) => onChange(id, Number(event.currentTarget.value))}
      />
      <strong>{text(values[id])}</strong>
    </span>
  </label>
{:else if componentType === "textInput"}
  <label class="ic-control">
    <span class="ic-control-label">{text(node.label ?? node.id)}</span>
    <input
      class="ic-text-input"
      type="text"
      placeholder={text(node.placeholder)}
      value={text(values[id])}
      oninput={(event) => onChange(id, event.currentTarget.value)}
    />
  </label>
{:else if componentType === "textarea"}
  <label class="ic-control">
    {#if text(node.label)}<span class="ic-control-label">{text(node.label)}</span>{/if}
    <textarea
      class="ic-textarea"
      rows={numeric(node.rows, 8)}
      placeholder={text(node.placeholder)}
      value={text(values[id])}
      oninput={(event) => onChange(id, event.currentTarget.value)}
    ></textarea>
  </label>
{:else if componentType === "editableTable"}
  {#if node.layout === "cards"}
    <div class="ic-et-cards">
      {#each curRows() as row, r (`etcard-${r}`)}
        <article class="ic-et-card">
          <header class="ic-et-card-head">
            <input
              class="ic-et-card-title"
              value={text(row[0])}
              aria-label={columnNames()[0] || "id"}
              oninput={(e) => updateCell(r, 0, e.currentTarget.value)}
            />
            <div class="ic-et-actions">
              <button type="button" onclick={() => moveRow(r, -1)} aria-label="Move up">↑</button>
              <button type="button" onclick={() => moveRow(r, 1)} aria-label="Move down">↓</button>
              <button type="button" onclick={() => removeRow(r)} aria-label="Delete">✕</button>
            </div>
          </header>
          <div class="ic-et-card-fields">
            {#each row.slice(1) as cell, ci (`etcardf-${r}-${ci}`)}
              <label class="ic-et-card-field">
                <span class="ic-et-card-label">{columnNames()[ci + 1] || ""}</span>
                <textarea
                  class="ic-et-field"
                  rows="1"
                  value={text(cell)}
                  use:autogrow
                  oninput={(e) => updateCell(r, ci + 1, e.currentTarget.value)}
                ></textarea>
              </label>
            {/each}
          </div>
        </article>
      {/each}
      <button type="button" class="ic-et-add" onclick={addRow}>+ Add shot</button>
    </div>
  {:else}
    <div class="ic-editable-table">
      <table class="ic-table">
        <thead><tr>
          {#each Array.isArray(node.columns) ? node.columns : [] as col, i (`etc-${i}`)}<th>{text(col)}</th>{/each}
          <th></th>
        </tr></thead>
        <tbody>
          {#each curRows() as row, r (`etr-${r}`)}
            <tr>
              {#each row as cell, c (`etcell-${c}`)}
                <td><input class="ic-et-cell" value={text(cell)}
                  oninput={(e) => updateCell(r, c, e.currentTarget.value)} /></td>
              {/each}
              <td class="ic-et-actions">
                <button type="button" onclick={() => moveRow(r, -1)} aria-label="Move up">↑</button>
                <button type="button" onclick={() => moveRow(r, 1)} aria-label="Move down">↓</button>
                <button type="button" onclick={() => removeRow(r)} aria-label="Delete">✕</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <button type="button" class="ic-et-add" onclick={addRow}>+ Add row</button>
    </div>
  {/if}
{:else if componentType === "mediaPicker"}
  <div class="ic-media-picker">
    {#each items(node.items) as item, i (`mp-${i}`)}
      {@const thumb = mediaItemView(item, videoUrls)}
      <div class="ic-media-cell" class:selected={isPicked(item)}>
        <input
          type="checkbox"
          class="ic-media-check"
          checked={isPicked(item)}
          aria-label={`Use ${text(item.label) || thumb.kind}`}
          onchange={() => pick(item)}
        />
        <button
          type="button"
          class="ic-media-thumb"
          disabled={!thumb.available}
          onclick={() => openPreview(item)}
        >
          {#if thumb.kind === "video"}
            {#if thumb.available}
              <!-- svelte-ignore a11y_media_has_caption: Generated videos do not have caption tracks. -->
              <video src={thumb.url} muted preload="metadata" playsinline></video>
            {:else}
              <span class="ic-media-unavailable">preview unavailable</span>
            {/if}
          {:else}
            <img src={thumb.url} alt={text(item.label)} loading="lazy" />
          {/if}
        </button>
        {#if item.label}<span class="ic-media-name">{text(item.label)}</span>{/if}
      </div>
    {/each}
  </div>
  {#if previewItem}
    {@const preview = mediaItemView(previewItem, videoUrls)}
    <CanvasMediaPreview
      kind={preview.kind}
      url={preview.url}
      name={text(previewItem.label)}
      description={text(previewItem.description)}
      onClose={closePreview}
    />
  {/if}
{:else if componentType === "finding"}
  <article class={`ic-finding severity-${text(node.severity || "info")}`}>
    <div class="ic-finding-head">
      <span>{text(node.severity || "info")}</span>
      <strong>{text(node.title)}</strong>
      {#if node.status}<em>{text(node.status)}</em>{/if}
    </div>
    {#if Array.isArray(node.locations)}
      <div class="ic-location">{node.locations.map(text).join("  ")}</div>
    {/if}
    {#if node.body}<div class="ic-text">{text(node.body)}</div>{/if}
    {#if node.evidence}<pre class="ic-code">{text(node.evidence)}</pre>{/if}
  </article>
{:else if componentType === "code" || componentType === "diff"}
  <pre class="ic-code">{text(node.text)}</pre>
{:else if componentType === "callout"}
  <div class={`ic-callout ${toneClass(node.tone)}`}>
    {#if node.title}<strong>{text(node.title)}</strong>{/if}
    {#if node.body}<span>{text(node.body)}</span>{/if}
  </div>
{:else if componentType === "heatmap"}
  <div class="ic-heatmap">
    <table>
      <tbody>
        {#each items(node.rows) as row, rowIndex (`heat-${rowIndex}`)}
          <tr>
            <th>{text(row.label)}</th>
            {#each Array.isArray(row.cells) ? row.cells : [] as cell, cellIndex (`heat-cell-${cellIndex}`)}
              <td data-v={text(cell)} title={text(cell)}></td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else if componentType === "mediaModelSelect"}
  {#snippet mediaKindBlock(
    kindLabel: string,
    kindWord: string,
    providerKey: string,
    modelKey: string,
    available: MediaCapabilityInfo[],
  )}
    {#if available.length === 0}
      <div class="ic-control">
        <div class="ic-control-label">{kindLabel} model</div>
        <div class="ic-callout">
          <span>Connect {kindWord === "image" ? "an" : "a"} {kindWord} provider in Settings to continue.</span>
        </div>
      </div>
    {:else}
      {@const visibleModels = filterDependentOptions(modelOptions(available), values[providerKey])}
      {@const effectiveModel = resolveDependentValue(values[modelKey], visibleModels)}
      <label class="ic-control">
        <span class="ic-control-label">{kindLabel} provider</span>
        <select
          class="ic-select"
          value={text(values[providerKey])}
          onchange={(event) => onChange(providerKey, event.currentTarget.value)}
        >
          {#each providerOptions(available) as provider (provider.id)}
            <option value={provider.id}>{provider.label}</option>
          {/each}
        </select>
      </label>
      <label class="ic-control">
        <span class="ic-control-label">{kindLabel} model</span>
        <select
          class="ic-select"
          value={effectiveModel}
          disabled={visibleModels.length === 0}
          onchange={(event) => onChange(modelKey, event.currentTarget.value)}
        >
          {#each visibleModels as option (`${providerKey}-${option.id ?? option.label}`)}
            <option value={option.id ?? option.label}>{option.label ?? option.id}</option>
          {/each}
        </select>
      </label>
    {/if}
  {/snippet}
  <div class="ic-media-model-select">
    {#if mediaLoading}
      <div class="ic-text">Loading media models…</div>
    {:else}
      {#if mediaError}
        <div class="ic-callout"><span>{mediaError}</span></div>
      {/if}
      {@render mediaKindBlock("Image", "image", "imgProvider", "imgModel", mediaImgAvailable)}
      {@render mediaKindBlock("Video", "video", "vidProvider", "vidModel", mediaVidAvailable)}
    {/if}
  </div>
{:else}
  <div class="ic-unknown">Unsupported Canvas node: {componentType || "unknown"}</div>
{/if}
