export type SubmitState = "idle" | "submitting" | "submitted" | "error";

/**
 * Prompt sent to the current agent when the user clicks Regenerate on a
 * `regenerable` canvas. `execute_canvas` always stamps a fresh canvasId, so a
 * new canvas card is expected; the id here only anchors WHICH draft to redo.
 */
export function regeneratePrompt(canvasId: string): string {
  return (
    `The user asked to regenerate the draft in canvas "${canvasId}". ` +
    `Using the current conversation context, produce a new, meaningfully ` +
    `different draft of that same content, then render it again as a Canvas ` +
    `for confirmation (a fresh canvas is expected) and end the turn. ` +
    `Do not ask me questions.`
  );
}

/** Whether the Regenerate button is interactable. */
export function canRegenerate(a: {
  regenerable: boolean;
  canSubmit: boolean;
  regenerating: boolean;
  submitState: SubmitState;
}): boolean {
  return (
    a.regenerable &&
    a.canSubmit &&
    !a.regenerating &&
    a.submitState !== "submitting" &&
    a.submitState !== "submitted"
  );
}
