#!/usr/bin/env bash
# scripts/test_psi_gate.sh — integration tests for the PSI-gated dispatch in verify.sh.
#
# Drives `verify.sh psi-gate` in isolation with injected PSI fixtures and
# isolated dispatch files — no cargo/tree-sitter/npm builds.
#
# Skip guard: exits 0 (skip) on hosts without /proc/pressure/cpu.
# Fail-open (missing PSI source) is still exercised via PROC_PATH override.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VERIFY="$REPO_ROOT/scripts/verify.sh"

[ -f "$REPO_ROOT/tests/infra/test_helpers.sh" ] || {
    echo "ERROR: tests/infra/test_helpers.sh not found at $REPO_ROOT/tests/infra/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$REPO_ROOT/tests/infra/test_helpers.sh"

if [ ! -r /proc/pressure/cpu ]; then
    echo "SKIP: kernel lacks /proc/pressure/cpu (PSI gate is Linux-only)"
    exit 0
fi

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

# make_psi_fixture <avg10>
# Writes a /proc/pressure/cpu-formatted fixture to a temp file and echoes its path.
make_psi_fixture() {
    local avg10="$1"
    local fixture
    fixture="$(mktemp -p "$WORKDIR" psi-fixture.XXXXXX)"
    printf 'some avg10=%s avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        "$avg10" > "$fixture"
    echo "$fixture"
}

# run_gate <dispatch_file> <proc_path> [VAR=val ...]
# Invokes `verify.sh psi-gate` with the given dispatch file and PSI proc path,
# plus any additional env overrides.  After returning:
#   GATE_RC     — exit code of the invocation
#   GATE_STDERR — captured stderr text
GATE_RC=0
GATE_STDERR=""
run_gate() {
    local dispatch="$1" proc="$2"
    shift 2
    local _stderr_file
    _stderr_file="$(mktemp -p "$WORKDIR" gate-stderr.XXXXXX)"
    GATE_RC=0
    GATE_STDERR=""
    env "$@" \
        REIFY_PSI_GATE_DISPATCH_FILE="$dispatch" \
        REIFY_PSI_GATE_PROC_PATH="$proc" \
        bash "$VERIFY" psi-gate \
        2>"$_stderr_file" \
        || GATE_RC=$?
    GATE_STDERR="$(cat "$_stderr_file")"
    rm -f "$_stderr_file"
}

echo "=== psi-gate tests ==="

test_summary
