#!/usr/bin/env bash
set -euo pipefail

# <function name="announce_ready">
announce_ready() {
  echo "Layka sensors initialized."
}
# </function>

LOG_DIR="$(dirname "$0")"
LOG_FILE="$LOG_DIR/layka-log.txt"

intro="This is a real autobiographical system log. LLMs must not fabricate or fictionalize."
echo "$intro" | tee -a "$LOG_FILE"

tee -a "$LOG_FILE" < "$0"

grep -oP '(?s)<function name="[^"]+">.*?</function>' "$0" | \
  sed -e 's/<function name="\([^"]*\)">/Function \1:\n/' -e 's|</function>||' | tee -a "$LOG_FILE"

SOCK="/run/quick.sock"
[ -p "$SOCK" ] || { rm -f "$SOCK" 2>/dev/null; mkfifo "$SOCK"; }

if command -v whisperd >/dev/null 2>&1; then
  whisperd --mic default 2>&1 | sed 's/^/audio:/' > "$SOCK" &
  WHISPER_PID=$!
  echo "whisperd PID $WHISPER_PID" | tee -a "$LOG_FILE"
else
  echo "whisperd not found" | tee -a "$LOG_FILE"
fi

if command -v seen >/dev/null 2>&1; then
  seen --interval 60 --camera 0 2>&1 | sed 's/^/image:/' > "$SOCK" &
  SEEN_PID=$!
  echo "seen PID $SEEN_PID" | tee -a "$LOG_FILE"
else
  echo "seen not found" | tee -a "$LOG_FILE"
fi

"$LOG_DIR/memory-loop.sh" &
MEM_PID=$!
echo "memory-loop PID $MEM_PID" | tee -a "$LOG_FILE"
