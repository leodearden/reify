#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo_budget_skip.sh
#
# Meta-test for tasks #4624 / #5962: budget-safe PTODO skip path.
#
# Invokes tests/infra/test_reify_audit_ptodo.sh as a subprocess under a
# controlled environment (twice — see Design below) to pin TWO orthogonal
# properties of the budget-safe path — one about cost, one about honesty:
#
#   COST (#4624)    REIFY_AUDIT_NO_COLD_BUILD=1 must NOT invoke `cargo build`.
#                   The knob exists so a budget-constrained caller can run the
#                   infra suite without paying for a cold detector build.
#
#   HONESTY (#5962, esc-5405-7)
#                   With the binary ABSENT, that skip leaves EVERY scenario
#                   unexecuted.  Exiting 0 there would be a hard gate reporting
#                   green having asserted nothing, so the script must refuse —
#                   loudly, exit 1.  Declining to do the work is legitimate;
#                   declining to do the work and calling it a pass is not.
#
# The two are easy to conflate and must be proven together: satisfying COST by
# skipping is only sound while HONESTY stops the skip from being laundered into
# a pass, and satisfying HONESTY by rebuilding would violate COST.
#
# Design — two invocations sharing one fixture, differing only in the knob:
#   - A shim `cargo` on PATH writes a marker file when invoked.
#   - REIFY_AUDIT_BIN is set to a nonexistent path in BOTH, so the freshness
#     guard sees an ABSENT binary — the zero-scenario partition, not the
#     present-but-stale one.
#   - Invocation 1 sets REIFY_AUDIT_NO_COLD_BUILD=1, arming the budget-safe skip
#     (guard rc 75); invocation 2 unsets it, so the guard takes the rebuild path
#     and the shim's binary-less build leaves it still stale (guard rc 125).
#     Both no-silent-green floors are therefore exercised from one fixture.
#
# Assertions — first invocation (REIFY_AUDIT_NO_COLD_BUILD=1, guard rc 75):
#   (1) test_reify_audit_ptodo.sh exits 1 — an ABSENT binary is refused, not
#       laundered into a pass
#   (2) its combined output contains the specific refusal diagnostic, so an
#       unrelated nonzero exit cannot satisfy (1)
#   (3) its combined output contains the rc-75 branch's budget-safe skip message
#   (4) the shim cargo marker was NOT created (no cold build attempted)
#
# Together (1)+(2) prove the gate became LOUD, while (3)+(4) prove it did so
# WITHOUT reintroducing the cold build #4624 removed.
#
# Assertions — second invocation (knob UNSET, guard rc 125): the sibling floor,
# reached only via the REBUILD path, which the first invocation cannot exercise:
#   (5) exits 1
#   (6) its output carries the rc-125 branch's own absent-binary diagnostic
#   (7) the shim cargo marker IS present — cost expectation deliberately
#       inverted here, proving the rebuild path ran (rc 125, not rc 75)
#
# Only the ABSENT half of the rc-125 branch is refused; with the binary PRESENT
# it degrades to RATCHET_SKIP=1 so the (c)+(d)+(e) hard gate still runs.  See the
# PARTITION NOTE below.
#
# PARTITION NOTE: this covers only the ABSENT-binary case, under both guard
# rcs.  The PRESENT-but-stale case remains a graceful exit-0 skip under EITHER
# rc — scenarios (c)+(d)+(e) still run against the stale binary, so the run does
# assert something — and is covered by
# tests/infra/test_reify_audit_ptodo_orphan_hardgate.sh, which passes a stale
# COPY and still expects exit 0.  Do not "unify" the two expectations.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PTODO_TEST="$SCRIPT_DIR/test_reify_audit_ptodo.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== PTODO budget-safe skip meta-test (tasks #4624 / #5962) ==="

# Graceful skip when bash or the PTODO test script are absent.
if [ ! -f "$PTODO_TEST" ]; then
    echo "test_reify_audit_ptodo_budget_skip.sh: $PTODO_TEST not found — skipping" >&2
    exit 0
fi

