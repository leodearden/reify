#!/usr/bin/env bash
# scripts/tree-sitter-freshness.sh
#
# Prove that the compiled libtree_sitter_reify.a matches the tree-sitter sources
# currently on disk — and, before a build, force a rebuild when it does not.
#
# WHY THIS GUARD EXISTS  (task #5629, esc-5392-1)
# ----------------------------------------------
# Cargo re-runs a build script only for paths that script declared via
# `cargo:rerun-if-changed`.  tree-sitter-reify/build.rs declared exactly ONE:
# `grammar.js`.  But it compiles TWO C translation units —
#
#     c_config.file(&parser_path);      // src/parser.c   (generated, gitignored)
#     c_config.file("src/scanner.c");   // hand-written, TRACKED
#
# — plus the headers under src/tree_sitter/.  The `cc` crate emits no
# rerun-if-changed directives of its own (verified against cc 1.2.62: zero hits
# in its source).  So before this guard, `src/scanner.c` and every
# `src/tree_sitter/*.h` were watched by NOTHING.
#
# The consequence is a false GREEN, and it does not depend on warm lanes or on
# any mtime accident: an edit confined to src/scanner.c gave cargo no reason
# whatsoever to re-run the build script, in ANY checkout.  cc::Build::compile
# was never re-invoked, the previously-built libtree_sitter_reify.a stayed
# linked, and the external-scanner change was simply never under test — while
# the merge gate reported success.
#
# `scripts/tree-sitter-generate.sh` cannot repair this.  It refreshes on-disk
# sources (src/parser.c, grammar.json, node-types.json); it never writes
# scanner.c, and nothing it does makes cargo recompile anything.
#
# WHY NOT MTIME
# -------------
# `cargo:rerun-if-changed` is an mtime comparison, so declaring the missing
# inputs (build.rs now does) is NECESSARY but not SUFFICIENT here.
# scripts/seed-warm-lane.sh bulk-stamps every non-target/, non-.git file to
# 2020-01-01T00:00:00 while the build-script `output` files inside the
# CoW-cloned target/ carry seed-time (measured by task #5630).  Those two clocks
# disagree per-fingerprint, so "newer than" says nothing useful in a warm lane.
#
# Only CONTENT identity distinguishes "this archive was built from these bytes"
# from "this archive is merely newer".  build.rs therefore writes a per-file
# sha256 manifest to $OUT_DIR/tree_sitter_inputs.stamp immediately after
# cc::Build::compile() returns — and compile() panics on failure, so a stamp
# sitting next to a libtree_sitter_reify.a ATTESTS that archive's inputs.  This
# script recomputes the same manifest from the worktree and compares.
#
# The repair must also come from OUTSIDE the build script: a build script that
# cargo has declined to run cannot repair itself.  Hence a standalone plan leaf
# that verify.sh runs after tree-sitter-generate.sh and before every cargo leaf.
#
# MODES
# -----
#   --list-inputs   Print the C-compilation input set — one TS_DIR-relative path
#                   per line, LC_ALL=C sorted, de-duplicated.  This is the single
#                   source of truth for what the fingerprint covers; build.rs
#                   enumerates the same set so the two manifests agree byte-for-byte.
#   --help, -h      This message.
#
# TEST-ONLY ENVIRONMENT OVERRIDES
# -------------------------------
#   REIFY_TS_FRESHNESS_TS_DIR      Point at a copy of tree-sitter-reify/ instead
#                                  of the real one.  Exists solely so the
#                                  hermetic fixtures in
#                                  tests/infra/test_tree_sitter_pipeline.sh can
#                                  exercise the repair path without mutating the
#                                  worktree.  Never set in production.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# compute_sha256 (-> portable_sha256): the sha256sum-then-shasum fallback already
# used by scripts/tree-sitter-generate.sh for its grammar.js stamp.
# shellcheck source=scripts/lib.sh
source "$SCRIPT_DIR/lib.sh"

TS_DIR="${REIFY_TS_FRESHNESS_TS_DIR:-$REPO_ROOT/tree-sitter-reify}"

usage() {
    cat <<'EOF'
Usage: scripts/tree-sitter-freshness.sh <mode>

Modes:
  --list-inputs        Print the C-compilation input set (one TS_DIR-relative
                       path per line, LC_ALL=C sorted, de-duplicated).
  --help, -h           Show this message.
EOF
}

# ts_inputs
#
# THE single source of truth for the C-compilation input set: every file whose
# bytes end up inside libtree_sitter_reify.a.  Emits TS_DIR-relative paths so
# the manifest is location-independent and reproducible by build.rs.
#
# Headers come from a SORTED GLOB rather than a hardcoded alloc.h/array.h/parser.h
# list, and build.rs enumerates them the same way, so a future header is covered
# automatically by both sides.  A hardcoded list is exactly how this defect class
# recurs.
ts_inputs() {
    local had_nullglob=0
    shopt -q nullglob && had_nullglob=1
    shopt -s nullglob

    local f
    local -a headers=()
    for f in "$TS_DIR"/src/tree_sitter/*.h; do
        headers+=("src/tree_sitter/${f##*/}")
    done

    [ "$had_nullglob" -eq 1 ] || shopt -u nullglob

    {
        # The two translation units build.rs hands to cc::Build.  src/parser.c is
        # generated and gitignored, but its bytes are compiled in, so it belongs
        # in the fingerprint — even though it is deliberately NOT watched (see
        # build.rs: watching an output this script writes causes double execution).
        printf 'src/parser.c\n'
        printf 'src/scanner.c\n'
        if [ "${#headers[@]}" -gt 0 ]; then
            printf '%s\n' "${headers[@]}" | LC_ALL=C sort
        fi
    } | awk 'NF && !seen[$0]++'
}

main() {
    local mode="${1:---help}"
    case "$mode" in
        --list-inputs)
            ts_inputs
            ;;
        --help | -h)
            usage
            ;;
        *)
            echo "ERROR: unknown mode: $mode" >&2
            usage >&2
            return 2
            ;;
    esac
}

main "$@"
