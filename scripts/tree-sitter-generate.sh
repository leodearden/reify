#!/usr/bin/env bash
# Regenerate tree-sitter parser from grammar.js.
# Produces: src/parser.c, src/grammar.json, src/node-types.json
#
# This script is idempotent — safe to run repeatedly.
# Called by: build.rs (auto), orchestrator verification, hooks/project-checks.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TS_DIR="$(cd "$SCRIPT_DIR/../tree-sitter-reify" && pwd)"

if ! command -v tree-sitter >/dev/null 2>&1; then
    echo "ERROR: tree-sitter CLI not found on PATH." >&2
    echo "Install via: cargo install tree-sitter-cli" >&2
    exit 1
fi

if [ ! -f "$TS_DIR/grammar.js" ]; then
    echo "ERROR: $TS_DIR/grammar.js not found." >&2
    exit 1
fi

cd "$TS_DIR"

# Compute grammar hash once before generation (avoids TOCTOU race between
# staleness check and stamp write — same pattern as build.rs).
GRAMMAR_HASH=$(sha256sum grammar.js | awk '{print $1}')
STAMP_FILE="src/.grammar_hash.stamp"

# Staleness check: skip generation if stamp matches and all outputs exist.
STALE=false
if [ ! -f "$STAMP_FILE" ]; then
    STALE=true
elif [ "$(cat "$STAMP_FILE" 2>/dev/null)" != "$GRAMMAR_HASH" ]; then
    STALE=true
else
    for f in src/parser.c src/grammar.json src/node-types.json; do
        if [ ! -f "$f" ]; then
            STALE=true
            break
        fi
    done
fi

if [ "$STALE" = false ]; then
    echo "tree-sitter: up to date (grammar.js unchanged)"
    exit 0
fi

tree-sitter generate

# Verify expected outputs exist.
for f in src/parser.c src/grammar.json src/node-types.json; do
    if [ ! -f "$f" ]; then
        echo "ERROR: tree-sitter generate did not produce $f" >&2
        exit 1
    fi
done

# Write stamp file with the pre-computed hash.
echo -n "$GRAMMAR_HASH" > "$STAMP_FILE"

echo "tree-sitter: generated parser files in $TS_DIR/src/"
