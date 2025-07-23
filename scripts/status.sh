#!/usr/bin/env bash
set -euo pipefail

# FIFO pipes
TMPDIR=$(mktemp -d)
PIPE_QUICK="$TMPDIR/quick"
PIPE_COMBO="$TMPDIR/combo"
PIPE_REFLECT="$TMPDIR/reflect"
mkfifo "$PIPE_QUICK" "$PIPE_COMBO" "$PIPE_REFLECT"

# Common input stream (e.g. dmesg)
cat /var/log/dmesg \
  | tee "$PIPE_QUICK" "$PIPE_COMBO" "$PIPE_REFLECT" >/dev/null &

# 🧠 Quick — factual distillation @ localhost (slow)
(
  distill -n 25 --continuous \
    --model gemma3:27b \
    --llm-url http://localhost:11434 \
    --prompt "Summarize the most important facts from the following system logs.\n\n{{current}}\n\nPrevious:\n{{previous}}\n\nRespond with one concise, factual sentence. Do not speculate." \
    < "$PIPE_QUICK" \
  | tee ~/.cache/psyche_quick.log
) &

# 🌀 Combobulator — narrative building @ 192.168.1.123
(
  distill -n 25 --continuous \
    --model gemma3:27b \
    --llm-url http://192.168.1.123:11434 \
    --prompt "Summarize the following system state summaries into a coherent description of what is going on. Stay grounded and do not speculate.\n\n{{current}}\n\nPrior summaries:\n\n{{previous}}\n\nRespond with one sentence." \
    < "$PIPE_COMBO" \
  | tee ~/.cache/psyche_combo.log
) &

# 🔬 Reflector — self-thought @ 10.0.0.180
(
  distill -n 25 --continuous \
    --model gemma3:27b \
    --llm-url http://10.0.0.180:11434 \
    --prompt "Read the following logs and summarize what I—the system—am experiencing, as if I were thinking to myself.\n\n{{current}}\n\nPrevious:\n\n{{previous}}\n\nRespond with a first-person sentence. Be factual but introspective." \
    < "$PIPE_REFLECT" \
  | tee ~/.cache/psyche_reflect.log
) &

# Wait for all
wait
rm -rf "$TMPDIR"
