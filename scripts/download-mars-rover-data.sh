#!/usr/bin/env bash
# Download Mars rover photo metadata and stream to a psycheOS socket.
# Usage: ./download-mars-rover-data.sh SOCKET [SOL]
# Requires jq and curl.

set -euo pipefail

SOCK=${1:-}
SOL=${2:-1000}

if [ -z "$SOCK" ]; then
    echo "Usage: download-mars-rover-data.sh SOCKET [SOL]" >&2
    exit 1
fi

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
INJECT="${SCRIPT_DIR}/inject-sensation.sh"

JQ_BIN=$(command -v jq || true)
CURL_BIN=$(command -v curl || true)

if [ -z "$JQ_BIN" ]; then
    echo "Error: jq not found in PATH" >&2
    exit 1
fi

if [ -z "$CURL_BIN" ]; then
    echo "Error: curl not found in PATH" >&2
    exit 1
fi

BASE_URL=${NASA_API_BASE_URL:-"https://api.nasa.gov"}
ROVER=${ROVER:-"perseverance"}
API_KEY=${NASA_API_KEY:-"DEMO_KEY"}

TMP_JSON=$(mktemp)
"$CURL_BIN" -sf "${BASE_URL}/mars-photos/api/v1/rovers/${ROVER}/photos?sol=${SOL}&api_key=${API_KEY}" -o "$TMP_JSON"

if [ "$SOCK" = "-" ]; then
    cat "$TMP_JSON" | "$JQ_BIN" -c '.photos[] | {path:"/telemetry/rover_photo", text:("Rover " + .rover.name + " " + .earth_date + " " + .img_src)}'
else
    cat "$TMP_JSON" | "$JQ_BIN" -c '.photos[] | {path:"/telemetry/rover_photo", text:("Rover " + .rover.name + " " + .earth_date + " " + .img_src)}' \
        | "$INJECT" "$SOCK"
fi

rm "$TMP_JSON"
