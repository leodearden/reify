#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo_ratchet_vacuity.sh
#
# Meta-test for task #6127 (esc-6087-3), extended by #6241 and #7001: the
# two PRECONDITIONS of scenario (a) in test_reify_audit_ptodo.sh must be WIRED
# INTO that scenario, not merely defined.
#
#   - RUN evidence (PRD §6.6): the generator's `@@PTODO_SCAN@@ files_scanned=<N>`
#     stderr line proves the detector RAN, since the empty set satisfies the
#     ratchet trivially.  Helper: _ratchet_check_scan_evidence, token
#     @@RATCHET_VACUITY_FIRED@@.
#   - DB-ABSENT evidence (PRD §19): the same line's `tasks_db=absent` token
#     proves the run was DB-absent, so liveness cannot reach the ratchet.
#     Helper: _ratchet_check_db_absent_evidence, token
#     @@RATCHET_DB_ABSENT_UNPROVEN@@.
#
# Both are cited by SECTION NUMBER only (a paragraph title is not a stable
# anchor) and argued there, not here.  The in-file meta-tests in
# test_reify_audit_ptodo.sh pin what each helper DOES; they cannot see whether
# scenario (a) ever calls it, so the call sites get this test.
#
# Each precondition is pinned in BOTH DIRECTIONS, because a one-directional
# wiring test cannot tell a live floor from one wired to a constant:
#   invocation 1 — NO scan line          → the run-evidence floor FIRES;
#   invocation 2 — HEALTHY generator     → BOTH floors stay SILENT (positive
#                                          control for both);
#   invocation 3 — scan line says
#                  tasks_db=present      → the DB-absent floor FIRES.
#
# The HEALTHY stub simulates the main checkout, where §6.7's default tasks.db
# path resolves: it reports `tasks_db=absent` only when REIFY_PTODO_TASKS_DB
# names a path that does not exist, and `present` otherwise.  So a scenario (a)
# that merely UNSETS the override (PRD §19 finding 2) reds invocation 2 on any
# host, not only on the main checkout.  Each child runs with the ambient
# override removed, so the caller's environment cannot mask that.
#
# Design:
#   - FRESHNESS INVERSION.  test_reify_audit_ptodo_orphan_hardgate.sh copies
#     the real reify-audit and backdates it (touch -t 200001010000) to force
#     STALE.  Here the copy keeps a NOW mtime, so reify_audit_is_stale judges
#     it FRESH, the guard returns 0, RATCHET_SKIP stays 0, and scenario (a)
#     actually EXECUTES.  Without this the run would go vacuous in any warm
#     lane holding a stale ambient binary.
#   - STUB GENERATORS via the documented REIFY_PTODO_GEN_BIN seam.  Being
#     executable, each also short-circuits the `[ ! -x "$GEN" ]` cargo-build
#     branch, so this test is cold-build-free.  Every stub is --project-root
#     AWARE: at the repo root it emits ZERO fingerprints (a clean tree); for
#     any other root — scenario (b)'s hermetic fixture — it emits one synthetic
#     untracked line, so (b) stays green and each RED stays attributable to a
#     single floor.  The stubs differ ONLY in the scan evidence they print on
#     stderr, which is why one writer builds all three.
#   - HERMETIC BASELINE via the REIFY_PTODO_BASELINE seam: every invocation
#     compares against an EMPTY file, so nothing here depends on the committed
#     baseline's contents.
#
# Assertions — invocation 1 (no scan evidence):
#   (1) test_reify_audit_ptodo.sh exits 1;
#   (2) for that reason: @@RATCHET_VACUITY_FIRED@@ is in the captured output.
#       Match the machine token, never the prose: assert() echoes every
#       description into the same stream, so an English anchor was OBSERVED
#       matching while this test was still RED;
#   (3) EXACTLY one assert failed.  The DB-absent floor defers when there is
#       no scan line at all, so the RED is the run-evidence floor alone.
# Assertions — invocation 2 (healthy):
#   (4) exits 0;
#   (5) `Results: <N> passed, 0 failed` and NEITHER floor's token present.  A
#       bare exit-0 is satisfied by any wholesale skip.
# Assertions — invocation 3 (tasks_db=present):
#   (6) exits 1;
#   (7) @@RATCHET_DB_ABSENT_UNPROVEN@@ present and @@RATCHET_VACUITY_FIRED@@
#       absent: the scan line is valid, so only the DB-absent floor may fire;
#   (8) EXACTLY one assert failed.
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

