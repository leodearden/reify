#!/usr/bin/env bash
# Infrastructure test for task 5252.
# Guards the MECHANICAL pre-merge/CI gate that fails verify EARLY (naming the
# offending file) whenever a tests/infra/test_*.sh exists on the tree without a
# corresponding row in tests/infra/run-all-classification.manifest (or, the
# reverse direction, a manifest row with no file). This supplements task 5160's
# prompt-only /prd authoring reminder, which was proven insufficient by two
# post-landing recurrences (tasks 5247 & 5249; see esc-5252-1).
#
# The gate and the pre-existing drift test (test_run_all_classification.sh
# Test 5) both derive from ONE library — tests/infra/run-all-classification-
# lib.sh — so the check and the manifest cannot silently diverge by
# construction (same shared-derivation pattern as scripts/release-scope-lib.sh,
# sourced by both scripts/verify.sh and test_release_scoped_scope.sh).
#
# Assertions:
#   (a) run-all-classification-lib.sh defines classification_undeclared and
#       classification_orphaned; both are EMPTY on the real (drift-free)
#       manifest (green on truth).
#   (b) On injected-drift fixtures (test_run_all_classification.sh Test 6
#       pattern: copy manifest -> mktemp, drop/insert a row) the accessors
#       report EXACTLY the drifted basename: classification_undeclared names a
#       declared row dropped from a still-present file; classification_orphaned
#       names an added row for a nonexistent file.
#   (c) scripts/check-infra-classification-manifest.sh exists, is executable,
#       and exits 0 on the real tree.
#   (d) The gate exits NON-zero AND its combined output NAMES the offending
#       file when RUN_ALL_CLASSIFICATION_MANIFEST points at a drift fixture
#       (acceptance: the failure clearly names the offending file).
#
# Hermetic: pure bash + filesystem; sources the lib in throwaway child shells
# and drives the gate against temp fixtures — never runs cargo/npm and never
# mutates the real manifest.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# Source the shared classification library — classification_declared_union /
# classification_discovered_set already exist and are used here to build
# fixtures; classification_undeclared / classification_orphaned are the two new
# accessors under test.
[ -f "$SCRIPT_DIR/run-all-classification-lib.sh" ] || { echo "ERROR: run-all-classification-lib.sh not found at $SCRIPT_DIR/run-all-classification-lib.sh"; exit 1; }
source "$SCRIPT_DIR/run-all-classification-lib.sh"

LIB="$SCRIPT_DIR/run-all-classification-lib.sh"
MANIFEST="$SCRIPT_DIR/run-all-classification.manifest"
GATE="$REPO_ROOT/scripts/check-infra-classification-manifest.sh"

echo "=== infra classification-manifest gate tests (task 5252) ==="

# ---------------------------------------------------------------------------
# Fixtures — mktemp + trap cleanup (mirrors test_run_all_classification.sh
# Test 6). Built from the REAL manifest, never mutating it.
# ---------------------------------------------------------------------------
_TMPDIR="$(mktemp -d)"
trap 'rm -rf "$_TMPDIR"' EXIT

# Drift fixture: real manifest minus one declared row. The dropped basename
# still resolves to a real file on disk (every declared entry does — Test 4 of
# test_run_all_classification.sh), so it becomes discovered-but-undeclared.
# `awk NR==1` (not head -n1) avoids the pipefail-SIGPIPE flake (esc-5172-1).
_DROPPED="$(classification_declared_union | awk 'NR==1')"
_DRIFT_MANIFEST="$_TMPDIR/drift.manifest"
awk -v drop="$_DROPPED" '$1 != drop' "$MANIFEST" > "$_DRIFT_MANIFEST"

# Orphan fixture: real manifest plus a bogus row for a file that does not exist
# -> declared-but-not-discovered (a stale / orphaned row).
_BOGUS="test_does_not_exist.sh"
_ORPHAN_MANIFEST="$_TMPDIR/orphan.manifest"
cp "$MANIFEST" "$_ORPHAN_MANIFEST"
echo "$_BOGUS pool" >> "$_ORPHAN_MANIFEST"

# ---------------------------------------------------------------------------
# (a) accessors defined + empty on the real (drift-free) manifest
# ---------------------------------------------------------------------------
echo ""
echo "--- (a) classification_undeclared/orphaned defined + empty on real manifest ---"

assert "classification_undeclared is defined after sourcing the lib" \
    declare -F classification_undeclared

assert "classification_orphaned is defined after sourcing the lib" \
    declare -F classification_orphaned

# Combined defined-AND-empty so this is genuinely RED before step-2 (an
# undefined function fails `declare -F` rather than vacuously printing empty).
assert "classification_undeclared is EMPTY on the real drift-free manifest (green on truth)" \
    bash -c 'source "$1"; declare -F classification_undeclared >/dev/null && [ -z "$(classification_undeclared)" ]' _ "$LIB"

assert "classification_orphaned is EMPTY on the real drift-free manifest (green on truth)" \
    bash -c 'source "$1"; declare -F classification_orphaned >/dev/null && [ -z "$(classification_orphaned)" ]' _ "$LIB"

# ---------------------------------------------------------------------------
# (b) accessors report injected drift/orphan EXACTLY
# ---------------------------------------------------------------------------
echo ""
echo "--- (b) accessors report injected drift/orphan exactly ---"

assert "classification_undeclared on a drift fixture prints EXACTLY the dropped basename ('$_DROPPED')" \
    bash -c 'source "$1"; [ "$(classification_undeclared "$2")" = "$3" ]' _ "$LIB" "$_DRIFT_MANIFEST" "$_DROPPED"

