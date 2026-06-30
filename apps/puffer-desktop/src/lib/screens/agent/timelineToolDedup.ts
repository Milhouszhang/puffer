import type { TimelineItem } from "../../types";

/** Collect the ids of every persisted tool card. Tool cards are keyed by
 *  `tool-{call_id}`, identical on the live and persisted sides, so an id-set
 *  lookup is an order-independent way to tell whether a transient tool item is
 *  already persisted. */
export function persistedToolIdSet(persisted: TimelineItem[]): Set<string> {
  const ids = new Set<string>();
  for (const item of persisted) {
    if (item.kind === "tool") ids.add(item.id);
  }
  return ids;
}

/** True when `item` is a tool card whose id is already in the persisted set.
 *  Non-tool items always return false so they fall back to the existing
 *  signature/anchor matching. */
export function isAlreadyPersistedTool(item: TimelineItem, ids: Set<string>): boolean {
  return item.kind === "tool" && ids.has(item.id);
}
