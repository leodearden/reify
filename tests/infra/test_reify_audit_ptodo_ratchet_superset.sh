#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo_ratchet_superset.sh
#
# Meta-test for tasks #6859 and #7001: scenario (a) of test_reify_audit_ptodo.sh
# is a TWO-DIRECTIONAL oracle over the baseline — live ⊆ baseline (the subset
# oracle, `comm -23`) AND baseline ⊆ live (the converse, `comm -13`).  The
# ruling lives in ONE place, PRD §19 (docs/prds/reify-audit-ptodo-detector.md),
# and is deliberately NOT restated here.  Cited by SECTION NUMBER only: a bolded
# paragraph title is not a stable anchor, a section number is.
#
# WHAT THIS FILE PINS, one invocation per direction:
#
#   DIRECTION (i): a baseline line ABSENT from the live set reds the ratchet
#   (PRD §19 flipped this direction; under §18 it was tolerated).  It is ALSO
#   the anti-degeneracy control for the converse oracle: a `comm -13` check
#   disarmed to `return 0` passes direction (ii) and fails here.
#
#   DIRECTION (ii): a LIVE fingerprint absent from the baseline reds the
#   ratchet, the anti-degeneracy control for the subset oracle: one disarmed
#   to `return 0` would tolerate everything.
#
# Each direction also asserts the OTHER oracle stayed silent, so each RED is
# attributable to exactly one oracle.
#
# Design (mirrors test_reify_audit_ptodo_ratchet_vacuity.sh end to end):
#   - FRESHNESS INVERSION.  The copied reify-audit keeps a NOW mtime, so the
#     freshness guard judges it FRESH, RATCHET_SKIP stays 0, and scenario (a)
#     actually EXECUTES in any warm lane.
#   - STUB GENERATORS via the REIFY_PTODO_GEN_BIN seam (task #4624), which also
#     short-circuit the cold-build branch.  Both are --project-root AWARE and
#     differ ONLY in the live set their repo-root branch prints.  Both emit
#     `@@PTODO_SCAN@@ … tasks_db=absent` on stderr, so the vacuity and DB-absent
#     floors stay silent and the oracles are the only thing under test.  For
#     any other root both emit the same synthetic untracked line, so scenario
#     (b) stays green and the exit code stays attributable to scenario (a).
#   - SYNTHETIC BASELINES via the REIFY_PTODO_BASELINE seam.  The committed
#     baseline is structural-only and empty (PRD §19), so it can exercise
#     neither direction.  Every line lives under crates/does-not-exist/, so it
#     can never collide with a real fingerprint.
#
# Assertions (direction (i): baseline {KEPT, STALE}, live {KEPT}):
#   (1) test_reify_audit_ptodo.sh exits 1;
#   (2) the RED is the converse oracle: @@RATCHET_STALE_BASELINE_FIRED@@ is
#       present and the STALE path is NAMED, while `RATCHET REGRESSION`,
#       @@RATCHET_VACUITY_FIRED@@ and @@RATCHET_DB_ABSENT_UNPROVEN@@ are absent;
#   (3) EXACTLY one assert failed.
#
# Assertions (direction (ii): baseline {KEPT}, live {KEPT, SYNTHETIC}):
#   (4) test_reify_audit_ptodo.sh exits 1;
#   (5) the RED is the subset oracle: `RATCHET REGRESSION` names the SYNTHETIC
#       path, while the stale and vacuity tokens are absent;
#   (6) EXACTLY one assert failed.
#
# Tokens and `RATCHET REGRESSION` are matched case-sensitively and appear
# nowhere else in the captured stream: every assert() DESCRIPTION in the
# underlying script spells the regression lowercase and names no token.
#
# SELF-MATCH SAFETY: this file must not contain any literal marker token the
# PTODO structural lane sweeps for.  Every fingerprint line assembles its
# token from $M at run time, so the written files carry real tokens while this
# .sh source stays clean.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PTODO_TEST="$SCRIPT_DIR/test_reify_audit_ptodo.sh"
REAL_BIN="$REPO_ROOT/target/release/reify-audit"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== PTODO ratchet two-directional oracle meta-test (tasks #6859, #7001) ==="

# Graceful skip when the PTODO test script is absent.
if [ ! -f "$PTODO_TEST" ]; then
    echo "test_reify_audit_ptodo_ratchet_superset.sh: $PTODO_TEST not found — skipping" >&2
    exit 0
fi

# Graceful skip when the real reify-audit binary is absent.
# (Scenarios (c)-(g) need a real binary to copy; without one the underlying
# script cannot reach a state where only one oracle is red.)
if [ ! -x "$REAL_BIN" ]; then
    echo "test_reify_audit_ptodo_ratchet_superset.sh: $REAL_BIN absent — skipping" >&2
    exit 0
fi

# Graceful skip when required tools are absent.
# Mirror the underlying test_reify_audit_ptodo.sh tool set EXACTLY: if any of
# these is missing that test exits 0 before any scenario runs, which would
# cause spurious assertion failures here.
for _tool in git cargo comm sort sqlite3; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "test_reify_audit_ptodo_ratchet_superset.sh: $_tool not on PATH — skipping" >&2
        exit 0
    fi
done

RSM_TMPDIR=$(mktemp -d /tmp/test-ptodo-ratchet-superset-XXXXXX)
trap 'rm -rf "$RSM_TMPDIR"' EXIT

# ---------------------------------------------------------------------------
# Fresh binary: copy the real one and leave its mtime at NOW.  `cp` without -p
# stamps the copy with the current time; the explicit touch makes that
# load-bearing property visible rather than incidental.
# ---------------------------------------------------------------------------
FRESH_BIN="$RSM_TMPDIR/reify-audit"
cp "$REAL_BIN" "$FRESH_BIN"
touch "$FRESH_BIN"

# ---------------------------------------------------------------------------
# Synthetic fingerprint lines.  STALE sorts after KEPT and SYNTHETIC after
# STALE, but nothing depends on that: scenario (a) sorts both sides itself.
# ---------------------------------------------------------------------------
M="TODO"
KEPT_PATH="crates/does-not-exist/kept.rs"
STALE_PATH="crates/does-not-exist/stale.rs"
SYNTHETIC_PATH="crates/does-not-exist/synthetic.rs"
KEPT_LINE="$KEPT_PATH :: untracked :: // $M: kept in both the baseline and the live set"
STALE_LINE="$STALE_PATH :: untracked :: // $M: in the baseline, no longer live"
SYNTHETIC_LINE="$SYNTHETIC_PATH :: untracked :: // $M: live, not in the baseline"

# ---------------------------------------------------------------------------
# _write_stub_gen <path> <live-set-file> — write a stub ptodo-baseline-gen
# that prints <live-set-file> as its repo-root live set.
#
# Written with an EXPANDING heredoc so $REPO_ROOT, the live-set path and the
# marker token are baked in; the stub's own positional parameters are escaped
# (\$#, \$1, \$2) so the outer shell leaves them alone.  The stub uses `set -u`
# but NOT `set -e`: `shift 2` on a trailing lone `--project-root` would abort an
# -e shell, and a stub that dies instead of emitting is a different failure
# than the one under test.  Scan evidence is emitted on every run, mirroring
# the real generator; scenario (b) discards generator stderr.
# ---------------------------------------------------------------------------
_write_stub_gen() {
    local _path="$1" _live="$2"
    cat > "$_path" <<EOF
#!/usr/bin/env bash
# Stub ptodo-baseline-gen — generated at run time by
# tests/infra/test_reify_audit_ptodo_ratchet_superset.sh.  Never committed.
set -u
_root=""
while [ "\$#" -gt 0 ]; do
    case "\$1" in
        --project-root)
            _root="\${2:-}"
            shift
            shift 2>/dev/null || true
            ;;
        *) shift ;;
    esac