assert "classification_orphaned on an orphan fixture prints EXACTLY the bogus basename ('$_BOGUS')" \
    bash -c 'source "$1"; [ "$(classification_orphaned "$2")" = "$3" ]' _ "$LIB" "$_ORPHAN_MANIFEST" "$_BOGUS"

# ---------------------------------------------------------------------------
# (c) gate script exists + executable + exits 0 on the real tree
# ---------------------------------------------------------------------------
echo ""
echo "--- (c) gate script exists + executable + exits 0 on the real tree ---"

assert "scripts/check-infra-classification-manifest.sh exists" \
    test -f "$GATE"

assert "scripts/check-infra-classification-manifest.sh is executable" \
    test -x "$GATE"

assert "gate exits 0 on the real (drift-free) tree" \
    bash "$GATE"

# ---------------------------------------------------------------------------
# (d) gate fails non-zero on drift AND names the offending file
# ---------------------------------------------------------------------------
echo ""
echo "--- (d) gate fails non-zero on drift and names the offending file ---"

# Guard on `-x "$GATE"` so a missing script fails this assert (a missing file
# also exits non-zero, which would otherwise false-pass the negation).
assert "gate exits NON-zero when RUN_ALL_CLASSIFICATION_MANIFEST points at a drift fixture" \
    bash -c '[ -x "$1" ] && ! RUN_ALL_CLASSIFICATION_MANIFEST="$2" bash "$1"' _ "$GATE" "$_DRIFT_MANIFEST"

assert "gate output NAMES the offending undeclared file ('$_DROPPED')" \
    bash -c 'out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$3" bash "$1" 2>&1 || true)"; printf "%s" "$out" | grep -qF -- "$2"' _ "$GATE" "$_DROPPED" "$_DRIFT_MANIFEST"

# ---------------------------------------------------------------------------
# (e) plan-shape: verify.sh emits the gate EARLY, before the expensive poles.
# Hermetic --print-plan oracle + `grep -n` relative-index technique
# (test_verify_failfast_order.sh:29-58; never runs cargo/npm). RED until
# step-4 wires build_plan().
#
# Two plan captures are needed because the two "expensive pole" anchors live in
# different tiers: the role=task `all --scope all` plan carries the psi-gate and
# check-manifold-deps.sh poles but NOT run_all.sh (the wholesale pool suite
# moved to the merge tier — test_verify_failfast_order.sh Test 6e), while the
# merge-tier plan carries run_all.sh. So the check-manifold/psi-gate anchors
# read the task plan and the run_all.sh anchor reads the merge plan. Both tiers
# run at RUN_RUST=1, where the gate is emitted.
#
# The NEGATIVE invariant — docs-only / gui-src-only diffs (RUN_RUST=0) must NOT
# emit the gate line — is deliberately NOT re-encoded here: it is delegated to
# and enforced by test_verify_scope.sh's existing `plan_cmdcount -eq 0`
# assertions (docs-only + branch/docs "zero command leaves"). The RUN_RUST=1
# gating in verify.sh preserves those: were the gate line to leak into a
# RUN_RUST=0 plan, those counts would become non-zero and test_verify_scope.sh
# would go RED.
# ---------------------------------------------------------------------------
echo ""
echo "--- (e) verify.sh emits the gate early, before the expensive poles (plan-shape) ---"

GATE_LINE='./scripts/check-infra-classification-manifest.sh'
TASK_ALL_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan | grep -v '^#')"
MERGE_ALL_PLAN="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan | grep -v '^#')"
export GATE_LINE TASK_ALL_PLAN MERGE_ALL_PLAN

assert "task all plan: contains the gate line ($GATE_LINE)" \
    bash -c 'printf "%s\n" "$2" | grep -qF -- "$1"' _ "$GATE_LINE" "$TASK_ALL_PLAN"

assert "merge all plan: contains the gate line ($GATE_LINE)" \
    bash -c 'printf "%s\n" "$2" | grep -qF -- "$1"' _ "$GATE_LINE" "$MERGE_ALL_PLAN"

# gate index < check-manifold-deps.sh index — the gate is the FIRST plan entry.
assert "task all plan: gate index < check-manifold-deps.sh index (gate emitted first)" \
    bash -c '
        G=$(printf "%s\n" "$2" | grep -nF -- "$1" | head -1 | cut -d: -f1)
        M=$(printf "%s\n" "$2" | grep -n "check-manifold-deps\.sh" | head -1 | cut -d: -f1)
        [ -n "$G" ] && [ -n "$M" ] && [ "$G" -lt "$M" ]
    ' _ "$GATE_LINE" "$TASK_ALL_PLAN"

# gate index < psi-gate index — before the expensive rust pole.
assert "task all plan: gate index < psi-gate index (before the expensive pole)" \
    bash -c '
        G=$(printf "%s\n" "$2" | grep -nF -- "$1" | head -1 | cut -d: -f1)
        P=$(printf "%s\n" "$2" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$G" ] && [ -n "$P" ] && [ "$G" -lt "$P" ]
    ' _ "$GATE_LINE" "$TASK_ALL_PLAN"

# gate index < run_all.sh index — before the wholesale pool suite (merge tier).
assert "merge all plan: gate index < run_all.sh index (before the wholesale pool suite)" \
    bash -c '
        G=$(printf "%s\n" "$2" | grep -nF -- "$1" | head -1 | cut -d: -f1)
        R=$(printf "%s\n" "$2" | grep -n "run_all\.sh" | head -1 | cut -d: -f1)
        [ -n "$G" ] && [ -n "$R" ] && [ "$G" -lt "$R" ]
    ' _ "$GATE_LINE" "$MERGE_ALL_PLAN"

test_summary
