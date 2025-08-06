#!/usr/bin/env bash
set -euo pipefail

# <function name="split_sentences">
split_sentences() {
  sed -E 's/([.?!]) +/\1\n/g' | tr -d '\r'
}
# </function>

if [[ "${BASH_SOURCE[0]}" != "$0" ]]; then
  return 0
fi

LOG_DIR="$(dirname "$0")"
SENSATION_FILE="$LOG_DIR/sensations.jsonl"
LAYKA_LOG="$LOG_DIR/layka-log.txt"
TICK_FILE="$LOG_DIR/tick.txt"

mkdir -p "$LOG_DIR"
touch "$SENSATION_FILE" "$LAYKA_LOG" "$TICK_FILE"

SOCK="/run/quick.sock"
[ -p "$SOCK" ] || { echo "Socket $SOCK missing, creating" | tee -a "$LAYKA_LOG"; rm -f "$SOCK" 2>/dev/null; mkfifo "$SOCK"; }

declare -a recent_ids=()
declare -a recent_texts=()
last_instant=$(date +%s)

while IFS= read -r line; do
  src="${line%%:*}"
  content="${line#*:}"

  echo "$content" | split_sentences | while IFS= read -r sentence; do
    [[ -z "$sentence" ]] && continue
    ts=$(date --iso-8601=seconds)
    id=$(printf '%s' "$sentence" | sha1sum | awk '{print $1}')
    json=$(jq -n --arg ts "$ts" --arg src "$src" --arg id "$id" --arg text "$sentence" '{ts:$ts, source:$src, id:$id, text:$text}')
    echo "$json" >> "$SENSATION_FILE"
    recent_ids+=("$id")
    recent_texts+=("$sentence")
  done

  now=$(date +%s)
  if (( now - last_instant >= 60 )); then
    ids_json=$(printf '%s\n' "${recent_ids[@]}" | jq -R . | jq -s .)
    joined=$(printf '%s. ' "${recent_texts[@]}")
    how="Observations: $joined"
    instant=$(jq -n --arg ts "$(date --iso-8601=seconds)" --argjson ids "$ids_json" --arg how "$how" '{ts:$ts, ids:$ids, how:$how}')
    echo "#INSTANT $instant" >> "$LAYKA_LOG"

    if [[ "${USE_MOCK_LLM:-0}" = "1" ]]; then
      llm="#SITUATION mock situation"
    else
      prompt="This is a live memory log. No fiction, hallucination, or speculation allowed. Only infer from data present.\nRecent instant:\n$instant\nLog tail:\n$(tail -n 20 "$LAYKA_LOG")"
      llm=$(curl -sS http://localhost:11434/api/generate -d "$(jq -n --arg prompt "$prompt" '{model:"llama2", prompt:$prompt}')" | jq -r '.response')
    fi
    echo "$llm" >> "$LAYKA_LOG"

    if grep -q '<function name=' <<<"$llm"; then
      func=$(grep -oP '(?s)<function name="[^"]+">.*?</function>' <<<"$llm")
      echo "$func" >> "$LAYKA_LOG"
    fi

    echo "$now" > "$TICK_FILE"
    recent_ids=()
    recent_texts=()
    last_instant=$now
  fi

done < "$SOCK"
