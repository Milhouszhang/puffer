import type { SessionListItem } from "./types";

const DEFAULT_SESSION_NAME = "New Session";
const GENERATED_SESSION_TITLE = /^session-[0-9a-f]{12,}$/i;

function clean(value: string | null | undefined): string {
  return value?.trim() ?? "";
}

function isGeneratedSessionTitle(value: string): boolean {
  return GENERATED_SESSION_TITLE.test(value);
}

/** Returns the user-facing primary name for a session. */
export function sessionDisplayName(session: SessionListItem | null | undefined): string {
  if (!session) return DEFAULT_SESSION_NAME;
  const displayName = clean(session.displayName);
  if (displayName) return displayName;
  const generatedTitle = clean(session.generatedTitle);
  if (generatedTitle) return generatedTitle;
  const title = clean(session.title);
  if (title && !isGeneratedSessionTitle(title)) return title;
  return DEFAULT_SESSION_NAME;
}

/** Returns the optional secondary title, suppressing generated or duplicate labels. */
export function sessionDisplayTitle(session: SessionListItem | null | undefined): string {
  if (!session) return "";
  const title = clean(session.title);
  if (!title || isGeneratedSessionTitle(title)) return "";
  return title === sessionDisplayName(session) ? "" : title;
}
