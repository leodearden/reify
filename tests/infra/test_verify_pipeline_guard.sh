#!/usr/bin/env bash
# Infrastructure test for task 4626.
# Drift guard for scripts/verify-pipeline-guard.sh — verifies the classifier's
# decision contract (load-bearing vs fast-path-safe paths).
#
# Auto-discovered by tests/infra/run_all.sh AND auto-pulled into task-scope
# when verify.sh changes (matches the task-4523 'scripts/verify.sh ->
# tests/infra/test_verify_*.sh' row in scripts/verify-pipeline-infra-tests.txt).
#
# This test is hermetic: it drives the classifier script directly with no
# cargo or git operations.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== verify-pipeline-guard.sh classifier tests ==="

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

GUARD_SH="$REPO_ROOT/scripts/verify-pipeline-guard.sh"

# run_guard <subcommand> [args...] — invoke the classifier under test.
run_guard() {
    bash "$GUARD_SH" "$@"
}

# assert_exit DESC EXPECTED CMD [args...] — assert CMD exits EXPECTED_CODE.
# Increments the global PASS/FAIL counters from test_helpers.sh.
assert_exit() {
    local desc="$1" expected="$2"
    shift 2
    local actual=0
    "$@" >/dev/null 2>&1 || actual=$?
    if [ "$actual" -eq "$expected" ]; then
        echo "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc (expected exit $expected, got $actual)"
        FAIL=$((FAIL + 1))
    fi
}

# ---------------------------------------------------------------------------
# Pair A — core decision contract
# ---------------------------------------------------------------------------

echo ""
echo "-- Pair A: core decision contract --"

# POSITIVE: anchor — scripts/verify.sh is always load-bearing
assert_exit "POSITIVE: scripts/verify.sh is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate scripts/verify.sh

# POSITIVE: static manifest data deps
assert_exit "POSITIVE: .config/nextest.toml is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate .config/nextest.toml

assert_exit "POSITIVE: scripts/occt-touching-crates.txt is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate scripts/occt-touching-crates.txt

# NEGATIVE: fast-path preserved for genuine config-only paths (exit 1).
# These prove the guard stays surgical and does not break the config-only
# fast-path throughput benefit.
assert_exit "NEGATIVE: docs/note.md is fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate docs/note.md

assert_exit "NEGATIVE: orchestrator.yaml is fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate orchestrator.yaml

assert_exit "NEGATIVE: README.md is fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate README.md

# MIXED: the incident shape — any load-bearing file in the diff forces the full gate.
assert_exit "MIXED: docs/note.md + scripts/verify.sh -> full gate required (exit 0)" 0 \
    run_guard requires-full-gate docs/note.md scripts/verify.sh

# STDIN form: pipe paths to 'requires-full-gate' (no args) — supports large diffs
# that would exceed ARG_MAX if passed as positional args.
assert_exit "STDIN: load-bearing file piped in -> full gate required (exit 0)" 0 \
    bash -c 'printf "docs/x.md\nscripts/verify.sh\n" | bash "$1" requires-full-gate' \
    _ "$GUARD_SH"

# --list contract: output must include scripts/verify.sh (one path per line).
assert "--list output includes scripts/verify.sh" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/verify.sh"' \
    _ "$GUARD_SH"

# Usage error: unknown subcommand/flag -> exit 2.
assert_exit "usage: unknown flag --bogus exits 2" 2 \
    run_guard --bogus

# Usage error: no subcommand -> exit 2.
assert_exit "usage: no subcommand exits 2" 2 \
    run_guard

test_summary
