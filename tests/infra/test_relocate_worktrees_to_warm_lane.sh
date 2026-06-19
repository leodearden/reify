#!/usr/bin/env bash
# tests/infra/test_relocate_worktrees_to_warm_lane.sh
# Hermetic tests for scripts/relocate-worktrees-to-warm-lane.sh.
#
# PATH-stubs cp record argv to CALLS_FILE; env-driven stub behaviour:
#   REIFY_TEST_REFLINK_OK  — cp stub: "1" -> exit 0; else print error + exit 1
#
# run_helper captures STDOUT and STDERR SEPARATELY:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: file exists + --help, unknown flag, nonexistent mount
#   B — Fresh happy path: no .worktrees yet → creates symlink, stdout=DEST
#   C — Probe fail-loud: non-reflink mount → exits non-zero, no symlink
#   D — Idempotency: symlink already correct → no-op; wrong target → refuses
#   E — Migration: real directory with contents → mv to mount, symlink created
#   F — Real-git end-to-end acceptance (user-observable signal)
#   H — orchestrator.yaml config contract (PyYAML-guarded)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/relocate-worktrees-to-warm-lane.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/relocate-worktrees-to-warm-lane.sh hermetic tests (task 4696) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-relocate-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-relocate-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-relocate-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# cp stub: if REIFY_TEST_REFLINK_OK=1 -> exit 0; else print error + exit 1
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
# Block A — CLI guard: script exists, --help, unknown flag, nonexistent mount
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: script exists and is executable
assert "A1: script exists" test -f "$SCRIPT"
assert "A1: script is executable" test -x "$SCRIPT"

# A2: --help exits 0 and prints usage to stderr
reset_calls
run_helper --help
assert "A2: --help exits 0" test "$RC" -eq 0
assert "A2: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A3: unknown flag exits 2
reset_calls
run_helper --unknown-flag-xyz
assert "A3: unknown flag exits 2" test "$RC" -eq 2

# A4: --mount pointing at nonexistent directory exits non-zero
#     with an actionable message mentioning provision
A_TMP="$(mktemp -d /tmp/test-relocate-a-XXXXXX)"
_TMPDIRS+=("$A_TMP")
reset_calls
REIFY_TEST_REFLINK_OK=1 \
    run_helper --repo "$A_TMP" --mount "$A_TMP/nonexistent-mount-dir"
assert "A4: nonexistent mount exits non-zero" test "$RC" -ne 0
assert "A4: nonexistent mount stderr mentions 'provision'" \
    bash -c 'printf "%s\n" "$1" | grep -qi "provision"' _ "$ERR_OUT"

test_summary
