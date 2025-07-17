#!/bin/sh

if [ -z "$1" ]; then
    set -- "layka"
fi

# Run the psyche daemon in development mode
cargo run -p psyched -- --soul all_souls/$1 --socket ./$1.sock --log-level debug
