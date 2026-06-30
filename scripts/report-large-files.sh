#!/usr/bin/env bash
set -euo pipefail

threshold="${1:-1000}"
cd "$(dirname "$0")/.."

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

while IFS= read -r -d '' file; do
  lines="$(wc -l < "$file" | tr -d ' ')"
  if [ "$lines" -gt "$threshold" ]; then
    printf "%7d %s\n" "$lines" "${file#./}" >> "$tmp"
  fi
done < <(
  find . \
    \( -path './.git' -o -path './target' -o -path './vendor' -o -path './.worktree' -o -path './node_modules' \) -prune -o \
    -type f \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' -o -name '*.js' -o -name '*.mjs' -o -name '*.svelte' -o -name '*.css' \) \
    -print0
)

if [ ! -s "$tmp" ]; then
  echo "No source files exceed ${threshold} lines."
  exit 0
fi

echo "Source files over ${threshold} lines:"
sort -nr "$tmp"
