#!/usr/bin/env bash
# Tests for scripts/check_event_inventory.sh.
# Verifies: existence, executability, smoke run exits 0 (no orphans),
# synthetic-orphan detection (orphan name in stderr, exit 0 in warning mode),
# --strict exits non-zero on orphans, known-channel no-false-positive,
# dynamic-emit no-false-positive.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CHECK_SCRIPT="$REPO_ROOT/scripts/check_event_inventory.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== check_event_inventory.sh tests ==="

_tmpdir=$(mktemp -d)
trap 'rm -rf "$_tmpdir"' EXIT

# ==============================================================================
# Check 0: script exists and is executable
# ==============================================================================
echo ""
echo "--- Check 0: script existence and executability ---"

assert "scripts/check_event_inventory.sh exists" \
    test -f "$CHECK_SCRIPT"

assert "scripts/check_event_inventory.sh is executable" \
    test -x "$CHECK_SCRIPT"

# ==============================================================================
# Check 1: smoke run — no orphans in actual worktree
# ==============================================================================
echo ""
echo "--- Check 1: smoke run against actual worktree ---"

_smoke_stderr="$_tmpdir/smoke_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$REPO_ROOT" 2>"$_smoke_stderr" || true

assert "smoke run exits 0 (warning mode, no orphans)" \
    "$CHECK_SCRIPT" --repo-root "$REPO_ROOT"

assert "smoke run produces no orphan-warning lines on stderr" \
    bash -c "! grep -q 'orphan' '$_smoke_stderr'"

# ==============================================================================
# Check 2: synthetic orphan detection
# ==============================================================================
echo ""
echo "--- Check 2: synthetic orphan detection ---"

_fix2dir="$_tmpdir/fix2"
mkdir -p "$_fix2dir/docs" "$_fix2dir/gui/src-tauri/src"

cat > "$_fix2dir/docs/gui-event-channels.md" <<'INVENTORY'
# Event Channels

| Channel | Notes |
|---|---|
| `mesh-update` | wired |
| `kernel-status` | wired |
INVENTORY

cat > "$_fix2dir/gui/src-tauri/src/test_emit.rs" <<'RUST'
fn emit_something(app: &AppHandle) {
    app.emit("orphan-channel-fixture", ()).ok();
}
RUST

_fix2_stderr="$_tmpdir/fix2_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix2dir" 2>"$_fix2_stderr" || true

assert "orphan-channel-fixture appears in stderr" \
    grep -q 'orphan-channel-fixture' "$_fix2_stderr"

assert "exit 0 in warning mode with orphan present" \
    "$CHECK_SCRIPT" --repo-root "$_fix2dir"

# ==============================================================================
# Check 2b: multi-line emit form is also detected
# — the perl -0777 slurp-mode contract must match .emit(\n    "name" too.
# ==============================================================================
echo ""
echo "--- Check 2b: multi-line emit form detected ---"

_fix2bdir="$_tmpdir/fix2b"
mkdir -p "$_fix2bdir/docs" "$_fix2bdir/gui/src-tauri/src"

cat > "$_fix2bdir/docs/gui-event-channels.md" <<'INVENTORY'
# Event Channels

| Channel | Notes |
|---|---|
| `mesh-update` | wired |
INVENTORY

cat > "$_fix2bdir/gui/src-tauri/src/test_multiline_emit.rs" <<'RUST'
fn emit_multiline(app: &AppHandle) {
    app.emit(
        "multiline-orphan-fixture",
        ()
    ).ok();
}
RUST

_fix2b_stderr="$_tmpdir/fix2b_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix2bdir" 2>"$_fix2b_stderr" || true

assert "multi-line orphan detected in stderr" \
    grep -q 'multiline-orphan-fixture' "$_fix2b_stderr"

assert "multi-line form exits 0 in warning mode" \
    "$CHECK_SCRIPT" --repo-root "$_fix2bdir"

# ==============================================================================
# Check 3: --strict exits non-zero when orphans present
# ==============================================================================
echo ""
echo "--- Check 3: --strict exits non-zero on orphan ---"

assert "--strict exits non-zero on orphan" \
    bash -c "! '$CHECK_SCRIPT' --repo-root '$_fix2dir' --strict"

# ==============================================================================
# Check 4: known-channel no-false-positive
# ==============================================================================
echo ""
echo "--- Check 4: known-channel no-false-positive ---"

_fix4dir="$_tmpdir/fix4"
mkdir -p "$_fix4dir/docs" "$_fix4dir/gui/src-tauri/src"

cat > "$_fix4dir/docs/gui-event-channels.md" <<'INVENTORY'
# Event Channels

| Channel | Notes |
|---|---|
| `mesh-update` | wired |
INVENTORY

cat > "$_fix4dir/gui/src-tauri/src/test_emit.rs" <<'RUST'
fn emit_something(app: &AppHandle) {
    app.emit("mesh-update", payload);
}
RUST

_fix4_stderr="$_tmpdir/fix4_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix4dir" 2>"$_fix4_stderr" || true

assert "known-channel emit produces no orphan warnings" \
    bash -c "! grep -q 'orphan' '$_fix4_stderr'"

assert "known-channel emit exits 0" \
    "$CHECK_SCRIPT" --repo-root "$_fix4dir"

# ==============================================================================
# Check 5: dynamic-emit no-false-positive
# ==============================================================================
echo ""
echo "--- Check 5: dynamic emit no-false-positive ---"

_fix5dir="$_tmpdir/fix5"
mkdir -p "$_fix5dir/docs" "$_fix5dir/gui/src-tauri/src"

cat > "$_fix5dir/docs/gui-event-channels.md" <<'INVENTORY'
# Event Channels

| Channel | Notes |
|---|---|
| `mesh-update` | wired |
INVENTORY

cat > "$_fix5dir/gui/src-tauri/src/test_emit.rs" <<'RUST'
fn emit_dynamic(app: &AppHandle, event_name: &str) {
    app.emit(&event_name, payload).ok();
    app.emit(event_name, payload).ok();
}
RUST

_fix5_stderr="$_tmpdir/fix5_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix5dir" 2>"$_fix5_stderr" || true

assert "dynamic emit produces no orphan warnings" \
    bash -c "! grep -q 'orphan' '$_fix5_stderr'"

assert "dynamic emit exits 0" \
    "$CHECK_SCRIPT" --repo-root "$_fix5dir"

# -- Summary ------------------------------------------------------------------
test_summary
