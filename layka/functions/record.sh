#!/usr/bin/env bash
set -euo pipefail
# record.sh: simulate recording audio for a given duration in seconds.
duration="${1:-1}"
# shellcheck disable=SC2034
file="/tmp/recording.wav" # placeholder for recording file
sleep "$duration"
echo "recorded $duration s" >&2
