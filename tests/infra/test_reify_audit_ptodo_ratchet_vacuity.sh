#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo_ratchet_vacuity.sh
#
# Meta-test for task #6127 (esc-6087-3), extended by #6241: the §6.6 ratchet's
# vacuity floor must be WIRED INTO scenario (a) of test_reify_audit_ptodo.sh,
# not merely defined.
#
# The in-file meta-test in test_reify_audit_ptodo.sh pins what the floor helper
# DOES; it cannot see whether scenario (a) ever calls it.  A helper that is
# defined and unit-pinned but never invoked is exactly the dead instrument this
# task exists to eliminate, so the call site gets its own test — this one.
#
# It pins BOTH DIRECTIONS of the floor, one invocation each:
#   - FIRES when the generator produced no scan evidence (invocation 1);
#   - stays SILENT when it did (invocation 2, the positive control).
# A one-directional wiring test cannot tell a live floor from one wired to a
# constant failure, and the second direction is the partition #6241 exists to
# deliver.
#
# THE GAP BEING PINNED: scenario (a)'s oracle is subset-of, which the empty set
# satisfies trivially, so it must be preconditioned on evidence the detector
# RAN — the generator's `@@PTODO_SCAN@@ files_scanned=<N> …` stderr line — not
# on what it FOUND.  See PRD §6.6 (cited by section number: a paragraph title
# is not a stable anchor) for the full
# argument.  Invocation 1 drives the generator into the no-scan-evidence state
# and requires a RED; invocation 2 supplies valid scan evidence with ZERO
# fingerprints and requires a GREEN.
#
# Design:
#   - FRESHNESS INVERSION.  test_reify_audit_ptodo_orphan_hardgate.sh copies
#     the real reify-audit and backdates it (touch -t 200001010000) to force
#     STALE.  Here the copy keeps a NOW mtime, so reify_audit_is_stale judges
#     it FRESH (mtime >= the crates/reify-audit commit epoch), the guard
#     returns 0, RATCHET_SKIP stays 0, and scenario (a) actually EXECUTES.
#     Without this the run would go vacuous in any warm lane holding a stale
#     ambient binary — the same failure class this test exists to close.
#   - STUB GENERATOR via the documented REIFY_PTODO_GEN_BIN seam
#     (test_reify_audit_ptodo.sh:138-143).  Being executable, it also
#     short-circuits the `[ ! -x "$GEN" ]` cargo-build branch, so this test is
#     cold-build-free.  The stub is --project-root AWARE: for the repo root it
#     emits zero fingerprints on stdout AND no scan evidence on stderr — it is
#     the MISSING @@PTODO_SCAN@@ line that drives scenario (a)'s floor, exactly
#     as a stale pre-#6241 binary would — and one
#     synthetic untracked line for any other root, so scenario (b) — which
#     drives the same binary against a hermetic fixture and asserts its output
#     is non-empty — still passes.  An unconditionally-silent stub would take
#     (b) down too, and a bare exit-code check could not then tell which
#     assertion went red.
#
#   - HEALTHY STUB (#6241, invocation 2).  Same seam, same freshness
#     inversion, but its repo-root branch ALSO emits a well-formed
#     `@@PTODO_SCAN@@ files_scanned=<N>` line on stderr while still emitting
#     ZERO fingerprints on stdout — a detector that demonstrably RAN over a
#     clean tree.  That is the state a floor keyed on the live finding count
#     could not tell apart from "the generator never ran".
#
# Assertions (invocation 1 — no scan evidence):
#   (1) test_reify_audit_ptodo.sh exits 1 — a generator that emitted NO scan
#       evidence must NOT report green.  The trigger is the ABSENT
#       @@PTODO_SCAN@@ line, NOT the zero fingerprints: zero fingerprints WITH
#       scan evidence is a PASS under this floor, which is precisely what
#       invocation 2 below pins.
#   (2) it exits 1 for THAT reason: the floor's @@RATCHET_VACUITY_FIRED@@
#       machine token appears in the captured output.  Same discrimination
#       discipline the sibling meta-tests state at
#       test_reify_audit_ptodo_budget_skip.sh:142-148 — a bare exit-code check
#       is satisfied by any unrelated failure and would leave this test green
#       after the behaviour it pins had disappeared.
#   (3) EXACTLY one assert failed.  With the project-root-aware stub keeping
#       (b) through (f) green, this turns "the floor fired" into a positive,
#       discriminating observation rather than an inference: the RED is the
#       floor and not collateral damage from the stub.
#
# Assertions (invocation 2 — valid scan evidence, zero fingerprints):
#   (4) test_reify_audit_ptodo.sh exits 0 — a detector that demonstrably RAN
#       over a clean tree must report GREEN.
#   (5) and for the right reason: the output matches
#       `Results: <N> passed, 0 failed` AND carries no
#       @@RATCHET_VACUITY_FIRED@@ token.  A bare exit-0 check would be
#       satisfied by any wholesale skip; pairing the count with token-absence
#       discriminates a real green from a vacuous one — the same discipline
#       assertions (2)/(3) apply to the RED direction.
#
# SELF-MATCH SAFETY: this file must not contain any literal marker token the
# PTODO structural lane sweeps for.  The stub's synthetic line assembles its
# token from a shell variable at heredoc-expansion time, so the written stub
# carries a real token while this .sh source stays clean.
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