done
# Run evidence, DB-absent: keeps both scenario-(a) floors silent.
printf '@@PTODO_SCAN@@ files_scanned=3067 markers_examined=42 tasks_db=absent\n' >&2
if [ "\$_root" = "${REPO_ROOT}" ]; then
    cat '${_live}'
    exit 0
fi
# Any other root is scenario (b)'s hermetic fixture — one synthetic untracked
# fingerprint keeps (b)'s asserts green.
printf 'src/fresh.rs :: untracked :: // %s: wire this into the real implementation\n' '${M}'
exit 0
EOF
    chmod +x "$_path"
}

# ---------------------------------------------------------------------------
# _invoke_ptodo_test <stub> <baseline-file> <output-file> — run
# test_reify_audit_ptodo.sh with the fresh binary, the stub generator and the
# synthetic baseline, no cold build, and the ambient REIFY_PTODO_TASKS_DB
# removed.  Captures combined output, prints its tail, and returns the child's
# exit code.
# ---------------------------------------------------------------------------
_invoke_ptodo_test() {
    local _stub="$1" _baseline="$2" _out="$3" _rc=0
    env -u REIFY_PTODO_TASKS_DB \
        REIFY_AUDIT_BIN="$FRESH_BIN" \
        REIFY_PTODO_GEN_BIN="$_stub" \
        REIFY_PTODO_BASELINE="$_baseline" \
        REIFY_AUDIT_NO_COLD_BUILD=1 \
        bash "$PTODO_TEST" >"$_out" 2>&1 || _rc=$?
    echo "test_reify_audit_ptodo.sh exited: $_rc"
    echo "--- Captured output (tail) ---"
    tail -20 "$_out"
    echo "--- End captured output ---"
    return "$_rc"
}

