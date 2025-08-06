#!/usr/bin/env bash
set -euo pipefail
# recall.sh: fuzzy search sensations for a query.
query="${1:-}"
if [[ -z "$query" ]]; then
  echo "usage: $0 <query>" >&2
  exit 1
fi
file="$(dirname "$0")/sensations.jsonl"
if [[ ! -f "$file" ]]; then
  echo "sensations file not found" >&2
  exit 1
fi
jq -r --arg q "$query" 'select(.text | test($q; "i")) | .text' "$file" | head -n 5