# ---------------------------------------------------------------------------
# Setup: temp dir with shim cargo that writes a marker file when invoked.
# The shim exits 0 (like a real cargo) but never creates any binary — so the
# freshness guard will always re-see an absent binary after the shim runs.
# ---------------------------------------------------------------------------
BS_META_TMPDIR=$(mktemp -d /tmp/test-ptodo-budget-skip-XXXXXX)
trap 'rm -rf "$BS_META_TMPDIR"' EXIT

BS_MARKER="$BS_META_TMPDIR/cargo-was-invoked"
FAKE_BIN_PATH="$BS_META_TMPDIR/nonexistent-reify-audit-$$"

# Shim cargo: writes marker to prove invocation, then exits 0.
cat > "$BS_META_TMPDIR/cargo" <<EOF
#!/usr/bin/env bash
# Shim cargo for budget-safe skip meta-test — writes marker and exits 0.
touch '$BS_MARKER'
exit 0
EOF
chmod +x "$BS_META_TMPDIR/cargo"

# ---------------------------------------------------------------------------
# Invoke test_reify_audit_ptodo.sh under controlled env:
#   REIFY_AUDIT_BIN=<nonexistent>  — overrides the binary path (after impl)
#   REIFY_AUDIT_NO_COLD_BUILD=1    — arms the budget-safe skip
#   PATH=<shimdir>:$PATH           — shim cargo intercepts any cargo invocation
#
# Capture combined stdout+stderr for skip-message assertion.
# Use set +e so we can inspect the exit code independently.
# ---------------------------------------------------------------------------
echo ""
echo "--- Invoking test_reify_audit_ptodo.sh under budget-safe env ---"

BS_OUTPUT_FILE="$BS_META_TMPDIR/ptodo-output"
set +e
REIFY_AUDIT_BIN="$FAKE_BIN_PATH" \
REIFY_AUDIT_NO_COLD_BUILD=1 \
PATH="$BS_META_TMPDIR:$PATH" \
    bash "$PTODO_TEST" >"$BS_OUTPUT_FILE" 2>&1
BS_EXIT=$?
set -e

# ---------------------------------------------------------------------------
# Assertions
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertions ---"

# (1) Script must exit 1.  REIFY_AUDIT_BIN points at a nonexistent path, so the
#     freshness guard returns 75 and every scenario below its `-x` guards is
#     skipped — the run asserts NOTHING.  Exiting 0 there would let a hard gate
#     report green on zero executed assertions, which is exactly the false-green
#     that #4733's residual left open (esc-5405-7, closed by #5962).  Skipping
#     the work is fine; skipping it and claiming a pass is not.
assert "test_reify_audit_ptodo.sh exits 1 — a budget-safe skip of an ABSENT binary is a hard failure, not a graceful skip" \
    bash -c "[ '$BS_EXIT' -eq 1 ]"

# (2) ...and it must exit 1 for THAT reason.  A bare exit-code check is satisfied
#     by any unrelated failure — a `set -e` abort, a crash, a genuinely failing
#     scenario assertion, or a future regression that breaks the script early —
#     which would leave this meta-test green while the behaviour it exists to pin
#     silently disappeared.  Same false-green class as the bug being fixed.
#     Fixed-string match on the floor's own refusal diagnostic ties the assertion
#     to the specific code path, mirroring the rationale for (3) below.
assert "exit 1 came from the no-silent-green floor (refusal diagnostic present)" \
    bash -c "grep -qF 'refusing to report green for a hard gate that asserted nothing' '$BS_OUTPUT_FILE'"

# (3) Combined output must contain the specific budget-safe skip message emitted
#     by test_reify_audit_ptodo.sh's rc==75 branch.  Fixed-string match, and the
#     string must be unique to THAT branch: the trailing
#     "REIFY_AUDIT_NO_COLD_BUILD=1 — SKIP (budget-safe)" alone is emitted by the
#     GEN-absent branch too, so if the freshness guard ever failed open (rc 0 —
#     e.g. run from a non-git tree, the documented FAIL-OPEN POLICY) while
#     ptodo-baseline-gen was absent, that branch would satisfy the match, RAN
#     would stay 0, the floor would fire, and all four assertions would pass with
#     the rc-75 path never exercised — the same false-green class this file
#     argues against.  Anchoring on the "reify-audit binary absent/stale" prefix
#     pins it to the rc-75 branch only.
assert "output contains the rc-75 budget-safe skip message (reify-audit binary absent/stale ... SKIP (budget-safe))" \
    bash -c "grep -qF 'reify-audit binary absent/stale and REIFY_AUDIT_NO_COLD_BUILD=1 — SKIP (budget-safe)' '$BS_OUTPUT_FILE'"

