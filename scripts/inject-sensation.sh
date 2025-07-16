#!/usr/bin/env bash
# Inject sensations from JSONL into the quick socket.
# Usage: cat soul/memory/sensation.jsonl | ./inject-sensation.sh /run/quick.sock

set -euo pipefail

SOCK=$1
if [ -z "$SOCK" ]; then
    echo "Usage: inject-sensation.sh SOCKET" >&2
    exit 1
fi

JQ_BIN=$(command -v jq || true)
NC_BIN=$(command -v nc || true)

if [ -z "$JQ_BIN" ]; then
    echo "Error: jq not found in PATH" >&2
    exit 1
fi

if [ -z "$NC_BIN" ]; then
    echo "Error: nc (netcat) not found in PATH" >&2
    exit 1
fi

while IFS= read -r line; do
    PATH=$(echo "$line" | "$JQ_BIN" -r '.path')
    TEXT=$(echo "$line" | "$JQ_BIN" -r '.text')
    printf '%s\n%s\n---\n' "$PATH" "$TEXT" | "$NC_BIN" -U "$SOCK"
done
