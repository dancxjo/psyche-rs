#!/usr/bin/env bash
set -euo pipefail
# speak.sh: synthesize speech for provided text or print to stdout.
text="${1:-}"
if command -v say >/dev/null 2>&1; then
  say "$text"
else
  echo "speak: $text"
fi
