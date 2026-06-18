#!/usr/bin/env bash
# tests/infra/test_warm_lane_preflight.sh
# Hermetic tests for scripts/warm-lane-preflight.sh.
#
# PATH stubs:
#   mountpoint — exit 0 when REIFY_TEST_MOUNTED=1; else exit 1
#   cp         — reflink probe: exit 0 when REIFY_TEST_REFLINK_OK=1; else error+exit 1
#   Both record argv to CALLS_FILE.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag
#   B — all-pass happy path: all 5 checks pass, exit 0
#   C — fail-closed failure modes: each failing check exits non-zero with
#         actionable stderr naming the remediation script
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-preflight.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-preflight.sh hermetic tests (task 4661) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-warm-lane-preflight-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-warm-lane-preflight-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-warm-lane-preflight-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# mountpoint stub: exit 0 when REIFY_TEST_MOUNTED=1; else exit 1
cat > "$STUB_DIR/mountpoint" << 'STUB_EOF'
#!/usr/bin/env bash
echo "mountpoint $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
[ "${REIFY_TEST_MOUNTED:-}" = "1" ] && exit 0
exit 1
STUB_EOF
chmod +x "$STUB_DIR/mountpoint"

# cp stub: reflink probe exits 0 when REIFY_TEST_REFLINK_OK=1; else error+exit 1
cat > "$STUB_DIR/cp" << 'STUB_EOF'
#!/usr/bin/env bash
echo "cp $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    exit 0
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
# Block B — all-pass happy path: all 5 checks pass, exit 0
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: all-pass happy path ---"

B_TMP="$(mktemp -d /tmp/test-warm-lane-preflight-b-XXXXXX)"
_TMPDIRS+=("$B_TMP")

# Build a tmp mount dir + base dir (non-empty) + stamp files
B_MNT="$B_TMP/mount"
B_BASE="$B_MNT/base/target"
mkdir -p "$B_BASE"
echo "some content" > "$B_BASE/rustc"

# Write matching stamps
printf '%s' "-C target-cpu=native" > "$B_MNT/base/target.rustflags"
printf '%s' "sha256:cafebabe" > "$B_MNT/base/target.invocation"

# B1: all-pass happy path exits 0
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    run_helper --mount "$B_MNT" --base-dir "$B_BASE" --invocation "sha256:cafebabe"
assert "B1: all-pass exits 0" test "$RC" -eq 0

# B2: cp --reflink=always probe was run (check #2 — reflink-capable)
assert "B2: cp --reflink=always probe ran" \
    bash -c 'grep "^cp " "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# B3: mountpoint was checked (check #1 — volume mounted)
assert "B3: mountpoint was checked" \
    bash -c 'grep "^mountpoint " "$1" | grep -qF "'"$B_MNT"'"' _ "$CALLS_FILE"

# B4: stdout is empty (all diagnostics on stderr)
assert "B4: stdout is empty (diagnostics on stderr)" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# B5: stderr is non-empty (progress diagnostics)
assert "B5: stderr is non-empty (preflight progress)" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# B6: env var defaults (REIFY_WARM_LANE_MOUNT, REIFY_WARM_LANE_BASE, REIFY_WARM_LANE_INVOCATION)
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    REIFY_WARM_LANE_MOUNT="$B_MNT" \
    REIFY_WARM_LANE_BASE="$B_BASE" \
    REIFY_WARM_LANE_INVOCATION="sha256:cafebabe" \
    run_helper
assert "B6: env-var defaults path exits 0" test "$RC" -eq 0

test_summary
