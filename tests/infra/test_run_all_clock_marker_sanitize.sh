#!/usr/bin/env bash
# tests/infra/test_run_all_clock_marker_sanitize.sh
#
# Regression guard for task 4998 / esc-4791-52 (the systemic root-cause fix for
# the recurring "180s heartbeat-idle backstop false-kills a slow AFFECTED=ALL
# compile" class — supersedes the per-source whack-a-mole of tasks 4802/4887/4931).
#
# CONTRACT UNDER TEST: run_all.sh must NEUTRALIZE every
# @@REIFY_CLOCK_{STOP,HEARTBEAT,START}@@ marker token in the per-test output it
# re-emits (via _ra_emit_sanitized -> `@@REIFY_QUOTED_CLOCK_` rewrite). run_all is
# a TEST RUNNER, never a real admission wait, so a live marker in its output would
# be misread by dark-factory's clock-stop verify-timeout parser as a real
# STOP/START transition — leaving the state machine wrongly STOPPED into the heavy
# `cargo nextest run --workspace` compile, whose >180s silent native-kernel link
# gap then trips the idle backstop and SIGKILLs a healthy compile.
#
# The pollution has TWO real shapes, both covered here:
#   1. quoted-in-prose  — assertion text like "PASS: ... contains @@REIFY_CLOCK_STOP@@ ..."
#                         (the DOMINANT shape; the test's own legitimate stdout,
#                          which per-test stderr-isolation patches can never catch)
#   2. bare column-0    — a test emitting a live marker as a fixture / nested leak
#
# RECURSION NOTE (mirrors test_run_all.sh): exercises run_all.sh only against a
# TEMP fixture dir, never the real tests/infra/ dir (this file is itself
# auto-discovered by the outer run_all).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RUN_ALL="$SCRIPT_DIR/run_all.sh"
LOAD_TOLERANCE_LIB="$SCRIPT_DIR/load_tolerance_lib.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

# The marker prefix under test, assembled at runtime so THIS file's own stdout
# (assert descriptions below deliberately avoid the raw token) is never itself a
# leak source. CP = live prefix; QP = the neutralized form run_all must produce.
CP='@@REIFY_CLOCK_'
QP='@@REIFY_QUOTED_CLOCK_'

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

# grep-based (non-)containment checkers used via assert (functions, so assert's
# no-subshell direct-call path runs them in this shell — esc-4959-57).
# `-- "$2"` end-of-options guard: several needles start with "---" (run_all
# headers / attempt delimiters), which grep would otherwise parse as options.
_out_contains()  { grep -qF -- "$2" "$1"; }     # <file> <needle>  -> 0 if present
_out_lacks()     { ! grep -qF -- "$2" "$1"; }   # <file> <needle>  -> 0 if ABSENT

echo "=== run_all.sh clock-marker sanitize (task 4998) ==="

if [ ! -f "$RUN_ALL" ] || [ ! -f "$LOAD_TOLERANCE_LIB" ]; then
    # Skip cleanly if the pool substrate is unavailable: this fix scopes the
    # sanitizer to the concurrent-pool re-emission path (the production
    # DF-clock-stop-parsed verify stream), so a legacy-only environment has
    # nothing to assert here.
    assert "run_all.sh + load_tolerance_lib.sh present (skipped - pool substrate missing)" false
    test_summary
    exit 0
fi

# --- Fixtures: two `pool` tests that each emit BOTH pollution shapes on stdout
# AND stderr. One passes (normal emit site) and one fails (retry re-emit sites),
# so all three run_all re-emit sites are exercised. Tokens are assembled from CP
# via an UNQUOTED heredoc; \$\$ stays literal so it expands at fixture runtime.
_write_marker_fixture() {  # <path> <exit_code>
    cat > "$1" <<MARKEREOF
#!/usr/bin/env bash
echo "  PASS: fixture: stderr contains ${CP}STOP@@ reason=psi_pressure (hold entered)"
echo "  PASS: fixture: stderr contains ${CP}START@@ (STOP/START balanced)"
echo "${CP}HEARTBEAT@@ reason=fixture waited=0"
echo "${CP}STOP@@ reason=fixture pid=\$\$" >&2
MARKEREOF
    echo "exit $2" >> "$1"
    chmod +x "$1"
}

TMPD="$(mktemp -d)"; _TMPDIRS+=("$TMPD")
cat > "$TMPD/classification.manifest" <<'EOF'
test_marker_pass.sh pool
test_marker_flaky.sh pool
EOF
_write_marker_fixture "$TMPD/test_marker_pass.sh" 0
_write_marker_fixture "$TMPD/test_marker_flaky.sh" 1

RA_OUT_FILE="$TMPD/ra_out.txt"
RUN_ALL_CLASSIFICATION_MANIFEST="$TMPD/classification.manifest" \
    REIFY_RUN_ALL_POOL_LOCK="$TMPD/pool.lock" \
    REIFY_RUN_ALL_POOL_CONCURRENCY=2 \
    REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
    bash "$RUN_ALL" "$TMPD" > "$RA_OUT_FILE" 2>&1 || true

echo ""
echo "--- Preconditions: pool path taken, both fixtures ran (non-vacuous) ---"

# Guard against silently validating the legacy fallback (which this fix does not
# sanitize): assert the pool-path-only INFO marker is present.
assert "pool path was taken (INFO: run_all.sh pool: N= present)" \
    _out_contains "$RA_OUT_FILE" "INFO: run_all.sh pool: N="

assert "fixture test_marker_pass.sh ran (normal re-emit site)" \
    _out_contains "$RA_OUT_FILE" "--- Running: test_marker_pass.sh ---"
assert "fixture test_marker_flaky.sh ran (retry re-emit sites)" \
    _out_contains "$RA_OUT_FILE" "--- Running: test_marker_flaky.sh ---"
# The flaky fixture fails twice -> exercises the Phase 2.5 retry + dual-attempt
# emit branch (run_all.sh lines under `if [ "${_h2_retried...}" = "1" ]`).
assert "flaky fixture took the retry/dual-attempt emit branch" \
    _out_contains "$RA_OUT_FILE" "--- attempt 2 (serial retry) ---"

echo ""
echo "--- Sanitizer actually ran (neutralized form present) ---"
assert "quoted STOP token was neutralized to the QUOTED form" \
    _out_contains "$RA_OUT_FILE" "${QP}STOP@@"
assert "bare HEARTBEAT token was neutralized to the QUOTED form" \
    _out_contains "$RA_OUT_FILE" "${QP}HEARTBEAT@@"
assert "START token was neutralized to the QUOTED form" \
    _out_contains "$RA_OUT_FILE" "${QP}START@@"

echo ""
echo "--- CORE: ZERO live clock-marker tokens survive in run_all output ---"
# Substring absence is the strongest guarantee: it means BOTH dark-factory
# matchers (the legacy substring one AND the task-4998 line-anchored one) see
# nothing, from either pollution shape (quoted-in-prose or bare column-0), on
# either stream (stdout or the 2>&1-merged stderr).
assert "no live CLOCK_STOP token leaks into run_all output" \
    _out_lacks "$RA_OUT_FILE" "${CP}STOP@@"
assert "no live CLOCK_HEARTBEAT token leaks into run_all output" \
    _out_lacks "$RA_OUT_FILE" "${CP}HEARTBEAT@@"
assert "no live CLOCK_START token leaks into run_all output" \
    _out_lacks "$RA_OUT_FILE" "${CP}START@@"

test_summary
