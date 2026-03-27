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
tree-sitter generate

# Verify expected outputs exist.
for f in src/parser.c src/grammar.json src/node-types.json; do
    if [ ! -f "$f" ]; then
        echo "ERROR: tree-sitter generate did not produce $f" >&2
        exit 1
    fi
done

echo "tree-sitter: generated parser files in $TS_DIR/src/"
