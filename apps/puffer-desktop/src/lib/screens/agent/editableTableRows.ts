// Row helpers for the editableTable canvas node. Kept pure so the add-row id
// behaviour can be unit-tested without mounting the Svelte component.

export type Rows = unknown[][];

function cellText(value: unknown): string {
  if (value === null || value === undefined) return "";
  return typeof value === "string" ? value : String(value);
}

/** An id-like first column (e.g. "shotId", "sceneID") whose ids we auto-generate. */
export function isAutoIdColumn(column: unknown): boolean {
  return typeof column === "string" && /id$/i.test(column.trim());
}

/** The id prefix derived from the column name: "shotId" -> "shot". */
function autoIdBase(column: string): string {
  return column.trim().replace(/id$/i, "").toLowerCase() || "row";
}

/**
 * The next "<base>-NNN" id for an id-like column, taken from the highest existing
 * numeric suffix so ids stay unique even after rows are removed or reordered.
 */
export function nextAutoId(column: string, rows: Rows): string {
  const base = autoIdBase(column);
  const re = new RegExp(`^${base}-(\\d+)$`, "i");
  let max = 0;
  for (const row of rows) {
    const match = re.exec(cellText(row?.[0]).trim());
    if (match) max = Math.max(max, Number(match[1]));
  }
  return `${base}-${String(max + 1).padStart(3, "0")}`;
}

/**
 * Append a blank row, auto-filling column 0 with the next "<base>-NNN" id when the
 * first column is id-like (e.g. the storyboard's "shotId"). Input rows are not mutated.
 */
export function appendRow(columns: unknown, rows: Rows): Rows {
  const cols = Array.isArray(columns) ? columns : [];
  const width = Math.max(rows[0]?.length ?? cols.length, 1);
  const blank: unknown[] = Array(width).fill("");
  if (isAutoIdColumn(cols[0])) blank[0] = nextAutoId(cols[0] as string, rows);
  return [...rows.map((row) => [...row]), blank];
}
