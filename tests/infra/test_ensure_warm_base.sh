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
    # rm -rf first: _dst may be a pre-existing dangling symlink (rung-2's
    # "dangling base" fixture), and mkdir -p would EEXIST on the symlink entry.
    rm -rf "$_dst" 2>/dev/null || true
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

# ──────────────────────────────────────────────────────────────────────────────
# Block B — Rung 1: base healthy -> no-op (idempotent)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: Rung 1 (base healthy -> no-op) ---"

B_TMP="$(mktemp -d /tmp/test-ensure-warm-base-b-XXXXXX)"
_TMPDIRS+=("$B_TMP")
B_MNT="$B_TMP/mount"
B_MV="$B_MNT/worktrees/_merge-verify"

# B1: plain non-empty base dir -> healthy, no-op
B1_BASE="$B_TMP/base-plain"
mkdir -p "$B1_BASE"
echo "healthy content" > "$B1_BASE/rustc"

reset_calls
run_helper --mount "$B_MNT" --base-dir "$B1_BASE" --merge-verify "$B_MV"
assert "B1: healthy plain base dir exits 0" test "$RC" -eq 0
assert "B1: refresh-cmd NOT invoked" bash -c '! grep -q "^refresh " "$1"' _ "$CALLS_FILE"
assert "B1: seed-cmd NOT invoked" bash -c '! grep -q "^seed " "$1"' _ "$CALLS_FILE"
assert "B1: zero escalation tokens on stdout" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c "REIFY_WARM_BASE_HEALTH_ESCALATION")" -eq 0 ]' _ "$OUT"

# B2: base is a symlink -> a non-empty gen dir (production shape after a real
# refresh-warm-base.sh swap) -> healthy, no-op (symlink-following predicate,
# matching warm-lane-preflight.sh Check 3 exactly).
B2_GEN="$B_TMP/base-gen.1"
mkdir -p "$B2_GEN"
echo "gen content" > "$B2_GEN/rustc"
B2_BASE="$B_TMP/base-symlink"
ln -sfn "$B2_GEN" "$B2_BASE"

reset_calls
run_helper --mount "$B_MNT" --base-dir "$B2_BASE" --merge-verify "$B_MV"
assert "B2: healthy symlinked (gen-dir) base exits 0" test "$RC" -eq 0
assert "B2: refresh-cmd NOT invoked (symlinked base)" bash -c '! grep -q "^refresh " "$1"' _ "$CALLS_FILE"
assert "B2: seed-cmd NOT invoked (symlinked base)" bash -c '! grep -q "^seed " "$1"' _ "$CALLS_FILE"
assert "B2: zero escalation tokens on stdout (symlinked base)" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c "REIFY_WARM_BASE_HEALTH_ESCALATION")" -eq 0 ]' _ "$OUT"

# B3: idempotence — two consecutive invocations on the same healthy base both
# exit 0 with no reseed/build/escalation.
reset_calls
run_helper --mount "$B_MNT" --base-dir "$B1_BASE" --merge-verify "$B_MV"
assert "B3: idempotence — first invocation exits 0" test "$RC" -eq 0
assert "B3: idempotence — first invocation zero escalation tokens" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c "REIFY_WARM_BASE_HEALTH_ESCALATION")" -eq 0 ]' _ "$OUT"
run_helper --mount "$B_MNT" --base-dir "$B1_BASE" --merge-verify "$B_MV"
assert "B3: idempotence — second invocation exits 0" test "$RC" -eq 0
assert "B3: idempotence — second invocation zero escalation tokens" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c "REIFY_WARM_BASE_HEALTH_ESCALATION")" -eq 0 ]' _ "$OUT"
assert "B3: idempotence — no refresh-cmd invoked across both runs" \
    bash -c '! grep -q "^refresh " "$1"' _ "$CALLS_FILE"
assert "B3: idempotence — no seed-cmd invoked across both runs" \
    bash -c '! grep -q "^seed " "$1"' _ "$CALLS_FILE"

# ──────────────────────────────────────────────────────────────────────────────
# Block C — Rung 2: dangling/absent base + surviving warm source -> reflink reseed
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: Rung 2 (reflink reseed from a surviving warm source) ---"

C_TMP="$(mktemp -d /tmp/test-ensure-warm-base-c-XXXXXX)"
_TMPDIRS+=("$C_TMP")
C_MNT="$C_TMP/mount"

