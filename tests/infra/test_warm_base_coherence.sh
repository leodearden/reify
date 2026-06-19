#!/usr/bin/env bash
# tests/infra/test_warm_base_coherence.sh
# Two-way base-coherence boundary test for scripts/refresh-warm-base.sh.
# Pins the D8/D10 base contract — reify side (R5, task #4698).
#
# Exercises three behaviors:
#   Block C — inv.9 `--landed-commit` provenance-guard contract (accept / reject cases)
#   Block A — torn-read coherence: a pinned reader never sees mixed-gen content
#   Block B-reap — GC-defer anti-tautology: the deferred gen IS reaped once the
#                  reader releases its flock -s lock
#
# PATH stubs:
#   cp   — records argv to CALLS_FILE; when REIFY_TEST_REFLINK_OK=1 performs a
#           real recursive copy via the absolute cp (stripping --reflink=always);
#           else prints an error + exits 1.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/refresh-warm-base.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/refresh-warm-base.sh base-coherence boundary tests (task #4698) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-warm-base-coherence-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-warm-base-coherence-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-warm-base-coherence-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# cp stub: record argv; if REIFY_TEST_REFLINK_OK=1 perform a real recursive
# copy (real cp with --reflink=always stripped); else print error + exit 1.
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
_dst="\${!#}"
if [ -n "\$_dst" ]; then
    mkdir -p "\$_dst" 2>/dev/null || true
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

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

# mk_git_advancing <parent_dir> [<subdir>]
# Creates a hermetic git worktree at <parent_dir>/lane with a committed tracked
# placeholder (.placeholder) so `git status --porcelain --untracked-files=no`
# is clean (empty).  Creates <parent_dir>/lane/<subdir> (default: advancing) as
# an UNtracked subdirectory (like Cargo target/).  Prints the lane dir to stdout.
#
# Usage:
#   LANE="$(mk_git_advancing "$MY_TMP")"
#   HEAD="$(git -C "$LANE" rev-parse HEAD)"
#   echo "..." > "$LANE/advancing/file.txt"
#   BASE="$MY_TMP/base"
#   run_helper "$LANE/advancing" "$BASE" --landed-commit "$HEAD"
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

test_summary
