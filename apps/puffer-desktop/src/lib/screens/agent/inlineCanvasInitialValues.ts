type CanvasSpec = Record<string, unknown>;
const INTERACTIVE = [
  "toggle", "singleSelect", "multiSelect", "slider", "barSelect", "textInput",
  "textarea", "editableTable", "mediaPicker", "dependentSelect",
];

function defaultValue(type: string, node: CanvasSpec): unknown {
  if (node.value !== undefined) return node.value;
  if (type === "toggle") return false;
  if (type === "multiSelect") return [];
  if (type === "slider") return typeof node.min === "number" ? node.min : 0;
  if (type === "textInput" || type === "textarea") return "";
  // Copy rows so the seeded value never shares identity with the spec prop.
  if (type === "editableTable") {
    return Array.isArray(node.rows) ? node.rows.map((r) => (Array.isArray(r) ? [...r] : r)) : [];
  }
  if (type === "mediaPicker") return node.multi ? [] : null;
  const options = Array.isArray(node.options) ? node.options : [];
  const first = options.find((i) => typeof i === "object" && i !== null) as CanvasSpec | undefined;
  return first?.id ?? first?.label ?? null;
}

function collectInitial(node: unknown, collected: Record<string, unknown>): void {
  if (Array.isArray(node)) { node.forEach((i) => collectInitial(i, collected)); return; }
  if (typeof node !== "object" || node === null) return;
  const rec = node as CanvasSpec;
  const type = typeof rec.type === "string" ? rec.type : "";
  const id = typeof rec.id === "string" ? rec.id : "";
  // The mediaModelSelect node owns four fixed keys instead of a single `id`.
  if (type === "mediaModelSelect") {
    collected.imgProvider = "";
    collected.imgModel = "";
    collected.vidProvider = "";
    collected.vidModel = "";
  }
  if (id && INTERACTIVE.includes(type)) collected[id] = defaultValue(type, rec);
  collectInitial(rec.children, collected);
  collectInitial(rec.body, collected);
}

export function initialValues(root: unknown): Record<string, unknown> {
  const collected: Record<string, unknown> = {};
  collectInitial(root, collected);
  return collected;
}
