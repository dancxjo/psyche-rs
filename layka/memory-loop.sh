#!/usr/bin/env bash
set -euo pipefail

# <function name="split_sentences">
split_sentences() {
  sed -E 's/([.?!]) +/\1\n/g' | tr -d '\r'
}
# </function>

# log_block TYPE CONTENT
log_block() {
  local type="$1"
  local content="$2"
  local ts
  ts=$(date --iso-8601=seconds)
  local color="\e[0m"
  case "$type" in
    INSTANT) color="\e[32m" ;;
    SITUATION) color="\e[34m" ;;
    FUNCTION) color="\e[35m" ;;
  esac
  echo -e "[$ts]\n${color}${content}\e[0m" >> "$LAYKA_LOG"
  rotate_log
}

rotate_log() {
  local max_bytes=1000000
  if [[ $(stat -c%s "$LAYKA_LOG") -gt $max_bytes ]]; then
    mv "$LAYKA_LOG" "${LAYKA_LOG}.1"
    : > "$LAYKA_LOG"
  fi
}

dispatch_function() {
  local block="$1"
  local name
  name=$(grep -oP '<function name="\K[^"]+' <<<"$block")
  local body
  body=$(grep -oP '(?s)<function name="[^"]+">\K.*?(?=</function>)' <<<"$block")
  log_block FUNCTION "Function $name: $body"
  if declare -F "$name" >/dev/null; then
    "$name" "$body"
  elif [[ -x "$FUNCTIONS_DIR/$name.sh" ]]; then
    "$FUNCTIONS_DIR/$name.sh" "$body"
  else
    echo "unknown function $name" >&2
  fi
}

if [[ "${BASH_SOURCE[0]}" != "$0" ]]; then
  return 0
fi

LOG_DIR="$(dirname "$0")"
SENSATION_FILE="$LOG_DIR/sensations.jsonl"
LAYKA_LOG="$LOG_DIR/layka-log.txt"
TICK_FILE="$LOG_DIR/tick.txt"
FUNCTIONS_DIR="$LOG_DIR/functions"

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
    device="unknown"
    case "$src" in
      image) device="camera0" ;;
      audio) device="mic0" ;;
      *) device="${src}0" ;;
    esac
    json=$(jq -n --arg ts "$ts" --arg modality "$src" --arg device "$device" --arg id "$id" --arg text "$sentence" '{ts:$ts, source:{modality:$modality, device:$device}, id:$id, text:$text}')
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
    log_block INSTANT "#INSTANT $instant"

    recall_text=""
    if [[ -s "$SENSATION_FILE" ]]; then
      recall_text=$("$LOG_DIR/recall.sh" "$joined" | tr '\n' ' ')
    fi

    if [[ "${USE_MOCK_LLM:-0}" = "1" ]]; then
      llm="#SITUATION mock situation"
    else
      prompt="This is a live memory log. No fiction, hallucination, or speculation allowed. Only infer from data present.\nRelevant memories: $recall_text\nRecent instant:\n$instant\nLog tail:\n$(tail -n 20 "$LAYKA_LOG")"
      for attempt in 1 2 3; do
        llm=$(curl -sS http://localhost:11434/api/generate -d "$(jq -n --arg prompt "$prompt" '{model:"llama2", prompt:$prompt}')" | jq -r '.response') && break
        sleep $((attempt * attempt))
      done
    fi
    log_block SITUATION "$llm"
    if [[ -x "$FUNCTIONS_DIR/speak.sh" ]]; then
      "$FUNCTIONS_DIR/speak.sh" "$llm" >/dev/null
    fi

    if grep -q '<function name=' <<<"$llm"; then
      func=$(grep -oP '(?s)<function name="[^"]+">.*?</function>' <<<"$llm")
      dispatch_function "$func"
    fi

    echo "$now" > "$TICK_FILE"
    recent_ids=()
    recent_texts=()
    last_instant=$now
  fi

done < "$SOCK"
