<script lang="ts">
  interface Props {
    url: string;
    name: string;
    description?: string;
    kind?: "image" | "video";
    // Video previews resolve their playable URL on demand: `loading` while the
    // access ticket is minted, `unavailable` if it can't be, `ready` to play.
    // Images are always `ready`.
    status?: "ready" | "loading" | "unavailable";
    onClose: () => void;
  }
  let { url, name, description, kind = "image", status = "ready", onClose }: Props = $props();

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="cip-overlay" role="dialog" aria-modal="true" aria-label="Media preview">
  <!-- Full-bleed button behind the panel: clicking outside the panel closes. -->
  <button type="button" class="cip-backdrop" aria-label="Close preview" onclick={onClose}></button>
  <div class="cip-panel">
    <button type="button" class="cip-close" aria-label="Close preview" onclick={onClose}>✕</button>
    {#if status !== "ready"}
      <div class="cip-media cip-status">
        {status === "loading" ? "Loading preview…" : "Preview unavailable"}
      </div>
    {:else if kind === "video"}
      <!-- svelte-ignore a11y_media_has_caption: Generated videos do not have caption tracks. -->
      <video class="cip-media" src={url} controls autoplay playsinline aria-label={name}></video>
    {:else}
      <img class="cip-media" src={url} alt={name} />
    {/if}
    <div class="cip-meta">
      <strong class="cip-name">{name}</strong>
      {#if description}<p class="cip-desc">{description}</p>{/if}
    </div>
  </div>
</div>

<style>
  .cip-overlay {
    position: fixed; inset: 0; z-index: 60;
    display: flex; align-items: center; justify-content: center;
    padding: 24px;
  }
  .cip-backdrop {
    position: absolute; inset: 0;
    border: none; padding: 0; cursor: default;
    /* Theme-aware scrim (matches the app's modals), not a fixed black. */
    background: color-mix(in oklch, var(--background) 30%, transparent 70%);
  }
  .cip-panel {
    position: relative; z-index: 1;
    max-width: min(86vw, 720px); max-height: 88vh;
    display: flex; flex-direction: column; align-items: center; gap: 12px;
    background: var(--card); color: var(--card-foreground);
    border: 1px solid var(--border); border-radius: 12px; padding: 20px;
    box-shadow: 0 24px 64px -12px oklch(0 0 0 / 0.35), 0 4px 16px -4px oklch(0 0 0 / 0.2);
    overflow: auto;
  }
  .cip-close {
    position: absolute; top: 8px; right: 8px;
    background: transparent; border: none; color: inherit;
    font-size: 16px; cursor: pointer; line-height: 1;
  }
  .cip-media { max-width: 100%; max-height: 64vh; object-fit: contain; border-radius: 8px; }
  .cip-status {
    display: flex; align-items: center; justify-content: center;
    width: min(72vw, 420px); aspect-ratio: 16 / 9;
    background: color-mix(in oklab, var(--muted) 40%, transparent);
    color: var(--muted-foreground); font-size: 14px;
  }
  .cip-meta { text-align: center; }
  .cip-name { display: block; font-size: 15px; }
  .cip-desc { margin: 6px 0 0; font-size: 13px; opacity: 0.85; }
</style>
