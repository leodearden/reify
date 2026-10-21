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
# Only the ABSENT half of the rc-125 branch is refused unconditionally; with the
# binary PRESENT it degrades to RATCHET_SKIP=1 so the (c)+(d)+(e) hard gate still
# runs.  That degradation is what the third and fourth invocations bound.
#
# Assertions — third and fourth invocations (#7006): guard rc 125 with the binary
# PRESENT, the cell this file's partition previously left uncovered.  The skip is
# sound for the (c)-(g) hard gate but silently drops the fingerprint ratchet, and
# that gate is High-severity only while phantom-tracking is MEDIUM — so on the
# hook-gated --scope staged main-landing path the run can exit green with the
# ratchet never having run.  REIFY_PTODO_RATCHET_REQUIRED=1 is the caller's
# declaration that the ratchet is not optional there:
#   (8)  with the knob armed, test_reify_audit_ptodo.sh exits 1
#   (9)  its output carries the ratchet-required refusal diagnostic, so a stub
#        detector failing (c)-(g) on its own merits cannot satisfy (8)
#   (10) with the knob UNSET, the rc-125 PRESENT-binary branch still fires and
#        still degrades to a skip — non-vacuity for (9) and (11)
#   (11) ...and emits no refusal, so the knob is genuinely opt-in
#
# PARTITION NOTE: the ABSENT-binary case is covered here under both guard rcs,
# and so is the PRESENT-but-stale case under the knob.  What is NOT covered here
# is the PRESENT-but-stale DEFAULT (knob unset), which remains a graceful exit-0
# skip under EITHER rc — scenarios (c)+(d)+(e) still run against the stale binary,
# so the run does assert something.  That cell is
# tests/infra/test_reify_audit_ptodo_orphan_hardgate.sh's, which passes a stale
# COPY and still expects exit 0; (10)+(11) pin only the diagnostics, never an exit
# code, precisely so the two files do not race over one contract.  Do not "unify"
# the expectations.
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

# ---------------------------------------------------------------------------
# Third and fourth invocations: guard rc 125 with the binary PRESENT — the
# partition cell the PARTITION NOTE above records as uncovered here.
#
# On the hook-gated `--scope staged` main-landing path REIFY_AUDIT_NO_COLD_BUILD
# is deliberately unset, so the guard degrades to mode=rebuild.  When that
# rebuild is a legitimate no-op — cargo's fingerprint says up-to-date — against
# an on-disk mtime still older than the last crates/reify-audit commit (a
# warm-lane seeded target/ with stamped mtimes), the guard returns 125 while the
# binary stays executable, and test_reify_audit_ptodo.sh sets RATCHET_SKIP=1.
# The fingerprint ratchet ((a)+(b)) then never runs while the (c)-(g) hard gate,
# which is High-severity only, still exits green — so a MEDIUM phantom-tracking
# marker can ride a main landing past a green gate.  REIFY_PTODO_RATCHET_REQUIRED=1
# is the caller's declaration that the ratchet is not optional on this path.
#
# Fixture: a two-line executable stub with a year-2000 mtime.  reify_audit_is_stale
# only STATS the binary (portable_mtime plus a `-f` presence check,
# scripts/reify-audit-freshness.sh:190-233) and never executes it, so the stub
# reproduces present-but-stale exactly — no real detector, no sqlite3, no real
# cargo.  The enforcement point under test fires before any fixture is minted or
# scenario runs, so the stub's uselessness as a detector costs nothing.
# ---------------------------------------------------------------------------
echo ""
echo "--- Invoking test_reify_audit_ptodo.sh with a PRESENT-but-stale binary, ratchet REQUIRED ---"

BS_STALE_BIN="$BS_META_TMPDIR/stale-reify-audit"
cat > "$BS_STALE_BIN" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$BS_STALE_BIN"
touch -t 200001010000 "$BS_STALE_BIN"

BS_OUTPUT_REQ="$BS_META_TMPDIR/ptodo-output-ratchet-required"
set +e
env -u REIFY_AUDIT_NO_COLD_BUILD \
    REIFY_AUDIT_BIN="$BS_STALE_BIN" \
    REIFY_PTODO_RATCHET_REQUIRED=1 \
    PATH="$BS_META_TMPDIR:$PATH" \
    bash "$PTODO_TEST" >"$BS_OUTPUT_REQ" 2>&1
