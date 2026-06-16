#!/usr/bin/env bash
# Infrastructure test for task 4626.
# Drift guard for scripts/verify-pipeline-guard.sh — verifies the classifier's
# decision contract (load-bearing vs fast-path-safe paths).
#
# Auto-discovered by tests/infra/run_all.sh AND auto-pulled into task-scope
# when verify.sh changes (matches the task-4523 'scripts/verify.sh ->
# tests/infra/test_verify_*.sh' row in scripts/verify-pipeline-infra-tests.txt).
#
# This test is hermetic: it drives the classifier script directly with no
# cargo or git operations.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== verify-pipeline-guard.sh classifier tests ==="

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

GUARD_SH="$REPO_ROOT/scripts/verify-pipeline-guard.sh"

# run_guard <subcommand> [args...] — invoke the classifier under test.
run_guard() {
    bash "$GUARD_SH" "$@"
}

# assert_exit DESC EXPECTED CMD [args...] — assert CMD exits EXPECTED_CODE.
# Increments the global PASS/FAIL counters from test_helpers.sh.
assert_exit() {
    local desc="$1" expected="$2"
    shift 2
    local actual=0
    "$@" >/dev/null 2>&1 || actual=$?
    if [ "$actual" -eq "$expected" ]; then
        echo "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc (expected exit $expected, got $actual)"
        FAIL=$((FAIL + 1))
    fi
}

# ---------------------------------------------------------------------------
# Pair A — core decision contract
# ---------------------------------------------------------------------------

echo ""
echo "-- Pair A: core decision contract --"

# POSITIVE: anchor — scripts/verify.sh is always load-bearing
assert_exit "POSITIVE: scripts/verify.sh is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate scripts/verify.sh

# POSITIVE: static manifest data deps
assert_exit "POSITIVE: .config/nextest.toml is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate .config/nextest.toml

assert_exit "POSITIVE: scripts/occt-touching-crates.txt is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate scripts/occt-touching-crates.txt

# NEGATIVE: fast-path preserved for genuine config-only paths (exit 1).
# These prove the guard stays surgical and does not break the config-only
# fast-path throughput benefit.
assert_exit "NEGATIVE: docs/note.md is fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate docs/note.md

assert_exit "NEGATIVE: orchestrator.yaml is fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate orchestrator.yaml

assert_exit "NEGATIVE: README.md is fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate README.md

# MIXED: the incident shape — any load-bearing file in the diff forces the full gate.
assert_exit "MIXED: docs/note.md + scripts/verify.sh -> full gate required (exit 0)" 0 \
    run_guard requires-full-gate docs/note.md scripts/verify.sh

# STDIN form: pipe paths to 'requires-full-gate' (no args) — supports large diffs
# that would exceed ARG_MAX if passed as positional args.
assert_exit "STDIN: load-bearing file piped in -> full gate required (exit 0)" 0 \
    bash -c 'printf "docs/x.md\nscripts/verify.sh\n" | bash "$1" requires-full-gate' \
    _ "$GUARD_SH"

# NORMALIZATION: a leading './' is stripped defensively — a caller that passes
# './scripts/verify.sh' instead of the canonical 'scripts/verify.sh' should
# still trigger the full gate (guards against a cross-repo caller prefixing './').
assert_exit "NORMALIZE: ./scripts/verify.sh stripped to scripts/verify.sh (exit 0)" 0 \
    run_guard requires-full-gate ./scripts/verify.sh

# --list contract: output must include scripts/verify.sh (one path per line).
assert "--list output includes scripts/verify.sh" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/verify.sh"' \
    _ "$GUARD_SH"

# Usage error: unknown subcommand/flag -> exit 2.
assert_exit "usage: unknown flag --bogus exits 2" 2 \
    run_guard --bogus

# Usage error: no subcommand -> exit 2.
assert_exit "usage: no subcommand exits 2" 2 \
    run_guard

# ---------------------------------------------------------------------------
# Pair B — live sourced-lib auto-derivation (self-healing coverage)
# ---------------------------------------------------------------------------

echo ""
echo "-- Pair B: sourced-lib auto-derivation --"

# REAL-LIB regression: independently derive the live sourced libs from
# verify.sh using the exact anchored grep idiom from make_branch_fixture in
# test_verify_throughput.sh.  For each derived lib L, assert that
# requires-full-gate scripts/L exits 0.  The test never hardcodes the lib set
# — it derives it dynamically, so future additions are automatically covered.
while IFS= read -r _lib; do
    assert_exit "REAL-LIB: scripts/$_lib is load-bearing (sourced by verify.sh)" 0 \
        run_guard requires-full-gate "scripts/$_lib"
done < <(grep -E '^[[:space:]]*source "\$SCRIPT_DIR/' "$REPO_ROOT/scripts/verify.sh" \
         | sed -n 's|.*source "\$SCRIPT_DIR/\([^"]*\)".*|\1|p')

# Incident-sim: a lib-only diff (the #4618/#4624 class) must NOT fast-path.
# These tasks bumped plan-affecting sourced libs and landed green via the
# config-only fast-path, ambushing the next Rust task (#4288) with a RED
# test_verify_throughput.sh (root-caused in esc-4288-206).
assert_exit "INCIDENT-SIM: scripts/occt-scope-lib.sh (lib diff) -> full gate required (exit 0)" 0 \
    run_guard requires-full-gate scripts/occt-scope-lib.sh

# GROUND-TRUTH: hard-coded assertions for the four libs KNOWN to be sourced by
# verify.sh today.  These are independent of the grep|sed derivation loop
# above — if the production extraction regex had a bug (e.g., mishandled an
# indented or multi-word source line), both the loop and the loop's expectation
# would compute the same wrong result, masking the regression.  This
# independent check catches that divergence.
assert "--list includes scripts/occt-scope-lib.sh (hard-coded ground truth)" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/occt-scope-lib.sh"' \
    _ "$GUARD_SH"
assert "--list includes scripts/release-scope-lib.sh (hard-coded ground truth)" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/release-scope-lib.sh"' \
    _ "$GUARD_SH"
assert "--list includes scripts/affected-crates-lib.sh (hard-coded ground truth)" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/affected-crates-lib.sh"' \
    _ "$GUARD_SH"
assert "--list includes scripts/lib_test_semaphore.sh (hard-coded ground truth)" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/lib_test_semaphore.sh"' \
    _ "$GUARD_SH"

# SYNTHETIC self-healing: build a throwaway verify.sh copy with a fake source
# line appended, prove the classifier auto-covers the new lib via
# REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH — no manifest edit needed.
_SYNTH_DIR="$(mktemp -d)"
_TMPDIRS+=("$_SYNTH_DIR")
_SYNTH_VERIFY="$_SYNTH_DIR/verify.sh"
cp "$REPO_ROOT/scripts/verify.sh" "$_SYNTH_VERIFY"
printf '\nsource "$SCRIPT_DIR/zzz-synthetic-lib.sh"\n' >> "$_SYNTH_VERIFY"

assert_exit "SYNTHETIC: zzz-synthetic-lib.sh auto-covered after injection (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-synthetic-lib.sh' \
    _ "$_SYNTH_VERIFY" "$GUARD_SH"

# DERIVATION PRECISION: a sibling that is NOT source'd must remain fast-path-safe.
# Proves the classifier flags ONLY actually-sourced libs, not every script
# under scripts/.
assert_exit "PRECISION: scripts/zzz-not-sourced.sh NOT sourced -> fast-path-safe (exit 1)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-not-sourced.sh' \
    _ "$_SYNTH_VERIFY" "$GUARD_SH"

test_summary
