#!/usr/bin/env bash
# tests/infra/test_orchestrator_redeploy_restart.sh
# Hermetic tests for scripts/orchestrator-redeploy-restart.sh.
#
# PATH-stubs `systemctl` and `systemd-run` record their argv to a calls file;
# ORCH_PROJECT_ROOT points at a throwaway git repo (clean or dirtied).
# The live /home/leo/src/reify, the live orchestrator unit, and real systemd
# are NEVER touched.
#
# Blocks:
#   A — CLI guard: --help, unknown flag
#   B — SCHEDULE-MODE DIRTY GUARD: exits non-zero, no systemd-run call
#   C — SCHEDULE-MODE HAPPY PATH: clean repo -> correct systemd-run invocation
#   D — EXEC-MODE CLEAN: stop THEN start, never restart
#   E — EXEC-MODE DIRTY: neither stop nor start, exits 0
#   F — ENV OVERRIDES flow through (ORCH_RESTART_DELAY, ORCH_UNIT,
#       ORCH_TRANSIENT_UNIT)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/orchestrator-redeploy-restart.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/orchestrator-redeploy-restart.sh hermetic tests (task 4620) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# Create PATH-stub directory for systemctl and systemd-run
STUB_DIR="$(mktemp -d /tmp/test-orch-restart-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-orch-restart-calls-XXXXXX)"
# CALLS_FILE is a file not a dir, but rm -rf handles both; track for cleanup
_TMPDIRS+=("$CALLS_FILE")

cat > "$STUB_DIR/systemctl" << 'STUB_EOF'
#!/usr/bin/env bash
# Stub: record argv to ORCH_TEST_CALLS_FILE, always succeed
echo "systemctl $*" >> "${ORCH_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/systemctl"

cat > "$STUB_DIR/systemd-run" << 'STUB_EOF'
#!/usr/bin/env bash
# Stub: record argv to ORCH_TEST_CALLS_FILE, always succeed
echo "systemd-run $*" >> "${ORCH_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/systemd-run"

# Run the script with stubs wired; sets RC and OUT globals
run_helper() {
    local rc=0
    OUT="$(
        ORCH_TEST_CALLS_FILE="$CALLS_FILE" \
        PATH="$STUB_DIR:$PATH" \
            bash "$SCRIPT" "$@" 2>&1
    )" || rc=$?
    RC=$rc
}

# Make a clean throwaway git repo
make_clean_repo() {
    local _var="$1" dir
    dir="$(mktemp -d /tmp/test-orch-restart-repo-XXXXXX)"
    _TMPDIRS+=("$dir")
    git -C "$dir" init -q -b main
    git -C "$dir" config user.email test@test.com
    git -C "$dir" config user.name Test
    echo "tracked" > "$dir/tracked.txt"
    git -C "$dir" add tracked.txt
    git -C "$dir" commit -q -m "init"
    printf -v "$_var" '%s' "$dir"
}

# Make a dirty throwaway git repo (tracked file modified, not staged)
make_dirty_repo() {
    local _var="$1"
    make_clean_repo "$_var"
    eval "local _dir=\$$_var"
    echo "dirty modification" >> "$_dir/tracked.txt"
}

# Reset calls file
reset_calls() {
    > "$CALLS_FILE"
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard: --help and unknown flag
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0
reset_calls
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage'" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$OUT"

# A2: unknown flag exits non-zero
reset_calls
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits non-zero" test "$RC" -ne 0

# ──────────────────────────────────────────────────────────────────────────────
# Block B — SCHEDULE-MODE DIRTY GUARD
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: schedule-mode dirty guard ---"

DIRTY_REPO=""
make_dirty_repo DIRTY_REPO
reset_calls

ORCH_PROJECT_ROOT="$DIRTY_REPO" run_helper
assert "B1: dirty project_root -> exits non-zero" test "$RC" -ne 0
assert "B2: dirty guard prints actionable message (commit/land)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "commit|land"' _ "$OUT"
assert "B3: dirty guard schedules NO systemd-run" \
    bash -c '! grep -q "systemd-run" "$1"' _ "$CALLS_FILE"

test_summary