BS_EXIT_REQ=$?
set -e

# (8) A caller that declared the ratchet REQUIRED must not get a green run in
#     which the ratchet was skipped.
assert "guard rc 125 + PRESENT binary + REIFY_PTODO_RATCHET_REQUIRED=1 → test_reify_audit_ptodo.sh exits 1" \
    bash -c "[ '$BS_EXIT_REQ' -eq 1 ]"

# (9) ...and for THAT reason — the same discrimination rationale as (2) and (6),
#     and load-bearing here rather than merely prudent: this invocation points the
#     hard gate at a stub that is not a detector, so several (c)-(g) scenarios fail
#     on their own merits and would satisfy (8) by themselves.  Only the fixed-string
#     match ties the exit code to the ratchet-required refusal.
assert "exit 1 came from the ratchet-required refusal (diagnostic present)" \
    bash -c "grep -qF 'REIFY_PTODO_RATCHET_REQUIRED=1 — the caller declared the fingerprint ratchet ((a)+(b)) REQUIRED on this path, but it was skipped' '$BS_OUTPUT_REQ'"

# (9b) ...and the refusal's REMEDY is the one that fits THIS rc.  rc 125 comes out
#      of the rebuild path: reify_audit_guard has already run `cargo build
#      --release -p reify-audit` and the binary is still judged stale, so the
#      generic "build a fresh detector" advice is the command that just ran — an
#      operator who follows it gets a no-op and a second identical refusal while
#      every `git commit` on main stays blocked.  The rc-125 arm names the mtime
#      as the thing that failed the check; this pin is what stops the two arms
#      collapsing back into one generic sentence.
assert "the rc-125 refusal names the MTIME as the cause, not a rebuild that already ran" \
    bash -c "grep -qF 'STILL judged stale, so its mtime' '$BS_OUTPUT_REQ'"

# ---------------------------------------------------------------------------
# Fourth invocation: the opt-in control.  Identical env with the knob removed
# via `env -u`, proving the refusal is the KNOB's doing and not this fixture's.
#
# Deliberately asserts NOTHING about the exit status.  A stub is not a detector,
# so scenarios (c)-(g) run and fail on their own merits; the exit code carries no
# information about the knob either way.  The default-off exit-0 contract for a
# REAL present-but-stale binary is already fenced by
# tests/infra/test_reify_audit_ptodo_orphan_hardgate.sh, which passes a stale COPY
# and still expects exit 0 — that file must stay green, and is not restated here.
# ---------------------------------------------------------------------------
echo ""
echo "--- Invoking test_reify_audit_ptodo.sh with a PRESENT-but-stale binary, knob UNSET ---"

BS_OUTPUT_OPTIN="$BS_META_TMPDIR/ptodo-output-ratchet-optional"
set +e
env -u REIFY_AUDIT_NO_COLD_BUILD -u REIFY_PTODO_RATCHET_REQUIRED \
    REIFY_AUDIT_BIN="$BS_STALE_BIN" \
    PATH="$BS_META_TMPDIR:$PATH" \
    bash "$PTODO_TEST" >"$BS_OUTPUT_OPTIN" 2>&1
set -e

# (10) Non-vacuity for (9) and (11): this env really does reach the
#      rc-125-with-present-binary branch, rather than some earlier exit that would
#      make the knob's absence trivially undetectable.  Fixed-string match on that
#      branch's own message, and deliberately NOT the shared "freshness guard
#      failed (rc=" prefix, which the ABSENT sibling branch also emits.
assert "knob UNSET → the rc-125 PRESENT-binary branch still fires (degrades to a skip)" \
    bash -c "grep -qF 'skipping the precision-sensitive ratchet' '$BS_OUTPUT_OPTIN'"

# (11) ...and the refusal is absent, so the knob is genuinely opt-in.  Without
#      this, (9) would be satisfied by an unconditional refusal that broke every
#      existing caller.
assert "knob UNSET → no ratchet-required refusal (default-off)" \
    bash -c "! grep -qF 'REIFY_PTODO_RATCHET_REQUIRED=1 — the caller declared the fingerprint ratchet ((a)+(b)) REQUIRED on this path, but it was skipped' '$BS_OUTPUT_OPTIN'"

test_summary
