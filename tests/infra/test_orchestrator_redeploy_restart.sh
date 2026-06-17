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
#       (env overrides ORCH_RESTART_DELAY, ORCH_UNIT, ORCH_TRANSIENT_UNIT
#        are asserted in this block)
#   D — EXEC-MODE CLEAN: stop THEN start, never restart
#   E — EXEC-MODE DIRTY: neither stop nor start, exits 0
#   F — NON-GIT PROJECT ROOT: git error is NOT treated as clean; aborts non-zero
#   G — SCHEDULE-MODE SYSTEMD-RUN FAILURE: exits non-zero, no false confirmation
#   H — EXEC-MODE START FAILURE: stop attempted, exits non-zero, no false success
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
# Stub: record argv to ORCH_TEST_CALLS_FILE.
# To simulate a specific subcommand failing, set:
#   ORCH_TEST_SYSTEMCTL_FAIL_SUBCMD=<subcommand>  (e.g. "start")
# That subcommand returns 1; all others return 0.
echo "systemctl $*" >> "${ORCH_TEST_CALLS_FILE:-/dev/null}"
if [ -n "${ORCH_TEST_SYSTEMCTL_FAIL_SUBCMD:-}" ]; then
    for _arg in "$@"; do
        [ "$_arg" = "$ORCH_TEST_SYSTEMCTL_FAIL_SUBCMD" ] && exit 1
    done
fi
exit 0
STUB_EOF
chmod +x "$STUB_DIR/systemctl"

cat > "$STUB_DIR/systemd-run" << 'STUB_EOF'
#!/usr/bin/env bash
# Stub: record argv to ORCH_TEST_CALLS_FILE.
# To simulate failure, set ORCH_TEST_SYSTEMD_RUN_RC to a non-zero exit code.
echo "systemd-run $*" >> "${ORCH_TEST_CALLS_FILE:-/dev/null}"
exit "${ORCH_TEST_SYSTEMD_RUN_RC:-0}"
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

# ──────────────────────────────────────────────────────────────────────────────
# Block D — EXEC-MODE CLEAN (--exec-restart, clean project_root)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: exec-mode clean (stop then start, never restart) ---"

CLEAN_REPO_D=""
make_clean_repo CLEAN_REPO_D
reset_calls

ORCH_PROJECT_ROOT="$CLEAN_REPO_D" \
ORCH_UNIT="exec-unit.service" \
    run_helper --exec-restart
assert "D1: exec-mode clean -> exits 0" test "$RC" -eq 0

# Must have systemctl --user stop <unit>
assert "D2: exec-mode records systemctl --user stop exec-unit.service" \
    bash -c 'grep -q "^systemctl --user stop exec-unit.service$" "$1"' _ "$CALLS_FILE"

# Must have systemctl --user start <unit>
assert "D3: exec-mode records systemctl --user start exec-unit.service" \
    bash -c 'grep -q "^systemctl --user start exec-unit.service$" "$1"' _ "$CALLS_FILE"

# stop must come BEFORE start
assert "D4: stop precedes start in exec-mode (ordering guarantee)" \
    bash -c '
        stop_ln=$(grep -n "^systemctl --user stop exec-unit.service$" "$1" | head -1 | cut -d: -f1)
        start_ln=$(grep -n "^systemctl --user start exec-unit.service$" "$1" | head -1 | cut -d: -f1)
        [ -n "$stop_ln" ] && [ -n "$start_ln" ] && [ "$stop_ln" -lt "$start_ln" ]
    ' _ "$CALLS_FILE"

# NO systemctl restart subcommand must appear
assert "D5: exec-mode NEVER uses systemctl restart subcommand" \
    bash -c '! grep -q "^systemctl.*restart" "$1"' _ "$CALLS_FILE"

# NO systemd-run call in exec-mode
assert "D6: exec-mode emits no systemd-run call" \
    bash -c '! grep -q "^systemd-run" "$1"' _ "$CALLS_FILE"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — EXEC-MODE DIRTY (--exec-restart, dirty project_root)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: exec-mode dirty (no stop/start, exits 0, orchestrator left running) ---"

DIRTY_REPO_E=""
make_dirty_repo DIRTY_REPO_E
reset_calls

ORCH_PROJECT_ROOT="$DIRTY_REPO_E" \
ORCH_UNIT="exec-unit.service" \
    run_helper --exec-restart
assert "E1: exec-mode dirty -> exits 0 (leave orchestrator running)" test "$RC" -eq 0

# Must NOT record any stop or start for the service unit
assert "E2: exec-mode dirty records NO systemctl stop" \
    bash -c '! grep -q "^systemctl.*stop exec-unit.service" "$1"' _ "$CALLS_FILE"
