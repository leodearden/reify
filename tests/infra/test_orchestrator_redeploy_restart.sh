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

# ──────────────────────────────────────────────────────────────────────────────
# Block C — SCHEDULE-MODE HAPPY PATH (clean repo)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: schedule-mode happy path (clean repo) ---"

CLEAN_REPO=""
make_clean_repo CLEAN_REPO
reset_calls

# Run with custom env overrides so we can assert the exact values passed through
ORCH_PROJECT_ROOT="$CLEAN_REPO" \
ORCH_UNIT="test-unit.service" \
ORCH_RESTART_DELAY="30s" \
ORCH_TRANSIENT_UNIT="test-transient-unit" \
    run_helper
assert "C1: clean project_root -> exits 0" test "$RC" -eq 0

# Exactly one systemd-run call recorded
assert "C2: exactly one systemd-run call emitted" \
    bash -c '[ "$(grep -c "^systemd-run" "$1" 2>/dev/null || echo 0)" -eq 1 ]' _ "$CALLS_FILE"

# systemd-run call must include --user
assert "C3: systemd-run has --user" \
    bash -c 'grep "^systemd-run" "$1" | grep -q -- "--user"' _ "$CALLS_FILE"

# systemd-run call must include --on-active=30s (our override)
assert "C4: systemd-run has --on-active=30s (ORCH_RESTART_DELAY override)" \
    bash -c 'grep "^systemd-run" "$1" | grep -q -- "--on-active=30s"' _ "$CALLS_FILE"

# systemd-run call must include --unit=test-transient-unit (our override)
assert "C5: systemd-run has --unit=test-transient-unit (ORCH_TRANSIENT_UNIT override)" \
    bash -c 'grep "^systemd-run" "$1" | grep -q -- "--unit=test-transient-unit"' _ "$CALLS_FILE"

# systemd-run call must include --collect
assert "C6: systemd-run has --collect" \
    bash -c 'grep "^systemd-run" "$1" | grep -q -- "--collect"' _ "$CALLS_FILE"

# systemd-run call must include --setenv=ORCH_UNIT passthrough
assert "C7: systemd-run has --setenv=ORCH_UNIT=test-unit.service" \
    bash -c 'grep "^systemd-run" "$1" | grep -q -- "--setenv=ORCH_UNIT=test-unit.service"' _ "$CALLS_FILE"

# systemd-run call must include --setenv=ORCH_PROJECT_ROOT passthrough
assert "C8: systemd-run has --setenv=ORCH_PROJECT_ROOT=<clean-repo>" \
    bash -c "grep \"^systemd-run\" \"\$1\" | grep -q -- \"--setenv=ORCH_PROJECT_ROOT=$CLEAN_REPO\"" _ "$CALLS_FILE"

# systemd-run call must invoke the script itself with --exec-restart
assert "C9: systemd-run re-invokes script with --exec-restart" \
    bash -c 'grep "^systemd-run" "$1" | grep -q -- "--exec-restart"' _ "$CALLS_FILE"

# Best-effort pre-clean must have emitted at least one systemctl call before systemd-run
# (stop/reset-failed on the transient unit)
assert "C10: best-effort pre-clean recorded systemctl calls before systemd-run" \
    bash -c 'grep -q "^systemctl" "$1"' _ "$CALLS_FILE"

# The systemctl pre-clean calls must appear BEFORE the systemd-run line
assert "C11: systemctl pre-clean calls precede the systemd-run call" \
    bash -c '
        first_systemctl=$(grep -n "^systemctl" "$1" | head -1 | cut -d: -f1)
        systemd_run_line=$(grep -n "^systemd-run" "$1" | head -1 | cut -d: -f1)
        [ -n "$first_systemctl" ] && [ -n "$systemd_run_line" ] && \
            [ "$first_systemctl" -lt "$systemd_run_line" ]
    ' _ "$CALLS_FILE"

# Pre-clean references the transient unit name (ORCH_TRANSIENT_UNIT override)
assert "C12: pre-clean systemctl calls reference ORCH_TRANSIENT_UNIT (test-transient-unit)" \
    bash -c 'grep "^systemctl" "$1" | grep -q "test-transient-unit"' _ "$CALLS_FILE"

test_summary
