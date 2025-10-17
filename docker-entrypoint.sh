#!/bin/bash
set -e

# Development mode entrypoint
# Compiles and runs the CLI with mounted source code

# Check if we need to build
if [ ! -f "/app/target/release/obamify" ] || [ "$FORCE_BUILD" = "true" ]; then
    echo "Building CLI..."
    cargo build --release --bin obamify
fi

# Execute the CLI with arguments
exec /app/target/release/obamify "$@"
