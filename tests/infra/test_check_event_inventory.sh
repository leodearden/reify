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

# Helper: initialise a minimal git repo in DIR and stage the given relative paths.
# No commit needed — git ls-files reads the index, so staging suffices; skipping
# commits avoids the need for user.name/user.email identity and hooks.
_init_repo() {
    local dir="$1"; shift
    git init -q "$dir"
    git -C "$dir" add "$@"
}

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
_init_repo "$_fix2dir" gui/src-tauri/src/test_emit.rs

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
_init_repo "$_fix2bdir" gui/src-tauri/src/test_multiline_emit.rs

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
_init_repo "$_fix4dir" gui/src-tauri/src/test_emit.rs

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
_init_repo "$_fix5dir" gui/src-tauri/src/test_emit.rs

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
_init_repo "$_fix6dir" gui/src-tauri/src/test_emit.rs

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
_init_repo "$_fix7dir" gui/src-tauri/src/test_dyn.rs

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
_init_repo "$_fix8dir" gui/src-tauri/src/test_emit.rs

_fix8_stderr="$_tmpdir/fix8_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix8dir" --bidirectional 2>"$_fix8_stderr" || true

assert "--bidirectional produces no warning for §2 fiction-channel" \
    bash -c "! grep -q 'fiction-channel' '$_fix8_stderr'"

assert "--bidirectional exits 0 when only §2 channel is unimplemented" \
    "$CHECK_SCRIPT" --repo-root "$_fix8dir" --bidirectional

# ==============================================================================
# Check 9: --bidirectional --strict exits non-zero on phantom
# ==============================================================================
echo ""
echo "--- Check 9: --bidirectional --strict exits non-zero on phantom ---"

# Reuse the Check 6 fixture (has phantom-channel with no source occurrence)
assert "--bidirectional --strict exits non-zero on phantom channel" \
    bash -c "! '$CHECK_SCRIPT' --repo-root '$_fix6dir' --bidirectional --strict"

# ==============================================================================
# Check 10: smoke --bidirectional against real worktree exits 0 (no phantoms)
# ==============================================================================
echo ""
echo "--- Check 10: smoke --bidirectional against real worktree ---"

_bidi_smoke_stderr="$_tmpdir/bidi_smoke_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$REPO_ROOT" --bidirectional 2>"$_bidi_smoke_stderr" || true

assert "smoke --bidirectional exits 0 (no §1 phantoms in real worktree)" \
    "$CHECK_SCRIPT" --repo-root "$REPO_ROOT" --bidirectional

assert "smoke --bidirectional produces no phantom stderr lines" \
    bash -c "! grep -q 'phantom' '$_bidi_smoke_stderr'"

# ==============================================================================
# Check 11: --help / -h output mentions --bidirectional
# ==============================================================================
echo ""
echo "--- Check 11: --help output contains --bidirectional ---"

_help_out="$_tmpdir/help_out.txt"
"$CHECK_SCRIPT" --help > "$_help_out" 2>&1 || true

assert "--help output contains --bidirectional" \
    grep -q '\-\-bidirectional' "$_help_out"

assert "--help output contains a description of the reverse pass" \
    grep -q 'reverse pass' "$_help_out"

# ==============================================================================
# Check 12: --bidirectional §10+ heading ambiguity guard
# A channel registered under §10 (or any §N with N ≥ 10) must NOT be classified
# as a §1 channel by the awk filter — the un-anchored /^## §1/ pattern matches
# §10, §11, … as a prefix, silently including their rows in the §1 phantom set.
# With the fixed /^## §1 / (trailing-space anchor) this test passes.
# ==============================================================================
echo ""
echo "--- Check 12: --bidirectional §10+ heading not misclassified as §1 ---"

_fix12dir="$_tmpdir/fix12"
mkdir -p "$_fix12dir/docs" "$_fix12dir/gui/src-tauri/src"

cat > "$_fix12dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

## §1 — Wired channels (production today)

| Channel | Notes |
|---|---|
| `wired-ok` | wired |

## §2 — Channels this PRD adds (FICTION → WIRED via GR-016 decomposition)

| Channel | Notes |
|---|---|

## §10 — Synthetic section (future expansion)

| Channel | Notes |
|---|---|
| `should-not-be-scanned` | future |
INVENTORY

cat > "$_fix12dir/gui/src-tauri/src/test_emit.rs" <<'RUST'
fn emit_something(app: &AppHandle) {
    app.emit("wired-ok", payload);
}
RUST
_init_repo "$_fix12dir" gui/src-tauri/src/test_emit.rs

_fix12_stderr="$_tmpdir/fix12_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix12dir" --bidirectional 2>"$_fix12_stderr" || true

assert "--bidirectional does not classify §10 channel as §1 phantom" \
    bash -c "! grep -q 'should-not-be-scanned' '$_fix12_stderr'"

assert "--bidirectional --strict exits 0 when §10 channel is not misclassified as §1 phantom" \
    "$CHECK_SCRIPT" --repo-root "$_fix12dir" --strict --bidirectional

