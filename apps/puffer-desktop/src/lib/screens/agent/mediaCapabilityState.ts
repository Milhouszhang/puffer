import type { MediaCapabilityInfo, MediaKind } from "../../types";

export function availableMediaCapabilities(
  capabilities: MediaCapabilityInfo[],
  kind: MediaKind
): MediaCapabilityInfo[] {
  return capabilities.filter(
    (capability) => capability.kind === kind && capability.status === "available"
  );
}

function unavailableMediaProviderLabels(
  capabilities: MediaCapabilityInfo[],
  kind: MediaKind
): string[] {
  const labels: string[] = [];
  const seen = new Set<string>();
  for (const capability of capabilities) {
    if (capability.kind !== kind || capability.status !== "unavailable") continue;
    if (capability.reason !== "missing_auth") continue;
    if (seen.has(capability.providerId)) continue;
    seen.add(capability.providerId);
    labels.push(capability.providerDisplayName || capability.providerId);
  }
  return labels;
}

export function mediaCapabilityConnectStateMessage(
  capabilities: MediaCapabilityInfo[],
  kind: MediaKind
): string | null {
  if (kind !== "video") return null;
  if (availableMediaCapabilities(capabilities, kind).length > 0) return null;
  const labels = unavailableMediaProviderLabels(capabilities, kind);
  if (labels.length === 0) return null;
  return `Connect ${formatProviderList(labels)} to enable video generation.`;
}

function formatProviderList(labels: string[]): string {
  if (labels.length <= 1) return labels[0] ?? "";
  return `${labels.slice(0, -1).join(", ")} or ${labels[labels.length - 1]}`;
}
