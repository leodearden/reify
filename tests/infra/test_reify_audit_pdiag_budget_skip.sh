#!/usr/bin/env bash
# tests/infra/test_reify_audit_pdiag_budget_skip.sh
#
# Meta-test for task #5964 (parity follow-up to #5962): pins the PDIAG side of
# the NO-SILENT-GREEN FLOOR that commit 07f36c5a73 (task/5405, esc-5405-7)
# added to tests/infra/test_reify_audit_pdiag.sh.  Modelled directly on
# tests/infra/test_reify_audit_ptodo_budget_skip.sh's first two invocations —
# PDIAG rides the same shared scripts/reify-audit-freshness.sh guard as
# PTODO, so the rc 75 / rc 125 split and both refusal diagnostics carry over
# verbatim.  PDIAG has no analogue of PTODO's later RATCHET_REQUIRED knob (its
# only precision-sensitive scenario is (a), gated solely by RATCHET_SKIP), so
# this file stops after the two invocations that pin the floor itself.
#
# Invokes tests/infra/test_reify_audit_pdiag.sh as a subprocess (twice — see
# Design below) to pin TWO orthogonal properties of the budget-safe path:
#
#   COST      REIFY_AUDIT_NO_COLD_BUILD=1 must NOT invoke `cargo build`.
#
#   HONESTY (esc-5405-7)
#             With the binary ABSENT, every scenario is guarded out and NOTHING
#             executes.  Exiting 0 there would be a hard gate reporting green
#             having asserted nothing, so the script must refuse — loudly,
#             exit 1.  Declining to do the work is legitimate; declining the
#             work and calling it a pass is not.
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
#   (1) test_reify_audit_pdiag.sh exits 1 — an ABSENT binary is refused, not
#       laundered into a pass
#   (2) its combined output contains the specific refusal diagnostic, so an
#       unrelated nonzero exit cannot satisfy (1)
#   (3) its combined output contains the rc-75 branch's budget-safe skip message
#   (4) the shim cargo marker was NOT created (no cold build attempted)
#
# Together (1)+(2) prove the gate became LOUD, while (3)+(4) prove it did so
# WITHOUT reintroducing a cold build.
#
# Assertions — second invocation (knob UNSET, guard rc 125): the sibling floor,
# reached only via the REBUILD path, which the first invocation cannot exercise:
#   (5) exits 1
#   (6) its output carries the rc-125 branch's own absent-binary diagnostic
#   (7) the shim cargo marker IS present — cost expectation deliberately
#       inverted here, proving the rebuild path ran (rc 125, not rc 75)
#
# PARTITION NOTE: this file covers the ABSENT-binary case under both guard
# rcs, matching the two invocations of test_reify_audit_ptodo_budget_skip.sh
# it is modelled on.  It does NOT attempt PTODO's later RATCHET_REQUIRED
# invocations (#7006) — PDIAG has no such knob, since scenarios (b)+(c) are
# staleness-stable and always run whenever the binary is PRESENT, independent
# of RATCHET_SKIP; only scenario (a) is gated, and it has no "required" mode.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PDIAG_TEST="$SCRIPT_DIR/test_reify_audit_pdiag.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== PDIAG budget-safe skip meta-test (task #5964) ==="

# Graceful skip when the PDIAG test script is absent.
if [ ! -f "$PDIAG_TEST" ]; then
    echo "test_reify_audit_pdiag_budget_skip.sh: $PDIAG_TEST not found — skipping" >&2
    exit 0
fi

# ---------------------------------------------------------------------------
# Setup: temp dir with shim cargo that writes a marker file when invoked.
# The shim exits 0 (like a real cargo) but never creates any binary — so the
# freshness guard will always re-see an absent binary after the shim runs.
# ---------------------------------------------------------------------------
BS_META_TMPDIR=$(mktemp -d /tmp/test-pdiag-budget-skip-XXXXXX)
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
# Invoke test_reify_audit_pdiag.sh under controlled env:
#   REIFY_AUDIT_BIN=<nonexistent>  — overrides the binary path
#   REIFY_AUDIT_NO_COLD_BUILD=1    — arms the budget-safe skip
#   PATH=<shimdir>:$PATH           — shim cargo intercepts any cargo invocation
#
# Capture combined stdout+stderr for skip-message assertion.
# Use set +e so we can inspect the exit code independently.
# ---------------------------------------------------------------------------
echo ""
echo "--- Invoking test_reify_audit_pdiag.sh under budget-safe env ---"

BS_OUTPUT_FILE="$BS_META_TMPDIR/pdiag-output"
set +e
REIFY_AUDIT_BIN="$FAKE_BIN_PATH" \
REIFY_AUDIT_NO_COLD_BUILD=1 \
PATH="$BS_META_TMPDIR:$PATH" \
    bash "$PDIAG_TEST" >"$BS_OUTPUT_FILE" 2>&1
BS_EXIT=$?
set -e

# ---------------------------------------------------------------------------
# Assertions
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertions ---"

