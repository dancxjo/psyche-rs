#!/usr/bin/env bash
set -euo pipefail
# blink_led.sh: simulate LED blink feedback.
count="${1:-1}"
for ((i=0;i<count;i++)); do
  echo "blink" >&2
  sleep 0.1
  echo "off" >&2
  sleep 0.1
done
