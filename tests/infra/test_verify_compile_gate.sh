#!/usr/bin/env bash
# tests/infra/test_verify_compile_gate.sh — hermetic --print-plan oracle tests
# for the compile-phase PSI admission gate (task 4618).
#
# Asserts:
#   W1: lint/typecheck/all plans each contain the compile-gate line
#   W2: in each, compile-gate precedes the first cargo check/clippy line
#   W3: in the 'all' plan, compile-gate precedes both clippy and psi-gate
#   W4: pure 'test' plan does NOT contain compile-gate (no double-gate)
#   W5: merge-role 'all' plan still contains the compile-gate line
#       (runtime-bypassed, plan-shape is role-invariant — CAVEAT 1)
#   W6: structural — verify.sh defines compile_gate() and contains the
#       DF_VERIFY_ROLE=merge bypass inside it (source grep)
#   W7: preservation — clippy, gui-feature cargo check, psi-gate, and nextest
#       run still present in the 'all' plan; compile-gate carries no
#       nice/ionice prefix and no 'cargo' token
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh) and auto-selected
# by the infra map (scripts/verify.sh -> tests/infra/test_verify_*.sh).
# No cargo/npm/tree-sitter builds — pure shell/grep hermetic oracle.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
VERIFY="$REPO_ROOT/scripts/verify.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== verify.sh compile-gate wiring tests (task 4618) ==="

# ---------------------------------------------------------------------------
# Capture plans (hermetic: --print-plan, no cargo/npm execution)
# ---------------------------------------------------------------------------
PLAN_LINT="$(bash "$VERIFY" lint --scope all --print-plan 2>/dev/null | grep -v '^#')"
PLAN_TC="$(bash "$VERIFY"   typecheck --scope all --print-plan 2>/dev/null | grep -v '^#')"
PLAN_ALL="$(bash "$VERIFY"  all --scope all --print-plan 2>/dev/null | grep -v '^#')"
PLAN_TEST="$(bash "$VERIFY" test --scope all --print-plan 2>/dev/null | grep -v '^#')"
PLAN_MERGE="$(env DF_VERIFY_ROLE=merge bash "$VERIFY" all --scope all --print-plan 2>/dev/null | grep -v '^#')"

# ---------------------------------------------------------------------------
# W1: compile-gate present in lint / typecheck / all plans
# ---------------------------------------------------------------------------
echo ""
echo "--- W1: compile-gate present in lint/typecheck/all plans ---"

assert "W1-lint: lint plan contains ./scripts/verify.sh compile-gate" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_LINT"

assert "W1-tc: typecheck plan contains ./scripts/verify.sh compile-gate" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_TC"

assert "W1-all: all plan contains ./scripts/verify.sh compile-gate" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_ALL"

# ---------------------------------------------------------------------------
# W2: compile-gate index < first cargo check/clippy line in each plan
# ---------------------------------------------------------------------------
echo ""
echo "--- W2: compile-gate precedes first cargo check/clippy ---"

# Helper: echo "1" if compile-gate index < first cargo (check OR clippy) index in $1
_compile_gate_before_cargo() {
    local cg_ln cargo_ln
    cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
    cargo_ln=$(printf "%s\n" "$1" | grep -nE "(^| )(cargo check|cargo clippy)" | head -1 | cut -d: -f1)
    [ -n "$cg_ln" ] && [ -n "$cargo_ln" ] && [ "$cg_ln" -lt "$cargo_ln" ]
}

assert "W2-lint: compile-gate index < first cargo check/clippy in lint plan" \
    bash -c '_compile_gate_before_cargo() {
        local cg_ln cargo_ln
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        cargo_ln=$(printf "%s\n" "$1" | grep -nE "(^| )(cargo check|cargo clippy)" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$cargo_ln" ] && [ "$cg_ln" -lt "$cargo_ln" ]
    }; _compile_gate_before_cargo "$1"' _ "$PLAN_LINT"

assert "W2-tc: compile-gate index < first cargo check/clippy in typecheck plan" \
    bash -c '_compile_gate_before_cargo() {
        local cg_ln cargo_ln
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        cargo_ln=$(printf "%s\n" "$1" | grep -nE "(^| )(cargo check|cargo clippy)" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$cargo_ln" ] && [ "$cg_ln" -lt "$cargo_ln" ]
    }; _compile_gate_before_cargo "$1"' _ "$PLAN_TC"

