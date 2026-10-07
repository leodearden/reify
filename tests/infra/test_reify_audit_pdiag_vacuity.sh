#!/usr/bin/env bash
# tests/infra/test_reify_audit_pdiag_vacuity.sh
#
# Meta-test for the three @@PDIAG_HARDGATE_*_PASSED@@ sentinels in
# tests/infra/test_reify_audit_pdiag.sh (task #5405).
#
# Those sentinels are emitted only when the corresponding scenario's asserts
# all passed, and the comment at their first emission site says they exist so
# "a meta-test grepping for the token stays RED rather than silently passing".
# Until this file there was no such meta-test: the sentinels were inert echoes
# and the comment documented a guard that did not exist.  A sentinel nothing
# reads cannot keep a gate honest, and the claim that it does is worse than
# silence — it is exactly what a future maintainer trusts without checking.
# The sibling PTODO gate has real vacuity meta-tests
# (test_reify_audit_ptodo_ratchet_vacuity.sh and its two siblings); this is the
# PDIAG analogue.
#
# WHAT IS PINNED.  The PDIAG gate's header declares a three-way partition on
# detector USABILITY, and each leg is asserted here by one invocation:
#
#   (1) FRESH binary    — nothing is skipped: scenarios (a), (b) and (c) all
#                         run and pass, so ALL THREE sentinels appear and the
#                         run is green.
#   (2) STALE binary    — the budget-safe skip covers ONLY the precision-
#                         sensitive ratchet: sentinel A is ABSENT while B and C
#                         are PRESENT, and the run is still green.  This is the
#                         leg that makes the file discriminating — a regression
#                         letting RATCHET_SKIP take the hard gate down with it
#                         still passes (1) and fails here, and one running the
#                         ratchet regardless of RATCHET_SKIP shows sentinel A.
#   (3) ABSENT binary   — no scenario can execute, so the NO-SILENT-GREEN floor
#                         must fire: no sentinel at all, "0 passed, 0 failed",
#                         and exit 1.  Reporting green from a hard gate that
#                         asserted nothing is the precise failure that let the
#                         PTODO hard gate be silently bypassed.
#
# DISCRIMINATION.  Every leg pairs its exit-code check with a token check, for
# the reason test_reify_audit_ptodo_ratchet_vacuity.sh states at its assertion
# (2): a bare exit-code check is satisfied by any unrelated failure, and would
# leave this file green after the behaviour it pins had disappeared.  Leg (3)
# pairs exit 1 with "Results: 0 passed, 0 failed" so the RED is attributable to
# the floor rather than to some assert going red on its own.
#
# NOT A SECOND SOURCE OF TRUTH.  No count, fixture or baseline is re-derived
# here; this file only reads the gate's own machine tokens and exit code.  If
# the live tree drifts out of the committed pdiag-baseline.txt, leg (1) goes
# RED *downstream of* test_reify_audit_pdiag.sh going RED — same cause, two
# reports, no new claim.
#
# COST.  Three invocations of a ~2s script.  Each supplies REIFY_AUDIT_BIN
# explicitly and arms REIFY_AUDIT_NO_COLD_BUILD=1, so reify_audit_guard never
# reaches its `cargo build` path (a fresh binary short-circuits at the top of
# reify_audit_guard; a stale or absent one returns 75 under that knob).  That
# is what keeps this file `pool` in run-all-classification.manifest while the
# gate it drives is intra-run-serial: the shared CoW target/ is never mutated.
#
# SELF-MATCH SAFETY: like the gate it drives, this file carries no literal
# swept anchor and no literal reviewed-opt-out token.  It needs neither — it
# asserts on tokens and exit codes, never on fixture content.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PDIAG_TEST="$SCRIPT_DIR/test_reify_audit_pdiag.sh"
REAL_BIN="$REPO_ROOT/target/release/reify-audit"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== PDIAG hard-gate sentinel wiring meta-test (task #5405) ==="

# Graceful skip when the gate under test is absent.
if [ ! -f "$PDIAG_TEST" ]; then
    echo "test_reify_audit_pdiag_vacuity.sh: $PDIAG_TEST not found — skipping" >&2
    exit 0
fi

# Graceful skip when the real binary is absent: legs (1) and (2) copy it, and
# without one there is no way to reach a state where the sentinels can appear.
if [ ! -x "$REAL_BIN" ]; then
    echo "test_reify_audit_pdiag_vacuity.sh: $REAL_BIN absent — skipping" >&2
    exit 0
fi

# Mirror the gate's own tool set exactly: it exits 0 before any scenario when
# one of these is missing, and asserting against that vacuous run would be a
# spurious RED here.
for _tool in git cargo; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "test_reify_audit_pdiag_vacuity.sh: $_tool not on PATH — skipping" >&2
        exit 0
    fi
done

PVM_TMPDIR=$(mktemp -d /tmp/test-pdiag-vacuity-XXXXXX)
trap 'rm -rf "$PVM_TMPDIR"' EXIT

# ---------------------------------------------------------------------------
# The two binaries, differing ONLY in mtime.  reify_audit_is_stale compares the
# binary's mtime against the last crates/reify-audit commit epoch, so the copy
# `cp` stamps at NOW is judged FRESH and the backdated one STALE while both
# remain byte-identical and fully executable.  Holding content constant is what
# makes leg (2)'s partition attributable to the freshness verdict alone.
# ---------------------------------------------------------------------------
FRESH_BIN="$PVM_TMPDIR/reify-audit-fresh"
STALE_BIN="$PVM_TMPDIR/reify-audit-stale"
cp "$REAL_BIN" "$FRESH_BIN"
cp "$REAL_BIN" "$STALE_BIN"
touch "$FRESH_BIN"
touch -t 200001010000 "$STALE_BIN"
ABSENT_BIN="$PVM_TMPDIR/reify-audit-absent"   # deliberately never created

# ---------------------------------------------------------------------------
# Drive the gate once, capturing combined stdout+stderr and the exit code.
# `set +e`/`set -e` around the call rather than `|| rc=$?` so the rc of a
# SIGNALLED run is captured too.
# ---------------------------------------------------------------------------
GATE_RC=0
_run_gate() {
    local _bin="$1" _out="$2"
    set +e
    env REIFY_AUDIT_BIN="$_bin" REIFY_AUDIT_NO_COLD_BUILD=1 \
        bash "$PDIAG_TEST" >"$_out" 2>&1
    GATE_RC=$?
    set -e
    echo "  test_reify_audit_pdiag.sh exited: $GATE_RC"
}

# On-FAIL actionability (the 4636 lesson): replay the gate's own tail into the
# failing assert's captured-output dump.  Silent and rc 0 on the passing path.
_has_token() {
    local _out="$1" _token="$2"
    grep -qF "$_token" "$_out" && return 0
    echo "expected token $_token in the gate's output; tail was:" >&2
    tail -25 "$_out" >&2
    return 1
}

_lacks_token() {
    local _out="$1" _token="$2"
    grep -qF "$_token" "$_out" || return 0
    echo "token $_token must NOT appear; tail was:" >&2
    tail -25 "$_out" >&2
    return 1
}

_matches() {
    local _out="$1" _re="$2"
    grep -qE "$_re" "$_out" && return 0
    echo "expected /$_re/ in the gate's output; tail was:" >&2
    tail -25 "$_out" >&2
    return 1
}

_rc_is() {
    local _want="$1" _got="$2" _out="$3"
    [ "$_got" -eq "$_want" ] && return 0
    echo "expected exit $_want, got $_got; tail was:" >&2
    tail -25 "$_out" >&2
    return 1
}

SENTINEL_A="@@PDIAG_HARDGATE_A_PASSED@@"
SENTINEL_B="@@PDIAG_HARDGATE_B_PASSED@@"
SENTINEL_C="@@PDIAG_HARDGATE_C_PASSED@@"

# ---------------------------------------------------------------------------
# (1) FRESH binary — nothing is skipped.
# ---------------------------------------------------------------------------
echo ""
echo "--- (1) Fresh binary: every scenario runs, all three sentinels ---"
OUT_FRESH="$PVM_TMPDIR/gate-fresh.out"
_run_gate "$FRESH_BIN" "$OUT_FRESH"

assert "(1) a fresh binary makes the PDIAG gate exit 0" \
    _rc_is 0 "$GATE_RC" "$OUT_FRESH"

assert "(1) scenario (a) RATCHET ran and passed (sentinel A)" \
    _has_token "$OUT_FRESH" "$SENTINEL_A"

assert "(1) scenario (b) HARD GATE ran and passed (sentinel B)" \
    _has_token "$OUT_FRESH" "$SENTINEL_B"

assert "(1) scenario (c) ESCAPE ran and passed (sentinel C)" \
    _has_token "$OUT_FRESH" "$SENTINEL_C"

# Pairs the sentinels with the count: three sentinels prove three scenarios
# passed, this proves NOTHING ELSE failed.
assert "(1) the green is a real green: 0 failed" \
    _matches "$OUT_FRESH" 'Results: [0-9]+ passed, 0 failed'

# ---------------------------------------------------------------------------
# (2) STALE binary — the budget-safe skip is SURGICAL.
#
# This is the leg the whole file exists for.  The gate's header states that the
# rc-75 skip "must never take the whole file down — that is precisely the bug
# that let the PTODO hard gate be silently bypassed"; before this assertion
# nothing held it to that.
# ---------------------------------------------------------------------------
echo ""
echo "--- (2) Stale binary: (a) skipped, (b)+(c) still run ---"
OUT_STALE="$PVM_TMPDIR/gate-stale.out"
_run_gate "$STALE_BIN" "$OUT_STALE"

assert "(2) a present-but-stale binary still exits 0 (budget-safe skip, not a failure)" \
    _rc_is 0 "$GATE_RC" "$OUT_STALE"

assert "(2) the precision-sensitive ratchet (a) IS skipped (no sentinel A)" \
    _lacks_token "$OUT_STALE" "$SENTINEL_A"

assert "(2) the hard gate (b) still ran and passed against the stale binary" \
    _has_token "$OUT_STALE" "$SENTINEL_B"

assert "(2) the escape (c) still ran and passed against the stale binary" \
    _has_token "$OUT_STALE" "$SENTINEL_C"

assert "(2) the skipped run is still a real green: 0 failed" \
    _matches "$OUT_STALE" 'Results: [0-9]+ passed, 0 failed'

# ---------------------------------------------------------------------------
# (3) ABSENT binary — the NO-SILENT-GREEN floor.
#
# Every scenario is guarded on `[ -x "$REIFY_AUDIT_BIN" ]`, so none execute and
# test_summary prints "0 passed, 0 failed".  run_all.sh grades on exit code
# alone, so without the $RAN floor that is a hard gate reporting green having
# asserted nothing.
# ---------------------------------------------------------------------------
echo ""
echo "--- (3) Absent binary: the no-silent-green floor fires ---"
OUT_ABSENT="$PVM_TMPDIR/gate-absent.out"
_run_gate "$ABSENT_BIN" "$OUT_ABSENT"

assert "(3) an absent binary makes the PDIAG gate exit 1, not a vacuous green" \
    _rc_is 1 "$GATE_RC" "$OUT_ABSENT"

# The discriminator: pairing exit 1 with a ZERO-assertion summary attributes
# the RED to the floor.  Exit 1 alone is satisfied by any assert going red,
# which would leave this leg green after the floor itself had been removed.
assert "(3) the RED is the floor: the run asserted nothing (0 passed, 0 failed)" \
    _matches "$OUT_ABSENT" 'Results: 0 passed, 0 failed'

assert "(3) ...and the floor names itself" \
    _matches "$OUT_ABSENT" 'refusing to report green'

for _sentinel in "$SENTINEL_A" "$SENTINEL_B" "$SENTINEL_C"; do
    assert "(3) no scenario claims to have passed ($_sentinel absent)" \
        _lacks_token "$OUT_ABSENT" "$_sentinel"
done

test_summary
