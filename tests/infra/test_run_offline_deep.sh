#!/usr/bin/env bash
# Infrastructure test for task 4916/A5 — scripts/run-offline-deep.sh, the
# real reify-local EXECUTING consumer of the A2 DF_VERIFY_ROLE=offline role
# (task 4913): the anti-orphan caller that actually RUNS the heavy offline
# set off-gate, not merely --print-plan.
#
# Drives the wrapper hermetically via two short-circuit paths that never
# build anything:
#   - PASS path: `--print-plan` (verify.sh exits 0, emits the plan).
#   - FAIL path: an unknown bogus arg (verify.sh's own arg parser exits 64
#     before any cargo work).
#
# Mirrors:
#   - tests/infra/test_verify_role_prio.sh Cycles D/E — the offline
#     --print-plan oracle idioms (header shape, CARGO_PRIO idle-class
#     prefix, positive heavy filter, --run-ignored all, no CARGO_MAKEFLAGS
#     export).
#   - tests/infra/test_run_gui_scripts.sh — wrapper-script drift-guard
#     structure (exists/executable/shebang/strict-mode, then behavior).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

RUN_OFFLINE_DEEP="$REPO_ROOT/scripts/run-offline-deep.sh"

echo "=== run-offline-deep.sh wrapper tests (task 4916/A5) ==="

# ---------------------------------------------------------------------------
# Test 1: file exists, is executable, shebang + strict mode
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: scripts/run-offline-deep.sh exists, executable, shebang, strict mode ---"

assert "scripts/run-offline-deep.sh exists" \
    test -f "$RUN_OFFLINE_DEEP"

assert "scripts/run-offline-deep.sh is executable" \
    test -x "$RUN_OFFLINE_DEEP"

assert "scripts/run-offline-deep.sh has '#!/usr/bin/env bash' shebang on line 1" \
    bash -c "head -n1 '$RUN_OFFLINE_DEEP' | grep -qE '^#!/usr/bin/env bash\$'"

assert "scripts/run-offline-deep.sh contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$RUN_OFFLINE_DEEP"

# ---------------------------------------------------------------------------
# Test 2: --print-plan is the hermetic PASS path — plan shape is the OFFLINE
# plan (verify.sh test --print-plan run under DF_VERIFY_ROLE=offline).
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 2: --print-plan yields the OFFLINE plan (role=offline, profile=release) ---"

# Capture rc via `|| PLAN_RC=$?` so a RED-phase missing wrapper (pre step-2,
# `bash <missing-file>` exits 127) reports clean assertion FAILs below
# instead of tripping this script's own set -e on the failing command
# substitution. Stderr discarded so PLAN_FULL is pure stdout.
PLAN_RC=0
PLAN_FULL="$(bash "$RUN_OFFLINE_DEEP" --print-plan 2>/dev/null)" || PLAN_RC=$?
PLAN_CMDS="$(printf '%s\n' "$PLAN_FULL" | grep -v '^#')"

assert "run-offline-deep.sh --print-plan exits 0" \
    test "$PLAN_RC" -eq 0

assert "--print-plan: header shows action=test" \
    bash -c 'printf "%s\n" "$1" | grep "^# verify.sh plan" | grep -q "action=test"' \
    _ "$PLAN_FULL"

assert "--print-plan: header shows role=offline" \
    bash -c 'printf "%s\n" "$1" | grep "^# verify.sh plan" | grep -q "role=offline"' \
    _ "$PLAN_FULL"

assert "--print-plan: header shows profile=release" \
    bash -c 'printf "%s\n" "$1" | grep "^# verify.sh plan" | grep -q "profile=release"' \
    _ "$PLAN_FULL"

assert "--print-plan: at least 1 cargo command line (sanity)" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -cE "(^| )cargo " || echo 0)" -ge 1 ]' \
    _ "$PLAN_CMDS"

assert "--print-plan: all cargo lines prefixed with 'nice -n 19 ionice -c3 cargo' (idle class)" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -vq "nice -n 19 ionice -c3 cargo"' \
    _ "$PLAN_CMDS"

assert "--print-plan: plan has NO 'export CARGO_MAKEFLAGS=' line (off the merge jobserver)" \
    bash -c '! printf "%s\n" "$1" | grep -q "export CARGO_MAKEFLAGS="' \
    _ "$PLAN_FULL"

# Positive heavy filter + --run-ignored all, guarded by nextest availability
# (mirrors test_verify_role_prio.sh Cycle E) — assert on a REAL atom sourced
# from the single source of truth so this fixture can never hand-drift from
# scripts/heavy-test-filter-lib.sh.
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

# nextest availability probe — guarded with `|| true` so an empty PLAN_FULL
# (RED phase) yields NEXTEST_AVAILABLE=0 instead of aborting this script
# under pipefail (grep finds no header line -> exit 1 in the pipe).
_HEADER="$(printf '%s\n' "$PLAN_FULL" | grep '^# verify.sh plan' || true)"
NEXTEST_AVAILABLE=0
case "$_HEADER" in
    *"nextest=1"*) NEXTEST_AVAILABLE=1 ;;