# (4) Shim cargo must NOT have been invoked — marker file must be absent.
#     A present marker proves a cold build was attempted, violating the budget-safe contract.
assert "shim cargo NOT invoked — no cold build attempted (marker file absent)" \
    bash -c "[ ! -f '$BS_MARKER' ]"

# ---------------------------------------------------------------------------
# Second invocation: the rc-125 floor (#5962 review).
#
# The floor above has a sibling that the first invocation cannot reach: the
# `elif [ "$_guard_rc" -ne 0 ]` branch, which fires when the guard took the
# REBUILD path and the binary is still unusable afterwards.  Without coverage a
# future edit could drop or invert it unnoticed.
#
# Same shim cargo + nonexistent REIFY_AUDIT_BIN, but with
# REIFY_AUDIT_NO_COLD_BUILD explicitly UNSET (`env -u` — run_all.sh/verify.sh
# export it, so a bare invocation would inherit =1 and re-run the rc-75 case).
# The guard then rebuilds: the shim cargo exits 0 without producing a binary, so
# the re-check still sees an ABSENT binary and the guard returns 125.  No usable
# detector ⇒ every scenario is guarded out ⇒ the script must refuse loudly.
#
# NOTE the INVERTED cost expectation.  Here the rebuild path legitimately runs,
# so the shim marker IS expected; asserting its PRESENCE is what proves this run
# exercised the rc-125 branch rather than repeating the rc-75 skip.  The #4624
# no-cold-build contract is scoped to REIFY_AUDIT_NO_COLD_BUILD=1 and is
# unaffected.
# ---------------------------------------------------------------------------
echo ""
echo "--- Invoking test_reify_audit_ptodo.sh with the budget-safe knob UNSET ---"

BS_OUTPUT_FILE_125="$BS_META_TMPDIR/ptodo-output-125"
rm -f "$BS_MARKER"   # independent of assertion (4)'s ordering

set +e
env -u REIFY_AUDIT_NO_COLD_BUILD \
    REIFY_AUDIT_BIN="$FAKE_BIN_PATH" \
    PATH="$BS_META_TMPDIR:$PATH" \
    bash "$PTODO_TEST" >"$BS_OUTPUT_FILE_125" 2>&1
BS_EXIT_125=$?
set -e

# (5) No budget-safe skip was requested and no usable detector could be
#     produced, so the script must refuse rather than report green.
assert "guard rc 125 + ABSENT binary → test_reify_audit_ptodo.sh exits 1" \
    bash -c "[ '$BS_EXIT_125' -eq 1 ]"

# (6) ...and for THAT reason.  Fixed-string match on the rc-125 branch's own
#     absent-binary diagnostic — the same discrimination rationale as (2), and
#     deliberately NOT the shared "freshness guard failed (rc=" prefix, which the
#     present-but-stale sibling branch (RATCHET_SKIP=1, keeps running) also
#     emits.
assert "exit 1 came from the rc-125 floor's absent-binary branch (refusal diagnostic present)" \
    bash -c "grep -qF 'the detector could not be made usable and no budget-safe skip was requested' '$BS_OUTPUT_FILE_125'"

# (7) Cost expectation INVERTED for this run — see the block comment above.
#     A present marker proves the guard took the rebuild path (rc 125), not the
#     budget-safe skip (rc 75), so (5)+(6) really did pin the other floor.
assert "shim cargo WAS invoked — the rebuild path ran, so this was rc 125 not rc 75 (marker file present)" \
    bash -c "[ -f '$BS_MARKER' ]"

test_summary
