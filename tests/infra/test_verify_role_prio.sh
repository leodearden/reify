#!/usr/bin/env bash
# Infrastructure test for task 4051 (Cycles A-B), task 4078 (Cycle C), and
# task 4913/A2 (Cycles D-E).
# Covers:
#   Cycle A — DF_VERIFY_ROLE validation / exit-64 contract (step-1 / step-2)
#   Cycle B — CARGO_PRIO prefix-wrapping contract         (step-3 / step-4)
#   Cycle C — PROFILE default by DF_VERIFY_ROLE            (task-4078 step-1 / step-2)
#             merge+no-profile=>both; explicit --profile wins; task/unset=>debug
#   Cycle D — offline role recognition + idle-class CARGO_PRIO + profile=release
#             (task 4913/A2 step-1 / step-2)
#   Cycle E — offline positive heavy filter + --run-ignored all + jobserver detach
#             (task 4913/A2 step-3 / step-4)
#
# Drives verify.sh via --print-plan (hermetic: never builds anything).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== DF_VERIFY_ROLE validation and cargo priority prefix tests ==="

# ---------------------------------------------------------------------------
# Cycle A: DF_VERIFY_ROLE validation / exit-64 contract
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle A: DF_VERIFY_ROLE validation ---"

# Capture exit code and stderr for a bogus role without triggering set -e.
_bogus_stderr_file="$(mktemp)"
_bogus_rc=0
DF_VERIFY_ROLE=bogus bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan \
    >"$_bogus_stderr_file" 2>&1 \
    || _bogus_rc=$?
_bogus_stderr="$(cat "$_bogus_stderr_file")"
rm -f "$_bogus_stderr_file"

# (a) bogus role must exit 64
assert "DF_VERIFY_ROLE=bogus: exits 64" \
    test "$_bogus_rc" -eq 64

# (b) stderr must contain the exact diagnostic (em-dash U+2014 is literal in the string below)
assert "DF_VERIFY_ROLE=bogus: stderr contains expected ERROR diagnostic" \
    bash -c 'printf "%s\n" "$1" | grep -qF "verify.sh: ERROR — unknown DF_VERIFY_ROLE '"'"'bogus'"'"' (want task|merge|offline)"' \
    _ "$_bogus_stderr"

# (c) valid role 'task' must exit 0
assert "DF_VERIFY_ROLE=task: exits 0" \
    bash -c 'DF_VERIFY_ROLE=task bash "$1/scripts/verify.sh" test --scope all --print-plan >/dev/null 2>&1' \
    _ "$REPO_ROOT"

# (d) valid role 'merge' must exit 0
assert "DF_VERIFY_ROLE=merge: exits 0" \
    bash -c 'DF_VERIFY_ROLE=merge bash "$1/scripts/verify.sh" test --scope all --print-plan >/dev/null 2>&1' \
    _ "$REPO_ROOT"

# (e) unset DF_VERIFY_ROLE must exit 0 (defaults to task)
assert "DF_VERIFY_ROLE unset: exits 0 (defaults to task)" \
    bash -c 'env -u DF_VERIFY_ROLE bash "$1/scripts/verify.sh" test --scope all --print-plan >/dev/null 2>&1' \
    _ "$REPO_ROOT"

# ---------------------------------------------------------------------------
# Cycle B: CARGO_PRIO prefix-wrapping contract
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle B: prefix-wrapping contract ---"

# Capture command lines for each role/action combination.
# grep -v '^#' strips env-export and comment lines; command lines remain.
TASK_TEST_PLAN="$(DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan \
    | grep -v '^#')"
TASK_ALL_PLAN="$(DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan \
    | grep -v '^#')"
UNSET_TEST_PLAN="$(env -u DF_VERIFY_ROLE bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan \
    | grep -v '^#')"
MERGE_PLAN_FULL="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan)"
MERGE_TEST_PLAN="$(printf '%s\n' "$MERGE_PLAN_FULL" | grep -v '^#')"
MERGE_ALL_PLAN_FULL="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan)"
MERGE_ALL_PLAN="$(printf '%s\n' "$MERGE_ALL_PLAN_FULL" | grep -v '^#')"

# --- task / test / all ---

# Sanity: at least 1 cargo command line in the plan.
# Task 4451 folds the two-pass (gated+ungated) into a single nextest pass, so
# the test action plan has one cargo line (debug nextest run --workspace).
# '(^| )cargo ' matches the real cargo token; 'cargo-test-occt-gated.sh' has a
# hyphen so it doesn't match the wrapper script name, only the 'cargo test' arg.
assert "task/test/all: at least 1 cargo command line (sanity, task 4451: single nextest pass)" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -cE "(^| )cargo " || echo 0)" -ge 1 ]' \
    _ "$TASK_TEST_PLAN"

