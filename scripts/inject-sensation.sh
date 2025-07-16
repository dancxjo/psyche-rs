#!/bin/sh
# Inject sensations from JSONL into the quick socket.
# Usage: cat soul/memory/sensation.jsonl | inject-sensation.sh /run/quick.sock

set -e

SOCK=$1
if [ -z "$SOCK" ]; then
    echo "Usage: inject-sensation.sh SOCKET" >&2
    exit 1
fi

while IFS= read -r line; do
    PATH=$(echo "$line" | jq -r '.path')
    TEXT=$(echo "$line" | jq -r '.text')
    printf '%s\n%s\n---\n' "$PATH" "$TEXT" | nc -U "$SOCK"
done
