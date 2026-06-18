#!/usr/bin/env bash
# tests/infra/test_seed_warm_lane.sh
# Hermetic tests for scripts/seed-warm-lane.sh.
#
# PATH-stubs: cp/find/touch/git (record argv to CALLS_FILE).
# Env-driven stub behaviour:
#   REIFY_TEST_REFLINK_OK    — cp stub: "1" → exit 0; else print error + exit 1
#   REIFY_TEST_GIT_DIFF_FILES — git stub: emitted as output of diff --name-only
#   REIFY_TEST_GIT_HEAD      — git stub: emitted as output of rev-parse HEAD
#
# run_helper captures STDOUT and STDERR SEPARATELY:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard (step-1 / step-2)
#   B — RUSTFLAGS guard / B5 (step-3 / step-4)
#   C — reflink clone + fail-closed / S2 (step-5 / step-6)
#   D — fresh-checkout mtime / D5 (step-7 / step-8)
#   E — reset-in-place / no bulk stamp (step-9 / step-10)
#   F — invocation fingerprint guard / S1 (step-11 / step-12)
#   G — --record-base writer (step-13 / step-14)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/seed-warm-lane.sh hermetic tests (task 4660) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-seed-warm-lane-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-seed-warm-lane-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-seed-warm-lane-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ────────────────────────────────────────────────────────────────

# cp stub: record argv; REIFY_TEST_REFLINK_OK=1 → exit 0, else error + exit 1
cat > "$STUB_DIR/cp" << 'STUB_EOF'
#!/usr/bin/env bash
echo "cp $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    # When REIFY_TEST_CP_CREATE_DEST=1, physically create the destination dir+file
    # so that mtime tests can assert on target/ contents.
    if [ "${REIFY_TEST_CP_CREATE_DEST:-}" = "1" ]; then
        dest="${*: -1}"
        mkdir -p "$dest/debug"
        echo "artifact" > "$dest/debug/artifact.a"
    fi
    exit 0
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# find stub: record argv, exit 0 (no-op; Block D uses real find)
cat > "$STUB_DIR/find" << 'STUB_EOF'
#!/usr/bin/env bash
echo "find $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/find"

# touch stub: record argv, exit 0 (no-op; Block D uses real touch)
cat > "$STUB_DIR/touch" << 'STUB_EOF'
#!/usr/bin/env bash
echo "touch $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/touch"

# git stub: record argv; controlled diff/rev-parse output via env vars
cat > "$STUB_DIR/git" << 'STUB_EOF'
#!/usr/bin/env bash
echo "git $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
# Detect diff --name-only and emit controlled file list
for arg in "$@"; do
    if [ "$arg" = "--name-only" ]; then
        if [ -n "${REIFY_TEST_GIT_DIFF_FILES:-}" ]; then
            printf "%s\n" "${REIFY_TEST_GIT_DIFF_FILES}"
        fi
        exit 0
    fi
done
# Detect rev-parse HEAD and emit controlled sha
for arg in "$@"; do
    if [ "$arg" = "rev-parse" ]; then
        echo "${REIFY_TEST_GIT_HEAD:-abc1234}"
        exit 0
    fi
done
exit 0
STUB_EOF
chmod +x "$STUB_DIR/git"

# ── run_helper ────────────────────────────────────────────────────────────────
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

# run_helper_real: like run_helper but without stubbing find/touch — for Block D
# which asserts actual mtime changes on a real fixture tree.
run_helper_real() {
    local rc=0
    > "$ERR_FILE"
    # Only stub cp and git; let find/touch be real binaries
    local real_stub_dir
    real_stub_dir="$(mktemp -d /tmp/test-seed-real-stub-XXXXXX)"
    # cp stub that physically copies src to dest (no --reflink needed for tests)
    cat > "$real_stub_dir/cp" << 'REAL_STUB_EOF'
#!/usr/bin/env bash
echo "cp $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    # Physically copy src→dest using plain cp -a (test environment is non-XFS)
    # Parse out: cp -a --reflink=always <src> <dest>
    src=""
    dest=""
    for arg in "$@"; do
        case "$arg" in
            -a|--reflink=always) ;;
            -*) ;;
            *) [ -z "$src" ] && src="$arg" || dest="$arg" ;;
        esac
    done
    if [ -n "$src" ] && [ -n "$dest" ]; then
        /bin/cp -a "$src" "$dest"
    fi
    exit 0
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
REAL_STUB_EOF
    chmod +x "$real_stub_dir/cp"
    cp "$STUB_DIR/git" "$real_stub_dir/git"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        PATH="$real_stub_dir:$PATH" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
    rm -rf "$real_stub_dir"
}

reset_calls() {
    > "$CALLS_FILE"
}

# ─────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0 with usage on stderr
reset_calls
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits non-zero
reset_calls
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits non-zero" test "$RC" -ne 0

# A3: missing positional args (only mode flag, no base/lane dirs) exits non-zero
reset_calls
run_helper --fresh-checkout
assert "A3: missing positional args exits non-zero" test "$RC" -ne 0

# A4: neither --fresh-checkout nor --reset-in-place exits non-zero
reset_calls
A_BASE="$(mktemp -d /tmp/test-seed-A-base-XXXXXX)"
A_LANE="$(mktemp -d /tmp/test-seed-A-lane-XXXXXX)"
_TMPDIRS+=("$A_BASE" "$A_LANE")
run_helper "$A_BASE" "$A_LANE"
assert "A4: neither mode flag exits non-zero" test "$RC" -ne 0

# A5: both --fresh-checkout and --reset-in-place exits non-zero
reset_calls
run_helper "$A_BASE" "$A_LANE" --fresh-checkout --reset-in-place
assert "A5: both mode flags exits non-zero" test "$RC" -ne 0

test_summary