# All cargo lines must carry the task prefix (zero unprefixed lines).
assert "task/test/all: all cargo lines prefixed with 'nice -n 15 ionice -c 2 -n 7 cargo'" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -vq "nice -n 15 ionice -c 2 -n 7 cargo"' \
    _ "$TASK_TEST_PLAN"

# --- task / all / all (covers clippy + gated + ungated) ---

assert "task/all/all: at least 2 cargo command lines (sanity)" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -cE "(^| )cargo " || echo 0)" -ge 2 ]' \
    _ "$TASK_ALL_PLAN"

assert "task/all/all: all cargo lines prefixed with 'nice -n 15 ionice -c 2 -n 7 cargo'" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -vq "nice -n 15 ionice -c 2 -n 7 cargo"' \
    _ "$TASK_ALL_PLAN"

# Negative contract: non-cargo lines (tree-sitter-generate.sh, npm) must NOT carry
# the prefix.  Every plan line that contains the nice/ionice prefix must also
# contain 'cargo' — i.e. zero prefix-bearing lines lack 'cargo'.
assert "task/all/all: only cargo lines carry the nice/ionice prefix (non-cargo lines clean)" \
    bash -c '! printf "%s\n" "$1" | grep -F "nice -n 15 ionice -c 2 -n 7 " | grep -vq "cargo"' \
    _ "$TASK_ALL_PLAN"

# --- unset role defaults to task ---

assert "unset role: plan contains task prefix 'nice -n 15 ionice -c 2 -n 7 cargo'" \
    bash -c 'printf "%s\n" "$1" | grep -qF "nice -n 15 ionice -c 2 -n 7 cargo"' \
    _ "$UNSET_TEST_PLAN"

# --- merge / test / all ---

# Every cargo line prefixed with 'nice -n 5 cargo' (mild CPU nice, no ionice).
assert "merge/test/all: all cargo lines prefixed with 'nice -n 5 cargo'" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -vq "nice -n 5 cargo"' \
    _ "$MERGE_TEST_PLAN"

# The full plan output (including header) must contain NO 'ionice'.
assert "merge/test/all: no 'ionice' anywhere in the full plan output" \
    bash -c '! printf "%s\n" "$1" | grep -q "ionice"' \
    _ "$MERGE_PLAN_FULL"

# --- merge / all / all (covers clippy + gated + ungated) ---
# Mirrors the task/all/all assertion so the merge path on lint/typecheck commands
# is also covered.

assert "merge/all/all: all cargo lines prefixed with 'nice -n 5 cargo'" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -vq "nice -n 5 cargo"' \
    _ "$MERGE_ALL_PLAN"

assert "merge/all/all: no 'ionice' anywhere in the full plan output" \
    bash -c '! printf "%s\n" "$1" | grep -q "ionice"' \
    _ "$MERGE_ALL_PLAN_FULL"

# ---------------------------------------------------------------------------
# Cycle C: PROFILE default by DF_VERIFY_ROLE (task 4078)
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle C: PROFILE default by DF_VERIFY_ROLE ---"

# Capture full plan and commands-only plan for each case.
# C1: merge + no explicit --profile => profile=both (release coverage on merge path)
C1_FULL="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan)"
C1_CMDS="$(printf '%s\n' "$C1_FULL" | grep -v '^#')"

# C2: merge + explicit --profile debug => profile=debug (explicit wins)
C2_FULL="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" test --scope all --profile debug --print-plan)"
C2_CMDS="$(printf '%s\n' "$C2_FULL" | grep -v '^#')"

# C2b: merge + explicit --profile release => profile=release (explicit wins; never coerced to both)
C2B_FULL="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" test --scope all --profile release --print-plan)"
C2B_CMDS="$(printf '%s\n' "$C2B_FULL" | grep -v '^#')"

# C3: task + no explicit --profile => profile=debug (task role unchanged)
C3_FULL="$(DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan)"
C3_CMDS="$(printf '%s\n' "$C3_FULL" | grep -v '^#')"

# C3b: unset DF_VERIFY_ROLE + no explicit --profile => profile=debug (unset defaults to task)
C3B_FULL="$(env -u DF_VERIFY_ROLE bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan)"
C3B_CMDS="$(printf '%s\n' "$C3B_FULL" | grep -v '^#')"

