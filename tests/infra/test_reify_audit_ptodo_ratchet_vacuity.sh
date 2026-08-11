#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo_ratchet_vacuity.sh
#
# Meta-test for task #6127 (esc-6087-3): the §6.6 ratchet's vacuity floor must
# be WIRED INTO scenario (a) of test_reify_audit_ptodo.sh, not merely defined.
#
# The in-file meta-test in test_reify_audit_ptodo.sh pins what
# _ratchet_check_nonempty DOES; it cannot see whether scenario (a) ever calls
# it.  A helper that is defined and unit-pinned but never invoked is exactly
# the dead instrument this task exists to eliminate, so the call site gets its
# own test — this one.
#
# THE GAP BEING PINNED.  Scenario (a)'s oracle is
# `comm -23 <(sort -u live) <(sort -u baseline)`, i.e. subset-of.  The empty
# set is a subset of everything, so a generator run that emitted ZERO
# fingerprints satisfies it trivially and the ratchet reports green having
# asserted nothing.  This test drives exactly that state and requires a RED.
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
#     cold-build-free.  The stub is --project-root AWARE: zero lines for the
#     repo root (driving scenario (a) into the vacuous state) and one
#     synthetic untracked line for any other root, so scenario (b) — which
#     drives the same binary against a hermetic fixture and asserts its output
#     is non-empty — still passes.  An unconditionally-silent stub would take
#     (b) down too, and a bare exit-code check could not then tell which
#     assertion went red.
#
# Assertions:
#   (1) test_reify_audit_ptodo.sh exits 1 — a zero-fingerprint generator run
#       must NOT report green.
#   (2) it exits 1 for THAT reason: the RATCHET VACUITY anchor appears in the
#       captured output.  Same discrimination discipline the sibling
#       meta-tests state at test_reify_audit_ptodo_budget_skip.sh:142-148 — a
#       bare exit-code check is satisfied by any unrelated failure and would
#       leave this test green after the behaviour it pins had disappeared.
#   (3) EXACTLY one assert failed.  With the project-root-aware stub keeping
#       (b) through (f) green, this turns "the floor fired" into a positive,
#       discriminating observation rather than an inference: the RED is the
#       floor and not collateral damage from the stub.
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

# (1) A zero-fingerprint generator run must not report green.
assert "zero-fingerprint generator run makes test_reify_audit_ptodo.sh exit 1 (not a vacuous green)" \
    bash -c '[ "$1" -eq 1 ]' -- "$RVM_EXIT"

# (2) ...and for the right reason.  Without this, any unrelated failure would
#     satisfy (1) and this meta-test would stay green after the floor was gone.
#
#     MATCH THE RENDERED DIAGNOSTIC, NOT THE BARE ANCHOR.  test_reify_audit_
#     ptodo.sh's in-file meta-test carries the words "RATCHET VACUITY" in its
#     own assert DESCRIPTION, and assert() echoes every description — passing
#     or failing — into this same captured stream.  A `grep -qF 'RATCHET
#     VACUITY'` therefore passes even when the floor never fired; it was
#     observed doing exactly that while this test was RED.  Keying on the
#     diagnostic's full first-line prefix, which only _ratchet_check_nonempty
#     itself ever prints, restores the discrimination.
assert "the RED is the vacuity floor: its rendered diagnostic is present in the output" \
    bash -c "grep -qF 'RATCHET VACUITY — ptodo-baseline-gen emitted 0 fingerprints' '$RVM_OUTPUT_FILE'"

# (3) ...and ONLY the floor.  Scenarios (b)-(f) stay green under the
#     project-root-aware stub, so exactly one assert may have failed.
assert "exactly one assert failed (the floor, not collateral damage from the stub)" \
    bash -c "grep -qE 'Results: [0-9]+ passed, 1 failed' '$RVM_OUTPUT_FILE'"

test_summary
