#!/usr/bin/env bash
# scripts/occt-scope-lib.sh — shared OCCT-touching crate-set logic, and host
# of the shared cargo-accurate compile-closure primitive.
#
# This library is the SINGLE implementation of "which workspace crates touch
# reify-kernel-occt". It is sourced by both:
#   - scripts/verify.sh                  (decides whether to run the gated pass)
#   - tests/infra/test_occt_gated_scope.sh (drift catcher)
# so the declared set and the cargo-metadata-derived set each have exactly one
# definition — divergence between the verify entrypoint and the drift test
# becomes impossible by construction.
#
# It also hosts `_reify_compile_closure`, the single shared implementation of
# the cargo-accurate compile-closure model (adj_normal/adj_dev +
# normal_closure). scripts/affected-crates-lib.sh sources this file to reach
# it, so the model has exactly one definition shared by both callers.
#
# Designed to be sourced, not executed directly:
#   source "$(dirname "${BASH_SOURCE[0]}")/occt-scope-lib.sh"
#
# Provides:
#   occt_declared_set       prints the declared OCCT-touching crates (one per
#                           line), reading scripts/occt-touching-crates.txt
#                           with comments/blank lines stripped and whitespace
#                           trimmed.
#   occt_touching_set       prints the cargo-metadata-derived OCCT-touching
#                           workspace members (sorted, one per line).
#   _reify_compile_closure  shared compile-closure primitive: reads `cargo
#                           metadata --format-version 1` JSON on stdin and
#                           seed crate NAMES from argv, prints the workspace
#                           member names whose test-compile closure reaches a
#                           seed (sorted-unique, one per line). Exits non-zero
#                           on malformed input (no try/except) — callers
#                           apply their own error policy at the call site.
#
# Environment:
#   OCCT_TOUCHING_CRATES_FILE  Override the declared-list path. Defaults to
#                              occt-touching-crates.txt next to this library.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_OCCT_SCOPE_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_OCCT_SCOPE_LIB_SOURCED=1

_OCCT_SCOPE_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OCCT_TOUCHING_CRATES_FILE="${OCCT_TOUCHING_CRATES_FILE:-$_OCCT_SCOPE_LIB_DIR/occt-touching-crates.txt}"

# occt_declared_set — print the declared OCCT-touching crate list, one crate per
# line, with comment lines (^\s*#) and blank lines removed and surrounding
# whitespace trimmed. This mirrors the historical inline computation in
# tests/infra/test_occt_gated_scope.sh.
occt_declared_set() {
    if [ ! -f "$OCCT_TOUCHING_CRATES_FILE" ]; then
        echo "ERROR: occt-touching-crates.txt not found at $OCCT_TOUCHING_CRATES_FILE" >&2
        return 1
    fi
    grep -v '^\s*#' "$OCCT_TOUCHING_CRATES_FILE" \
        | grep -v '^\s*$' \
        | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
}

# _REIFY_COMPILE_CLOSURE_PY / _reify_compile_closure — the SINGLE shared
# implementation of the cargo-accurate compile-closure model, used by both
# occt_touching_set (below) and scripts/affected-crates-lib.sh:_reverse_closure.
#
# Reads `cargo metadata --format-version 1` JSON on stdin and seed crate
# NAMES from argv (sys.argv[1:]). Builds separate adjacency maps for
# normal/build vs dev deps (dep_kinds[].kind: null -> normal, 'build' ->
# build dep, 'dev' -> dev dep — these must NOT be conflated: dev-deps of a
# transitive dep are never compiled when testing a crate that only has a
# normal dep on it). A workspace member is affected iff its test-compile
# closure — normal_closure(member) UNION normal_closure(each DIRECT dev-dep
# of member) — intersects the seed ids. Prints the affected workspace member
# names, sorted-unique, one per line.
#
# No try/except: malformed stdin/JSON raises and exits non-zero. Each caller
# applies its own error policy at the call site (occt_touching_set leaves
# stderr as-is; _reverse_closure suppresses stderr and falls back to ALL).
_REIFY_COMPILE_CLOSURE_PY=$(cat <<'PYEOF'
import sys, json

seed_names = set(sys.argv[1:])

m = json.load(sys.stdin)
members = set(m['workspace_members'])
id_to_name = {p['id']: p['name'] for p in m['packages']}
name_to_ids = {}
for p in m['packages']:
    name_to_ids.setdefault(p['name'], []).append(p['id'])

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

seed_ids = set()
for sn in seed_names:
    seed_ids.update(name_to_ids.get(sn, []))

result = []
for pkg_id in members:
    # A crate's test compilation includes:
    #   - normal/build closure of the crate itself, PLUS
    #   - normal/build closure of each DIRECT dev-dep of the crate
    # Dev-deps of transitive normal deps do NOT propagate (Cargo does not
    # propagate dev-deps transitively).
    compiled = normal_closure(pkg_id)
    for dev_dep_id in adj_dev.get(pkg_id, []):
        compiled |= normal_closure(dev_dep_id)
    if compiled & seed_ids:
        result.append(id_to_name[pkg_id])

for name in sorted(set(result)):
    print(name)
PYEOF
)

_reify_compile_closure() {
    python3 -c "$_REIFY_COMPILE_CLOSURE_PY" "$@"
}

# occt_touching_set — derive the ACTUAL OCCT-touching set from a SINGLE
# `cargo metadata` invocation (the workspace-unified resolve graph), via the
# shared _reify_compile_closure primitive above, seeded with the OCCT crate
# itself. Prints the OCCT-touching workspace member names, sorted, one per
# line.
#
# Using the workspace-unified resolve graph is both faster (one cargo process
# instead of one per workspace member) and more accurate: workspace feature
# unification can activate optional deps (e.g. a future crate that enables the
# reify-gui 'gui' feature would pull in reify-kernel-occt via a normal dep, but
# per-crate `cargo tree -p <crate>` only sees each crate's own default features
# and would miss it).
occt_touching_set() {
    cargo metadata --format-version 1 2>/dev/null | _reify_compile_closure reify-kernel-occt
}
