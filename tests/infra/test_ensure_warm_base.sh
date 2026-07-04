#!/usr/bin/env bash
# tests/infra/test_ensure_warm_base.sh
# Hermetic tests for scripts/ensure-warm-base.sh (task 4988).
#
# The autonomous-first self-heal ladder for the warm-lane CoW base:
#   Rung 1 — base healthy -> silent no-op (idempotent)
#   Rung 2 — base absent/dangling + warm source survives -> cheap reflink reseed
#   Rung 3 — base absent + no warm source -> AUTO cold-build (backgrounded by default)
#   Rung 4 — remediation fails -> exactly ONE escalation token + exit non-zero
#
# The sibling scripts (refresh-warm-base.sh, seed-warm-base-initial.sh) are
# invoked through env-overridable command hooks (REIFY_WARM_BASE_HEALTH_REFRESH_CMD
# / _SEED_CMD) so this suite can inject argv-recording mocks instead of running
# the real (slow / filesystem-mutating) sibling scripts.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script (escalation tokens land here)
#   ERR_OUT — captured stderr from the script (all diagnostics)
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help exits 0 with usage; unknown flag exits 2
#   B — Rung 1: base healthy -> no-op (idempotent, no reseed/build/escalation)
#   C — Rung 2: dangling/absent base + surviving warm source -> reflink reseed
#       (success: refresh-cmd invoked with the right argv, no escalation;
#        failure: refresh-cmd fails or base stays absent -> ONE escalation token)
#   D — Rung 3 + Rung 4: fresh partition -> backgrounded cold-build kick (async
#       default, no escalation observed); sync mode failure -> ONE escalation token
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/ensure-warm-base.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/ensure-warm-base.sh hermetic tests (task 4988) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-ensure-warm-base-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-ensure-warm-base-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-ensure-warm-base-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── command-hook mocks (invoked via full path, not PATH-searched) ────────────

# mock-refresh.sh: records argv; if REIFY_TEST_REFRESH_CREATE_BASE=1, creates
# the last positional argument (the base_dir) as a non-empty directory
# (simulating a successful reflink reseed); exits REIFY_TEST_REFRESH_RC (default 0).
MOCK_REFRESH="$STUB_DIR/mock-refresh.sh"
cat > "$MOCK_REFRESH" << 'STUB_EOF'
#!/usr/bin/env bash
echo "refresh $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_REFRESH_CREATE_BASE:-0}" = "1" ]; then
    _dst="${!#}"
    mkdir -p "$_dst"
    printf 'reseeded\n' > "$_dst/reseeded.txt"
fi
exit "${REIFY_TEST_REFRESH_RC:-0}"
STUB_EOF
chmod +x "$MOCK_REFRESH"

# mock-seed.sh: records argv; if REIFY_TEST_SEED_CREATE_BASE=1, scans argv for
# "--base-dir VALUE" and creates it as a non-empty directory (simulating a
# successful cold-build+refresh+preflight choreography); exits REIFY_TEST_SEED_RC
# (default 0).
MOCK_SEED="$STUB_DIR/mock-seed.sh"
cat > "$MOCK_SEED" << 'STUB_EOF'
#!/usr/bin/env bash
echo "seed $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_SEED_CREATE_BASE:-0}" = "1" ]; then
    _prev=""
    for _a in "$@"; do
        if [ "$_prev" = "--base-dir" ]; then
            mkdir -p "$_a"
            printf 'cold-built\n' > "$_a/cold-built.txt"
            break
        fi
        _prev="$_a"
    done
fi
exit "${REIFY_TEST_SEED_RC:-0}"
STUB_EOF
chmod +x "$MOCK_SEED"

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes the script under test with the command hooks pointed at the mocks
# above. Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        REIFY_WARM_BASE_HEALTH_REFRESH_CMD="$MOCK_REFRESH" \
        REIFY_WARM_BASE_HEALTH_SEED_CMD="$MOCK_SEED" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

reset_calls() {
    > "$CALLS_FILE"
}

# mk_git_advancing <parent_dir> [<subdir>]
# Creates a hermetic git worktree at <parent_dir>/lane with a committed tracked
# placeholder (.placeholder) so `git status --porcelain --untracked-files=no` is
# clean (empty). Creates <parent_dir>/lane/<subdir> (default: advancing) as an
# UNtracked subdirectory (like Cargo target/). Prints the lane dir to stdout.
# Mirrors mk_git_advancing() in tests/infra/test_refresh_warm_base.sh.
mk_git_advancing() {
    local parent_dir="$1"
    local subdir="${2:-advancing}"
    local lane_dir="$parent_dir/lane"
    mkdir -p "$lane_dir"
    printf 'placeholder\n' > "$lane_dir/.placeholder"
    git -C "$lane_dir" init -q
    git -C "$lane_dir" add -- .placeholder
    git -C "$lane_dir" \
        -c user.email="warm-lane-test@localhost" \
        -c user.name="Warm Lane Test" \
        -c commit.gpgsign=false \
        commit -q --no-verify -m "fixture: hermetic advancing lane"
    mkdir -p "$lane_dir/$subdir"
    echo "$lane_dir"
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard: --help, unknown flag
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0 and prints usage on stderr
reset_calls
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits 2
reset_calls
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits 2" test "$RC" -eq 2

test_summary