assert "W2-all: compile-gate index < first cargo check/clippy in all plan" \
    bash -c '_compile_gate_before_cargo() {
        local cg_ln cargo_ln
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        cargo_ln=$(printf "%s\n" "$1" | grep -nE "(^| )(cargo check|cargo clippy)" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$cargo_ln" ] && [ "$cg_ln" -lt "$cargo_ln" ]
    }; _compile_gate_before_cargo "$1"' _ "$PLAN_ALL"

# ---------------------------------------------------------------------------
# W3: in 'all' plan, compile-gate < clippy AND compile-gate < psi-gate
# ---------------------------------------------------------------------------
echo ""
echo "--- W3: compile-gate precedes clippy AND psi-gate in all plan ---"

assert "W3-all: compile-gate index < cargo clippy index" \
    bash -c '
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        clippy_ln=$(printf "%s\n" "$1" | grep -n "cargo clippy" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$clippy_ln" ] && [ "$cg_ln" -lt "$clippy_ln" ]
    ' _ "$PLAN_ALL"

assert "W3-all: compile-gate index < psi-gate index" \
    bash -c '
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        psi_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$psi_ln" ] && [ "$cg_ln" -lt "$psi_ln" ]
    ' _ "$PLAN_ALL"

# ---------------------------------------------------------------------------
# W4: pure 'test' plan does NOT contain compile-gate
# ---------------------------------------------------------------------------
echo ""
echo "--- W4: pure test plan does NOT contain compile-gate ---"

assert "W4: 'test --print-plan' does NOT contain compile-gate" \
    bash -c '! printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_TEST"

# ---------------------------------------------------------------------------
# W5: merge-role 'all' plan still contains compile-gate (runtime-bypassed)
# ---------------------------------------------------------------------------
echo ""
echo "--- W5: merge-role all plan still contains compile-gate (runtime bypass) ---"

assert "W5: merge-role all plan contains compile-gate line (plan-shape role-invariant)" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_MERGE"

# ---------------------------------------------------------------------------
# W6: structural — verify.sh source contains compile_gate() definition and
#     DF_VERIFY_ROLE=merge bypass inside compile_gate
# ---------------------------------------------------------------------------
echo ""
echo "--- W6: verify.sh structural checks ---"

assert "W6: verify.sh defines compile_gate()" \
    grep -q "^compile_gate()" "$VERIFY"

assert "W6: compile_gate() contains DF_VERIFY_ROLE=merge bypass" \
    bash -c 'awk "/^compile_gate\(\)/,/^\}/" "$1" | grep -q "DF_VERIFY_ROLE.*merge"' _ "$VERIFY"

# ---------------------------------------------------------------------------
# W7: preservation — all expected plan components still present in 'all' plan,
#     and the compile-gate line carries no nice/ionice prefix and no cargo token
# ---------------------------------------------------------------------------
echo ""
echo "--- W7: preservation and compile-gate line properties in all plan ---"

assert "W7: all plan still contains cargo clippy" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo clippy"' _ "$PLAN_ALL"

assert "W7: all plan still contains gui-feature cargo check -p reify-gui" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo check -p reify-gui"' _ "$PLAN_ALL"

assert "W7: all plan still contains psi-gate" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh psi-gate"' _ "$PLAN_ALL"

assert "W7: all plan still contains cargo nextest run (or cargo test fallback)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo (nextest run|test)"' _ "$PLAN_ALL"

# The compile-gate line should be a bare ./scripts/verify.sh compile-gate with
# no nice/ionice priority prefix and no 'cargo' token.
CG_LINE="$(printf "%s\n" "$PLAN_ALL" | grep "verify\.sh compile-gate" | head -1)"

assert "W7: compile-gate line has no nice/ionice prefix" \
    bash -c '! printf "%s\n" "$1" | grep -qE "^(nice|ionice)"' _ "$CG_LINE"

assert "W7: compile-gate line contains no 'cargo' token" \
    bash -c '! printf "%s\n" "$1" | grep -qE "(^| )cargo( |$)"' _ "$CG_LINE"

test_summary
