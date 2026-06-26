// Pure rendering-decision logic for the Canvas `mediaPicker` node. Kept out of
// InlineCanvasNode.svelte so the image-vs-video branching, video access-URL
// dedup, and preview wiring are unit-testable without mounting a component.

export type MediaItemKind = "image" | "video";

export type MediaPickerItem = {
  id?: unknown;
  label?: unknown;
  description?: unknown;
  kind?: unknown;
  url?: unknown;
  path?: unknown;
};

/** Resolved video access URLs keyed by item id. A present key means the item has
 *  been resolved already; an empty-string value is the failure sentinel (the
 *  access RPC returned non-available or threw) so the id is never re-resolved. */
export type ResolvedVideoUrls = Readonly<Record<string, string>>;

/** Explicit per-item discrimination — never URL/extension sniffing. Anything
 *  that is not exactly `"video"` is treated as an image (the default). */
export function mediaItemKind(item: MediaPickerItem): MediaItemKind {
  return item.kind === "video" ? "video" : "image";
}

export function mediaItemId(item: MediaPickerItem): string {
  return typeof item.id === "string" ? item.id : "";
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

/** Video items (with a usable id + path) whose access URL has not yet been
 *  resolved, so the caller mints each one exactly once. */
export function videoItemsToResolve(
  items: MediaPickerItem[],
  resolved: ResolvedVideoUrls
): { id: string; path: string }[] {
  const out: { id: string; path: string }[] = [];
  for (const item of items) {
    if (mediaItemKind(item) !== "video") continue;
    const id = mediaItemId(item);
    const path = asString(item.path);
    if (!id || !path) continue;
    if (Object.prototype.hasOwnProperty.call(resolved, id)) continue;
    out.push({ id, path });
  }
  return out;
}

/** Resolved render state of one media item, shared by the grid thumbnail and the
 *  click-to-open preview popup: its kind, the URL to load, and whether it can be
 *  interacted with. An image always renders from its `url`; a video is available
 *  only once a non-empty access URL is resolved, otherwise the cell is shown
 *  disabled ("preview unavailable") and never opens an empty popup. */
export type MediaItemView = { kind: MediaItemKind; url: string; available: boolean };

export function mediaItemView(item: MediaPickerItem, resolved: ResolvedVideoUrls): MediaItemView {
  if (mediaItemKind(item) === "video") {
    const url = asString(resolved[mediaItemId(item)]);
    return { kind: "video", url, available: url.length > 0 };
  }
  return { kind: "image", url: asString(item.url), available: true };
}
