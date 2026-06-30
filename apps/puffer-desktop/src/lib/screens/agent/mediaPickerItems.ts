// Pure rendering-decision logic for the Canvas `mediaPicker` node. Kept out of
// InlineCanvasNode.svelte so the image-vs-video branching, poster-URL dedup, and
// preview wiring are unit-testable without mounting a component.

export type MediaItemKind = "image" | "video";

export type MediaPickerItem = {
  id?: unknown;
  label?: unknown;
  description?: unknown;
  kind?: unknown;
  url?: unknown;
  artifactId?: unknown;
};

/** Resolved poster URLs keyed by artifactId. A present key means the artifact's
 *  poster has been resolved already; an empty-string value is the failure
 *  sentinel (the preview RPC returned non-available or threw) so the artifact is
 *  never re-resolved. */
export type ResolvedPosterUrls = Readonly<Record<string, string>>;

/** Explicit per-item discrimination — never URL/extension sniffing. Anything
 *  that is not exactly `"video"` is treated as an image (the default). */
export function mediaItemKind(item: MediaPickerItem): MediaItemKind {
  return item.kind === "video" ? "video" : "image";
}

export function mediaItemArtifactId(item: MediaPickerItem): string {
  return typeof item.artifactId === "string" ? item.artifactId : "";
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

/** The artifactIds of video items (with a usable artifactId) whose poster has not
 *  yet been resolved, so the caller mints each one exactly once. Keyed purely on
 *  artifactId — independent of the item's selectable `id` — and deduped against
 *  the already-resolved map. */
export function videoItemsToResolve(
  items: MediaPickerItem[],
  resolved: ResolvedPosterUrls
): string[] {
  const out: string[] = [];
  for (const item of items) {
    if (mediaItemKind(item) !== "video") continue;
    const artifactId = mediaItemArtifactId(item);
    if (!artifactId) continue;
    if (Object.prototype.hasOwnProperty.call(resolved, artifactId)) continue;
    out.push(artifactId);
  }
  return out;
}

/** Resolved render state of one media item's thumbnail: its kind, the URL to load
 *  for the still image, and whether that image can be shown. An image always
 *  renders from its `url`; a video renders its first-frame poster, available only
 *  once a non-empty poster URL is resolved, otherwise the cell shows a
 *  placeholder. The click-to-open video preview resolves its playable URL
 *  separately, on demand. */
export type MediaItemView = { kind: MediaItemKind; url: string; available: boolean };

export function mediaItemView(item: MediaPickerItem, resolved: ResolvedPosterUrls): MediaItemView {
  if (mediaItemKind(item) === "video") {
    const url = asString(resolved[mediaItemArtifactId(item)]);
    return { kind: "video", url, available: url.length > 0 };
  }
  return { kind: "image", url: asString(item.url), available: true };
}
