#!/usr/bin/env bash
# tests/infra/test_lock_charter_decompose_guard.sh
#
# β wiring test for the lock-charter guard (task 4677).
# Asserts that the reify /prd skill overlay documents the guard call site
# at the decompose filing step (anti-orphan / wiring assertions) and that
# the guard itself fires the expected REJECT/ACCEPT responses when invoked
# as documented (block-fires / G6 negative-assertion).
#
# Mirrors test_hooks_call_verify.sh: grep a script-of-record for a sibling
# tool's call site, then observe the rejection firing live.
#
# Cycles:
#   1 — Wiring (anti-orphan): decompose-mode.md contains the α guard call site
#       (`scripts/lock-charter-guard.sh` + `check`)
#   2 — Block fires (G6 rejection-check): guard BLOCKS a directory-shaped leaf
#       and ACCEPTS a file-level/empty list
#
# (Cycle 3 — project.md anti-orphan — added in step-3.)
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

DECOMPOSE_MD="$REPO_ROOT/.claude/skills/prd/references/decompose-mode.md"
GUARD="$REPO_ROOT/scripts/lock-charter-guard.sh"

# ---------------------------------------------------------------------------
# Cycle 1 — Wiring (anti-orphan): decompose-mode.md contains the guard call site
# ---------------------------------------------------------------------------
echo "=== Cycle 1: decompose-mode.md wiring (anti-orphan) ==="

assert "decompose-mode.md exists" test -f "$DECOMPOSE_MD"

assert "decompose-mode.md references scripts/lock-charter-guard.sh (guard call site)" \
    bash -c "grep -q 'lock-charter-guard.sh' '$DECOMPOSE_MD'"

assert "decompose-mode.md references the 'check' subcommand (guard invocation)" \
    bash -c "grep -q 'check' '$DECOMPOSE_MD'"

# ---------------------------------------------------------------------------
# Cycle 2 — Block fires (G6 rejection-check): guard BLOCKS directories, ACCEPTS files/empty
# ---------------------------------------------------------------------------
echo ""
echo "=== Cycle 2: block fires (G6 rejection-check) ==="

assert "scripts/lock-charter-guard.sh exists" test -f "$GUARD"
assert "scripts/lock-charter-guard.sh is executable" test -x "$GUARD"

# Directory-shaped paths → exit 1 + REJECT in stdout (block observable)
_dir_out="$(bash "$GUARD" check "crates/reify-eval/src/" "compute_targets" 2>/dev/null)" \
    && _dir_rc=$? || _dir_rc=$?
assert "check dir-shaped leaf exits 1 (BLOCK)" test "$_dir_rc" -eq 1
assert "check dir-shaped leaf stdout contains 'REJECT crates/reify-eval/src/'" \
    test "${_dir_out#*REJECT crates/reify-eval/src/}" != "$_dir_out"
assert "check dir-shaped leaf stdout contains 'REJECT compute_targets'" \
    test "${_dir_out#*REJECT compute_targets}" != "$_dir_out"

# File-level list → exit 0 (ACCEPT)
_file_rc=0
bash "$GUARD" check "crates/x/src/foo.rs" "examples/b.ri" >/dev/null 2>&1 \
    && _file_rc=$? || _file_rc=$?
assert "check file-level list exits 0 (ACCEPT)" test "$_file_rc" -eq 0

# Empty input ([] defer-to-architect value) → exit 0
_empty_rc=0
bash "$GUARD" check </dev/null >/dev/null 2>&1 \
    && _empty_rc=$? || _empty_rc=$?
assert "check empty list exits 0 ([] ACCEPT)" test "$_empty_rc" -eq 0

# ---------------------------------------------------------------------------
test_summary
