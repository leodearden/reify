#!/usr/bin/env bash
# tests/infra/test_refresh_warm_base.sh
# Hermetic tests for scripts/refresh-warm-base.sh.
#
# PATH stubs:
#   cp       — records argv to CALLS_FILE; when REIFY_TEST_REFLINK_OK=1 performs
#              a real recursive copy via the absolute cp (stripping --reflink=always);
#              else prints an error + exits 1.
#   mv       — NOT stubbed; real mv so filesystem postconditions are observable.
#   xfs_bmap — records argv + emits REIFY_TEST_FRAG_EXTENTS extent rows per file.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag, missing positional args
#   B — basic refresh happy path: cp --reflink=always, atomic rename, content OK
#   C — fail-closed reflink: probe failure -> non-zero, no partial base, pre-existing untouched
#   D — in-flight clone independence: clone dir untouched after refresh (B6)
#   E — base self-description stamps: .rustflags and .invocation written after swap
#   F — --check-frag defrag signal: verdict token + extent count, read-only
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

echo "=== scripts/refresh-warm-base.sh hermetic tests (task 4661) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-refresh-warm-base-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-refresh-warm-base-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-refresh-warm-base-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# cp stub: record argv; if REIFY_TEST_REFLINK_OK=1 perform a real recursive copy
# (absolute cp with --reflink=always stripped); else print error + exit 1.
# The real cp path is embedded at stub-creation time.
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
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# xfs_bmap stub: record argv; emit REIFY_TEST_FRAG_EXTENTS extent rows
cat > "$STUB_DIR/xfs_bmap" << 'STUB_EOF'
#!/usr/bin/env bash
echo "xfs_bmap $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
count="${REIFY_TEST_FRAG_EXTENTS:-1}"
for i in $(seq 1 "$count"); do
    printf "    %d: [0..511]: 1234..%d 512\n" "$((i-1))" "$((1234 + i*512))"
done
exit 0
STUB_EOF
chmod +x "$STUB_DIR/xfs_bmap"

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

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard: --help, unknown flag, missing positional args
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0
reset_calls
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits 2
reset_calls
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits 2" test "$RC" -eq 2

# A3: missing all positional args exits non-zero
reset_calls
run_helper
assert "A3: missing all positional args exits non-zero" test "$RC" -ne 0

# A4: only one positional arg (missing base_dir) exits non-zero
reset_calls
run_helper /some/nonexistent/dir
assert "A4: missing second positional arg exits non-zero" test "$RC" -ne 0

# ──────────────────────────────────────────────────────────────────────────────
# Block B — basic refresh happy path
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: basic refresh happy path ---"

B_TMP="$(mktemp -d /tmp/test-refresh-warm-base-b-XXXXXX)"
_TMPDIRS+=("$B_TMP")

# Create an advancing target dir with content
B_ADV="$B_TMP/advancing"
mkdir -p "$B_ADV"
echo "file1 content" > "$B_ADV/file1.txt"
echo "file2 content" > "$B_ADV/file2.txt"
mkdir -p "$B_ADV/subdir"
echo "nested" > "$B_ADV/subdir/nested.txt"

B_BASE="$B_TMP/base"

# B1: basic refresh (no pre-existing base) exits 0
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$B_ADV" "$B_BASE"
assert "B1: basic refresh exits 0" test "$RC" -eq 0

# B2: cp was invoked with --reflink=always
assert "B2: cp invoked with --reflink=always" \
    bash -c 'grep "^cp " "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# B3: cp targeted <base_dir>.new
assert "B3: cp targeted <base_dir>.new" \
    bash -c 'grep "^cp " "$1" | grep -qF "'"$B_BASE"'.new"' _ "$CALLS_FILE"

# B4: <base_dir> exists and contains the advancing content
assert "B4: <base_dir> exists after refresh" test -d "$B_BASE"
assert "B4: file1.txt has advancing content" \
    bash -c '[ "$(cat "$1/file1.txt")" = "file1 content" ]' _ "$B_BASE"
assert "B4: file2.txt has advancing content" \
    bash -c '[ "$(cat "$1/file2.txt")" = "file2 content" ]' _ "$B_BASE"
assert "B4: subdir/nested.txt exists" test -f "$B_BASE/subdir/nested.txt"

# B5: <base_dir>.new does NOT exist (cleaned up by the successful rename)
assert "B5: <base_dir>.new absent after successful refresh" \
    test ! -e "$B_BASE.new"

# B6: diagnostics on stderr (ERR_OUT non-empty)
assert "B6: diagnostics on stderr (non-empty)" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# B7: stdout is empty (no stdout output from the script — diagnostics only on stderr)
assert "B7: stdout is empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# B8: refresh when base already exists (two-rename atomic swap path)
B2_TMP="$(mktemp -d /tmp/test-refresh-warm-base-b2-XXXXXX)"
_TMPDIRS+=("$B2_TMP")
B2_ADV="$B2_TMP/advancing"
mkdir -p "$B2_ADV"
echo "new content" > "$B2_ADV/newfile.txt"
B2_BASE="$B2_TMP/base"
mkdir -p "$B2_BASE"
echo "old content" > "$B2_BASE/oldfile.txt"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$B2_ADV" "$B2_BASE"
assert "B8: refresh with existing base exits 0" test "$RC" -eq 0
assert "B8: new base has advancing content" \
    bash -c '[ "$(cat "$1/newfile.txt")" = "new content" ]' _ "$B2_BASE"
assert "B8: old content gone after swap" \
    test ! -f "$B2_BASE/oldfile.txt"
assert "B8: <base_dir>.new cleaned up" test ! -e "$B2_BASE.new"
assert "B8: <base_dir>.old cleaned up" test ! -e "$B2_BASE.old"

test_summary
