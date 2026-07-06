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
#  11. (task 4938) dev-dep non-transitivity: an OCCT-seed closure excludes a
#      dev-dep-of-a-dev-dep (reify-eval-fea-tests) whose own test binary links
#      no OCCT, while still including the seed (reify-kernel-occt) and a
#      direct dev-dependent (reify-eval) that does compile OCCT
#  12. (task 4938) tests/infra/* non-crate allowlist composes with crate
#      accumulation in a mixed diff (narrows; does not force ALL)

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

assert "tests/infra shell/python -> empty (no ALL)" \
    test -z "$(affected_crates tests/infra/test_cpu_load_governance.sh)"

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
# Task 4938 Fix #1: dev-dep non-transitivity (cargo-accurate compile closure)
#
# cargo's compile semantics are NOT transitive over dev-deps: testing crate X
# compiles normal/build-closure(X) plus normal/build-closure(each DIRECT
# dev-dep of X); dev-deps of X's transitive deps never compile. A reverse
# closure that walks ALL dep kinds (null/build/dev) transitively
# over-approximates this.
#
# Ground-truth facts (independently known, not a clone of _reverse_closure's
# algorithm — verified against scripts/occt-scope-lib.sh:occt_touching_set,
# which computes the OCCT-touching set from the FORWARD direction):
#   - reify-eval dev-deps reify-kernel-occt (crates/reify-eval/Cargo.toml) ->
#     reify-eval's own tests DO compile OCCT.
#   - reify-eval-fea-tests dev-deps reify-eval (its [dependencies] is empty) ->
#     a two-hop dev chain (occt <- reify-eval <- reify-eval-fea-tests) exists,
#     but reify-eval-fea-tests's test binary links no OCCT: normal-closure of
#     its direct dev-dep reify-eval does not carry reify-eval's OWN dev-dep on
#     OCCT. It must be EXCLUDED from an OCCT-seeded affected set.
#
# Placed BEFORE the cargo-metadata-failure stub below: that stub's `cargo()`
# shell function is defined in the current shell (assert invokes checkers
# directly, not in a subshell, per esc-4959-57) and is never unset, so it
# would otherwise leak into every assertion that follows it.
# ---------------------------------------------------------------------------
echo ""
echo "--- Task 4938 Fix #1: dev-dep non-transitivity ---"

_check_not_contains() {
    # Usage: _check_not_contains <unexpected-crate> <input-file-path>
    local unexpected="$1" input_path="$2"
    ! affected_crates "$input_path" | grep -qx "$unexpected"
}

_check_occt_seed_not_ALL() {
    local out
    out="$(affected_crates crates/reify-kernel-occt/src/lib.rs)"
    [ "$out" != "ALL" ]
}
assert "OCCT-seed closure is NOT the ALL sentinel" _check_occt_seed_not_ALL

assert "OCCT-seed closure does NOT contain reify-eval-fea-tests (dev-dep of a dev-dep; its test binary links no OCCT)" \
    _check_not_contains reify-eval-fea-tests crates/reify-kernel-occt/src/lib.rs

assert "OCCT-seed closure contains reify-kernel-occt (the seed itself)" \
    _check_contains reify-kernel-occt crates/reify-kernel-occt/src/lib.rs

assert "OCCT-seed closure contains reify-eval (direct dev-dependent whose own tests compile OCCT)" \
    _check_contains reify-eval crates/reify-kernel-occt/src/lib.rs

# ---------------------------------------------------------------------------
# Task 4938 Fix #2 composition guard: the tests/infra/* non-crate allowlist
# (already landed, ab021821fa) must compose with crate accumulation in a
# mixed diff — a tests/infra/* path contributes no crates but must not force
# ALL nor prevent a co-changed crate path from narrowing the set normally.
# ---------------------------------------------------------------------------
echo ""
echo "--- Task 4938 Fix #2 composition: tests/infra/* + crate path narrows together ---"

_check_mixed_not_ALL() {
    local out
    out="$(affected_crates tests/infra/test_cpu_load_governance.sh crates/reify-doc/src/lib.rs)"
    [ "$out" != "ALL" ]
}
assert "mixed tests/infra + crate diff is NOT the ALL sentinel" _check_mixed_not_ALL

_check_mixed_contains_reify_doc() {
    affected_crates tests/infra/test_cpu_load_governance.sh crates/reify-doc/src/lib.rs | grep -qx reify-doc
}
assert "mixed tests/infra + crate diff contains reify-doc" _check_mixed_contains_reify_doc

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