# (1) Script must exit 1.  REIFY_AUDIT_BIN points at a nonexistent path, so the
#     freshness guard returns 75 and (b)+(c)'s `-x` guard skips them too — the
#     run asserts NOTHING.  Exiting 0 there would let a hard gate report green
#     on zero executed assertions, exactly the false-green esc-5405-7 closed.
#     Skipping the work is fine; skipping it and claiming a pass is not.
assert "test_reify_audit_pdiag.sh exits 1 — a budget-safe skip of an ABSENT binary is a hard failure, not a graceful skip" \
    bash -c "[ '$BS_EXIT' -eq 1 ]"

# (2) ...and it must exit 1 for THAT reason.  A bare exit-code check is
#     satisfied by any unrelated failure — a `set -e` abort, a crash, or a
#     future regression that breaks the script early — which would leave this
#     meta-test green while the behaviour it exists to pin silently disappeared.
#     Fixed-string match on the floor's own refusal diagnostic ties the
#     assertion to the specific code path.
assert "exit 1 came from the no-silent-green floor (refusal diagnostic present)" \
    bash -c "grep -qF 'refusing to report green for a hard gate that asserted nothing' '$BS_OUTPUT_FILE'"

# (3) Combined output must contain the specific budget-safe skip message
#     emitted by test_reify_audit_pdiag.sh's rc==75 branch.  Fixed-string
#     match anchored on the "reify-audit binary absent/stale" prefix, pinning
#     it to the rc-75 branch specifically.
assert "output contains the rc-75 budget-safe skip message (reify-audit binary absent/stale ... SKIP (budget-safe))" \
    bash -c "grep -qF 'reify-audit binary absent/stale and REIFY_AUDIT_NO_COLD_BUILD=1 — (a) SKIP (budget-safe)' '$BS_OUTPUT_FILE'"

# (4) Shim cargo must NOT have been invoked — marker file must be absent.
#     A present marker proves a cold build was attempted, violating the
#     budget-safe contract.
assert "shim cargo NOT invoked — no cold build attempted (marker file absent)" \
    bash -c "[ ! -f '$BS_MARKER' ]"

# ---------------------------------------------------------------------------
# Second invocation: the rc-125 floor.
#
# The floor above has a sibling that the first invocation cannot reach: the
# `elif [ "$_guard_rc" -ne 0 ]` branch, which fires when the guard took the
# REBUILD path and the binary is still unusable afterwards.  Without coverage
# a future edit could drop or invert it unnoticed.
#
# Same shim cargo + nonexistent REIFY_AUDIT_BIN, but with
# REIFY_AUDIT_NO_COLD_BUILD explicitly UNSET (`env -u` — run_all.sh/verify.sh
# export it, so a bare invocation would inherit =1 and re-run the rc-75 case).
# The guard then rebuilds: the shim cargo exits 0 without producing a binary,
# so the re-check still sees an ABSENT binary and the guard returns 125.  No
# usable detector ⇒ every scenario is guarded out ⇒ the script must refuse
# loudly.
#
# NOTE the INVERTED cost expectation.  Here the rebuild path legitimately
# runs, so the shim marker IS expected; asserting its PRESENCE is what proves
# this run exercised the rc-125 branch rather than repeating the rc-75 skip.
# ---------------------------------------------------------------------------
echo ""
echo "--- Invoking test_reify_audit_pdiag.sh with the budget-safe knob UNSET ---"

BS_OUTPUT_FILE_125="$BS_META_TMPDIR/pdiag-output-125"
rm -f "$BS_MARKER"   # independent of assertion (4)'s ordering

set +e
env -u REIFY_AUDIT_NO_COLD_BUILD \
    REIFY_AUDIT_BIN="$FAKE_BIN_PATH" \
    PATH="$BS_META_TMPDIR:$PATH" \
    bash "$PDIAG_TEST" >"$BS_OUTPUT_FILE_125" 2>&1
BS_EXIT_125=$?
set -e

# (5) No budget-safe skip was requested and no usable detector could be
#     produced, so the script must refuse rather than report green.
assert "guard rc 125 + ABSENT binary → test_reify_audit_pdiag.sh exits 1" \
    bash -c "[ '$BS_EXIT_125' -eq 1 ]"

# (6) ...and for THAT reason.  Fixed-string match on the rc-125 branch's own
#     absent-binary diagnostic — the same discrimination rationale as (2), and
#     deliberately NOT the shared "freshness guard failed (rc=" prefix, which
#     the present-but-stale sibling branch (RATCHET_SKIP=1, keeps (b)+(c)
#     running) also emits.
assert "exit 1 came from the rc-125 floor's absent-binary branch (refusal diagnostic present)" \
    bash -c "grep -qF 'the detector could not be made usable and no budget-safe skip was requested' '$BS_OUTPUT_FILE_125'"

# (7) Cost expectation INVERTED for this run — see the block comment above.
#     A present marker proves the guard took the rebuild path (rc 125), not
#     the budget-safe skip (rc 75), so (5)+(6) really did pin the other floor.
assert "shim cargo WAS invoked — the rebuild path ran, so this was rc 125 not rc 75 (marker file present)" \
    bash -c "[ -f '$BS_MARKER' ]"

test_summary
