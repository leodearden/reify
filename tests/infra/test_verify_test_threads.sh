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

test_summary
