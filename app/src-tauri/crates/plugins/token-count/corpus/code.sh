#!/usr/bin/env bash
set -euo pipefail

root="${1:-.}"
count=0
while IFS= read -r -d '' file; do
  lines=$(wc -l < "$file")
  printf '%6d  %s\n' "$lines" "${file#"$root"/}"
  count=$((count + 1))
done < <(find "$root" -type f -name '*.md' -print0 | sort -z)
echo "total: $count files" >&2