# ==============================================================================
# Check 13: forward-pass / orphan-pass hermeticity
# Untracked build-artifact .rs files must NOT be scanned by the forward pass.
# git ls-files enumerates only tracked (indexed) sources, so untracked/transient
# artifacts are excluded by construction regardless of which directory they land
# in (esc-4357-20, esc-3798-78, task-4529 → task-4572).
# Fixture: only gui/src-tauri/src/real.rs is staged; gen/ and target/ artifacts
# are left UNTRACKED so git ls-files never lists them.
# ==============================================================================
echo ""
echo "--- Check 13: forward-pass hermeticity (untracked build artifacts excluded by git ls-files) ---"

_fix13dir="$_tmpdir/fix13"
mkdir -p "$_fix13dir/docs" \
         "$_fix13dir/gui/src-tauri/src" \
         "$_fix13dir/gui/src-tauri/gen/schemas" \
         "$_fix13dir/gui/src-tauri/target/debug/build"

cat > "$_fix13dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

| Channel | Notes |
|---|---|
| `mesh-update` | wired |
INVENTORY

# Legitimate source file — registered channel, no orphan.
cat > "$_fix13dir/gui/src-tauri/src/real.rs" <<'RUST'
fn emit_real(app: &AppHandle) {
    app.emit("mesh-update", ());
}
RUST

# Transient codegen artifact in gen/ — left UNTRACKED, must NOT be scanned.
cat > "$_fix13dir/gui/src-tauri/gen/schemas/transient.rs" <<'RUST'
fn emit_transient(app: &AppHandle) {
    app.emit("transient-build-artifact", ());
}
RUST

# Transient build artifact in target/ — left UNTRACKED, must NOT be scanned.
cat > "$_fix13dir/gui/src-tauri/target/debug/build/gen.rs" <<'RUST'
fn emit_target(app: &AppHandle) {
    app.emit("target-build-artifact", ());
}
RUST
_init_repo "$_fix13dir" gui/src-tauri/src/real.rs

_fix13_stderr="$_tmpdir/fix13_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix13dir" 2>"$_fix13_stderr" || true

assert "Check 13: gen/ artifact 'transient-build-artifact' does NOT appear in stderr" \
    bash -c "! grep -q 'transient-build-artifact' '$_fix13_stderr'"

assert "Check 13: target/ artifact 'target-build-artifact' does NOT appear in stderr" \
    bash -c "! grep -q 'target-build-artifact' '$_fix13_stderr'"

assert "Check 13: no 'orphan' line in stderr (no false-positive orphans)" \
    bash -c "! grep -q 'orphan' '$_fix13_stderr'"

assert "Check 13: exits 0 in warning mode" \
    "$CHECK_SCRIPT" --repo-root "$_fix13dir"

assert "Check 13: exits 0 under --strict (no false-positive orphan)" \
    "$CHECK_SCRIPT" --repo-root "$_fix13dir" --strict

# ==============================================================================
# Check 14: bidirectional / reverse-pass hermeticity
# A §1 channel whose ONLY literal occurrence is inside an untracked build-artifact
# file (gen/) must still be reported as a phantom. git ls-files excludes untracked
# files, so the gen/ literal does NOT count as source wiring (esc-4357-20,
# task-4529 → task-4572).
# Fixture: only gui/src-tauri/src/real.rs is staged; gen/ artifact is UNTRACKED.
# ==============================================================================
echo ""
echo "--- Check 14: bidirectional reverse-pass hermeticity (untracked gen/ artifact excluded by git ls-files) ---"

_fix14dir="$_tmpdir/fix14"
mkdir -p "$_fix14dir/docs" \
         "$_fix14dir/gui/src-tauri/src" \
         "$_fix14dir/gui/src-tauri/gen/schemas"

cat > "$_fix14dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

## §1 — Wired channels (production today)

| Channel | Notes |
|---|---|
| `wired-ok` | wired |
| `only-in-gen` | wired |

## §2 — Channels this PRD adds (FICTION → WIRED via GR-016 decomposition)

| Channel | Notes |
|---|---|
INVENTORY

# Legitimately wired channel in src/.
cat > "$_fix14dir/gui/src-tauri/src/real.rs" <<'RUST'
fn emit_real(app: &AppHandle) {
    app.emit("wired-ok", ());
}
RUST

# The ONLY literal occurrence of "only-in-gen" lives inside an untracked gen/ artifact.
# With the fix, the reverse pass must NOT find it — only-in-gen should be phantom.
cat > "$_fix14dir/gui/src-tauri/gen/schemas/built.rs" <<'RUST'
// Codegen artifact — this literal must NOT count as source wiring.
const CHANNEL: &str = "only-in-gen";
RUST
_init_repo "$_fix14dir" gui/src-tauri/src/real.rs

_fix14_stderr="$_tmpdir/fix14_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix14dir" --bidirectional 2>"$_fix14_stderr" || true

assert "Check 14: --bidirectional still flags 'only-in-gen' as phantom (gen/ must not count as wiring)" \
    grep -q 'only-in-gen' "$_fix14_stderr"

assert "Check 14: --bidirectional does NOT flag 'wired-ok' as phantom" \
    bash -c "! grep -q 'wired-ok' '$_fix14_stderr'"

assert "Check 14: --bidirectional --strict exits non-zero (only-in-gen is a phantom)" \
    bash -c "! '$CHECK_SCRIPT' --repo-root '$_fix14dir' --bidirectional --strict"

# -- Summary ------------------------------------------------------------------
test_summary
