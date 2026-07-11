#!/usr/bin/env bash
# tests/infra/test_thin_warm_lane.sh
# Hermetic tests for scripts/thin-warm-lane.sh (task 5174, PRD
# docs/prds/warm-lane-pool-sizing-lifecycle.md §9.3).
#
# Real rm/flock scoped to mktemp lane fixtures (hermetic-by-default, no PATH
# stubbing of coreutils needed — only --seed-script is stubbed, via the
# script's own hermetic test seam).
#
# run_helper captures STDOUT and STDERR SEPARATELY:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI/usage contract + lane_dir existence guard (step-1/step-2)
#   B — precondition-refusal + T3 flock guard (step-3/step-4)
#   C — FREE-FIRST reclaim + T1 source-intact (step-5/step-6)
#   D — --reseed opt-in + free-BEFORE-stage ordering (step-7/step-8)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/thin-warm-lane.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/thin-warm-lane.sh hermetic tests (task 5174) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp /tmp/test-thin-warm-lane-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes thin-warm-lane.sh, capturing OUT (stdout), ERR_OUT (stderr), RC.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI/usage contract + lane_dir existence guard
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI/usage contract + lane_dir existence guard ---"

assert "A1: script exists" test -f "$SCRIPT"
assert "A2: script is executable" test -x "$SCRIPT"
assert "A3: script has the #!/usr/bin/env bash shebang" \
    bash -c 'head -1 "$1" | grep -qx "#!/usr/bin/env bash"' _ "$SCRIPT"

# A4/A5: --help exits 0 and prints a usage line to stderr
run_helper --help
assert "A4: --help exits 0" test "$RC" -eq 0
assert "A5: --help prints a 'Usage' line to stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi usage' _ "$ERR_OUT"

# A6: an unknown flag exits 2
run_helper /tmp --totally-bogus-flag-xyz
assert "A6: unknown flag exits 2" test "$RC" -eq 2

# A7: a missing positional lane_dir exits 2
run_helper
assert "A7: missing positional lane_dir exits 2" test "$RC" -eq 2

# A8: a nonexistent lane_dir exits 1 with a diagnostic on stderr, touches nothing
A8_LANE="$(mktemp -u /tmp/test-thin-warm-lane-a8-XXXXXX)"
run_helper "$A8_LANE"
assert "A8: nonexistent lane_dir exits 1" test "$RC" -eq 1
assert "A8: nonexistent lane_dir prints a diagnostic on stderr" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"
assert "A8: nonexistent lane_dir still does not exist afterward (touches nothing)" \
    bash -c '[ ! -e "$1" ]' _ "$A8_LANE"

# A9: a lane_dir that exists but is a regular file (not a directory) exits 1
A9_LANE="$(mktemp /tmp/test-thin-warm-lane-a9-XXXXXX)"
_TMPDIRS+=("$A9_LANE")
run_helper "$A9_LANE"
assert "A9: lane_dir that is a regular file exits 1" test "$RC" -eq 1
assert "A9: regular-file lane_dir is untouched" test -f "$A9_LANE"

test_summary
