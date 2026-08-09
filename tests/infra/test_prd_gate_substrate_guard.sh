#!/usr/bin/env bash
# Infrastructure test for scripts/prd-gate-substrate-guard.sh (task 5897).
#
# WHAT IS UNDER TEST
# ------------------
# The shared, sourced skip-guard library that the two prd_gate wrappers
# (test_prd_gate_corpus.sh, test_prd_gate_compiler_type_hygiene.sh) use to
# survive a lane where the tree-sitter grammar substrate is unusable — the
# sandboxed-role case where tree-sitter cannot write ~/.cache/tree-sitter/lock/
# and a grammar probe therefore reports HARNESS_ERROR (exit 70), turning a
# missing toolchain into a spurious gate FAIL.
#
# WHY THE LOGIC LIVES IN A LIBRARY, AND WHY THAT MATTERS HERE
# -----------------------------------------------------------
# Both gates derive REPO_ROOT from their own location ("$SCRIPT_DIR/../.."), so
# a test cannot point them at a synthetic tree, and every interesting branch of
# the guard is environment-dependent (is parser.c generated? is the cache
# writable? is the CLI launchable?).  The library instead takes repo_root as an
# ARGUMENT, so the unit layer below can drive it against temp roots holding a
# STUB scripts/prd-capability-check.py that exits 0 / 75 / 64 on demand.  That
# gives genuine hermetic RED/GREEN cycles on every lane, whatever that lane's
# real substrate happens to be.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_prd_gate_substrate_guard ==="

# One per-run scratch root; every fixture is minted UNDER it, so a single
# rm -rf reclaims the lot and no fixture helper needs to append to a cleanup
# array from inside a command-substitution subshell (where the append would be
# silently discarded).
_RUN_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/prd-gate-substrate-guard-XXXXXX")"
cleanup() { rm -rf "$_RUN_ROOT"; }
trap cleanup EXIT

# ── Load the library under test ────────────────────────────────────────────
GUARD_LIB="$REPO_ROOT/scripts/prd-gate-substrate-guard.sh"
if [ ! -f "$GUARD_LIB" ]; then
    echo "  FAIL: $GUARD_LIB not found — the guard library does not exist"
    FAIL=$((FAIL + 1))
    test_summary
fi
# shellcheck source=/dev/null
source "$GUARD_LIB"

# ── Fixture: a synthetic repo root with a stubbed checker ──────────────────
# _mk_stub_root <exit_code> <stdout_line> — mints <root>/scripts/prd-capability-check.py
# as a stub that prints <stdout_line> and exits <exit_code>, then echoes <root>.
#
# The stub reads its exit code and stdout from sibling fixture files rather
# than having them interpolated into its body: the reason strings under test
# contain quotes, parentheses and colons, and baking them through two layers of
# shell + python quoting is exactly the kind of fixture that silently stops
# testing what it claims to.
_mk_stub_root() {
    local rc="$1" line="$2" root
    root="$(mktemp -d "$_RUN_ROOT/root-XXXXXX")" || return 1
    mkdir -p "$root/scripts"
    printf '%s\n' "$line" > "$root/stub-stdout.txt"
    printf '%s\n' "$rc" > "$root/stub-rc.txt"
    cat > "$root/scripts/prd-capability-check.py" <<'PYEOF'
import os, sys

# <root>/scripts/prd-capability-check.py -> <root>
here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
with open(os.path.join(here, "stub-stdout.txt")) as f:
    sys.stdout.write(f.read())
with open(os.path.join(here, "stub-rc.txt")) as f:
    sys.exit(int(f.read().strip()))
PYEOF
    printf '%s\n' "$root"
}

# ── Assertion helpers ──────────────────────────────────────────────────────
# Each returns 0/1 and prints its own diagnostic; assert() captures that output
# and dumps it only on FAIL.  They read the library's output globals with
# ${...:-} defaults so a library that never sets them fails on the comparison
# rather than aborting the whole suite under `set -u`.

_want_usable() {
    local root="$1"
    if ! resolve_grammar_substrate "$root"; then
        echo "expected resolve_grammar_substrate to return 0 (usable), got non-zero"
        return 1
    fi
    if [ "${GRAMMAR_SUBSTRATE_OK:-<unset>}" != "1" ]; then
        echo "GRAMMAR_SUBSTRATE_OK=${GRAMMAR_SUBSTRATE_OK:-<unset>}, want 1"
        return 1
    fi
    return 0
}

_want_unusable_reason() {
    local root="$1" want="$2"
    if resolve_grammar_substrate "$root"; then
        echo "expected resolve_grammar_substrate to return non-zero (unusable), got 0"
        return 1
    fi
    if [ "${GRAMMAR_SUBSTRATE_OK:-<unset>}" != "0" ]; then
        echo "GRAMMAR_SUBSTRATE_OK=${GRAMMAR_SUBSTRATE_OK:-<unset>}, want 0"
        return 1
    fi
    if [ "${GRAMMAR_SUBSTRATE_REASON:-<unset>}" != "$want" ]; then
        echo "GRAMMAR_SUBSTRATE_REASON=${GRAMMAR_SUBSTRATE_REASON:-<unset>}"
        echo "                    want=$want"
        return 1
    fi
    return 0
}

_want_unusable_reason_contains() {
    local root="$1"
    shift
    if resolve_grammar_substrate "$root"; then
        echo "expected resolve_grammar_substrate to return non-zero (unusable), got 0"
        return 1
    fi
    if [ "${GRAMMAR_SUBSTRATE_OK:-<unset>}" != "0" ]; then
        echo "GRAMMAR_SUBSTRATE_OK=${GRAMMAR_SUBSTRATE_OK:-<unset>}, want 0"
        return 1
    fi
    local needle
    for needle in "$@"; do
        case "${GRAMMAR_SUBSTRATE_REASON:-}" in
            *"$needle"*) ;;
            *)  echo "reason does not mention '$needle': ${GRAMMAR_SUBSTRATE_REASON:-<unset>}"
                return 1 ;;
        esac
    done
    return 0
}

# ── Block A: resolve_grammar_substrate ─────────────────────────────────────
echo "-- resolve_grammar_substrate (hermetic, stubbed checker) --"

_ROOT_OK="$(_mk_stub_root 0 'grammar substrate: usable')"

# The exact reason wording is irrelevant to the guard; what matters is that the
# "grammar substrate: unusable: " prefix is stripped, so a caller can splice the
# bare reason into its own sentence.  A reason carrying a colon and parentheses
# is used deliberately: a prefix strip implemented with a greedy match or a
# field split on ':' would mangle it.
_UNUSABLE_REASON='cache/lock unwritable: Permission denied (os error 13) (~/.cache/tree-sitter/lock/reify.lock)'
_ROOT_75="$(_mk_stub_root 75 "grammar substrate: unusable: $_UNUSABLE_REASON")"

# 64 is EX_USAGE — what the checker returns for a malformed invocation. It is
# NOT the skip contract, and must never be laundered into one.
_ROOT_64="$(_mk_stub_root 64 'error: PROBE_SET_JSON is required unless --grammar-substrate-status is given')"

assert "usable checker (exit 0) => returns 0, GRAMMAR_SUBSTRATE_OK=1" \
    _want_usable "$_ROOT_OK"

assert "unusable checker (exit 75) => returns 1, OK=0, reason has the 'unusable: ' prefix stripped" \
    _want_unusable_reason "$_ROOT_75" "$_UNUSABLE_REASON"

assert "unexpected exit code (64) => returns 1, OK=0, and the reason names the code" \
    _want_unusable_reason_contains "$_ROOT_64" "64" "unexpected"

test_summary
