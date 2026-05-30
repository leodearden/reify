#!/usr/bin/env bash
# tests/infra/test_affected_crates_lib.sh — drift test for scripts/affected-crates-lib.sh
#
# Validates that affected_crates() correctly maps changed files to the
# affected workspace-crate set per docs/prds/verify-scope-contract.md §3 C3/C4/C5.
#
# Assertions (B8 battery):
#   1. C4: Cargo.lock (global) forces ALL
#   2. C1/§5: non-crate paths (docs/**, gui/src/**) contribute no crates (no ALL)
#   3. C5: unmappable path forces ALL
#   4. crates/<leaf-crate>/** maps to itself (reify-cli has no dependents)
#   5. gui/src-tauri/** maps to reify-gui
#   6. reverse closure of reify-core is NOT the ALL sentinel
#   7. reverse closure of reify-core contains sampled dependents (reify-ir, reify-eval, reify-cli, reify-gui)
#   8. reverse closure of reify-ir is NOT the ALL sentinel
#   9. cargo metadata failure -> ALL (C5)
#  10. global anywhere in arg list -> ALL

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$REPO_ROOT/scripts/affected-crates-lib.sh" ] || { echo "ERROR: affected-crates-lib.sh not found at $REPO_ROOT/scripts/affected-crates-lib.sh"; exit 1; }
source "$REPO_ROOT/scripts/affected-crates-lib.sh"

echo "=== affected-crates-lib drift tests ==="

# ---------------------------------------------------------------------------
# Step 1: C4 global-force — Cargo.lock forces ALL
# ---------------------------------------------------------------------------
echo ""
echo "--- C4: global files force ALL ---"

assert "Cargo.lock forces ALL" \
    test "$(affected_crates Cargo.lock)" = "ALL"

# ---------------------------------------------------------------------------
# Step 3: C1/§5 non-crate paths — no crates contributed, must NOT force ALL
# ---------------------------------------------------------------------------
echo ""
echo "--- §5 non-crate paths contribute nothing (not ALL) ---"

assert "docs path -> empty" \
    test -z "$(affected_crates docs/architecture/x.md)"

assert "gui frontend -> empty" \
    test -z "$(affected_crates gui/src/App.tsx)"

# ---------------------------------------------------------------------------
# Step 5: C5 fail-wide — unmappable path forces ALL
# ---------------------------------------------------------------------------
echo ""
echo "--- C5: unmappable path forces ALL ---"

assert "unmappable path -> ALL" \
    test "$(affected_crates some/unknown/place.zzz)" = "ALL"

# ---------------------------------------------------------------------------
# Step 7: direct-set printing — crate-mapped paths emit the crate name
# ---------------------------------------------------------------------------
echo ""
echo "--- Direct-set printing (no closure yet) ---"

assert "leaf crate cli -> itself" \
    test "$(affected_crates crates/reify-cli/src/main.rs)" = "reify-cli"

_check_gui_maps_reify_gui() {
    affected_crates gui/src-tauri/src/main.rs | grep -qx reify-gui
}
assert "gui/src-tauri maps to reify-gui" _check_gui_maps_reify_gui

# ---------------------------------------------------------------------------
# Step 9: C3 reverse-closure — low-level crate expands to dependents
# Ground-truth assertions encode independently-known dependency facts rather
# than comparing against a clone of the implementation (which would make any
# shared logic bug silently pass on both sides).
# ---------------------------------------------------------------------------
echo ""
echo "--- C3: reverse-dependency closure ---"

_check_reify_core_not_ALL() {
    local out
    out="$(affected_crates crates/reify-core/src/lib.rs)"
    [ "$out" != "ALL" ]
}
assert "reify-core closure is NOT the ALL sentinel" _check_reify_core_not_ALL

_check_contains() {
    # Usage: _check_contains <expected-crate> <input-file-path>
    local expected="$1" input_path="$2"
    affected_crates "$input_path" | grep -qx "$expected"
}
assert "reify-core closure contains reify-ir"    _check_contains reify-ir    crates/reify-core/src/lib.rs
assert "reify-core closure contains reify-eval"  _check_contains reify-eval  crates/reify-core/src/lib.rs
assert "reify-core closure contains reify-cli"   _check_contains reify-cli   crates/reify-core/src/lib.rs
assert "reify-core closure contains reify-gui"   _check_contains reify-gui   crates/reify-core/src/lib.rs

_check_reify_ir_not_ALL() {
    local out
    out="$(affected_crates crates/reify-ir/src/lib.rs)"
    [ "$out" != "ALL" ]
}
assert "reify-ir closure is NOT the ALL sentinel" _check_reify_ir_not_ALL

# ---------------------------------------------------------------------------
# Step 11: C5 metadata-failure fail-wide + C4 global precedes crates in list
# ---------------------------------------------------------------------------
echo ""
echo "--- C5: cargo metadata failure -> ALL; C4: global anywhere -> ALL ---"

# Stub cargo as a shell function that returns 1 (failure).
# The stub is defined locally so it shadows the real cargo only within the
# subshell created by $(...), which is what _reverse_closure calls.
_check_cargo_fail_all() {
    cargo() { return 1; }
    local result
    result="$(affected_crates crates/reify-core/src/lib.rs)"
    [ "$result" = "ALL" ]
}
assert "cargo metadata failure -> ALL" _check_cargo_fail_all

assert "global anywhere in list -> ALL" \
    test "$(affected_crates crates/reify-cli/src/main.rs Cargo.lock)" = "ALL"

test_summary
