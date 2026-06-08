#!/usr/bin/env bash
# Infrastructure test for task 4378.
# Tests scripts/ensure-gui-sidecar-placeholder.sh behaviour:
#   (a) creates an executable reify-sidecar-<triple> stub under
#       <root>/gui/src-tauri/sidecar/ when it does not exist;
#   (b) is idempotent — never clobbers an already-existing file.
#
# The test drives the script against an isolated tmpdir root so it never
# mutates the real worktree, mirroring the hermetic pattern used by
# test_setup_worktree_debug_port.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# Cleanup helper.
_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== ensure-gui-sidecar-placeholder.sh behaviour tests ==="

# ---------------------------------------------------------------------------
# Scenario 1: stub is created when absent
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 1: creates executable stub when absent ---"

TMP1="$(mktemp -d)"
_TMPDIRS+=("$TMP1")

_exit_code=0
bash "$REPO_ROOT/scripts/ensure-gui-sidecar-placeholder.sh" "$TMP1" || _exit_code=$?

assert "exits 0" test "$_exit_code" -eq 0

# Exactly one file matching the glob.
_matches=("$TMP1"/gui/src-tauri/sidecar/reify-sidecar-*)
assert "exactly one sidecar stub created" test "${#_matches[@]}" -eq 1

# The created file exists (glob didn't expand to the literal pattern string).
assert "stub file exists on disk" test -f "${_matches[0]}"

# The created file is executable.
assert "stub file is executable" test -x "${_matches[0]}"

# ---------------------------------------------------------------------------
# Scenario 2: idempotency — existing file is NOT clobbered
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 2: idempotent — does not clobber existing file ---"

TMP2="$(mktemp -d)"
_TMPDIRS+=("$TMP2")

# Derive the same triple the script would use (rustc -vV host, fallback).
_triple=""
if command -v rustc >/dev/null 2>&1; then
    _triple="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p' || true)"
fi
if [ -z "$_triple" ]; then
    _triple="x86_64-unknown-linux-gnu"
fi

_sidecar_dir="$TMP2/gui/src-tauri/sidecar"
mkdir -p "$_sidecar_dir"
_sidecar_path="$_sidecar_dir/reify-sidecar-$_triple"

# Pre-create with a unique sentinel byte that a real stub would never contain.
_sentinel="SENTINEL_DO_NOT_CLOBBER_4378"
printf '%s\n' "$_sentinel" > "$_sidecar_path"
chmod +x "$_sidecar_path"

# Run the script — it should detect the file already exists and leave it alone.
_exit_code2=0
bash "$REPO_ROOT/scripts/ensure-gui-sidecar-placeholder.sh" "$TMP2" || _exit_code2=$?

assert "idempotent: exits 0" test "$_exit_code2" -eq 0

_got_content="$(cat "$_sidecar_path")"
assert "idempotent: sentinel content unchanged" test "$_got_content" = "$_sentinel"

test_summary
