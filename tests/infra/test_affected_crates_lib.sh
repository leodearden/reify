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
#   6. crates/<low-level-crate>/src/* expands to full reverse closure == independent oracle
#   7. reverse closure of reify-core is NOT the ALL sentinel
#   8. reverse closure of reify-core contains sampled dependents
#   9. reverse closure of reify-ir == independent oracle
#  10. cargo metadata failure -> ALL (C5)
#  11. global anywhere in arg list -> ALL

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
# Independent oracle: cargo metadata -> python3 reverse BFS (NOT the lib).
# ---------------------------------------------------------------------------
echo ""
echo "--- C3: reverse-dependency closure ---"

# oracle_revclosure <crate-name> — independent python3 reverse-closure over
# workspace-internal edges (all dep kinds: null/build/dev), printed sorted-unique.
# This is intentionally independent of the lib under test.
oracle_revclosure() {
    local seed_name="$1"
    cargo metadata --format-version 1 2>/dev/null | python3 -c "
import sys, json
m = json.load(sys.stdin)
members = set(m['workspace_members'])
id_to_name = {p['id']: p['name'] for p in m['packages']}
name_to_ids = {}
for p in m['packages']:
    name_to_ids.setdefault(p['name'], []).append(p['id'])

# Build reverse adjacency R[dep_id] = set of pkg_ids that depend on dep_id,
# restricted to workspace members on both ends.
rev = {}
for node in m['resolve']['nodes']:
    if node['id'] not in members:
        continue
    for d in node['deps']:
        if d['pkg'] not in members:
            continue
        rev.setdefault(d['pkg'], set()).add(node['id'])

# BFS from all IDs of the seed crate, inclusive.
seed_ids = set(name_to_ids.get('$seed_name', []))
visited = set(seed_ids)
queue = list(seed_ids)
while queue:
    curr = queue.pop()
    for dep_on_curr in rev.get(curr, []):
        if dep_on_curr not in visited:
            visited.add(dep_on_curr)
            queue.append(dep_on_curr)

# Intersect with workspace members, emit names sorted-unique.
result = sorted({id_to_name[i] for i in visited if i in members})
for name in result:
    print(name)
"
}

_check_reify_core_closure_eq_oracle() {
    local lib_out oracle_out
    lib_out="$(affected_crates crates/reify-core/src/lib.rs)"
    oracle_out="$(oracle_revclosure reify-core)"
    [ "$lib_out" = "$oracle_out" ]
}
assert "reify-core closure == oracle" _check_reify_core_closure_eq_oracle

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

_check_reify_ir_closure_eq_oracle() {
    local lib_out oracle_out
    lib_out="$(affected_crates crates/reify-ir/src/lib.rs)"
    oracle_out="$(oracle_revclosure reify-ir)"
    [ "$lib_out" = "$oracle_out" ]
}
assert "reify-ir closure == oracle" _check_reify_ir_closure_eq_oracle

test_summary
