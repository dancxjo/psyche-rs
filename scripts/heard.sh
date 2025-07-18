#!/bin/sh

if [ -z "$1" ]; then
    set -- "layka"
fi

cargo run -p heard -- --socket ./quick.sock --listen ./ear.sock --whisper-model whisper-base.en.bin