# --- C1: merge + no --profile => both ---
assert "C1: merge+no-profile: header shows profile=both" \
    bash -c 'printf "%s\n" "$1" | grep "^# verify.sh plan" | grep -q "profile=both"' \
    _ "$C1_FULL"

assert "C1: merge+no-profile: a release test pass is present (sensitivity-scoped, no --workspace)" \
    bash -c 'printf "%s\n" "$1" | grep -v "cargo-test-occt-gated.sh" | grep -qE "cargo (test|nextest run).*--release"' \
    _ "$C1_CMDS"

assert "C1: merge+no-profile: a non-release (debug) pass is also present" \
    bash -c 'printf "%s\n" "$1" | grep -E "cargo (test|nextest run) --workspace" | grep -qv -- "--release"' \
    _ "$C1_CMDS"

# --- C2: merge + explicit --profile debug => debug (explicit wins) ---
assert "C2: merge+--profile debug: header shows profile=debug (explicit wins)" \
    bash -c 'printf "%s\n" "$1" | grep "^# verify.sh plan" | grep -q "profile=debug"' \
    _ "$C2_FULL"

assert "C2: merge+--profile debug: no release workspace pass (explicit wins)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo (test|nextest run) --workspace.*--release"' \
    _ "$C2_CMDS"

# --- C2b: merge + explicit --profile release => release (fully authoritative) ---
assert "C2b: merge+--profile release: header shows profile=release (explicit fully authoritative)" \
    bash -c 'printf "%s\n" "$1" | grep "^# verify.sh plan" | grep -q "profile=release"' \
    _ "$C2B_FULL"

# --- C3: task + no --profile => debug ---
assert "C3: task+no-profile: header shows profile=debug" \
    bash -c 'printf "%s\n" "$1" | grep "^# verify.sh plan" | grep -q "profile=debug"' \
    _ "$C3_FULL"

assert "C3: task+no-profile: no release workspace pass" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo (test|nextest run) --workspace.*--release"' \
    _ "$C3_CMDS"

# --- C3b: unset role + no --profile => debug ---
assert "C3b: unset-role+no-profile: header shows profile=debug (unset defaults to task)" \
    bash -c 'printf "%s\n" "$1" | grep "^# verify.sh plan" | grep -q "profile=debug"' \
    _ "$C3B_FULL"

# ---------------------------------------------------------------------------
# Cycle D: offline role — recognition, idle-class CARGO_PRIO, profile=release
# (task 4913 / A2, step-1 / step-2)
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle D: offline role (task 4913 / A2) ---"

# (a) valid role 'offline' must exit 0 (recognized role)
assert "DF_VERIFY_ROLE=offline: exits 0" \
    bash -c 'DF_VERIFY_ROLE=offline bash "$1/scripts/verify.sh" test --scope all --print-plan >/dev/null 2>&1' \
    _ "$REPO_ROOT"

# Capture the offline plan. Guarded with '|| true' so an as-yet-unrecognized
# role (RED phase, pre step-2) reports clean assertion FAILs below instead of
# tripping this script's own `set -e` on the failing command substitution.
OFFLINE_FULL="$(DF_VERIFY_ROLE=offline bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan || true)"
OFFLINE_CMDS="$(printf '%s\n' "$OFFLINE_FULL" | grep -v '^#')"

# (c) idle-class CARGO_PRIO prefix contract — mirrors the task/merge prefix idiom (Cycle B).
assert "offline/test/all: at least 1 cargo command line (sanity)" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -cE "(^| )cargo " || echo 0)" -ge 1 ]' \
    _ "$OFFLINE_CMDS"

assert "offline/test/all: all cargo lines prefixed with 'nice -n 19 ionice -c3 cargo'" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -vq "nice -n 19 ionice -c3 cargo"' \
    _ "$OFFLINE_CMDS"

assert "offline/test/all: only cargo lines carry the nice/ionice prefix (non-cargo lines clean)" \
    bash -c '! printf "%s\n" "$1" | grep -F "nice -n 19 ionice -c3 " | grep -vq "cargo"' \
    _ "$OFFLINE_CMDS"

# (d) profile default: offline + no explicit --profile => release (single profile, not both)
assert "offline+no-profile: header shows profile=release" \
    bash -c 'printf "%s\n" "$1" | grep "^# verify.sh plan" | grep -q "profile=release"' \
    _ "$OFFLINE_FULL"

assert "offline+no-profile: a release test pass is present" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo (test|nextest run).*--release"' \
    _ "$OFFLINE_CMDS"

