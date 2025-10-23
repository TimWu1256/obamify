#!/usr/bin/env sh
set -eu

# Simple installer for obamify CLI
# Strategy: if a prebuilt binary URL is provided via OBAMIFY_RELEASE_URL, download it.
# Otherwise fall back to `cargo install --path .` which requires Rust toolchain.

RELEASE_URL=${OBAMIFY_RELEASE_URL:-}
PREFIX=${PREFIX:-$HOME/.local}
BIN_DIR="$PREFIX/bin"

mkdir -p "$BIN_DIR"

if [ -n "$RELEASE_URL" ]; then
    echo "Downloading prebuilt binary from: $RELEASE_URL"
    tmpfile=$(mktemp)
    trap 'rm -f "$tmpfile"' EXIT
    if command -v curl >/dev/null 2>&1; then
        curl -L -o "$tmpfile" "$RELEASE_URL"
    else
        wget -O "$tmpfile" "$RELEASE_URL"
    fi
    chmod +x "$tmpfile"
    mv "$tmpfile" "$BIN_DIR/obamify"
    echo "Installed obamify to $BIN_DIR/obamify"
    exit 0
fi

echo "No OBAMIFY_RELEASE_URL set — falling back to cargo install (requires Rust)"
if command -v cargo >/dev/null 2>&1; then
    cargo install --path . --root "$PREFIX"
    echo "Installed obamify to $BIN_DIR"
else
    echo "Error: cargo not found in PATH. Install Rust or set OBAMIFY_RELEASE_URL to a prebuilt binary." >&2
    exit 2
fi
