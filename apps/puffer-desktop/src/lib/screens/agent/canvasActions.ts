// Builds the follow-up message a finding action sends to the agent. Mirrors
// actionMessage() in the HTML fallback (canvas_template.html) so an action
// reads the same to the agent from either surface.

type CanvasNode = Record<string, unknown>;

const VERBS: Record<string, string> = {
  fix: "Fix this issue",
  test: "Add tests for this",
  explain: "Explain this in depth",
};

const TAILS: Record<string, string> = {
  fix: "\n\nPropose the concrete change and apply it.",
  test: "\n\nWrite tests covering the risk path and edge cases.",
};

export function actionIntent(action: CanvasNode): string {
  if (typeof action.intent === "string") return action.intent;
  if (typeof action.kind === "string") return action.kind;
  return "";
}

export function actionLabel(action: CanvasNode): string {
  if (typeof action.label === "string" && action.label) return action.label;
  return actionIntent(action) || "Action";
}

function str(value: unknown): string {
  return typeof value === "string" ? value : "";
}

export function locationText(location: unknown): string {
  if (typeof location === "string") return location;
  if (typeof location !== "object" || location === null) return "";
  const loc = location as CanvasNode;
  const path = str(loc.path) || str(loc.file) || str(loc.location) || str(loc.name);
  if (!path) return "";
  const line = typeof loc.line === "number" || typeof loc.line === "string" ? `:${loc.line}` : "";
  return `${path}${line}`;
}

export function actionMessage(node: CanvasNode, action: CanvasNode, canvasId: string): string {
  const intent = actionIntent(action);
  const verb = VERBS[intent] ?? (str(action.label) || "Continue on this");
  const locations = (Array.isArray(node.locations) ? node.locations : [])
    .map(locationText)
    .filter(Boolean)
    .join(", ");
  let message = `${verb} (from Canvas "${canvasId}").\n\nFinding: ${str(node.title)}`;
  if (str(node.severity)) message += `  [${str(node.severity)}]`;
  if (locations) message += `\nLocation: ${locations}`;
  if (str(node.body)) message += `\nContext: ${str(node.body)}`;
  if (str(node.evidence)) message += `\nEvidence:\n${str(node.evidence)}`;
  message += TAILS[intent] ?? "";
  return message;
}
