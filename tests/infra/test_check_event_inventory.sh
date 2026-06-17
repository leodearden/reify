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

# ==============================================================================
# Check 15: forward-pass tracked-only hermeticity (esc-3798-78 / task-4572)
# A transient .rs file under src/ (a directory PRUNE_DIRS never pruned by name)
# must NOT be scanned if it is UNTRACKED — it models a concurrent build lane
# writing a file that is not yet in the index. git ls-files excludes it by
# construction; the old find-based scan would flag it as an orphan.
# RED against the current find-based script: find scans the untracked
# src/transient.rs → orphan flagged → assertions 1/2/4 fail.
# GREEN only after the git ls-files implementation excludes untracked files.
# ==============================================================================
echo ""
echo "--- Check 15: forward-pass tracked-only hermeticity (src/ transient, esc-3798-78) ---"

_fix15dir="$_tmpdir/fix15"
mkdir -p "$_fix15dir/docs" "$_fix15dir/gui/src-tauri/src"

cat > "$_fix15dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

| Channel | Notes |
|---|---|
| `mesh-update` | wired |
INVENTORY

# TRACKED: legitimate source file with a registered channel — no orphan.
cat > "$_fix15dir/gui/src-tauri/src/real.rs" <<'RUST'
fn emit_real(app: &AppHandle) {
    app.emit("mesh-update", ());
}
RUST

# UNTRACKED: transient file dropped by a concurrent build lane under src/.
# NOT staged — models esc-3798-78 where the transient landed outside gen/target.
cat > "$_fix15dir/gui/src-tauri/src/transient.rs" <<'RUST'
fn emit_transient(app: &AppHandle) {
    app.emit("untracked-orphan", ());
}
RUST

# Stage only real.rs; transient.rs stays untracked.
_init_repo "$_fix15dir" gui/src-tauri/src/real.rs

_fix15_stderr="$_tmpdir/fix15_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix15dir" 2>"$_fix15_stderr" || true

assert "Check 15: 'untracked-orphan' does NOT appear in stderr (untracked file excluded)" \
    bash -c "! grep -q 'untracked-orphan' '$_fix15_stderr'"

assert "Check 15: no 'orphan' line in stderr (no false-positive from src/ transient)" \
    bash -c "! grep -q 'orphan' '$_fix15_stderr'"

assert "Check 15: exits 0 in warning mode" \
    "$CHECK_SCRIPT" --repo-root "$_fix15dir"

assert "Check 15: exits 0 under --strict (no false-positive orphan from untracked transient)" \
    "$CHECK_SCRIPT" --repo-root "$_fix15dir" --strict

# ==============================================================================
# Check 16: reverse-pass (--bidirectional) tracked-only hermeticity (task-4572)
# A §1 channel whose ONLY literal lives in an UNTRACKED src/extra.rs must be
# flagged as a phantom — the untracked file must NOT count as source wiring.
# The untracked file is placed under src/ (a directory PRUNE_DIRS never prunes
# by name), proving git ls-files closes the gap directory-name pruning left.
# RED against the current reverse pass: grep -r "$SRC_DIR" finds the untracked
# extra.rs literal → only-in-untracked NOT flagged → assertions 1/3 fail.
# GREEN only after the git ls-files implementation excludes untracked files.
# ==============================================================================
echo ""
echo "--- Check 16: reverse-pass tracked-only hermeticity (src/ untracked, task-4572) ---"

_fix16dir="$_tmpdir/fix16"
mkdir -p "$_fix16dir/docs" "$_fix16dir/gui/src-tauri/src"

cat > "$_fix16dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

## §1 — Wired channels (production today)

| Channel | Notes |
|---|---|
| `wired-ok` | wired |
| `only-in-untracked` | wired |

## §2 — Channels this PRD adds (FICTION → WIRED via GR-016 decomposition)

| Channel | Notes |
|---|---|
INVENTORY

# TRACKED: legitimately wired channel in src/.
cat > "$_fix16dir/gui/src-tauri/src/real.rs" <<'RUST'
fn emit_real(app: &AppHandle) {
    app.emit("wired-ok", ());
}
RUST

# UNTRACKED: the ONLY occurrence of "only-in-untracked" is here.
# NOT staged — models a concurrent build lane's transient file under src/.
# This must NOT count as source wiring for the reverse pass.
cat > "$_fix16dir/gui/src-tauri/src/extra.rs" <<'RUST'
// Untracked transient — literal must not count as wiring.
const C: &str = "only-in-untracked";
RUST

