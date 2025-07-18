#!/bin/sh

if [ -z "$1" ]; then
    set -- "layka"
fi

WHISPER_MODEL=whisper-base.en.bin cargo run -p heard -- --socket ./quick.sock --listen ./ear.sock --log-level debug
