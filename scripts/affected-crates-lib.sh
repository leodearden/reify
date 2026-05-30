#!/usr/bin/env bash
# scripts/affected-crates-lib.sh — maps a changed-file list to the affected
# workspace-crate set (direct crates ∪ their reverse-dependency closure).
#
# Contract references:
#   docs/prds/verify-scope-contract.md
#     §3  C3 — Reverse-closure completeness
#         C4 — Global changes force ALL
#         C5 — Fail safe, fail wide
#     §5  File→crate mapping table
#     §6  Algorithm
#
# Designed to be sourced, not executed directly:
#   source "$(dirname "${BASH_SOURCE[0]}")/affected-crates-lib.sh"
#
# Provides:
#   affected_crates <file>...  prints the affected workspace crate names
#                              (sorted, one per line), or the literal ALL.
#                              Always returns 0.
#
# Sourced by:
#   scripts/verify.sh           (Phase 2 narrowing)
#   tests/infra/test_affected_crates_lib.sh  (drift catcher)

# Source guard — prevent double-sourcing.
if [ "${_REIFY_AFFECTED_CRATES_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_AFFECTED_CRATES_LIB_SOURCED=1

set -euo pipefail

_AFFECTED_CRATES_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# _is_global <path> — returns 0 (true) if the path is a C4 workspace-global file.
# Matches: root Cargo.toml, Cargo.lock, .cargo/**, tree-sitter-reify/**,
#          rust-toolchain and rust-toolchain.toml.
_is_global() {
    local path="$1"
    case "$path" in
        Cargo.toml|Cargo.lock) return 0 ;;
        .cargo/*)              return 0 ;;
        tree-sitter-reify/*)   return 0 ;;
        rust-toolchain*)       return 0 ;;
    esac
    return 1
}

# _is_noncrate <path> — returns 0 (true) if the path is a non-crate file that
# contributes no crates and must NOT force ALL.
# Matches: docs/** (documentation) and gui/src/** (frontend-only).
_is_noncrate() {
    local path="$1"
    case "$path" in
        docs/*)    return 0 ;;
        gui/src/*) return 0 ;;
    esac
    return 1
}

# _file_to_crate <path> — map a crate-owned path to its crate name, or print
# nothing if the path is not under a known crate location.
# Mapping rules (§5):
#   crates/<name>/**  -> <name>
#   gui/src-tauri/**  -> reify-gui
_file_to_crate() {
    local path="$1"
    case "$path" in
        crates/*/*)
            # Extract the crate name: crates/<name>/...
            local rest="${path#crates/}"
            echo "${rest%%/*}"
            ;;
        gui/src-tauri/*)
            echo "reify-gui"
            ;;
        *)
            # No mapping found.
            ;;
    esac
}

# _reverse_closure — read seed crate names from stdin (one per line), emit the
# BFS reverse-dependency closure (seeds + all workspace crates that transitively
# depend on them), sorted-unique, one per line.
#
# Technique mirrors occt-scope-lib.sh:occt_touching_set (lines 59-110):
#   - single `cargo metadata --format-version 1` piped into python3
#   - reverse adjacency R[dep_id] += pkg_id over workspace-internal edges of
#     ALL kinds (null/build/dev)
#   - BFS from the seed IDs, inclusive
#   - intersect with workspace_members, print sorted-unique names
#
# On any cargo failure or python error, prints ALL (C5).
_reverse_closure() {
    local seeds
    seeds="$(cat)"
    [ -n "$seeds" ] || return 0

    # Collect metadata once; guard failure -> ALL.
    local meta
    meta="$(cargo metadata --format-version 1 2>/dev/null)" || { echo ALL; return 0; }
    [ -n "$meta" ] || { echo ALL; return 0; }

    printf '%s\n' "$meta" | python3 -c "
import sys, json
try:
    seeds_raw = '''$seeds'''
    seed_names = set(s.strip() for s in seeds_raw.strip().splitlines() if s.strip())

    m = json.load(sys.stdin)
    members = set(m['workspace_members'])
    id_to_name = {p['id']: p['name'] for p in m['packages']}
    name_to_ids = {}
    for p in m['packages']:
        name_to_ids.setdefault(p['name'], []).append(p['id'])

    # Build reverse adjacency over workspace-internal edges, all dep kinds.
    # R[dep_id] = set of pkg_ids in workspace that depend on dep_id.
    rev = {}
    for node in m['resolve']['nodes']:
        if node['id'] not in members:
            continue
        for d in node['deps']:
            if d['pkg'] not in members:
                continue
            rev.setdefault(d['pkg'], set()).add(node['id'])

    # BFS from all IDs matching any seed name, inclusive.
    seed_ids = set()
    for sn in seed_names:
        seed_ids.update(name_to_ids.get(sn, []))

    visited = set(seed_ids)
    queue = list(seed_ids)
    while queue:
        curr = queue.pop()
        for dep_on_curr in rev.get(curr, []):
            if dep_on_curr not in visited:
                visited.add(dep_on_curr)
                queue.append(dep_on_curr)

    result = sorted({id_to_name[i] for i in visited if i in members})
    for name in result:
        print(name)
except Exception:
    print('ALL')
    sys.exit(0)
" || { echo ALL; return 0; }
}

# affected_crates <file>... — print the affected workspace crate set, one name
# per line, sorted; or print the literal ALL if any C4/C5 condition fires.
# Always returns 0 so callers are safe under set -e and inside $() capture.
affected_crates() {
    # C4: if any arg is a global file, immediately emit ALL.
    local arg
    for arg in "$@"; do
        if _is_global "$arg"; then
            echo ALL
            return 0
        fi
    done

    # Accumulate the direct crate set from crate-mappable paths.
    local direct=()
    local crate
    for arg in "$@"; do
        if _is_noncrate "$arg"; then
            # Non-crate path: skip, contributes nothing.
            continue
        fi
        crate="$(_file_to_crate "$arg")"
        if [ -n "$crate" ]; then
            direct+=("$crate")
        else
            # C5: unmappable path — fail wide.
            echo ALL
            return 0
        fi
    done

    # If no direct crates were accumulated, print nothing.
    if [ "${#direct[@]}" -eq 0 ]; then
        return 0
    fi

    # Expand the direct crate set through the reverse-dependency closure, then
    # emit sorted-unique (one crate per line).
    printf '%s\n' "${direct[@]}" | _reverse_closure | sort -u
    return 0
}
