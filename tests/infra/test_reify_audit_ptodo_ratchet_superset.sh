#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo_ratchet_superset.sh
#
# Meta-test for task #6859: the §6.6 ratchet oracle is SUBSET-OF BY RULING, and
# that ruling is pinned here rather than left to prose.
#
# The question "should the baseline also assert its entries are still live?"
# (the second, `comm -13 <live> <baseline>` assertion) was considered and
# DECLINED.  The decision, its measurement basis, the alternatives rejected and
# the revisit condition live in ONE place — PRD §18
# (docs/prds/reify-audit-ptodo-detector.md) — and are deliberately NOT restated
# here.  Cited by SECTION NUMBER only: a bolded paragraph title is not a stable
# anchor (#6241's retitle stranded six such cites at once), a section number is.
#
# WHAT THIS FILE PINS — the mechanical consequence of that NO, in BOTH
# DIRECTIONS, one invocation each:
#
#   DIRECTION (i): a committed-baseline entry that is ABSENT from the live set
#   must NOT red the ratchet.  This is the §6.7 DB-degradation tolerance: the
#   committed baseline is generated WITH the task DB and so carries
#   liveness-lane kinds (orphaned, g-allow-orphaned) that a degraded
#   structural-only run cannot reproduce.  Every context the gate actually runs
#   in — task worktrees and the _merge-verify lane — lacks .taskmaster/, so a
#   set-equality oracle would red there unconditionally.
#
#   DIRECTION (ii), the ANTI-DEGENERACY control: a LIVE fingerprint absent from
#   the committed baseline must STILL red the ratchet.  Direction (i) alone
#   cannot distinguish a live subset-oracle from one disarmed to `return 0` —
#   both tolerate everything — so a one-directional guard would stay green
#   after the behaviour it pins had disappeared.  This is the same
#   both-directions discipline test_reify_audit_ptodo_ratchet_vacuity.sh states
#   in its header and applies across its two invocations.
#
# Design (mirrors test_reify_audit_ptodo_ratchet_vacuity.sh end to end):
#   - FRESHNESS INVERSION.  test_reify_audit_ptodo_orphan_hardgate.sh copies the
#     real reify-audit and backdates it to force STALE; here the copy keeps a
#     NOW mtime, so reify_audit_is_stale judges it FRESH, RATCHET_SKIP stays 0,
#     and scenario (a) actually EXECUTES.  Without this the run would go vacuous
#     in any warm lane holding a stale ambient binary.
#   - STUB GENERATOR via the documented REIFY_PTODO_GEN_BIN seam
#     (test_reify_audit_ptodo.sh, task #4624).  Being executable it also
#     short-circuits the `[ ! -x "$GEN" ]` cargo-build branch, so this test is
#     cold-build-free.  Both stubs are --project-root AWARE and differ ONLY in
#     their repo-root branch: direction (i) emits a PROPER SUBSET of the
#     committed baseline (its first line, read AT RUNTIME); direction (ii)
#     emits one synthetic fingerprint that is NOT in the baseline.  Both emit
#     well-formed scan evidence on stderr, so the vacuity floor passes in both
#     and the subset oracle is the only thing under test.  For any other root
#     both emit the same synthetic untracked line, so scenario (b) — which
#     drives the same binary against a hermetic fixture and asserts its output
#     is non-empty — still passes.  A root-blind stub would take (b) down too
#     and the exit code could not then discriminate.
#   - READ AT RUNTIME, not inlined.  Required twice over: it keeps this .sh
#     source free of literal baseline text (SELF-MATCH SAFETY below — the
#     entries are δ-A attribute anchors carrying real marker text), and it keeps
#     this test correct after any future drain of the baseline.
#
# Assertions (direction (i)):
#   (1) test_reify_audit_ptodo.sh exits 0 — a baseline entry missing from the
#       live set is TOLERATED, not a regression.
#   (2) ...and for the RIGHT reason: the output matches
#       `Results: <N> passed, 0 failed`, carries no @@RATCHET_VACUITY_FIRED@@
#       token, and carries no `RATCHET REGRESSION` diagnostic.  A bare exit-0
#       check is satisfied by any wholesale skip; pairing the count with
#       token-absence discriminates a real green from a vacuous one — the same
#       discipline test_reify_audit_ptodo_ratchet_vacuity.sh applies at its
#       assertions (4)/(5) and test_reify_audit_ptodo_budget_skip.sh at its.
#       Both tokens are matched case-sensitively and appear nowhere else in the
#       captured stream: every assert() DESCRIPTION in the underlying script
#       spells the regression lowercase, so only the real diagnostic matches.
#
# Assertions (direction (ii)):
#   (3) test_reify_audit_ptodo.sh exits 1 — a live fingerprint absent from the
#       baseline is a REGRESSION, and the subset oracle must still say so.
#   (4) ...and for the RIGHT reason: the captured output carries
#       _ratchet_check_subset's `RATCHET REGRESSION` diagnostic AND names the
#       synthetic path, AND @@RATCHET_VACUITY_FIRED@@ is ABSENT.  The token
#       absence is what discriminates the SUBSET ORACLE firing from the VACUITY
#       FLOOR firing — both produce exit 1 from the same scenario, and without
#       it the two failure modes are indistinguishable.
#   (5) EXACTLY one assert failed (`Results: <N> passed, 1 failed`), which turns
#       "the subset oracle fired" into a positive observation rather than an
#       inference that the RED was collateral damage from the stub.  Mirrors
#       test_reify_audit_ptodo_ratchet_vacuity.sh's own assertion (3).
#
# SELF-MATCH SAFETY: this file must not contain any literal marker token the
# PTODO structural lane sweeps for.  The stub's synthetic line assembles its
# token from a shell variable at heredoc-expansion time, and the baseline
# subset is read from disk at stub run time, so the written stub carries real
# tokens while this .sh source stays clean.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PTODO_TEST="$SCRIPT_DIR/test_reify_audit_ptodo.sh"
REAL_BIN="$REPO_ROOT/target/release/reify-audit"
BASELINE_FILE="$REPO_ROOT/crates/reify-audit/ptodo-baseline.txt"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== PTODO ratchet subset-of-by-ruling meta-test (task #6859) ==="

# Graceful skip when the PTODO test script is absent.
if [ ! -f "$PTODO_TEST" ]; then
    echo "test_reify_audit_ptodo_ratchet_superset.sh: $PTODO_TEST not found — skipping" >&2
    exit 0
fi

# Graceful skip when the real reify-audit binary is absent.
# (Scenarios (c)-(f) need a real binary to copy; without one the underlying
# script cannot reach an all-green state and assertion (2) would be spurious.)
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

# ---------------------------------------------------------------------------
# LOAD-BEARING SKIP: the stub can only emit a PROPER subset of the committed
# baseline if the baseline has at least two entries.  With zero entries no
# subset relation is exercisable at all; with exactly one, `head -n1` returns
# the WHOLE baseline, nothing is missing from the live set, and direction (i)
# would go green having asserted nothing about the property it exists to pin —
# the silent-vacuity failure this suite's floors were written to prevent.
#
# Skipping (rather than failing) is deliberate: a drained baseline is the §6.4
# ZERO-RESIDUAL END STATE this ruling actively wants, and this guard must never
# become an obstacle to reaching it.
# ---------------------------------------------------------------------------
BASELINE_LINES="$(grep -c . "$BASELINE_FILE" 2>/dev/null || echo 0)"
if [ "$BASELINE_LINES" -lt 2 ]; then
    echo "test_reify_audit_ptodo_ratchet_superset.sh: committed baseline has $BASELINE_LINES entry/entries (<2) — no proper subset to exercise; skipping" >&2
    exit 0
fi

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
# DIRECTION (i) stub ptodo-baseline-gen.  Written with an EXPANDING heredoc so
# $REPO_ROOT, $BASELINE_FILE and the marker token are baked in; the stub's own
# positional parameters are escaped (\$#, \$1, \$2) so the outer shell leaves
# them alone.
#
# The stub uses `set -u` but NOT `set -e`: `shift 2` on a trailing lone
# `--project-root` would abort an -e shell, and a stub that dies instead of
# emitting is a different failure than the one under test.
#
# Scan evidence is emitted UNCONDITIONALLY, before the root branch, mirroring
# the real generator's contract (one @@PTODO_SCAN@@ line on every run).
# Scenario (b) discards generator stderr, so the extra line is inert there.
# ---------------------------------------------------------------------------
SUBSET_GEN="$RSM_TMPDIR/ptodo-baseline-gen-subset"
M="TODO"
cat > "$SUBSET_GEN" <<EOF
#!/usr/bin/env bash
# Stub ptodo-baseline-gen (direction i) — generated at run time by
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
# Run evidence on stderr, every run — the §6.6 machine contract the floor reads.
printf '@@PTODO_SCAN@@ files_scanned=3067 markers_examined=42\n' >&2
if [ "\$_root" = "${REPO_ROOT}" ]; then
    # Scenario (a): the real repo root — emit a PROPER SUBSET of the committed
    # baseline, read from disk at run time so no baseline text is inlined into
    # the test source.  Every other baseline entry is therefore ABSENT from the
    # live set: exactly the state a set-equality oracle would red on, and the
    # state the subset-of oracle must TOLERATE (PRD §18).
    head -n1 "${BASELINE_FILE}"
    exit 0
fi
# Any other root is scenario (b)'s hermetic fixture — emit one synthetic
# untracked fingerprint so (b)'s two asserts still pass and the exit code stays
# attributable to scenario (a) alone.
printf 'src/fresh.rs :: untracked :: // %s: wire this into the real implementation\n' '${M}'
exit 0
EOF
chmod +x "$SUBSET_GEN"

# ---------------------------------------------------------------------------
# Invoke test_reify_audit_ptodo.sh under controlled env:
#   REIFY_AUDIT_BIN=<fresh copy>    — judged FRESH → guard rc 0 → RATCHET_SKIP=0
#   REIFY_PTODO_GEN_BIN=<stub>      — proper-subset live set at the repo root
#   REIFY_AUDIT_NO_COLD_BUILD=1     — no cold build even if something goes stale
#
# Capture combined stdout+stderr; `set +e` so the exit code can be inspected.
# ---------------------------------------------------------------------------
echo ""
echo "--- (i) Invoking test_reify_audit_ptodo.sh with a PROPER-SUBSET generator ---"

RSM_OUTPUT_FILE="$RSM_TMPDIR/ptodo-output-subset"
set +e
REIFY_AUDIT_BIN="$FRESH_BIN" \
REIFY_PTODO_GEN_BIN="$SUBSET_GEN" \
REIFY_AUDIT_NO_COLD_BUILD=1 \
    bash "$PTODO_TEST" >"$RSM_OUTPUT_FILE" 2>&1
RSM_EXIT=$?
set -e

echo "test_reify_audit_ptodo.sh (proper-subset generator) exited: $RSM_EXIT"
echo "--- Captured output (tail) ---"
tail -20 "$RSM_OUTPUT_FILE"
echo "--- End captured output ---"

# ---------------------------------------------------------------------------
# Assertions — direction (i)
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertions (direction i: baseline ⊄ live is TOLERATED) ---"

# (1) The oracle is subset-of, so baseline entries absent from the live set are
#     tolerated.  This is what makes the gate runnable in every DB-less context
#     the merge path actually uses (PRD §6.7, §18).
assert "a committed-baseline entry ABSENT from the live set does NOT red the ratchet (exit 0)" \
    bash -c '[ "$1" -eq 0 ]' -- "$RSM_EXIT"

# (2) ...and the green is real, not a skip.  Match the assert count AND the
#     absence of both failure tokens; see the header for why a bare exit-0
#     check is not a discriminator.
assert "the GREEN is real: 0 failed, no vacuity token, no RATCHET REGRESSION diagnostic" \
    bash -c "grep -qE 'Results: [0-9]+ passed, 0 failed' '$RSM_OUTPUT_FILE' \
             && ! grep -qF '@@RATCHET_VACUITY_FIRED@@' '$RSM_OUTPUT_FILE' \
             && ! grep -qF 'RATCHET REGRESSION' '$RSM_OUTPUT_FILE'"

# ---------------------------------------------------------------------------
# DIRECTION (ii) stub ptodo-baseline-gen — the ANTI-DEGENERACY control.
#
# Same seam, same freshness inversion, same heredoc discipline as direction (i);
# only the repo-root branch differs.  Here it emits ONE synthetic fingerprint
# that is NOT in the committed baseline, so `comm -23 <live> <baseline>` is
# non-empty and _ratchet_check_subset must fire.
#
# The synthetic path is deliberately non-existent: it can never collide with a
# real baseline entry, and it survives any future drain of the baseline (unlike
# a real path, which would start matching once its own entry was removed).
#
# Scan evidence is emitted here too, and that is load-bearing rather than
# cosmetic: it keeps the VACUITY FLOOR silent, so the exit-1 under test is
# attributable to the subset oracle alone.  Assertion (4) below checks that
# attribution explicitly rather than trusting it.
#
# The non-repo-root branch is IDENTICAL to direction (i)'s, so scenario (b)
# still passes and assertion (5)'s "exactly one failure" is exercisable.
# ---------------------------------------------------------------------------
SYNTHETIC_PATH="crates/does-not-exist/synthetic.rs"
REGRESSION_GEN="$RSM_TMPDIR/ptodo-baseline-gen-regression"
cat > "$REGRESSION_GEN" <<EOF
#!/usr/bin/env bash
# Stub ptodo-baseline-gen (direction ii) — generated at run time by
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
# Run evidence on stderr, every run — keeps the vacuity floor SILENT so the
# RED below is unambiguously the subset oracle.
printf '@@PTODO_SCAN@@ files_scanned=3067 markers_examined=42\n' >&2
if [ "\$_root" = "${REPO_ROOT}" ]; then
    # Scenario (a): one live fingerprint that is NOT in the committed baseline.
    # A live entry absent from the baseline is a REGRESSION and must red the
    # ratchet — the direction that keeps the subset oracle from degenerating
    # into a constant-true (PRD §18).
    printf '%s :: untracked :: // %s: synthetic fingerprint minted by the direction-(ii) stub\n' \
        '${SYNTHETIC_PATH}' '${M}'
    exit 0
fi
# Any other root is scenario (b)'s hermetic fixture — identical to direction
# (i), so (b)'s two asserts still pass and exactly one failure is expected.
printf 'src/fresh.rs :: untracked :: // %s: wire this into the real implementation\n' '${M}'
exit 0
EOF
chmod +x "$REGRESSION_GEN"

echo ""
echo "--- (ii) Invoking test_reify_audit_ptodo.sh with a NOT-IN-BASELINE generator ---"

RSM_REGRESSION_OUTPUT_FILE="$RSM_TMPDIR/ptodo-output-regression"
set +e
REIFY_AUDIT_BIN="$FRESH_BIN" \
REIFY_PTODO_GEN_BIN="$REGRESSION_GEN" \
REIFY_AUDIT_NO_COLD_BUILD=1 \
    bash "$PTODO_TEST" >"$RSM_REGRESSION_OUTPUT_FILE" 2>&1
RSM_REGRESSION_EXIT=$?
set -e

echo "test_reify_audit_ptodo.sh (not-in-baseline generator) exited: $RSM_REGRESSION_EXIT"
echo "--- Captured output (tail) ---"
tail -20 "$RSM_REGRESSION_OUTPUT_FILE"
echo "--- End captured output ---"

# ---------------------------------------------------------------------------
# Assertions — direction (ii)
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertions (direction ii: live ⊄ baseline still REDS) ---"

# (3) The subset oracle is still live: a fingerprint the baseline does not
#     grandfather is a regression.  Without this, direction (i) alone would
#     stay green against an oracle disarmed to `return 0`.
assert "a LIVE fingerprint ABSENT from the committed baseline DOES red the ratchet (exit 1)" \
    bash -c '[ "$1" -eq 1 ]' -- "$RSM_REGRESSION_EXIT"

# (4) ...and the RED is the subset oracle, not the vacuity floor.  Both fire
#     from scenario (a) and both produce exit 1; the token absence is the
#     discriminator.  The offending path must be NAMED (the item-3 contract
#     _ratchet_check_subset carries, task 5260).
assert "the RED is the subset oracle: RATCHET REGRESSION names the synthetic path, vacuity token absent" \
    bash -c "grep -qF 'RATCHET REGRESSION' '$RSM_REGRESSION_OUTPUT_FILE' \
             && grep -qF '$SYNTHETIC_PATH' '$RSM_REGRESSION_OUTPUT_FILE' \
             && ! grep -qF '@@RATCHET_VACUITY_FIRED@@' '$RSM_REGRESSION_OUTPUT_FILE'"

# (5) Exactly one assert failed — the subset oracle, not collateral damage from
#     the stub taking other scenarios down with it.
assert "exactly one assert failed (the subset oracle, not stub collateral damage)" \
    bash -c "grep -qE 'Results: [0-9]+ passed, 1 failed' '$RSM_REGRESSION_OUTPUT_FILE'"

test_summary
