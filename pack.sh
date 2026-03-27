#!/bin/bash
set -e

# Builds the Rust binary and packages the GNOME extension into a zip.
# Output: claude-usage (binary) and claude-usage@kenkoooo.zip

echo "Building claude-usage binary..."
cargo build --release

BINARY=$(cargo build --release --message-format=json 2>/dev/null | \
    grep '"executable"' | tail -1 | sed 's/.*"executable":"\([^"]*\)".*/\1/')
if [ -z "$BINARY" ]; then
    BINARY=$(find ~/.cache/rust-target/release target/release -name "claude-usage" -type f 2>/dev/null | head -1)
fi

cp "$BINARY" ./claude-usage
echo "Binary ready: ./claude-usage"

ZIPFILE="claude-usage@kenkoooo.zip"
rm -f "$ZIPFILE"
(cd extension && zip -r "../$ZIPFILE" .)
echo "Extension ready: ./$ZIPFILE"
