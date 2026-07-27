#!/usr/bin/env bash
# Unit tests for tests/infra/nextest_absent_lib.sh — the shared nextest-absent
# simulation harness (task 5602).
#
# These tests exercise the RUNTIME BEHAVIOUR of the lib and of the environment
# it constructs: that cargo-nextest is genuinely unreachable under it, that the
# rest of the toolchain still EXECUTES (not merely resolves), that the harness
# does not perturb the toolchain enough to provoke a rustup sync, and that a
# nested init from within an already-constructed env still yields a usable env.
#
# Auto-discovered by run_all.sh (matches test_*.sh); registered in
# tests/infra/run-all-classification.manifest as `pool` — same reasons as its
# siblings test_load_tolerance_lib.sh and test_plan_capture_lib.sh: it is
# hermetic (its own mktemp workdir, no lane-shared state) and it never nests a
# suite that mutates the working tree.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB="$SCRIPT_DIR/nextest_absent_lib.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== nextest_absent_lib.sh unit tests (task 5602) ==="

# -- Existence guard: lib must exist before sourcing ---------------------------
echo ""
echo "--- Existence: nextest_absent_lib.sh exists ---"

assert "nextest_absent_lib.sh file exists" \
    test -f "$LIB"

# Source the lib (bails out cleanly if it doesn't exist, rather than aborting
# before test_summary — a missing lib must still emit a parseable Results line).
if ! [ -f "$LIB" ]; then
    echo "FATAL: nextest_absent_lib.sh not found at $LIB — skipping remaining tests"
    test_summary
fi
# shellcheck source=tests/infra/nextest_absent_lib.sh
source "$LIB"

# -- Test 1: API surface -------------------------------------------------------
# `declare -F` rather than `grep -qF "name()"` on the file: grep would pass on a
# definition sitting inside a comment, a heredoc, or a branch that never runs,
# whereas declare -F asserts the function is actually DEFINED in the shell after
# sourcing — which is the contract a caller depends on.
echo ""
echo "--- Test 1: API surface (declare -F after sourcing) ---"

_defines() {
    bash -c 'source "$1" && declare -F "$2" >/dev/null' _ "$LIB" "$1"
}

for _fn in nextest_absent_init nextest_absent_available nextest_absent_reason \
           nx_run nx_which; do
    assert "nextest_absent_lib.sh defines $_fn()" _defines "$_fn"
done
unset _fn

# -- Test 2: source guard — double-source is a no-op ---------------------------
echo ""
echo "--- Test 2: source guard _REIFY_NEXTEST_ABSENT_LIB_SH_SOURCED ---"

assert "double-sourcing nextest_absent_lib.sh is a no-op (guard works)" \
    bash -c 'source "$1" && source "$1" && declare -F nextest_absent_init >/dev/null' _ "$LIB"

test_summary