echo "=== PTODO ratchet vacuity-floor wiring meta-test (task #6127) ==="

# Graceful skip when the PTODO test script is absent.
if [ ! -f "$PTODO_TEST" ]; then
    echo "test_reify_audit_ptodo_ratchet_vacuity.sh: $PTODO_TEST not found — skipping" >&2
    exit 0
fi

# Graceful skip when the real reify-audit binary is absent.
# (Scenarios (c)-(f) need a real binary to copy; without one the underlying
# script cannot reach a state where only the floor is red.)
if [ ! -x "$REAL_BIN" ]; then
    echo "test_reify_audit_ptodo_ratchet_vacuity.sh: $REAL_BIN absent — skipping" >&2
    exit 0
fi

# Graceful skip when required tools are absent.
# Mirror the underlying test_reify_audit_ptodo.sh tool set exactly: if any of
# these is missing that test exits 0 before any scenario runs, which would
# cause spurious assertion failures here.
for _tool in git cargo comm sort sqlite3; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "test_reify_audit_ptodo_ratchet_vacuity.sh: $_tool not on PATH — skipping" >&2
        exit 0
    fi
done

RVM_TMPDIR=$(mktemp -d /tmp/test-ptodo-ratchet-vacuity-XXXXXX)
trap 'rm -rf "$RVM_TMPDIR"' EXIT

# ---------------------------------------------------------------------------
# Fresh binary: copy the real one and leave its mtime at NOW.  `cp` without -p
# stamps the copy with the current time; the explicit touch makes that
# load-bearing property visible rather than incidental.
# ---------------------------------------------------------------------------
FRESH_BIN="$RVM_TMPDIR/reify-audit"
cp "$REAL_BIN" "$FRESH_BIN"
touch "$FRESH_BIN"

# ---------------------------------------------------------------------------
# Stub ptodo-baseline-gen.  Written with an EXPANDING heredoc so $REPO_ROOT and
# the marker token are baked in; the stub's own positional parameters are
# escaped (\$#, \$1, \$2) so the outer shell leaves them alone.
#
# The stub uses `set -u` but NOT `set -e`: `shift 2` on a trailing lone
# `--project-root` would abort an -e shell, and a stub that dies instead of
# emitting is a different failure than the one under test.
# ---------------------------------------------------------------------------
STUB_GEN="$RVM_TMPDIR/ptodo-baseline-gen"
M="TODO"
cat > "$STUB_GEN" <<EOF
#!/usr/bin/env bash
# Stub ptodo-baseline-gen — generated at run time by
# tests/infra/test_reify_audit_ptodo_ratchet_vacuity.sh.  Never committed.
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
if [ "\$_root" = "${REPO_ROOT}" ]; then
    # Scenario (a): the real repo root — emit ZERO fingerprints.  This is the
    # degraded run the vacuity floor exists to catch.
    exit 0
fi
# Any other root is scenario (b)'s hermetic fixture — emit one synthetic
# untracked fingerprint so (b)'s two asserts still pass and the RED stays
# attributable to the floor alone.
printf 'src/fresh.rs :: untracked :: // %s: wire this into the real implementation\n' '${M}'
exit 0
EOF
chmod +x "$STUB_GEN"

# ---------------------------------------------------------------------------
# Invoke test_reify_audit_ptodo.sh under controlled env:
#   REIFY_AUDIT_BIN=<fresh copy>    — judged FRESH → guard rc 0 → RATCHET_SKIP=0
#   REIFY_PTODO_GEN_BIN=<stub>      — zero fingerprints at the repo root
#   REIFY_AUDIT_NO_COLD_BUILD=1     — no cold build even if something goes stale
#
# Capture combined stdout+stderr; `set +e` so the exit code can be inspected.
# ---------------------------------------------------------------------------
echo ""
echo "--- Invoking test_reify_audit_ptodo.sh with a zero-fingerprint generator ---"

RVM_OUTPUT_FILE="$RVM_TMPDIR/ptodo-output"
set +e
REIFY_AUDIT_BIN="$FRESH_BIN" \
REIFY_PTODO_GEN_BIN="$STUB_GEN" \
REIFY_AUDIT_NO_COLD_BUILD=1 \
    bash "$PTODO_TEST" >"$RVM_OUTPUT_FILE" 2>&1
RVM_EXIT=$?
set -e

echo "test_reify_audit_ptodo.sh exited: $RVM_EXIT"
echo "--- Captured output (tail) ---"
tail -20 "$RVM_OUTPUT_FILE"
echo "--- End captured output ---"

# ---------------------------------------------------------------------------
# Assertions
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertions ---"

# (1) A generator that emitted no scan evidence must not report green.  Zero
# fingerprints alone is NOT the trigger — invocation 2 asserts that state PASSES.
assert "generator emitting NO scan evidence makes test_reify_audit_ptodo.sh exit 1 (not a vacuous green)" \
    bash -c '[ "$1" -eq 1 ]' -- "$RVM_EXIT"

