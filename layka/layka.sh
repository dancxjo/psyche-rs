#!/usr/bin/env bash
set -euo pipefail
cmd="${1:-}"
case "$cmd" in
  say)
    shift
    msg="$*"
    sock="/run/quick.sock"
    if [[ ! -p "$sock" ]]; then
      echo "socket $sock not found" >&2
      exit 1
    fi
    echo "audio:$msg" > "$sock"
    ;;
  recall)
    shift
    "$(dirname "$0")/recall.sh" "$*"
    ;;
  *)
    echo "usage: $0 {say <text>|recall <query>}" >&2
    exit 1
    ;;
 esac
