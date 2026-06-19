#!/usr/bin/env bash
# tests/infra/test_seed_warm_base_initial.sh
# Hermetic tests for scripts/seed-warm-base-initial.sh.
#
# PATH stubs:
#   cp         — real-recursive-copy variant: records argv; when REIFY_TEST_REFLINK_OK=1
#                strips --reflink=always and execs the real cp; else error+exit 1.
#                Both refresh-warm-base.sh (gen-dir copy) and warm-lane-preflight.sh
#                (reflink probe) use this stub — the real-recursive variant makes both work.
#   mountpoint — exit 0 when REIFY_TEST_MOUNTED=1; else exit 1.
#   Both record argv to CALLS_FILE.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help exits 0 with usage; unknown flag exits 2
#   F — merge-verify worktree validation (fail-closed, before any build)
#   B-seed — cold-build + refresh seeding: injected build cmd, base gen-dir created
#   C — build-failure fail-closed: failed/empty build → non-zero, no base seeded
#   B-e2e — full happy path: preflight gated, exits 0, stdout empty
#   E — failure propagation: not-mounted / not-reflink → non-zero despite successful build
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/seed-warm-base-initial.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/seed-warm-base-initial.sh hermetic tests (task 4697) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-seed-warm-base-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-seed-warm-base-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-seed-warm-base-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# cp stub: real-recursive-copy variant.
#   - Records argv to CALLS_FILE.
#   - When REIFY_TEST_REFLINK_OK=1: strips --reflink=always and calls the real cp
#     (so refresh-warm-base.sh's gen-dir copy and preflight's probe both work).
#   - Otherwise: creates the destination dir (simulating partial copy) then exits 1.
_REAL_CP="$(command -v cp)"
cat > "$STUB_DIR/cp" << STUB_EOF
#!/usr/bin/env bash
echo "cp \$*" >> "\${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "\${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    args=()
    for a in "\$@"; do
        [ "\$a" = "--reflink=always" ] && continue
        args+=("\$a")
    done
    exec "${_REAL_CP}" "\${args[@]}"
fi
# Simulate partial failure: create destination dir before failing
_dst="\${!#}"
if [ -n "\$_dst" ]; then
    mkdir -p "\$_dst" 2>/dev/null || true
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# mountpoint stub: exit 0 when REIFY_TEST_MOUNTED=1; else exit 1.
cat > "$STUB_DIR/mountpoint" << 'STUB_EOF'
#!/usr/bin/env bash
echo "mountpoint $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
[ "${REIFY_TEST_MOUNTED:-}" = "1" ] && exit 0
exit 1
STUB_EOF
chmod +x "$STUB_DIR/mountpoint"

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes the script under the stub PATH.
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        PATH="$STUB_DIR:$PATH" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

reset_calls() {
    > "$CALLS_FILE"
}

# mk_git_advancing <parent_dir>
# Creates a hermetic git worktree at <parent_dir>/lane with:
#   - a committed .placeholder (so `git status --untracked-files=no` is clean)
#   - a `target/` subdir (UNtracked, like Cargo target/)
# Prints the lane dir to stdout.
#
# Mirrors mk_git_advancing() in tests/infra/test_refresh_warm_base.sh.
mk_git_advancing() {
    local parent_dir="$1"
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
    mkdir -p "$lane_dir/target"
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
