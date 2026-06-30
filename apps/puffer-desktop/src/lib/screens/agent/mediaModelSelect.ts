import type {
  MediaCapabilityInfo,
  MediaGenerationSettings,
  MediaSettings,
} from "../../types";

export type ProviderOption = { id: string; label: string };
export type ModelOption = { id: string; label: string; group: string };
export type Selection = { provider: string; model: string };

function str(value: unknown): string {
  return typeof value === "string" ? value : "";
}

/** Distinct providers across the available capabilities, in first-seen order. */
export function providerOptions(available: MediaCapabilityInfo[]): ProviderOption[] {
  const seen = new Set<string>();
  const options: ProviderOption[] = [];
  for (const capability of available) {
    if (seen.has(capability.providerId)) continue;
    seen.add(capability.providerId);
    options.push({
      id: capability.providerId,
      label: capability.providerDisplayName || capability.providerId,
    });
  }
  return options;
}

/** One model option per capability, grouped by its provider id. */
export function modelOptions(available: MediaCapabilityInfo[]): ModelOption[] {
  return available.map((capability) => ({
    id: capability.modelId,
    label: capability.modelDisplayName || capability.modelId,
    group: capability.providerId,
  }));
}

/**
 * Seed a kind's (provider, model) from the saved global default:
 * - no capabilities → empty;
 * - saved provider + model still available → keep both;
 * - saved provider available but model gone → keep provider, first model under it;
 * - saved provider gone (or none) → first available provider + its first model.
 */
export function seedSelection(
  available: MediaCapabilityInfo[],
  saved: MediaGenerationSettings | null,
): Selection {
  if (available.length === 0) return { provider: "", model: "" };

  const savedProvider = saved?.providerId ?? "";
  const providerCaps = available.filter((c) => c.providerId === savedProvider);
  if (savedProvider && providerCaps.length > 0) {
    const savedModel = saved?.logicalModelId ?? "";
    if (savedModel && providerCaps.some((c) => c.modelId === savedModel)) {
      return { provider: savedProvider, model: savedModel };
    }
    return { provider: savedProvider, model: providerCaps[0].modelId };
  }

  const first = available[0];
  return { provider: first.providerId, model: first.modelId };
}

/** Both an image and a video model must be chosen for the canvas to submit. */
export function selectionComplete(values: Record<string, unknown>): boolean {
  return str(values.imgModel).length > 0 && str(values.vidModel).length > 0;
}

/** Whether a canvas spec contains a `mediaModelSelect` node anywhere in its tree. */
export function hasMediaModelSelect(spec: unknown): boolean {
  if (Array.isArray(spec)) return spec.some((node) => hasMediaModelSelect(node));
  if (typeof spec !== "object" || spec === null) return false;
  const record = spec as Record<string, unknown>;
  if (record.type === "mediaModelSelect") return true;
  return hasMediaModelSelect(record.children) || hasMediaModelSelect(record.body);
}

function writeBackKind(
  provider: string,
  model: string,
  baseline: MediaGenerationSettings | null,
): MediaGenerationSettings | null {
  // No chosen model (kind has no provider, or not yet picked) → keep the baseline as-is.
  if (!model) return baseline;
  const unchanged =
    baseline !== null && baseline.providerId === provider && baseline.logicalModelId === model;
  return {
    providerId: provider,
    logicalModelId: model,
    selections: unchanged ? baseline.selections : {},
  };
}

/**
 * Build the full `{ image, video }` object to persist so a single `updateConfig({ media })`
 * never clobbers the other kind. Per kind: preserve baseline axis selections when the
 * (provider, model) pair is unchanged, reset to `{}` on any change, keep the baseline
 * (including `null`) when no model is chosen.
 */
export function buildMediaWriteBack(
  values: Record<string, unknown>,
  loaded: MediaSettings,
): MediaSettings {
  return {
    image: writeBackKind(str(values.imgProvider), str(values.imgModel), loaded.image),
    video: writeBackKind(str(values.vidProvider), str(values.vidModel), loaded.video),
  };
}
