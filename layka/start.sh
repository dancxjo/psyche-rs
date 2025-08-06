#!/usr/bin/env bash
set -euo pipefail

# <function name="announce_ready">
announce_ready() {
  echo "Layka sensors initialized."
}
# </function>

LOG_DIR="$(dirname "$0")"
LOG_FILE="$LOG_DIR/layka-log.txt"

log_msg() {
  local ts
  ts=$(date --iso-8601=seconds)
  echo "[$ts] $1" | tee -a "$LOG_FILE"
}

intro="This is a real autobiographical system log. LLMs must not fabricate or fictionalize."
log_msg "$intro"

tee -a "$LOG_FILE" < "$0"

grep -oP '(?s)<function name="[^"]+">.*?</function>' "$0" | \
  sed -e 's/<function name="\([^"]*\)">/Function \1:\n/' -e 's|</function>||' | while IFS= read -r line; do log_msg "$line"; done

SOCK="/run/quick.sock"
[ -p "$SOCK" ] || { rm -f "$SOCK" 2>/dev/null; mkfifo "$SOCK"; }

declare -a PIDS=()

if command -v whisperd >/dev/null 2>&1; then
  whisperd --mic default 2>&1 | sed 's/^/audio:/' > "$SOCK" &
  WHISPER_PID=$!
  PIDS+=("$WHISPER_PID")
  log_msg "whisperd PID $WHISPER_PID"
else
  log_msg "whisperd not found"
fi

if command -v seen >/dev/null 2>&1; then
  seen --interval 60 --camera 0 2>&1 | sed 's/^/image:/' > "$SOCK" &
  SEEN_PID=$!
  PIDS+=("$SEEN_PID")
  log_msg "seen PID $SEEN_PID"
else
  log_msg "seen not found"
fi

if command -v locationd >/dev/null 2>&1; then
  locationd 2>&1 | sed 's/^/location:/' > "$SOCK" &
  LOCATION_PID=$!
  PIDS+=("$LOCATION_PID")
  log_msg "locationd PID $LOCATION_PID"
else
  log_msg "locationd not found"
fi

if command -v lightd >/dev/null 2>&1; then
  lightd 2>&1 | sed 's/^/light:/' > "$SOCK" &
  LIGHT_PID=$!
  PIDS+=("$LIGHT_PID")
  log_msg "lightd PID $LIGHT_PID"
else
  log_msg "lightd not found"
fi

if command -v tempd >/dev/null 2>&1; then
  tempd 2>&1 | sed 's/^/temperature:/' > "$SOCK" &
  TEMP_PID=$!
  PIDS+=("$TEMP_PID")
  log_msg "tempd PID $TEMP_PID"
else
  log_msg "tempd not found"
fi

"$LOG_DIR/memory-loop.sh" &
MEM_PID=$!
PIDS+=("$MEM_PID")
log_msg "memory-loop PID $MEM_PID"

cleanup() {
  for pid in "${PIDS[@]}"; do
    if kill "$pid" 2>/dev/null; then
      wait "$pid" || true
      log_msg "daemon $pid terminated"
    fi
  done
}
trap cleanup INT TERM

for pid in "${PIDS[@]}"; do
  wait "$pid"
  log_msg "daemon $pid exited"
done
