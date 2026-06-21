#!/usr/bin/env bash
# scripts/assert-crate-dag.sh — Workspace-wide DAG invariant assertion.
#
# Permanent machine-checked gate for the §3 contract table and §8 boundary
# table from docs/prds/core-ast-ir-layering.md.
#
# Checks B1–B6 using `cargo metadata --format-version 1` (the workspace-unified
# resolve graph) and prints:
#   B<n> OK          — invariant passes
#   B<n> FAIL: ...   — invariant violated (details follow)
#   SUMMARY: OK / FAIL
#
# Exits 0 if all checks pass; exits 1 on any failure.
#
# Pattern: bash wrapper shells to python3 via pipe — mirrors scripts/occt-scope-lib.sh.
# The shell wrapper provides a stable CLI interface; python does structural reasoning.
#
# Usage:
#   bash scripts/assert-crate-dag.sh          # from workspace root
#   bash scripts/assert-crate-dag.sh --quiet  # suppress per-check OK lines

set -euo pipefail

QUIET=0
for arg in "$@"; do
    case "$arg" in
        --quiet) QUIET=1 ;;
    esac
done

# Capture `cargo metadata` with retries. In a shared-CARGO_HOME environment
# (e.g. concurrent warm-lane verifies sharing ~/.cargo), the package-cache lock
# can be held exclusively by a sibling cargo invocation. With stderr discarded,
# a blocked/failed `cargo metadata` yields EMPTY stdout, which previously made
# the downstream `json.load` crash with "Expecting value: line 1 column 1"
# (esc-4419-41). Retry on empty/failed output instead of false-failing the gate.
METADATA=""
META_ERR=""
for attempt in 1 2 3 4 5; do
    META_ERR="$(mktemp)"
    if METADATA="$(cargo metadata --format-version 1 2>"$META_ERR")" && [ -n "$METADATA" ]; then
        rm -f "$META_ERR"
        break
    fi
    if [ "$attempt" -lt 5 ]; then
        echo "assert-crate-dag.sh: cargo metadata produced no output (attempt $attempt/5); retrying after lock contention..." >&2
        sleep "$attempt"
    fi
    METADATA=""
done

if [ -z "$METADATA" ]; then
    echo "assert-crate-dag.sh: FATAL — cargo metadata produced no usable output after 5 attempts." >&2
    [ -n "$META_ERR" ] && [ -f "$META_ERR" ] && { echo "--- last cargo metadata stderr ---" >&2; cat "$META_ERR" >&2; rm -f "$META_ERR"; }
    exit 1
fi

printf '%s' "$METADATA" | python3 -c "
import sys, json

QUIET = int('$QUIET')

m = json.load(sys.stdin)

# Build lookup: package-id → package dict
packages = {p['id']: p for p in m['packages']}
id_to_name = {p['id']: p['name'] for p in m['packages']}
name_to_id = {}
for p in m['packages']:
    name_to_id.setdefault(p['name'], []).append(p['id'])

workspace_member_ids = set(m['workspace_members'])
workspace_member_names = {id_to_name[i] for i in workspace_member_ids}

# Build adjacency by dep kind.
# dep_kinds[].kind: null (None) -> normal, 'build' -> build dep, 'dev' -> dev dep.
# normal_deps[pkg_id] = set of pkg_ids reachable via kind=None or kind='build'
# dev_deps[pkg_id]    = set of pkg_ids reachable via kind='dev'
# all_deps[pkg_id]    = union of both (for B6 which checks all kinds)
normal_deps = {}
dev_deps = {}
all_deps = {}
for node in m['resolve']['nodes']:
    nid = node['id']
    normal_deps[nid] = set()
    dev_deps[nid] = set()
    all_deps[nid] = set()
    for d in node['deps']:
        kinds = {dk.get('kind') for dk in d.get('dep_kinds', [])}
        if None in kinds or 'build' in kinds:
            normal_deps[nid].add(d['pkg'])
        if 'dev' in kinds:
            dev_deps[nid].add(d['pkg'])
        all_deps[nid].add(d['pkg'])

def workspace_normal_deps(pkg_name):
    '''Return set of workspace-member dep names for pkg_name (non-dev kinds only).'''
    ids = name_to_id.get(pkg_name, [])
    result = set()
    for pid in ids:
        if pid not in workspace_member_ids:
            continue
        for dep_id in normal_deps.get(pid, []):
            dep_name = id_to_name.get(dep_id, '')
            if dep_id in workspace_member_ids:
                result.add(dep_name)
    return result

