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
#   3 — Project.md anti-orphan: project.md references the guard predicate
#       (`lock-charter-guard.sh`)
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
PROJECT_MD="$REPO_ROOT/.claude/skills/prd/project.md"
GUARD="$REPO_ROOT/scripts/lock-charter-guard.sh"

# ---------------------------------------------------------------------------
# Cycle 1 — Wiring (anti-orphan): decompose-mode.md contains the guard call site
# ---------------------------------------------------------------------------
echo "=== Cycle 1: decompose-mode.md wiring (anti-orphan) ==="

assert "decompose-mode.md exists" test -f "$DECOMPOSE_MD"

assert "decompose-mode.md references scripts/lock-charter-guard.sh (guard call site)" \
    bash -c "grep -q 'lock-charter-guard.sh' '$DECOMPOSE_MD'"

assert "decompose-mode.md contains the 'scripts/lock-charter-guard.sh check' invocation token (guard call site)" \
    bash -c "grep -q 'lock-charter-guard.sh check' '$DECOMPOSE_MD'"

# ---------------------------------------------------------------------------
# Cycle 2 — Block fires (G6 rejection-check): guard BLOCKS a directory-shaped
# leaf and ACCEPTS a file-level path (minimal smoke; exhaustive behavioral
# coverage lives in test_lock_charter_guard.sh)
# ---------------------------------------------------------------------------
echo ""
echo "=== Cycle 2: block fires (G6 rejection-check) ==="

assert "scripts/lock-charter-guard.sh exists" test -f "$GUARD"
assert "scripts/lock-charter-guard.sh is executable" test -x "$GUARD"

# Directory-shaped path → exit 1 + REJECT in stdout (block observable)
_dir_out="$(bash "$GUARD" check "crates/reify-eval/src/" 2>/dev/null)" \
    && _dir_rc=$? || _dir_rc=$?
assert "check dir-shaped leaf exits 1 (BLOCK)" test "$_dir_rc" -eq 1
assert "check dir-shaped leaf stdout contains 'REJECT crates/reify-eval/src/'" \
    test "${_dir_out#*REJECT crates/reify-eval/src/}" != "$_dir_out"

# File-level path → exit 0 (ACCEPT)
_file_rc=0
bash "$GUARD" check "crates/x/src/foo.rs" >/dev/null 2>&1 \
    && _file_rc=$? || _file_rc=$?
assert "check file-level path exits 0 (ACCEPT)" test "$_file_rc" -eq 0

# ---------------------------------------------------------------------------
# Cycle 3 — Project.md anti-orphan: project.md references the guard predicate
# ---------------------------------------------------------------------------
echo ""
echo "=== Cycle 3: project.md anti-orphan ==="

assert "project.md exists" test -f "$PROJECT_MD"

assert "project.md references lock-charter-guard.sh (predicate reference)" \
    bash -c "grep -q 'lock-charter-guard.sh' '$PROJECT_MD'"

# ---------------------------------------------------------------------------
test_summary
