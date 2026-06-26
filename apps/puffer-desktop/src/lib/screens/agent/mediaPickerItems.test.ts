import { describe, it, expect } from "vitest";
import {
  mediaItemKind,
  mediaItemId,
  videoItemsToResolve,
  mediaItemView,
} from "./mediaPickerItems";

describe("mediaItemKind", () => {
  it("returns video only for an explicit kind:'video'", () => {
    expect(mediaItemKind({ kind: "video" })).toBe("video");
  });
  it("defaults to image for image/absent/garbage kinds (no URL sniffing)", () => {
    expect(mediaItemKind({ kind: "image" })).toBe("image");
    expect(mediaItemKind({})).toBe("image");
    expect(mediaItemKind({ kind: 7 })).toBe("image");
    expect(mediaItemKind({ kind: "VIDEO" })).toBe("image");
  });
});

describe("mediaItemId", () => {
  it("reads a string id, else empty", () => {
    expect(mediaItemId({ id: "shot-001" })).toBe("shot-001");
    expect(mediaItemId({ id: 3 })).toBe("");
    expect(mediaItemId({})).toBe("");
  });
});

describe("videoItemsToResolve", () => {
  it("returns each video item's id+path exactly once, skipping images", () => {
    const items = [
      { id: "a", kind: "video", path: "x/a.mp4" },
      { id: "b", kind: "image", url: "http://img/b.png" },
      { id: "c", kind: "video", path: "x/c.mp4" },
    ];
    expect(videoItemsToResolve(items, {})).toEqual([
      { id: "a", path: "x/a.mp4" },
      { id: "c", path: "x/c.mp4" },
    ]);
  });
  it("skips video items missing a usable path or id", () => {
    const items = [
      { id: "a", kind: "video" },
      { id: "", kind: "video", path: "x/a.mp4" },
      { kind: "video", path: "x/b.mp4" },
    ];
    expect(videoItemsToResolve(items, {})).toEqual([]);
  });
  it("does not re-resolve an id already present (success or failure sentinel)", () => {
    const items = [
      { id: "a", kind: "video", path: "x/a.mp4" },
      { id: "b", kind: "video", path: "x/b.mp4" },
    ];
    // a resolved to a url, b resolved to the failure sentinel "" — both are done.
    expect(videoItemsToResolve(items, { a: "http://t/a", b: "" })).toEqual([]);
  });
});

describe("mediaItemView", () => {
  it("image item renders its url and is always available", () => {
    expect(mediaItemView({ kind: "image", url: "http://img/b.png" }, {})).toEqual({
      kind: "image",
      url: "http://img/b.png",
      available: true,
    });
  });
  it("video item with a resolved url is available", () => {
    expect(
      mediaItemView({ id: "a", kind: "video", path: "x/a.mp4" }, { a: "http://t/a" })
    ).toEqual({ kind: "video", url: "http://t/a", available: true });
  });
  it("video item that is unresolved or failed is unavailable with no url", () => {
    const item = { id: "a", kind: "video", path: "x/a.mp4" };
    expect(mediaItemView(item, {})).toEqual({ kind: "video", url: "", available: false });
    expect(mediaItemView(item, { a: "" })).toEqual({ kind: "video", url: "", available: false });
  });
});