# Stage only real.rs; extra.rs stays untracked.
_init_repo "$_fix16dir" gui/src-tauri/src/real.rs

_fix16_stderr="$_tmpdir/fix16_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix16dir" --bidirectional 2>"$_fix16_stderr" || true

assert "Check 16: 'only-in-untracked' IS flagged as phantom (untracked literal must not count as wiring)" \
    grep -q 'only-in-untracked' "$_fix16_stderr"

assert "Check 16: 'wired-ok' is NOT flagged as phantom" \
    bash -c "! grep -q 'wired-ok' '$_fix16_stderr'"

assert "Check 16: --bidirectional --strict exits non-zero (only-in-untracked is a phantom)" \
    bash -c "! '$CHECK_SCRIPT' --repo-root '$_fix16dir' --bidirectional --strict"

# ==============================================================================
# Check 17: non-git repo-root exits with a clear error
# When --repo-root points at a plain directory (not a git work tree), the script
# must exit non-zero with an error message. Without this check the tool degrades
# asymmetrically: the forward pass silently exits 0 (empty source list → no
# orphans ever flagged) while the reverse pass flags every §1 channel as phantom
# — an inconsistent and surprising failure mode.
# ==============================================================================
echo ""
echo "--- Check 17: non-git repo-root exits with error ---"

_fix17dir="$_tmpdir/fix17"
mkdir -p "$_fix17dir/docs" "$_fix17dir/gui/src-tauri/src"

cat > "$_fix17dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

## §1 — Wired channels (production today)

| Channel | Notes |
|---|---|
| `some-channel` | wired |

## §2 — Channels this PRD adds (FICTION → WIRED via GR-016 decomposition)

| Channel | Notes |
|---|---|
INVENTORY

# No git init — _fix17dir is a plain directory, intentionally not a git work tree.

_fix17_stderr="$_tmpdir/fix17_stderr.txt"
"$CHECK_SCRIPT" --repo-root "$_fix17dir" 2>"$_fix17_stderr" || true

assert "Check 17: non-git repo-root exits non-zero" \
    bash -c "! '$CHECK_SCRIPT' --repo-root '$_fix17dir' 2>/dev/null"

assert "Check 17: stderr contains ERROR about non-git work tree" \
    grep -qi 'error.*git' "$_fix17_stderr"

assert "Check 17: --bidirectional also exits non-zero on non-git repo-root" \
    bash -c "! '$CHECK_SCRIPT' --repo-root '$_fix17dir' --bidirectional 2>/dev/null"

# ==============================================================================
# Check 18: registered-set truncation recovery (forward pass) — task-4586
# Simulates under-load single-line truncation via REIFY_EVENT_INVENTORY_DROP_REGISTERED
# seam: drops mesh-update from the extracted registered set.  The recovery guard
# (step-2 re-check) must confirm that mesh-update IS in the inventory file before
# flagging an orphan, and must silently skip it.
# RED today: no re-check → false orphan flagged → assertions a/b/d fail.
# GREEN after step-2 adds the per-orphan inventory re-confirm.
# ==============================================================================
echo ""
echo "--- Check 18: registered-set truncation recovery (task-4586) ---"

_fix18dir="$_tmpdir/fix18"
mkdir -p "$_fix18dir/docs" "$_fix18dir/gui/src-tauri/src"

cat > "$_fix18dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

## §1 — Wired channels (production today)

| Channel | Notes |
|---|---|
| `mesh-update` | wired |
| `kernel-status` | wired |

## §2 — Channels this PRD adds (FICTION → WIRED via GR-016 decomposition)

| Channel | Notes |
|---|---|
INVENTORY

cat > "$_fix18dir/gui/src-tauri/src/main.rs" <<'RUST'
fn emit_wired(app: &AppHandle) {
    app.emit("mesh-update", ());
}
RUST

_init_repo "$_fix18dir" gui/src-tauri/src/main.rs

_fix18_stderr="$_tmpdir/fix18_stderr.txt"
REIFY_EVENT_INVENTORY_DROP_REGISTERED=mesh-update \
    "$CHECK_SCRIPT" --repo-root "$_fix18dir" 2>"$_fix18_stderr" || true

