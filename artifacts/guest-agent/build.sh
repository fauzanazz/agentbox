#!/usr/bin/env bash
set -euo pipefail

ARCH="$1"
OUTPUT_DIR="$2"

BINARY="${OUTPUT_DIR}/guest-agent"

if [ -f "$BINARY" ]; then
    echo "Guest agent already built: $BINARY"
    exit 0
fi

echo "Building guest-agent for ${ARCH}..."

# Map to Rust target
case "$ARCH" in
    x86_64)  TARGET="x86_64-unknown-linux-musl" ;;
    aarch64) TARGET="aarch64-unknown-linux-musl" ;;
    *) echo "Unsupported: $ARCH"; exit 1 ;;
esac

# Build (static binary with musl)
cd ../../crates/guest-agent
cargo build --release --target "$TARGET"
cp "../../target/${TARGET}/release/guest-agent" "$BINARY"

echo "Guest agent built: $BINARY"