assert "E3: exec-mode dirty records NO systemctl start" \
    bash -c '! grep -q "^systemctl.*start exec-unit.service" "$1"' _ "$CALLS_FILE"

# Must log a "dirty, skipping" or similar message
assert "E4: exec-mode dirty logs a warning about dirty project_root" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "dirty|skipping"' _ "$OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block F — INVALID PROJECT ROOT (non-git directory)
# is_clean() must NOT treat a git error as "clean" — a misconfigured
# ORCH_PROJECT_ROOT (non-existent or not a git repo) must hard-abort, not
# silently pass the clean-guard and schedule/exec a restart.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: non-git project_root (git error treated as abort, not clean) ---"

NON_GIT_DIR="$(mktemp -d /tmp/test-orch-restart-nongit-XXXXXX)"
_TMPDIRS+=("$NON_GIT_DIR")
# NON_GIT_DIR is a plain temp directory with no git repo

# F1-F3: schedule mode with non-git project_root
reset_calls
ORCH_PROJECT_ROOT="$NON_GIT_DIR" \
    run_helper
assert "F1: non-git project_root in schedule mode -> exits non-zero" test "$RC" -ne 0
assert "F2: non-git project_root emits error about git/project_root (not 'commit/land')" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "ERROR.*project_root|git.*failed"' _ "$OUT"
assert "F3: non-git project_root in schedule mode schedules NO systemd-run" \
    bash -c '! grep -q "^systemd-run" "$1"' _ "$CALLS_FILE"

# F4-F6: exec mode with non-git project_root
reset_calls
ORCH_PROJECT_ROOT="$NON_GIT_DIR" \
    run_helper --exec-restart
assert "F4: non-git project_root in exec mode -> exits non-zero" test "$RC" -ne 0
assert "F5: non-git project_root in exec mode emits error about git/project_root" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "ERROR.*project_root|git.*failed"' _ "$OUT"
assert "F6: non-git project_root in exec mode does NOT stop or start the service" \
    bash -c '! grep -q "^systemctl" "$1"' _ "$CALLS_FILE"

# ──────────────────────────────────────────────────────────────────────────────
# Block G — SCHEDULE-MODE SYSTEMD-RUN FAILURE
# If systemd-run returns non-zero, the script must exit non-zero and NOT print
# a false "scheduled restart" confirmation.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: schedule-mode systemd-run failure ---"

CLEAN_REPO_G=""
make_clean_repo CLEAN_REPO_G
reset_calls

ORCH_PROJECT_ROOT="$CLEAN_REPO_G" \
ORCH_TEST_SYSTEMD_RUN_RC="1" \
    run_helper
assert "G1: systemd-run failure in schedule mode -> exits non-zero" test "$RC" -ne 0
assert "G2: systemd-run failure emits error message" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "error|fail"' _ "$OUT"
assert "G3: no false 'scheduled restart' confirmation on systemd-run failure" \
    bash -c '! printf "%s\n" "$1" | grep -qi "scheduled restart"' _ "$OUT"
assert "G4: systemd-run WAS called (failure was from its return code, not a pre-guard)" \
    bash -c 'grep -q "^systemd-run" "$1"' _ "$CALLS_FILE"

# ──────────────────────────────────────────────────────────────────────────────
# Block H — EXEC-MODE START FAILURE
# If systemctl start returns non-zero, the script must exit non-zero with an
# error message; the stop must still have been attempted before the failure.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: exec-mode start failure ---"

CLEAN_REPO_H=""
make_clean_repo CLEAN_REPO_H
reset_calls

ORCH_PROJECT_ROOT="$CLEAN_REPO_H" \
ORCH_UNIT="fail-start-unit.service" \
ORCH_TEST_SYSTEMCTL_FAIL_SUBCMD="start" \
    run_helper --exec-restart
assert "H1: exec-mode start failure -> exits non-zero" test "$RC" -ne 0
assert "H2: exec-mode start failure emits error message about start" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "ERROR.*start.*fail|start.*failed"' _ "$OUT"
assert "H3: exec-mode stop was still attempted before start failure" \
    bash -c 'grep -q "^systemctl --user stop fail-start-unit.service$" "$1"' _ "$CALLS_FILE"
assert "H4: exec-mode start was attempted (stop failure does not short-circuit it)" \
    bash -c 'grep -q "^systemctl --user start fail-start-unit.service$" "$1"' _ "$CALLS_FILE"
assert "H5: no false 'restarted successfully' confirmation on start failure" \
    bash -c '! printf "%s\n" "$1" | grep -qi "restarted successfully"' _ "$OUT"

test_summary
