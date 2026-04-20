#!/usr/bin/env bash
# Infrastructure test for task 2000.
# Validates that the OCCT-touching crate list is correct and that
# orchestrator.yaml routes exactly those crates through the flock gate.
#
# Assertions:
#   1. scripts/occt-touching-crates.txt exists and is non-empty (after stripping comments/blanks).
#   2. Every declared entry is a real workspace member.
#   3. Declared set EQUALS the cargo-tree-derived OCCT-touching set (drift catcher).
#   4. Each declared crate has -p <crate> in the gated debug AND release invocations.
#   5. The gated invocations do NOT contain --workspace.
#   6. Each declared crate has --exclude <crate> in the ungated debug AND release invocations.
#   7. Each ungated invocation is wrapped with timeout --kill-after=60 [0-9]+m.
#   8. Gated debug invocation appears before ungated debug invocation (ordering).
#   9. Gated release invocation appears before ungated release invocation (ordering).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

CRATE_LIST="$REPO_ROOT/scripts/occt-touching-crates.txt"
ORCH="$REPO_ROOT/orchestrator.yaml"

echo "=== OCCT gated scope tests ==="

# ---------------------------------------------------------------------------
# Test 1: declared list file exists and is non-empty
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: scripts/occt-touching-crates.txt exists and is non-empty ---"

assert "scripts/occt-touching-crates.txt exists" \
    test -f "$CRATE_LIST"

assert "scripts/occt-touching-crates.txt is non-empty after stripping comments/blanks" \
    bash -c "[ -f '$CRATE_LIST' ] && [ -n \"\$(grep -v '^\s*#' '$CRATE_LIST' | grep -v '^\s*\$')\" ]"

# ---------------------------------------------------------------------------
# Test 2: every declared entry is a real workspace member
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 2: every declared crate is a real workspace member ---"

# Collect workspace members via cargo metadata.
WORKSPACE_MEMBERS="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | python3 -c "import sys,json; m=json.load(sys.stdin); [print(p['name']) for p in m['packages']]")"

if [ -f "$CRATE_LIST" ]; then
    DECLARED_CRATES="$(grep -v '^\s*#' "$CRATE_LIST" | grep -v '^\s*$' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
else
    DECLARED_CRATES=""
fi

while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "declared crate '$crate' is a real workspace member" \
        grep -qxF "$crate" <<< "$WORKSPACE_MEMBERS"
done <<< "$DECLARED_CRATES"

# ---------------------------------------------------------------------------
# Test 3: declared set equals cargo-metadata-derived OCCT-touching set
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: declared set equals cargo-metadata-derived OCCT-touching set ---"

# Derive the actual OCCT-touching set from a SINGLE `cargo metadata` invocation.
# Using the workspace-unified resolve graph is both faster (one cargo process instead
# of one per workspace member) and more accurate: workspace feature unification can
# activate optional deps (e.g. a future crate that enables the reify-gui 'gui' feature
# would pull in reify-kernel-occt via normal dep, but per-crate `cargo tree -p <crate>`
# only sees each crate's own default features and would miss it).
ACTUAL_TOUCHING="$(cargo metadata --format-version 1 2>/dev/null | python3 -c "
import sys, json
m = json.load(sys.stdin)
id_to_name = {p['id']: p['name'] for p in m['packages']}

# Build separate adjacency maps for normal/build vs dev deps.
# dep_kinds[].kind: null -> normal, 'build' -> build dep, 'dev' -> dev dep.
# We must NOT conflate them: dev-deps of a transitive dep are never compiled when
# testing a crate that only has a normal dep on it.
adj_normal = {}  # kind=null or kind='build' (compiled transitively)
adj_dev = {}     # kind='dev' (only the DIRECT dev-deps of the tested crate matter)
for node in m['resolve']['nodes']:
    adj_normal[node['id']] = set()
    adj_dev[node['id']] = set()
    for d in node['deps']:
        kinds = {dk.get('kind') for dk in d.get('dep_kinds', [])}
        if None in kinds or 'build' in kinds:
            adj_normal[node['id']].add(d['pkg'])
        if 'dev' in kinds:
            adj_dev[node['id']].add(d['pkg'])

