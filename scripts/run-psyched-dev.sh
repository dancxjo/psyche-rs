#!/bin/sh
# Run the psyche daemon in development mode
cargo run -p psyched --pipeline psyched/config/default.toml --soul soul