# Hermetic git-worktree _merge-verify fixture (clean worktree) with a non-empty
# target/ subdir — the rung-2 warm source. mk_git_advancing's subdir param is
# "target" so the fixture matches <merge-verify>/target exactly.
C_MV="$(mk_git_advancing "$C_TMP" "target")"
C_HEAD="$(git -C "$C_MV" rev-parse HEAD)"
echo "warm content" > "$C_MV/target/rustc"

# C1: base absent (not yet created) -> rung-2 reseed success
C1_BASE="$C_TMP/base-absent"

reset_calls
REIFY_TEST_REFRESH_CREATE_BASE=1 run_helper --mount "$C_MNT" --base-dir "$C1_BASE" --merge-verify "$C_MV"
assert "C1: rung-2 reseed (absent base) exits 0" test "$RC" -eq 0
assert "C1: refresh-cmd invoked with argv --landed-commit <HEAD> <mv>/target <base>" \
    bash -c 'grep -qF -- "--landed-commit $1 $2/target $3" "$4"' \
    _ "$C_HEAD" "$C_MV" "$C1_BASE" "$CALLS_FILE"
assert "C1: seed-cmd NOT invoked" bash -c '! grep -q "^seed " "$1"' _ "$CALLS_FILE"
assert "C1: zero escalation tokens on stdout" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c "REIFY_WARM_BASE_HEALTH_ESCALATION")" -eq 0 ]' _ "$OUT"
assert "C1: base is healthy after reseed (mock created it)" \
    bash -c 'test -d "$1" && [ -n "$(ls -A "$1" 2>/dev/null)" ]' _ "$C1_BASE"

# C2: base is a dangling symlink -> rung-2 reseed success (same as absent)
C2_BASE="$C_TMP/base-dangling"
ln -sfn "$C_TMP/nonexistent-gen-dir" "$C2_BASE"

reset_calls
REIFY_TEST_REFRESH_CREATE_BASE=1 run_helper --mount "$C_MNT" --base-dir "$C2_BASE" --merge-verify "$C_MV"
assert "C2: rung-2 reseed (dangling symlink base) exits 0" test "$RC" -eq 0
assert "C2: refresh-cmd invoked (dangling base)" bash -c 'grep -q "^refresh " "$1"' _ "$CALLS_FILE"
assert "C2: seed-cmd NOT invoked (dangling base)" bash -c '! grep -q "^seed " "$1"' _ "$CALLS_FILE"
assert "C2: zero escalation tokens on stdout (dangling base)" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c "REIFY_WARM_BASE_HEALTH_ESCALATION")" -eq 0 ]' _ "$OUT"

# C3: refresh-cmd fails (non-zero exit) -> escalate + exit non-zero; seed-cmd NOT invoked
C3_BASE="$C_TMP/base-fail-nonzero"

reset_calls
REIFY_TEST_REFRESH_RC=1 run_helper --mount "$C_MNT" --base-dir "$C3_BASE" --merge-verify "$C_MV"
assert "C3: refresh-cmd failure (non-zero) exits non-zero" test "$RC" -ne 0
assert "C3: exactly ONE escalation token on stdout" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c "REIFY_WARM_BASE_HEALTH_ESCALATION")" -eq 1 ]' _ "$OUT"
assert "C3: seed-cmd NOT invoked" bash -c '! grep -q "^seed " "$1"' _ "$CALLS_FILE"

# C4: refresh-cmd exits 0 but leaves the base still absent -> escalate + exit non-zero
# (distinct arm from C3: the OR condition in "refresh failure OR base still
# unhealthy" must catch a refresh-cmd that silently no-ops too.)
C4_BASE="$C_TMP/base-fail-silent"

reset_calls
run_helper --mount "$C_MNT" --base-dir "$C4_BASE" --merge-verify "$C_MV"
assert "C4: refresh-cmd silent no-op (base still absent) exits non-zero" test "$RC" -ne 0
assert "C4: exactly ONE escalation token on stdout (silent no-op)" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c "REIFY_WARM_BASE_HEALTH_ESCALATION")" -eq 1 ]' _ "$OUT"
assert "C4: seed-cmd NOT invoked (silent no-op)" bash -c '! grep -q "^seed " "$1"' _ "$CALLS_FILE"

test_summary