def normal_closure(start):
    '''All packages reachable via normal/build edges only.'''
    visited, queue = set(), [start]
    while queue:
        curr = queue.pop()
        if curr in visited:
            continue
        visited.add(curr)
        queue.extend(adj_normal.get(curr, []))
    return visited

occt_ids = {p['id'] for p in m['packages'] if p['name'] == 'reify-kernel-occt'}
workspace_ids = set(m['workspace_members'])
touching = []
for pkg_id in workspace_ids:
    # A crate's test compilation includes:
    #   - normal/build closure of the crate itself, PLUS
    #   - normal/build closure of each DIRECT dev-dep of the crate
    # Dev-deps of transitive normal deps do NOT propagate (Cargo does not
    # propagate dev-deps transitively).
    compiled = normal_closure(pkg_id)
    for dev_dep_id in adj_dev.get(pkg_id, []):
        compiled |= normal_closure(dev_dep_id)
    if compiled & occt_ids:
        touching.append(id_to_name[pkg_id])

for name in sorted(touching):
    print(name)
")"

# Write both sets to temp files and diff for actionable failure output.
# On mismatch the diff is printed so the reader can see exactly which crate
# drifted without re-running locally.
_DECLARED_TMP="$(mktemp)"
_ACTUAL_TMP="$(mktemp)"
echo "$DECLARED_CRATES" | sort > "$_DECLARED_TMP"
echo "$ACTUAL_TOUCHING" | sort > "$_ACTUAL_TMP"
_DIFF_OUT="$(diff "$_DECLARED_TMP" "$_ACTUAL_TMP" 2>&1 || true)"
rm -f "$_DECLARED_TMP" "$_ACTUAL_TMP"
if [ -n "$_DIFF_OUT" ]; then
    echo "  OCCT-touching set drift detected (< declared, > cargo-metadata-derived):"
    echo "$_DIFF_OUT" | sed 's/^/    /'
fi
assert "declared OCCT-touching set equals cargo-metadata-derived set (no missing or extra entries)" \
    test -z "$_DIFF_OUT"

# ---------------------------------------------------------------------------
# Tests 4–5: gated invocations use -p <crate> (not --workspace)
# ---------------------------------------------------------------------------
# Split the test_command line on ' && ' to extract per-invocation segments.
TEST_CMD_LINE="$(grep 'test_command:' "$ORCH")"
GATED_DEBUG="$(printf '%s' "$TEST_CMD_LINE" | sed 's/ && /\n/g' \
    | grep 'cargo-test-occt-gated\.sh' | grep -v -- '--release' || true)"
GATED_RELEASE="$(printf '%s' "$TEST_CMD_LINE" | sed 's/ && /\n/g' \
    | grep 'cargo-test-occt-gated\.sh' | grep -- '--release' || true)"
export GATED_DEBUG GATED_RELEASE

echo ""
echo "--- Test 4: gated debug invocation has '-p <crate>' for each declared crate ---"
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "gated debug invocation has '-p $crate'" \
        bash -c "printf '%s' \"\$GATED_DEBUG\" | grep -qF ' -p $crate'"
done <<< "$DECLARED_CRATES"

echo ""
echo "--- Test 5: gated release invocation has '-p <crate>' for each declared crate ---"
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "gated release invocation has '-p $crate'" \
        bash -c "printf '%s' \"\$GATED_RELEASE\" | grep -qF ' -p $crate'"
done <<< "$DECLARED_CRATES"

echo ""
echo "--- Test 6: gated invocations do not contain --workspace ---"
assert "gated debug invocation does not contain --workspace" \
    bash -c "! printf '%s' \"\$GATED_DEBUG\" | grep -qF ' --workspace'"
assert "gated release invocation does not contain --workspace" \
    bash -c "! printf '%s' \"\$GATED_RELEASE\" | grep -qF ' --workspace'"

