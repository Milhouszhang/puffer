// The model often serializes array arguments as a JSON-encoded string. A
// stringified JSON array has one unambiguous decoding, so we coerce that single
// case; anything else is returned untouched so the caller's `Array.isArray(body)`
// gate correctly treats it as a non-canvas tool call.
//
// When no coercion is needed the input is returned by reference (not copied), so
// callers must treat the result as immutable — the same contract `inputJson`
// already carries everywhere else in ToolCard.
export function normalizeCanvasSpec(
  input: Record<string, unknown> | null,
): Record<string, unknown> | null {
  if (!input || typeof input.body !== "string") return input;
  try {
    const parsed = JSON.parse(input.body);
    if (Array.isArray(parsed)) return { ...input, body: parsed };
  } catch {
    /* not coercible — leave as-is */
  }
  return input;
}
