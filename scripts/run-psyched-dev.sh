#!/bin/sh
# Run the psyche daemon in development mode
cargo run -p psyched -- --socket ./layka.sock --log-level debug