assert "Check 18a: no 'orphan' line in stderr (re-check must recover dropped mesh-update)" \
    bash -c "! grep -q 'orphan' '$_fix18_stderr'"

assert "Check 18b: 'mesh-update' does NOT appear as orphan in stderr" \
    bash -c "! grep -q 'mesh-update' '$_fix18_stderr'"

assert "Check 18c: exits 0 in warning mode with fault injection" \
    bash -c "REIFY_EVENT_INVENTORY_DROP_REGISTERED=mesh-update '$CHECK_SCRIPT' --repo-root '$_fix18dir'"

assert "Check 18d: exits 0 under --strict (no false-positive orphan)" \
    bash -c "REIFY_EVENT_INVENTORY_DROP_REGISTERED=mesh-update '$CHECK_SCRIPT' --repo-root '$_fix18dir' --strict"

# ==============================================================================
# Check 19: registered extraction completeness / --print-registered — task-4586
# --print-registered prints the extracted registered set (one channel per line)
# and exits 0.  Guards extract_registered_channels() against silently dropping
# a channel, and doubles as a field-debug tool for esc-4578-61.
# RED today: --print-registered is an unknown option → exits 1, empty stdout →
# all presence assertions fail.
# GREEN after step-4 adds the flag and extract_registered_channels().
# ==============================================================================
echo ""
echo "--- Check 19: --print-registered extraction completeness (task-4586) ---"

_fix19dir="$_tmpdir/fix19"
mkdir -p "$_fix19dir/docs" "$_fix19dir/gui/src-tauri/src"

cat > "$_fix19dir/docs/gui-event-channels.md" <<'INVENTORY'
# GUI Event Channel Inventory

## §1 — Wired channels (production today)

| Channel | Notes |
|---|---|
| `mesh-update` | wired |
| `kernel-status` | wired |
| `file-changed` | wired |
| `value-update` | wired |
| `claude-done` | wired |

## §2 — Channels this PRD adds (FICTION → WIRED via GR-016 decomposition)

| Channel | Notes |
|---|---|
INVENTORY

cat > "$_fix19dir/gui/src-tauri/src/main.rs" <<'RUST'
fn noop() {}
RUST

_init_repo "$_fix19dir" gui/src-tauri/src/main.rs

_fix19_stdout="$_tmpdir/fix19_stdout.txt"
"$CHECK_SCRIPT" --repo-root "$_fix19dir" --print-registered > "$_fix19_stdout" 2>/dev/null || true

assert "Check 19: --print-registered exits 0" \
    "$CHECK_SCRIPT" --repo-root "$_fix19dir" --print-registered

assert "Check 19: mesh-update appears in --print-registered output" \
    grep -qx 'mesh-update' "$_fix19_stdout"

assert "Check 19: kernel-status appears in --print-registered output" \
    grep -qx 'kernel-status' "$_fix19_stdout"

assert "Check 19: file-changed appears in --print-registered output" \
    grep -qx 'file-changed' "$_fix19_stdout"

assert "Check 19: value-update appears in --print-registered output" \
    grep -qx 'value-update' "$_fix19_stdout"

assert "Check 19: claude-done appears in --print-registered output" \
    grep -qx 'claude-done' "$_fix19_stdout"

# Generic property assertion: every | `name` | row in the real inventory must
# appear in --print-registered output.  Decoupled from specific channel names —
# a legitimate rename/removal of any channel won't break this test for the wrong
# reason.  The esc-4578-61 regression (file-changed dropping under load) is
# covered generically: if ANY registered channel is missing the assertion fails.
_fix19_real_stdout="$_tmpdir/fix19_real.txt"
"$CHECK_SCRIPT" --repo-root "$REPO_ROOT" --print-registered > "$_fix19_real_stdout" 2>/dev/null || true

_fix19_inv_channels=$(grep -oP '\| `\K[a-z0-9-]+(?=` \|)' \
    "$REPO_ROOT/docs/gui-event-channels.md" 2>/dev/null | sort -u || true)
while IFS= read -r _ch19; do
    [[ -z "$_ch19" ]] && continue
    assert "Check 19: '$_ch19' (real inventory) in --print-registered output" \
        grep -qx "$_ch19" "$_fix19_real_stdout"
done <<< "$_fix19_inv_channels"

# -- Summary ------------------------------------------------------------------
test_summary
