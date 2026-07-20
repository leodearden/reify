#!/usr/bin/env bash
# Infrastructure drift-guard for task 5264 — scripts/verify.sh must accept the
# --test-threads=N offline parallelism cap.
#
# Background: the dark-factory offline-deep-test-lane worker (β3) invokes
#   scripts/run-offline-deep.sh --test-threads=N …
# which forwards its args verbatim to `verify.sh test --test-threads=N`
# (run-offline-deep.sh delegates `"$SCRIPT_DIR/verify.sh" test "$@"`). Before
# this task verify.sh's arg parser had no --test-threads case, so the flag hit
# the `*)` catch-all, verify.sh exited 64 ("unknown argument '--test-threads=1'")
# BEFORE any cargo work, and the offline lane went red.
#
# This test locks in the primitive (CLAUDE.md seam: "reify ships the primitive,
# dark-factory wires the invocation"). verify.sh must:
#   - accept --test-threads=N in both the '=N' and space-separated forms,
#   - validate N as a positive integer (reject 0/negative/non-numeric/float),
#   - thread N into the emitted cargo nextest / cargo test plan, while leaving
#     the no-flag DEFAULT plan byte-for-byte unchanged,
#   - document --test-threads in `--help`.
#
# Hermetic: drives ONLY `verify.sh --print-plan` and `run-offline-deep.sh
# --print-plan` (verify.sh builds the plan and exits 0 — no cargo build, no
# tests executed). Nextest-vs-fallback command-shape assertions are guarded on
# a NEXTEST_AVAILABLE probe of the plan header's `nextest=` token, the sibling
# idiom from test_run_offline_deep.sh / test_verify_offline_partition.sh; the
# host-independent invariants (accept/exit-0, validation/exit-64,
# default-has-no-flag) are asserted unconditionally.
#
# Mirrors:
#   - tests/infra/test_run_offline_deep.sh — --print-plan oracle + NEXTEST
#     availability probe idiom, wrapper-drift structure.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY="$REPO_ROOT/scripts/verify.sh"
RUN_OFFLINE_DEEP="$REPO_ROOT/scripts/run-offline-deep.sh"

echo "=== verify.sh --test-threads=N tests (task 5264) ==="

# ---------------------------------------------------------------------------
# Test 1: ACCEPTANCE — verify.sh accepts --test-threads=N in both the '=N' and
# the space-separated forms and still exits 0 (--print-plan is hermetic). rc is
# captured via `|| rc=$?` so a RED-phase exit-64 reports a clean assertion FAIL
# here instead of tripping this script's own `set -e`.
# RED (base): verify.sh has no --test-threads case, so both forms hit the `*)`
# catch-all and exit 64 ("unknown argument …").
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: --test-threads=N is accepted (exit 0) — both '=N' and space forms ---"

EQ_RC=0
bash "$VERIFY" test --scope all --print-plan --test-threads=4 >/dev/null 2>&1 || EQ_RC=$?
assert "verify.sh test --print-plan --test-threads=4 ('=N' form) exits 0" \
    test "$EQ_RC" -eq 0

SPACE_RC=0
bash "$VERIFY" test --scope all --print-plan --test-threads 4 >/dev/null 2>&1 || SPACE_RC=$?
assert "verify.sh test --print-plan --test-threads 4 (space form) exits 0" \
    test "$SPACE_RC" -eq 0

# ---------------------------------------------------------------------------
# Test 2: THREADING + DEFAULT-INVARIANT. --test-threads=N must reach the
# emitted cargo command; the no-flag default plan must stay byte-identical to
# today. The command shape depends on whether nextest is installed on this
# host, so those assertions are guarded on a NEXTEST_AVAILABLE probe of the
# plan header's `nextest=` token (sibling idiom, test_run_offline_deep.sh).
# RED (step-2 only parses): emit_nextest_pass does not yet thread the value,
# so the '=4 reaches the plan' assertion fails; the default-invariant
# assertion is already green (it guards step-4 against regressing the default).
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 2: --test-threads=N threads into the plan; default plan unchanged ---"

PLAN_TT4="$(bash "$VERIFY" test --scope all --print-plan --test-threads=4 2>/dev/null)" || true
PLAN_DEFAULT="$(bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true

# nextest availability probe — `|| true` so an empty plan (RED) yields
# NEXTEST_AVAILABLE=0 rather than aborting under pipefail.
_HEADER="$(printf '%s\n' "$PLAN_DEFAULT" | grep '^# verify.sh plan' || true)"
NEXTEST_AVAILABLE=0
case "$_HEADER" in
    *"nextest=1"*) NEXTEST_AVAILABLE=1 ;;