assert "offline+no-profile: no non-release (debug) --workspace pass present" \
    bash -c '! printf "%s\n" "$1" | grep -E "cargo (test|nextest run) --workspace" | grep -qv -- "--release"' \
    _ "$OFFLINE_CMDS"

# ---------------------------------------------------------------------------
# Cycle E: offline positive heavy filter + --run-ignored all + jobserver detach
# (task 4913 / A2, step-3 / step-4)
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle E: offline heavy filter + jobserver detach (task 4913 / A2) ---"

# Single source of truth for the `heavy` filter expression (A1 / task 4912) —
# assert on a real atom substring instead of hand-duplicating the expression,
# so this fixture can never silently drift from scripts/heavy-test-filter-lib.sh.
# Mirrors tests/infra/test_verify_gate_exclude_heavy.sh's (A4) pattern.
HEAVY_LIB="$REPO_ROOT/scripts/heavy-test-filter-lib.sh"
if [ ! -f "$HEAVY_LIB" ]; then
    echo "ERROR: scripts/heavy-test-filter-lib.sh not found (task 4912/A1 not landed?)"
    exit 1
fi
# shellcheck source=scripts/heavy-test-filter-lib.sh
source "$HEAVY_LIB"

if [ -z "${REIFY_HEAVY_NEXTEST_FILTER:-}" ]; then
    echo "ERROR: REIFY_HEAVY_NEXTEST_FILTER not defined after sourcing $HEAVY_LIB"
    exit 1
fi

HEAVY_ATOM="binary(determinism)"
case "$REIFY_HEAVY_NEXTEST_FILTER" in
    *"$HEAVY_ATOM"*) ;;
    *)
        echo "ERROR: fixture atom '$HEAVY_ATOM' not found in REIFY_HEAVY_NEXTEST_FILTER — this test's fixture has drifted from scripts/heavy-test-filter-lib.sh"
        exit 1
        ;;
esac

POSITIVE_PATTERN='-E "('
NEGATIVE_PATTERN='-E "not ('

# nextest availability — reuse the Cycle D offline plan header (NEXTEST is
# role/knob-invariant, computed once in verify.sh before role logic runs;
# same probe idiom as the A4 test).
_OFFLINE_HEADER="$(printf '%s\n' "$OFFLINE_FULL" | grep '^# verify.sh plan')"
NEXTEST_AVAILABLE=0
case "$_OFFLINE_HEADER" in
    *"nextest=1"*) NEXTEST_AVAILABLE=1 ;;
esac
echo "(nextest available on this host: $NEXTEST_AVAILABLE)"

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    echo ""
    echo "--- nextest available: expect positive heavy filter + --run-ignored all + --workspace --release ---"

    assert "offline: plan contains $POSITIVE_PATTERN (positive heavy filter)" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$OFFLINE_CMDS" "$POSITIVE_PATTERN"

    assert "offline: plan contains a real heavy atom ($HEAVY_ATOM)" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$OFFLINE_CMDS" "$HEAVY_ATOM"

    assert "offline: plan contains --run-ignored all" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "--run-ignored all"' \
        _ "$OFFLINE_CMDS"

    assert "offline: release pass uses --workspace (sole membership determinant is the heavy filter)" \
        bash -c 'printf "%s\n" "$1" | grep -qE "cargo nextest run --workspace --release"' \
        _ "$OFFLINE_CMDS"

    assert "offline: plan does NOT contain the negated gate-exclude pattern $NEGATIVE_PATTERN" \
        bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$OFFLINE_CMDS" "$NEGATIVE_PATTERN"
else
    echo ""
    echo "--- positive assertions SKIPPED (nextest not available on this host) ---"
    echo "--- nextest unavailable: expect fallback cargo-test path NEVER emits $POSITIVE_PATTERN ---"

    assert "offline, nextest unavailable: plan has NO $POSITIVE_PATTERN (cargo-test fallback has no -E support)" \
        bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$OFFLINE_CMDS" "$POSITIVE_PATTERN"
fi

# Jobserver detach — nextest-independent, always run. offline draws from
# neither the task nor merge FIFO (PRD §8 invariant): the full plan output
# must contain NO CARGO_MAKEFLAGS export line, regardless of whether the
# task/merge jobserver FIFOs exist on this host.
assert "offline: plan has NO 'export CARGO_MAKEFLAGS=' line (off the merge jobserver)" \
    bash -c '! printf "%s\n" "$1" | grep -q "export CARGO_MAKEFLAGS="' \
    _ "$OFFLINE_FULL"

test_summary
