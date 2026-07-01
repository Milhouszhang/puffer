import { expect, test } from "vitest";
import type { MessageAttachment } from "../../types";
import { attachmentOverlayAction } from "./attachmentOverlayAction";

function attachment(overrides: Partial<MessageAttachment>): MessageAttachment {
  return {
    id: "attachment-1",
    name: "pixel.png",
    mimeType: "image/png",
    size: 12,
    extension: "PNG",
    kind: "image",
    state: "available",
    source: { kind: "local_file", path: "/tmp/puffer/pixel.png" },
    ...overrides
  };
}

test("returns null for uploaded local image files", () => {
  expect(attachmentOverlayAction(attachment({}))).toBeNull();
});

test("returns download for remote URL image files", () => {
  expect(
    attachmentOverlayAction(
      attachment({
        source: { kind: "remote_url", url: "https://example.test/pixel.png" },
        previewUrl: "https://example.test/preview.png"
      })
    )
  ).toEqual({
    kind: "download",
    url: "https://example.test/pixel.png",
    suggestedName: "pixel.png"
  });
});

test("returns open folder for generated media with a local path", () => {
  expect(
    attachmentOverlayAction(
      attachment({
        source: {
          kind: "generated_media",
          jobId: "job-1",
          artifactId: "artifact-1",
          index: 0,
          localPath: "/tmp/puffer/.puffer/media/images/artifact-1/pixel.png"
        }
      })
    )
  ).toEqual({
    kind: "open_folder",
    path: "/tmp/puffer/.puffer/media/images/artifact-1/pixel.png"
  });
});

test("returns open folder for generated video with a local path", () => {
  expect(
    attachmentOverlayAction({
      id: "generated-video:artifact-1",
      name: "Generated video",
      mimeType: "video/mp4",
      size: 9,
      extension: "MP4",
      kind: "video",
      state: "available",
      source: {
        kind: "generated_media",
        jobId: "job-1",
        artifactId: "artifact-1",
        index: 0,
        localPath: "/tmp/puffer/.puffer/media/artifacts/artifact-1/generated.mp4"
      }
    })
  ).toEqual({
    kind: "open_folder",
    path: "/tmp/puffer/.puffer/media/artifacts/artifact-1/generated.mp4"
  });
});

test("returns null for generated media without a local path even when it has a preview URL", () => {
  expect(
    attachmentOverlayAction(
      attachment({
        source: {
          kind: "generated_media",
          jobId: "job-1",
          artifactId: "artifact-1",
          index: 0
        },
        previewUrl: "blob:generated-preview"
      })
    )
  ).toBeNull();
});

test("returns null for uploaded local non-image files", () => {
  expect(
    attachmentOverlayAction(
      attachment({
        name: "report.pdf",
        kind: "file",
        mimeType: "application/pdf",
        extension: "PDF",
        source: { kind: "local_file", path: "/tmp/puffer/report.pdf" }
      })
    )
  ).toBeNull();
});

test("returns null for remote-only files with no local path", () => {
  expect(
    attachmentOverlayAction(
      attachment({
        name: "report.pdf",
        kind: "file",
        mimeType: "application/pdf",
        extension: "PDF",
        source: { kind: "remote_url", url: "https://example.test/report.pdf" }
      })
    )
  ).toBeNull();
});