echo "=== PTODO ratchet scenario-(a) precondition wiring meta-test (tasks #6127, #7001) ==="

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

EMPTY_BASELINE="$RVM_TMPDIR/ptodo-baseline.txt"
: > "$EMPTY_BASELINE"

# ---------------------------------------------------------------------------
# _write_stub_gen <path> <scan-evidence-snippet> — write a stub
# ptodo-baseline-gen whose only varying part is <snippet>, shell code run
# before the root branch to print scan evidence on stderr (or nothing).
#
# Written with an EXPANDING heredoc so $REPO_ROOT, the snippet and the marker
# token are baked in; the stub's own positional parameters are escaped (\$#,
# \$1, \$2) so the outer shell leaves them alone.  The stub uses `set -u` but
# NOT `set -e`: `shift 2` on a trailing lone `--project-root` would abort an -e
# shell, and a stub that dies instead of emitting is a different failure than
# the one under test.  <snippet> is spliced in VERBATIM (an expansion result is
# never re-expanded), so it is written exactly as the stub should read it.
# Scenario (b) discards generator stderr, so the snippet is inert there.
# ---------------------------------------------------------------------------
M="TODO"
_write_stub_gen() {
    local _path="$1" _scan_snippet="$2"
    cat > "$_path" <<EOF
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
${_scan_snippet}
if [ "\$_root" = "${REPO_ROOT}" ]; then
    # Scenario (a): the real repo root — a clean tree, zero fingerprints.
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
# _invoke_ptodo_test <stub> <output-file> — run test_reify_audit_ptodo.sh with:
#   REIFY_AUDIT_BIN=<fresh copy>        — judged FRESH → RATCHET_SKIP=0
#   REIFY_PTODO_GEN_BIN=<stub>
#   REIFY_PTODO_BASELINE=<empty file>   — hermetic against the committed file
#   REIFY_AUDIT_NO_COLD_BUILD=1         — no cold build
#   REIFY_PTODO_TASKS_DB removed        — scenario (a) must set it itself
# Captures combined output, prints its tail, and returns the child's exit code.
# ---------------------------------------------------------------------------
_invoke_ptodo_test() {
    local _stub="$1" _out="$2" _rc=0
    env -u REIFY_PTODO_TASKS_DB \
        REIFY_AUDIT_BIN="$FRESH_BIN" \
        REIFY_PTODO_GEN_BIN="$_stub" \
        REIFY_PTODO_BASELINE="$EMPTY_BASELINE" \
        REIFY_AUDIT_NO_COLD_BUILD=1 \
        bash "$PTODO_TEST" >"$_out" 2>&1 || _rc=$?
    echo "test_reify_audit_ptodo.sh exited: $_rc"
    echo "--- Captured output (tail) ---"
    tail -20 "$_out"
    echo "--- End captured output ---"
    return "$_rc"
}

# ---------------------------------------------------------------------------
# INVOCATION 1 — no scan evidence, exactly as a stale pre-#6241 binary.
# ---------------------------------------------------------------------------
NO_SCAN_GEN="$RVM_TMPDIR/ptodo-baseline-gen-no-scan"
_write_stub_gen "$NO_SCAN_GEN" ': # no @@PTODO_SCAN@@ line'

echo ""
echo "--- Invoking test_reify_audit_ptodo.sh with a NO-SCAN-EVIDENCE generator ---"
RVM_OUTPUT_FILE="$RVM_TMPDIR/ptodo-output"
RVM_EXIT=0
_invoke_ptodo_test "$NO_SCAN_GEN" "$RVM_OUTPUT_FILE" || RVM_EXIT=$?

echo ""
echo "--- Assertions (invocation 1: no scan evidence) ---"

# (1) A generator that emitted no scan evidence must not report green.  Zero
# fingerprints alone is NOT the trigger — invocation 2 asserts that state PASSES.
assert "generator emitting NO scan evidence makes test_reify_audit_ptodo.sh exit 1 (not a vacuous green)" \
    bash -c '[ "$1" -eq 1 ]' -- "$RVM_EXIT"

# (2) ...and for the right reason: the floor's machine token, never its prose.
assert "the RED is the vacuity floor: its machine token is present in the output" \
    bash -c "grep -qF '@@RATCHET_VACUITY_FIRED@@' '$RVM_OUTPUT_FILE'"

# (3) ...and ONLY the floor.
assert "exactly one assert failed (the floor, not collateral damage from the stub)" \
    bash -c "grep -qE 'Results: [0-9]+ passed, 1 failed' '$RVM_OUTPUT_FILE'"

# ---------------------------------------------------------------------------
# INVOCATION 2 — THE POSITIVE CONTROL for both floors.  Valid run evidence
# every run, and a tasks_db token that follows §6.7 the way the main checkout
# does: `absent` only when the override names a path that does not exist.
# ---------------------------------------------------------------------------
HEALTHY_GEN="$RVM_TMPDIR/ptodo-baseline-gen-healthy"
_write_stub_gen "$HEALTHY_GEN" '# §6.7 as on the main checkout: the default tasks.db path resolves unless an
# override names a path that does not exist.
if [ -n "${REIFY_PTODO_TASKS_DB:-}" ] && [ ! -e "$REIFY_PTODO_TASKS_DB" ]; then
    _tasks_db=absent
else
    _tasks_db=present
fi
printf "@@PTODO_SCAN@@ files_scanned=1755 markers_examined=0 tasks_db=%s\n" "$_tasks_db" >&2'

echo ""
echo "--- Invoking test_reify_audit_ptodo.sh with a HEALTHY (§6.7-aware) generator ---"
RVM_HEALTHY_OUTPUT_FILE="$RVM_TMPDIR/ptodo-output-healthy"
RVM_HEALTHY_EXIT=0
_invoke_ptodo_test "$HEALTHY_GEN" "$RVM_HEALTHY_OUTPUT_FILE" || RVM_HEALTHY_EXIT=$?

echo ""
echo "--- Assertions (invocation 2: positive control) ---"

# (4) A detector that demonstrably RAN, DB-absent, over a clean tree is GREEN.
assert "scan-evidence generator run makes test_reify_audit_ptodo.sh exit 0 (clean tree is a PASS)" \
    bash -c '[ "$1" -eq 0 ]' -- "$RVM_HEALTHY_EXIT"

# (5) ...and for the right reason: 0 failed, and neither floor's token present.
assert "the GREEN is real: 0 failed and neither the vacuity nor the DB-absent token is present" \
    bash -c "grep -qE 'Results: [0-9]+ passed, 0 failed' '$RVM_HEALTHY_OUTPUT_FILE' \
             && ! grep -qF '@@RATCHET_VACUITY_FIRED@@' '$RVM_HEALTHY_OUTPUT_FILE' \
             && ! grep -qF '@@RATCHET_DB_ABSENT_UNPROVEN@@' '$RVM_HEALTHY_OUTPUT_FILE'"

# ---------------------------------------------------------------------------
# INVOCATION 3 — valid run evidence, but tasks_db=present UNCONDITIONALLY: the
# DB-dependent lanes ran.  A DB-absent floor wired to constant-true passes
# invocation 2 and fails here.
# ---------------------------------------------------------------------------
DB_PRESENT_GEN="$RVM_TMPDIR/ptodo-baseline-gen-db-present"
_write_stub_gen "$DB_PRESENT_GEN" \
    "printf '@@PTODO_SCAN@@ files_scanned=1755 markers_examined=0 tasks_db=present\n' >&2"

echo ""
echo "--- Invoking test_reify_audit_ptodo.sh with a DB-PRESENT generator ---"
RVM_DB_PRESENT_OUTPUT_FILE="$RVM_TMPDIR/ptodo-output-db-present"
RVM_DB_PRESENT_EXIT=0
_invoke_ptodo_test "$DB_PRESENT_GEN" "$RVM_DB_PRESENT_OUTPUT_FILE" || RVM_DB_PRESENT_EXIT=$?

echo ""
echo "--- Assertions (invocation 3: tasks_db=present) ---"

# (6) A run whose DB-dependent lanes ran must not report green.
assert "a tasks_db=present generator makes test_reify_audit_ptodo.sh exit 1" \
    bash -c '[ "$1" -eq 1 ]' -- "$RVM_DB_PRESENT_EXIT"

# (7) ...and the RED is the DB-absent floor, not the vacuity floor.
assert "the RED is the DB-absent floor: its token is present and the vacuity token is absent" \
    bash -c "grep -qF '@@RATCHET_DB_ABSENT_UNPROVEN@@' '$RVM_DB_PRESENT_OUTPUT_FILE' \
             && ! grep -qF '@@RATCHET_VACUITY_FIRED@@' '$RVM_DB_PRESENT_OUTPUT_FILE'"

# (8) ...and ONLY that floor.
assert "exactly one assert failed (the DB-absent floor, not stub collateral damage)" \
    bash -c "grep -qE 'Results: [0-9]+ passed, 1 failed' '$RVM_DB_PRESENT_OUTPUT_FILE'"

test_summary