def all_workspace_deps(pkg_name):
    '''Return set of workspace-member dep names for pkg_name (all dep kinds).'''
    ids = name_to_id.get(pkg_name, [])
    result = set()
    for pid in ids:
        if pid not in workspace_member_ids:
            continue
        for dep_id in all_deps.get(pid, []):
            dep_name = id_to_name.get(dep_id, '')
            if dep_id in workspace_member_ids:
                result.add(dep_name)
    return result

def normal_dep_names_all(pkg_name):
    '''Return ALL non-workspace and workspace dep names (kind None/build) for pkg_name.'''
    ids = name_to_id.get(pkg_name, [])
    result = set()
    for pid in ids:
        if pid not in workspace_member_ids:
            continue
        for dep_id in normal_deps.get(pid, []):
            result.add(id_to_name.get(dep_id, dep_id))
    return result

failures = []

def check(bn, condition, detail=''):
    if condition:
        if not QUIET:
            print(f'B{bn} OK')
    else:
        print(f'B{bn} FAIL: {detail}')
        failures.append(bn)

# ── B1: reify-core has zero reify-* dependencies (kind None/build) ──────────
core_reify_deps = {d for d in workspace_normal_deps('reify-core') if d.startswith('reify-')}
check(1,
      len(core_reify_deps) == 0,
      f'reify-core has reify-* normal/build deps: {sorted(core_reify_deps)}')

# ── B2: reify-ast normal/build deps == {{reify-core}} ────────────────────────
#        AND no tree-sitter dep at kind None/build
ast_ws_deps = workspace_normal_deps('reify-ast')
ast_all_deps = normal_dep_names_all('reify-ast')
ast_tree_sitter_deps = {d for d in ast_all_deps if d.startswith('tree-sitter')}
ast_reify_ws_deps = {d for d in ast_ws_deps if d.startswith('reify-')}
b2_ok = (ast_reify_ws_deps == {'reify-core'}) and len(ast_tree_sitter_deps) == 0
b2_detail = ''
if ast_reify_ws_deps != {'reify-core'}:
    b2_detail += f'reify-ast workspace deps={sorted(ast_reify_ws_deps)} (expected {{reify-core}}); '
if ast_tree_sitter_deps:
    b2_detail += f'reify-ast has tree-sitter normal/build deps: {sorted(ast_tree_sitter_deps)}'
check(2, b2_ok, b2_detail.strip())

# ── B3: reify-ir normal/build workspace deps ⊆ {{reify-core, reify-ast}} ────
ir_ws_deps = {d for d in workspace_normal_deps('reify-ir') if d.startswith('reify-')}
allowed_ir = {'reify-core', 'reify-ast'}
ir_extra = ir_ws_deps - allowed_ir
check(3,
      len(ir_extra) == 0,
      f'reify-ir has forbidden workspace deps: {sorted(ir_extra)}')

# ── B4: reify-syntax normal/build workspace deps ⊆ {{reify-core, reify-ast}} ─
syntax_ws_deps = {d for d in workspace_normal_deps('reify-syntax') if d.startswith('reify-')}
allowed_syntax = {'reify-core', 'reify-ast'}
syntax_extra = syntax_ws_deps - allowed_syntax
check(4,
      len(syntax_extra) == 0,
      f'reify-syntax has forbidden normal/build workspace deps: {sorted(syntax_extra)}')

# ── B5: reify-ast does NOT depend on reify-ir, reify-syntax, reify-types ─────
forbidden_ast_deps = {'reify-ir', 'reify-syntax', 'reify-types'}
ast_all_ws_deps = all_workspace_deps('reify-ast')
ast_forbidden_found = ast_all_ws_deps & forbidden_ast_deps
check(5,
      len(ast_forbidden_found) == 0,
      f'reify-ast has back-edge deps: {sorted(ast_forbidden_found)}')

# ── B6: reify-types is NOT a workspace member AND ────────────────────────────
#        no workspace package depends on reify-types (any dep kind)
reify_types_is_member = 'reify-types' in workspace_member_names
reify_types_dependents = []
for pkg_id in workspace_member_ids:
    pkg_name = id_to_name[pkg_id]
    for dep_id in all_deps.get(pkg_id, []):
        if id_to_name.get(dep_id) == 'reify-types':
            reify_types_dependents.append(pkg_name)
b6_ok = (not reify_types_is_member) and (len(reify_types_dependents) == 0)
b6_detail = ''
if reify_types_is_member:
    b6_detail += 'reify-types is still a workspace member; '
if reify_types_dependents:
    b6_detail += f'workspace packages still depend on reify-types: {sorted(set(reify_types_dependents))}'
check(6, b6_ok, b6_detail.strip())

# ── Summary ──────────────────────────────────────────────────────────────────
if failures:
    print(f'SUMMARY: FAIL (checks failed: {failures})')
    sys.exit(1)
else:
    print('SUMMARY: OK')
    sys.exit(0)
"
