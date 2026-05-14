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

# ==============================================================================
# Check 6: --bidirectional flag accepted and detects §1 phantom channel
# ==============================================================================
echo ""
echo "--- Check 6: --bidirectional flag accepted and detects §1 phantom ---"

_fix6dir="$_tmpdir/fix6"
mkdir -p "$_fix6dir/docs" "$_fix6dir/gui/src-tauri/src"

cat > "$_fix6dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

## §1 — Wired channels (production today)

| Channel | Notes |
|---|---|
| `mesh-update` | wired |
| `phantom-channel` | wired |

## §2 — Channels this PRD adds (FICTION → WIRED via GR-016 decomposition)

| Channel | Notes |
|---|---|
INVENTORY

cat > "$_fix6dir/gui/src-tauri/src/test_emit.rs" <<'RUST'
fn emit_something(app: &AppHandle) {
    app.emit("mesh-update", payload);
}
RUST

_fix6_stderr="$_tmpdir/fix6_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix6dir" --bidirectional 2>"$_fix6_stderr" || true

assert "--bidirectional flag does not produce 'Unknown option' error" \
    bash -c "! grep -q 'Unknown option' '$_fix6_stderr'"

assert "--bidirectional emits phantom warning for phantom-channel" \
    grep -q 'phantom-channel' "$_fix6_stderr"

assert "--bidirectional exits 0 in warning mode with phantom present" \
    "$CHECK_SCRIPT" --repo-root "$_fix6dir" --bidirectional

# ==============================================================================
# Check 7: --bidirectional dynamic-emit false-positive guard
# A §1 channel whose name appears as a .to_string() literal (not .emit("…"))
# must NOT be flagged as a phantom — permissive literal scan covers it.
# ==============================================================================
echo ""
echo "--- Check 7: --bidirectional dynamic-emit no-false-positive ---"

_fix7dir="$_tmpdir/fix7"
mkdir -p "$_fix7dir/docs" "$_fix7dir/gui/src-tauri/src"

cat > "$_fix7dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

## §1 — Wired channels (production today)

| Channel | Notes |
|---|---|
| `dyn-channel` | wired |

## §2 — Channels this PRD adds (FICTION → WIRED via GR-016 decomposition)

| Channel | Notes |
|---|---|
INVENTORY

cat > "$_fix7dir/gui/src-tauri/src/test_dyn.rs" <<'RUST'
fn push_event(events: &mut Vec<(String, Payload)>, payload: Payload) {
    events.push(("dyn-channel".to_string(), payload));
}
RUST

_fix7_stderr="$_tmpdir/fix7_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix7dir" --bidirectional 2>"$_fix7_stderr" || true

assert "--bidirectional produces no phantom warning for dynamic-emit channel" \
    bash -c "! grep -q 'phantom' '$_fix7_stderr'"

assert "--bidirectional exits 0 for dynamic-emit channel" \
    "$CHECK_SCRIPT" --repo-root "$_fix7dir" --bidirectional

# ==============================================================================
# Check 8: --bidirectional §2 FICTION exclusion
# A channel in §2 with no source occurrence must NOT produce a phantom warning —
# §2 is pre-implementation by design.
# ==============================================================================
echo ""
echo "--- Check 8: --bidirectional §2 FICTION exclusion ---"

_fix8dir="$_tmpdir/fix8"
mkdir -p "$_fix8dir/docs" "$_fix8dir/gui/src-tauri/src"

cat > "$_fix8dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

## §1 — Wired channels (production today)

| Channel | Notes |
|---|---|
| `wired-ok` | wired |

## §2 — Channels this PRD adds (FICTION → WIRED via GR-016 decomposition)

| Channel | Notes |
|---|---|
| `fiction-channel` | pre-implementation |
INVENTORY

cat > "$_fix8dir/gui/src-tauri/src/test_emit.rs" <<'RUST'
fn emit_something(app: &AppHandle) {
    app.emit("wired-ok", payload);
}
RUST

_fix8_stderr="$_tmpdir/fix8_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix8dir" --bidirectional 2>"$_fix8_stderr" || true

assert "--bidirectional produces no warning for §2 fiction-channel" \
    bash -c "! grep -q 'fiction-channel' '$_fix8_stderr'"

assert "--bidirectional exits 0 when only §2 channel is unimplemented" \
    "$CHECK_SCRIPT" --repo-root "$_fix8dir" --bidirectional

# -- Summary ------------------------------------------------------------------
test_summary