# (2) ...and for the right reason.  Without this, any unrelated failure would
#     satisfy (1) and this meta-test would stay green after the floor was gone.
#
#     MATCH THE MACHINE TOKEN, NOT THE PROSE.  _ratchet_check_scan_evidence emits
#     @@RATCHET_VACUITY_FIRED@@ as the first line of its diagnostic for exactly
#     this cross-file handshake — the same idiom as the @@HARDGATE_*_PASSED@@
#     sentinels in test_reify_audit_ptodo.sh.  An English anchor is the wrong
#     discriminator both ways: `grep -qF 'RATCHET VACUITY'` was OBSERVED
#     matching while this test was still RED (assert() echoes every
#     description, passing or failing, into this same captured stream, and the
#     in-file meta-test's description carried those words), and keying on a
#     longer sentence instead only trades a false match for a false RED the
#     next time the wording is edited.  The token appears nowhere else.
assert "the RED is the vacuity floor: its machine token is present in the output" \
    bash -c "grep -qF '@@RATCHET_VACUITY_FIRED@@' '$RVM_OUTPUT_FILE'"

# (3) ...and ONLY the floor.  Scenarios (b)-(f) stay green under the
#     project-root-aware stub, so exactly one assert may have failed.
assert "exactly one assert failed (the floor, not collateral damage from the stub)" \
    bash -c "grep -qE 'Results: [0-9]+ passed, 1 failed' '$RVM_OUTPUT_FILE'"

# ---------------------------------------------------------------------------
# INVOCATION 2 (task #6241) — THE POSITIVE CONTROL.
#
# Same seam and same freshness inversion, but the stub's repo-root branch now
# emits valid scan evidence on stderr while still emitting ZERO fingerprints on
# stdout: a detector that demonstrably RAN over a clean tree.  Under a floor
# keyed on run evidence that must be GREEN.  Without this direction the wiring
# test cannot distinguish a live floor from one wired to a constant failure.
#
# The scan line is emitted UNCONDITIONALLY, before the root branch, mirroring
# the real generator's contract (`@@PTODO_SCAN@@` on every run).  Scenario (b)
# discards generator stderr, so the extra line is inert there.
# ---------------------------------------------------------------------------
HEALTHY_GEN="$RVM_TMPDIR/ptodo-baseline-gen-healthy"
cat > "$HEALTHY_GEN" <<EOF
#!/usr/bin/env bash
# Healthy stub ptodo-baseline-gen — generated at run time by
# tests/infra/test_reify_audit_ptodo_ratchet_vacuity.sh.  Never committed.
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
printf '@@PTODO_SCAN@@ files_scanned=1755 markers_examined=0\n' >&2
if [ "\$_root" = "${REPO_ROOT}" ]; then
    # Scenario (a): the real repo root — the detector RAN and the tree is
    # clean, so zero fingerprints is the CORRECT end state, not a vacuous pass.
    exit 0
fi
# Any other root is scenario (b)'s hermetic fixture — emit one synthetic
# untracked fingerprint so (b)'s asserts still pass.
printf 'src/fresh.rs :: untracked :: // %s: wire this into the real implementation\n' '${M}'
exit 0
EOF
chmod +x "$HEALTHY_GEN"

echo ""
echo "--- Invoking test_reify_audit_ptodo.sh with a HEALTHY (scan-evidence) generator ---"

RVM_HEALTHY_OUTPUT_FILE="$RVM_TMPDIR/ptodo-output-healthy"
set +e
REIFY_AUDIT_BIN="$FRESH_BIN" \
REIFY_PTODO_GEN_BIN="$HEALTHY_GEN" \
REIFY_AUDIT_NO_COLD_BUILD=1 \
    bash "$PTODO_TEST" >"$RVM_HEALTHY_OUTPUT_FILE" 2>&1
RVM_HEALTHY_EXIT=$?
set -e

echo "test_reify_audit_ptodo.sh (healthy generator) exited: $RVM_HEALTHY_EXIT"
echo "--- Captured output (tail) ---"
tail -20 "$RVM_HEALTHY_OUTPUT_FILE"
echo "--- End captured output ---"

echo ""
echo "--- Assertions (positive control) ---"

# (4) A detector that demonstrably RAN over a clean tree must report GREEN.
assert "scan-evidence generator run makes test_reify_audit_ptodo.sh exit 0 (clean tree is a PASS)" \
    bash -c '[ "$1" -eq 0 ]' -- "$RVM_HEALTHY_EXIT"

# (5) ...and for the right reason.  A bare exit-0 check is satisfied by any
#     wholesale skip, so pair the assert count with absence of the floor's
#     machine token — the same discrimination discipline as (2)/(3).
assert "the GREEN is real: 0 failed and the vacuity token is absent" \
    bash -c "grep -qE 'Results: [0-9]+ passed, 0 failed' '$RVM_HEALTHY_OUTPUT_FILE' \
             && ! grep -qF '@@RATCHET_VACUITY_FIRED@@' '$RVM_HEALTHY_OUTPUT_FILE'"

test_summary
