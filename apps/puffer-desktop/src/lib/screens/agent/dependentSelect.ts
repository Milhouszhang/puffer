export type DependentOption = { id?: string; label?: string; group?: string };

function optionKey(option: DependentOption): string {
  return option.id ?? option.label ?? "";
}

/** Options whose `group` equals the parent select's current value. */
export function filterDependentOptions(
  options: DependentOption[],
  parentValue: unknown,
): DependentOption[] {
  const parent = typeof parentValue === "string" ? parentValue : "";
  if (!parent) return [];
  return options.filter((option) => option.group === parent);
}

/** Keep the current value if still valid; otherwise reset to the first filtered option. */
export function resolveDependentValue(current: unknown, filtered: DependentOption[]): string {
  const keys = filtered.map(optionKey);
  const cur = typeof current === "string" ? current : "";
  if (cur && keys.includes(cur)) return cur;
  return keys[0] ?? "";
}
