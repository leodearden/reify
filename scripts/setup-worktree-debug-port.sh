#!/usr/bin/env bash
# scripts/setup-worktree-debug-port.sh — Allocate a per-worktree reify-debug port
# and patch the worktree's .mcp.json so the dispatched agent's MCP client targets
# the correct GUI instance.
#
# Usage:
#   port=$(scripts/setup-worktree-debug-port.sh [worktree_dir])
#   export REIFY_DEBUG_PORT=$port
#
# Stdout: the resolved port integer (bare, no trailing newline except from echo).
# Stderr: all diagnostics and progress messages.
#
# If REIFY_DEBUG_PORT is already set to a valid port (strict ^[0-9]+$, 1..65535,
# no whitespace), that port is used verbatim.  Otherwise a free ephemeral port is
# allocated via allocate_free_port() from scripts/lib_portable.sh.
#
# The resolved port is written to BOTH:
#   - .mcp.json .mcpServers["reify-debug"] URL  (so the agent's MCP client targets it)
#   - stdout                                      (so the caller can export REIFY_DEBUG_PORT)
#
# After patching, `git update-index --skip-worktree .mcp.json` is run (guarded by
# `git rev-parse --is-inside-work-tree`) so the per-worktree ephemeral port never
# shows in `git status`/diffs, never lands in a task commit, and never trips
# land.sh's clean-working-tree gate.
# Undo with: git update-index --no-skip-worktree .mcp.json

set -euo pipefail

# ── resolve worktree dir ──────────────────────────────────────────────────────

# Optional positional arg: target worktree directory.
# Default: the repo root (two levels up from this script's location).
_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ $# -gt 1 ]; then
    echo "Usage: $(basename "$0") [worktree_dir]" >&2
    echo "  worktree_dir  Optional path to the worktree root (default: repo root)" >&2
    exit 1
fi

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    echo "Usage: $(basename "$0") [worktree_dir]" >&2
    echo "" >&2
    echo "  Allocate a free reify-debug port, patch <worktree>/.mcp.json, and print" >&2
    echo "  the port to stdout so the caller can export REIFY_DEBUG_PORT." >&2
    echo "" >&2
    echo "  Honors a pre-set REIFY_DEBUG_PORT when valid (digits, 1-65535)." >&2
    exit 0
fi

WORKTREE_DIR="${1:-"$(cd "$_SCRIPT_DIR/.." && pwd)"}"

if [ ! -d "$WORKTREE_DIR" ]; then
    echo "ERROR: worktree directory does not exist: $WORKTREE_DIR" >&2
    exit 1
fi

# ── guard: .mcp.json must exist ───────────────────────────────────────────────

MCP_JSON="$WORKTREE_DIR/.mcp.json"

if [ ! -f "$MCP_JSON" ]; then
    echo "ERROR: .mcp.json not found at $MCP_JSON" >&2
    echo "  The worktree must have a .mcp.json with a reify-debug entry." >&2
    exit 1
fi

# ── source helpers ────────────────────────────────────────────────────────────

# Locate lib_portable.sh relative to this script so it works from any CWD.
_LIB_PORTABLE="$_SCRIPT_DIR/lib_portable.sh"
if [ ! -f "$_LIB_PORTABLE" ]; then
    echo "ERROR: lib_portable.sh not found at $_LIB_PORTABLE" >&2
    exit 1
fi
# shellcheck source=scripts/lib_portable.sh
source "$_LIB_PORTABLE"

# ── resolve port ──────────────────────────────────────────────────────────────
# Honor REIFY_DEBUG_PORT only when it is a strict decimal integer in 1..65535
# (no whitespace, no leading zeros that push it out of range).
# Mirrors the contract of:
#   debug_server.rs   parse_debug_port / DEFAULT_DEBUG_PORT
#   endpoint.ts       resolveDebugPort / debugUrlForPort
#   session.ts        resolveReifyDebugUrl

_resolve_port() {
    local env_val="${REIFY_DEBUG_PORT:-}"

    # Strict validation: digits only, no whitespace, value in 1..65535.
    if [[ "$env_val" =~ ^[0-9]+$ ]] && \
       [ "$env_val" -ge 1 ] && [ "$env_val" -le 65535 ]; then
        echo "$env_val"
        return 0
    fi

    if [ -n "$env_val" ]; then
        echo "INFO: REIFY_DEBUG_PORT='$env_val' is not a valid port; allocating a free one." >&2
    fi

    allocate_free_port
}

PORT=$(_resolve_port)
echo "INFO: resolved debug port: $PORT" >&2

# ── patch .mcp.json atomically ────────────────────────────────────────────────
# Set the full reify-debug object so the patch is correct whether the entry
# pre-exists or is being inserted.  Use a temp file + mv for atomicity.

if ! command -v jq >/dev/null 2>&1; then
    echo "ERROR: jq not found on PATH; cannot patch .mcp.json" >&2
    exit 1
fi

_MCP_TMP="$(mktemp "$WORKTREE_DIR/.mcp.json.XXXXXX")"
jq \
    --arg url "http://127.0.0.1:${PORT}/mcp" \
    '.mcpServers["reify-debug"] = {"type": "http", "url": $url}' \
    "$MCP_JSON" > "$_MCP_TMP"
mv "$_MCP_TMP" "$MCP_JSON"
echo "INFO: patched $MCP_JSON → reify-debug url http://127.0.0.1:${PORT}/mcp" >&2

# ── git skip-worktree (guarded) ───────────────────────────────────────────────
# Marks .mcp.json so git ignores the local modification, keeping the per-worktree
# ephemeral port out of git status/diffs and preventing it from landing via
# land.sh's clean-tree gate.
# Undo with: git update-index --no-skip-worktree .mcp.json

if git -C "$WORKTREE_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git -C "$WORKTREE_DIR" update-index --skip-worktree .mcp.json 2>/dev/null || true
    echo "INFO: set skip-worktree on .mcp.json in $WORKTREE_DIR" >&2
fi

# ── emit port to stdout ───────────────────────────────────────────────────────
# This is the ONLY line written to stdout.
# Callers consume it as: port=$(setup-worktree-debug-port.sh); export REIFY_DEBUG_PORT=$port
echo "$PORT"