# ---------------------------------------------------------------------------
# DIRECTION (i) — baseline {KEPT, STALE}, live {KEPT}.
# ---------------------------------------------------------------------------
STALE_BASELINE="$RSM_TMPDIR/baseline-kept-stale.txt"
printf '%s\n' "$KEPT_LINE" "$STALE_LINE" | sort > "$STALE_BASELINE"
KEPT_LIVE="$RSM_TMPDIR/live-kept.txt"
printf '%s\n' "$KEPT_LINE" > "$KEPT_LIVE"
STALE_GEN="$RSM_TMPDIR/ptodo-baseline-gen-stale"
_write_stub_gen "$STALE_GEN" "$KEPT_LIVE"

echo ""
echo "--- (i) Invoking test_reify_audit_ptodo.sh with a baseline line that is no longer live ---"
RSM_STALE_OUTPUT_FILE="$RSM_TMPDIR/ptodo-output-stale"
RSM_STALE_EXIT=0
_invoke_ptodo_test "$STALE_GEN" "$STALE_BASELINE" "$RSM_STALE_OUTPUT_FILE" || RSM_STALE_EXIT=$?

echo ""
echo "--- Assertions (direction i: baseline ⊄ live REDS) ---"

# (1) A grandfather line that is no longer live is stale, and reds.
assert "a baseline line ABSENT from the live set DOES red the ratchet (exit 1)" \
    bash -c '[ "$1" -eq 1 ]' -- "$RSM_STALE_EXIT"

# (2) ...and the RED is the converse oracle, naming the stale line.
assert "the RED is the converse oracle: its token names the stale path; no subset, vacuity or DB-absent failure" \
    bash -c "grep -qF '@@RATCHET_STALE_BASELINE_FIRED@@' '$RSM_STALE_OUTPUT_FILE' \
             && grep -qF '$STALE_PATH' '$RSM_STALE_OUTPUT_FILE' \
             && ! grep -qF 'RATCHET REGRESSION' '$RSM_STALE_OUTPUT_FILE' \
             && ! grep -qF '@@RATCHET_VACUITY_FIRED@@' '$RSM_STALE_OUTPUT_FILE' \
             && ! grep -qF '@@RATCHET_DB_ABSENT_UNPROVEN@@' '$RSM_STALE_OUTPUT_FILE'"

# (3) ...and ONLY that oracle.
assert "exactly one assert failed (the converse oracle, not stub collateral damage)" \
    bash -c "grep -qE 'Results: [0-9]+ passed, 1 failed' '$RSM_STALE_OUTPUT_FILE'"

# ---------------------------------------------------------------------------
# DIRECTION (ii) — baseline {KEPT}, live {KEPT, SYNTHETIC}.
# ---------------------------------------------------------------------------
KEPT_BASELINE="$RSM_TMPDIR/baseline-kept.txt"
printf '%s\n' "$KEPT_LINE" > "$KEPT_BASELINE"
REGRESSION_LIVE="$RSM_TMPDIR/live-kept-synthetic.txt"
printf '%s\n' "$KEPT_LINE" "$SYNTHETIC_LINE" | sort > "$REGRESSION_LIVE"
REGRESSION_GEN="$RSM_TMPDIR/ptodo-baseline-gen-regression"
_write_stub_gen "$REGRESSION_GEN" "$REGRESSION_LIVE"

echo ""
echo "--- (ii) Invoking test_reify_audit_ptodo.sh with a live fingerprint not in the baseline ---"
RSM_REGRESSION_OUTPUT_FILE="$RSM_TMPDIR/ptodo-output-regression"
RSM_REGRESSION_EXIT=0
_invoke_ptodo_test "$REGRESSION_GEN" "$KEPT_BASELINE" "$RSM_REGRESSION_OUTPUT_FILE" \
    || RSM_REGRESSION_EXIT=$?

echo ""
echo "--- Assertions (direction ii: live ⊄ baseline REDS) ---"

# (4) A fingerprint the baseline does not grandfather is a regression.
assert "a LIVE fingerprint ABSENT from the baseline DOES red the ratchet (exit 1)" \
    bash -c '[ "$1" -eq 1 ]' -- "$RSM_REGRESSION_EXIT"

# (5) ...and the RED is the subset oracle, naming the new fingerprint (the
#     item-3 contract _ratchet_check_subset carries, task 5260).
assert "the RED is the subset oracle: RATCHET REGRESSION names the synthetic path; no stale or vacuity failure" \
    bash -c "grep -qF 'RATCHET REGRESSION' '$RSM_REGRESSION_OUTPUT_FILE' \
             && grep -qF '$SYNTHETIC_PATH' '$RSM_REGRESSION_OUTPUT_FILE' \
             && ! grep -qF '@@RATCHET_STALE_BASELINE_FIRED@@' '$RSM_REGRESSION_OUTPUT_FILE' \
             && ! grep -qF '@@RATCHET_VACUITY_FIRED@@' '$RSM_REGRESSION_OUTPUT_FILE'"

# (6) ...and ONLY that oracle.
assert "exactly one assert failed (the subset oracle, not stub collateral damage)" \
    bash -c "grep -qE 'Results: [0-9]+ passed, 1 failed' '$RSM_REGRESSION_OUTPUT_FILE'"

test_summary