esac
echo "(nextest available on this host: $NEXTEST_AVAILABLE)"

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "nextest: --test-threads=4 threads into the cargo nextest run line" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "--test-threads=4"' \
        _ "$PLAN_TT4"

    assert "nextest: DEFAULT plan (no flag) has NO --test-threads= token on any cargo line (byte-identical default)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -qF -- "--test-threads="' \
        _ "$PLAN_DEFAULT"
else
    assert "fallback: --test-threads=4 threads into the cargo test line" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo test " | grep -qF -- "--test-threads=4"' \
        _ "$PLAN_TT4"

    assert "fallback: --test-threads=4 replaces the default (no residual --test-threads=1 on the cargo test line)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo test " | grep -qF -- "--test-threads=1"' \
        _ "$PLAN_TT4"

    assert "fallback: DEFAULT plan (no flag) still carries -- --test-threads=1 (byte-identical default)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo test " | grep -qF -- "-- --test-threads=1"' \
        _ "$PLAN_DEFAULT"
fi

# ---------------------------------------------------------------------------
# Test 3: E2E — faithful β3 repro through the run-offline-deep.sh wrapper,
# which forwards args verbatim to `verify.sh test`. The forwarded
# --test-threads=2 must reach the offline plan's cargo line. Host-independent
# (nextest -> `cargo nextest run … --test-threads=2 …`; fallback ->
# `cargo test … -- --test-threads=2`), so asserted unconditionally.
# RED (step-2 only parses): the value is accepted (exit 0) but not threaded,
# so the 'carries --test-threads=2' assertion fails.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: E2E run-offline-deep.sh --test-threads=2 --print-plan (β3 repro) ---"

E2E_RC=0
PLAN_E2E="$(bash "$RUN_OFFLINE_DEEP" --test-threads=2 --print-plan 2>/dev/null)" || E2E_RC=$?

assert "run-offline-deep.sh --test-threads=2 --print-plan exits 0" \
    test "$E2E_RC" -eq 0

assert "run-offline-deep.sh --test-threads=2 plan carries --test-threads=2 on a cargo line" \
    bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -qF -- "--test-threads=2"' \
    _ "$PLAN_E2E"

# ---------------------------------------------------------------------------
# Test 4: VALIDATION — N must be a positive integer. Reject zero, leading-zero
# forms ('00'/'007' — cargo/nextest parse '00' as 0 and reject it only at
# runtime, so reject at parse time; net validated set is exactly ^[1-9][0-9]*$),
# negative, non-numeric, float, and the explicit empty-value form '--test-threads=' with
# exit 64 (the same invalid-value convention as --profile / --scope). The
# explicit empty value ('--test-threads=') is an ERROR distinct from an UNSET
# flag (no --test-threads at all, which stays exit 0 / default plan — asserted
# in Test 2); telling them apart requires a "flag-was-seen" sentinel, since
# both leave TEST_THREADS empty after parsing. The bare no-value '--test-threads'
# form is deliberately NOT asserted here: it exits 1 via bash ${2:?}, the same
# as the bare '--profile' / '--scope' forms.
# RED (step-4): values are stored/threaded without validation, so they exit 0.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 4: invalid --test-threads values exit 64 (parse-time validation) ---"

for _v in 0 00 007 -1 abc 2.5 ""; do
    _rc=0
    bash "$VERIFY" test --scope all --print-plan --test-threads="$_v" >/dev/null 2>&1 || _rc=$?
    assert "invalid --test-threads='$_v' exits 64 (want positive integer)" \
        test "$_rc" -eq 64
done

# ---------------------------------------------------------------------------
# Test 5: CLI-surface existence — the flag must be documented in usage().
# Existence grep only (one token, not a prose pin), on STDOUT (the -h|--help
# path prints usage to stdout and exits 0). RED (base usage() header does not
# yet mention --test-threads).
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 5: --help / -h document the --test-threads flag ---"

assert "verify.sh --help stdout documents the --test-threads flag" \
    bash -c 'bash "$1" --help 2>/dev/null | grep -qF -- "--test-threads"' \
    _ "$VERIFY"

assert "verify.sh -h stdout documents the --test-threads flag" \
    bash -c 'bash "$1" -h 2>/dev/null | grep -qF -- "--test-threads"' \
    _ "$VERIFY"

test_summary