esac
echo "(nextest available on this host: $NEXTEST_AVAILABLE)"

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    echo ""
    echo "--- nextest available: expect positive heavy filter + --run-ignored all ---"

    assert "--print-plan: plan contains $POSITIVE_PATTERN (positive heavy filter)" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$PLAN_CMDS" "$POSITIVE_PATTERN"

    assert "--print-plan: plan contains a real heavy atom ($HEAVY_ATOM)" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$PLAN_CMDS" "$HEAVY_ATOM"

    assert "--print-plan: plan contains --run-ignored all" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "--run-ignored all"' \
        _ "$PLAN_CMDS"
else
    echo ""
    echo "--- positive assertions SKIPPED (nextest not available on this host) ---"
    echo "--- nextest unavailable: expect fallback cargo-test path NEVER emits $POSITIVE_PATTERN ---"

    assert "--print-plan, nextest unavailable: plan has NO $POSITIVE_PATTERN (cargo-test fallback has no -E support)" \
        bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$PLAN_CMDS" "$POSITIVE_PATTERN"
fi

# ---------------------------------------------------------------------------
# Test 3: exit-code propagation — the hermetic FAIL path. An unknown bogus
# arg trips verify.sh's own arg parser (exit 64) before any cargo work.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: exit-code propagation (--print-plan -> 0, bogus arg -> 64) ---"

BOGUS_RC=0
bash "$RUN_OFFLINE_DEEP" --bogus-zzz >/dev/null 2>&1 || BOGUS_RC=$?

assert "run-offline-deep.sh --bogus-zzz exits 64 (verify.sh's own arg-parse exit propagates unchanged)" \
    test "$BOGUS_RC" -eq 64

# ---------------------------------------------------------------------------
# Test 4: human-readable pass/fail outcome summary (the task's headline
# user-observable signal). Reports pass/fail via BOTH the exit code (Test 3
# above) AND a human-readable stderr line. Capture stdout/stderr into
# SEPARATE files on both paths so we can prove the summary lives on stderr
# only and never pollutes --print-plan's stdout. Grep with the distinctive
# 'offline deep-test lane' token so the assertion targets the WRAPPER's own
# line, never verify.sh's own diagnostic/usage output.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 4: human-readable pass/fail outcome summary (stderr only) ---"

OUTCOME_TOKEN="offline deep-test lane"

# --- FAIL path: bogus arg ---
_t4_fail_stdout="$(mktemp)"
_t4_fail_stderr="$(mktemp)"
bash "$RUN_OFFLINE_DEEP" --bogus-zzz >"$_t4_fail_stdout" 2>"$_t4_fail_stderr" || true
T4_FAIL_STDERR="$(cat "$_t4_fail_stderr")"
rm -f "$_t4_fail_stdout" "$_t4_fail_stderr"

assert "FAIL path: stderr contains the wrapper outcome line ('$OUTCOME_TOKEN')" \
    bash -c 'printf "%s\n" "$1" | grep -qiF -- "$2"' \
    _ "$T4_FAIL_STDERR" "$OUTCOME_TOKEN"

assert "FAIL path: wrapper outcome line carries a failure token (fail|exit)" \
    bash -c 'printf "%s\n" "$1" | grep -iF -- "$2" | grep -Eqi "fail|exit"' \
    _ "$T4_FAIL_STDERR" "$OUTCOME_TOKEN"

# --- PASS path: --print-plan ---
_t4_pass_stdout="$(mktemp)"
_t4_pass_stderr="$(mktemp)"
bash "$RUN_OFFLINE_DEEP" --print-plan >"$_t4_pass_stdout" 2>"$_t4_pass_stderr" || true
T4_PASS_STDOUT="$(cat "$_t4_pass_stdout")"
T4_PASS_STDERR="$(cat "$_t4_pass_stderr")"
rm -f "$_t4_pass_stdout" "$_t4_pass_stderr"

assert "PASS path: stderr contains the wrapper outcome line ('$OUTCOME_TOKEN')" \
    bash -c 'printf "%s\n" "$1" | grep -qiF -- "$2"' \
    _ "$T4_PASS_STDERR" "$OUTCOME_TOKEN"

assert "PASS path: wrapper outcome line carries a success token (pass|ok|success)" \
    bash -c 'printf "%s\n" "$1" | grep -iF -- "$2" | grep -Eqi "pass|ok|success"' \
    _ "$T4_PASS_STDERR" "$OUTCOME_TOKEN"

assert "PASS path: stdout still contains the plan header (outcome summary did not pollute stdout)" \
    bash -c 'printf "%s\n" "$1" | grep -qF "# verify.sh plan"' \
    _ "$T4_PASS_STDOUT"

test_summary