# ---------------------------------------------------------------------------
# Tests 7–11: ungated-exclude assertions
# ---------------------------------------------------------------------------
# Extract ungated workspace passes: segments with 'cargo test --workspace' but
# NOT containing 'cargo-test-occt-gated.sh'.
UNGATED_DEBUG="$(printf '%s' "$TEST_CMD_LINE" | sed 's/ && /\n/g' \
    | grep 'cargo test --workspace' | grep -v 'cargo-test-occt-gated\.sh' | grep -v -- '--release' || true)"
UNGATED_RELEASE="$(printf '%s' "$TEST_CMD_LINE" | sed 's/ && /\n/g' \
    | grep 'cargo test --workspace' | grep -v 'cargo-test-occt-gated\.sh' | grep -- '--release' || true)"
export UNGATED_DEBUG UNGATED_RELEASE

echo ""
echo "--- Test 7: ungated workspace passes exist (one debug, one release) ---"
assert "ungated debug pass (cargo test --workspace, no gate, no --release) exists" \
    test -n "$UNGATED_DEBUG"
assert "ungated release pass (cargo test --workspace --release, no gate) exists" \
    test -n "$UNGATED_RELEASE"

echo ""
echo "--- Test 8: ungated passes have --exclude <crate> for each declared crate ---"
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "ungated debug has '--exclude $crate'" \
        bash -c "printf '%s' \"\$UNGATED_DEBUG\" | grep -qF ' --exclude $crate'"
    assert "ungated release has '--exclude $crate'" \
        bash -c "printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' --exclude $crate'"
done <<< "$DECLARED_CRATES"

echo ""
echo "--- Test 9: ungated passes are wrapped in 'timeout --kill-after=60 [0-9]+m' ---"
assert "ungated debug invocation contains 'timeout --kill-after=60 [0-9]+m'" \
    bash -c "printf '%s' \"\$UNGATED_DEBUG\" | grep -qE 'timeout[[:space:]]+--kill-after=60[[:space:]]+[0-9]+m[[:space:]]'"
assert "ungated release invocation contains 'timeout --kill-after=60 [0-9]+m'" \
    bash -c "printf '%s' \"\$UNGATED_RELEASE\" | grep -qE 'timeout[[:space:]]+--kill-after=60[[:space:]]+[0-9]+m[[:space:]]'"

echo ""
echo "--- Test 10: gated debug appears before ungated debug in test_command ---"
_ALL_SEGS="$(printf '%s' "$TEST_CMD_LINE" | sed 's/ && /\n/g')"
_GATED_DEBUG_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -n 'cargo-test-occt-gated\.sh' | grep -v -- '--release' | head -1 | cut -d: -f1 || true)"
_UNGATED_DEBUG_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -n 'cargo test --workspace' | grep -v 'cargo-test-occt-gated' | grep -v -- '--release' | head -1 | cut -d: -f1 || true)"
assert "gated debug (segment ${_GATED_DEBUG_IDX:-?}) precedes ungated debug (segment ${_UNGATED_DEBUG_IDX:-?})" \
    bash -c "[ '${_GATED_DEBUG_IDX:-0}' -gt 0 ] && [ '${_UNGATED_DEBUG_IDX:-0}' -gt 0 ] && [ '${_GATED_DEBUG_IDX:-0}' -lt '${_UNGATED_DEBUG_IDX:-0}' ]"

echo ""
echo "--- Test 11: gated release appears before ungated release in test_command ---"
_GATED_RELEASE_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -n 'cargo-test-occt-gated\.sh' | grep -- '--release' | head -1 | cut -d: -f1 || true)"
_UNGATED_RELEASE_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -n 'cargo test --workspace' | grep -v 'cargo-test-occt-gated' | grep -- '--release' | head -1 | cut -d: -f1 || true)"
assert "gated release (segment ${_GATED_RELEASE_IDX:-?}) precedes ungated release (segment ${_UNGATED_RELEASE_IDX:-?})" \
    bash -c "[ '${_GATED_RELEASE_IDX:-0}' -gt 0 ] && [ '${_UNGATED_RELEASE_IDX:-0}' -gt 0 ] && [ '${_GATED_RELEASE_IDX:-0}' -lt '${_UNGATED_RELEASE_IDX:-0}' ]"

test_summary
