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
#   --print-fingerprint
#                   Print the per-file sha256 manifest for that input set —
#                   '<hash>  <relpath>' lines, sorted by relpath.  Exactly what
#                   build.rs writes to $OUT_DIR/tree_sitter_inputs.stamp.
#   check           Hard assertion.  Exit 1 if ANY built archive's sibling stamp
#                   is missing or disagrees with the current fingerprint, naming
#                   the offending fingerprint dir and the first differing input.
#                   For a human/agent checkpoint.
#   --help, -h      This message.
#
# FAIL POLICY
# -----------
# A missing stamp beside an archive means that archive's provenance is UNPROVEN,
# which is treated as STALE — never as fresh.  Two conditions instead fail OPEN
# to a labelled skip, matching the reify-bin-freshness.sh / reify-audit-freshness.sh
# precedent: a host with neither sha256sum nor shasum (nothing can be attested,
# so a hard failure would be a permanent spurious RED), and a target tree with
# nothing built yet (nothing built means nothing can be stale).
#
# TEST-ONLY ENVIRONMENT OVERRIDES
# -------------------------------
#   REIFY_TS_FRESHNESS_TS_DIR      Point at a copy of tree-sitter-reify/ instead
#                                  of the real one.  Exists solely so the
#                                  hermetic fixtures in
#                                  tests/infra/test_tree_sitter_pipeline.sh can
#                                  exercise the repair path without mutating the
#                                  worktree.  Never set in production.
#   REIFY_TS_FRESHNESS_TARGET_DIR  Point at a fake cargo build tree instead of
#                                  ./target.  Same rationale.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# compute_sha256 (-> portable_sha256): the sha256sum-then-shasum fallback already
# used by scripts/tree-sitter-generate.sh for its grammar.js stamp.
# shellcheck source=scripts/lib.sh
source "$SCRIPT_DIR/lib.sh"

TS_DIR="${REIFY_TS_FRESHNESS_TS_DIR:-$REPO_ROOT/tree-sitter-reify}"
TARGET_DIR="${REIFY_TS_FRESHNESS_TARGET_DIR:-$REPO_ROOT/target}"

# The manifest build.rs writes next to each libtree_sitter_reify.a it produces.
STAMP_NAME="tree_sitter_inputs.stamp"

# Distinct return code meaning "no hasher on this host" — callers map it to a
# labelled SKIP rather than to a failure or to a repair attempt.
TS_RC_UNAVAILABLE=3

usage() {
    cat <<'EOF'
Usage: scripts/tree-sitter-freshness.sh <mode>

Modes:
  --list-inputs        Print the C-compilation input set (one TS_DIR-relative
                       path per line, LC_ALL=C sorted, de-duplicated).
  --print-fingerprint  Print the per-file sha256 manifest for that input set
                       ('<hash>  <relpath>' lines, sorted by relpath) — exactly
                       what build.rs writes to $OUT_DIR/tree_sitter_inputs.stamp.
  check                Assert every built libtree_sitter_reify.a was compiled
                       from the sources currently on disk. Exit 1 on any
                       mismatch or missing attestation.
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

# ts_fingerprint
#
# Print the per-file sha256 manifest for the input set: '<hash>  <relpath>'
# lines in ts_inputs() order (already LC_ALL=C sorted by relpath).  The relpath
# column is normalised to TS_DIR-relative, so the manifest is location-
# independent and byte-reproducible by build.rs from a different cwd.
#
# Returns $TS_RC_UNAVAILABLE (printing the single literal line UNAVAILABLE) when
# the host has no hasher; returns 1 naming the path when an input is missing.
ts_fingerprint() {
    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        printf 'UNAVAILABLE\n'
        return "$TS_RC_UNAVAILABLE"
    fi

    local rel abs hash
    while IFS= read -r rel; do
        [ -n "$rel" ] || continue
        abs="$TS_DIR/$rel"
        if [ ! -f "$abs" ]; then
            echo "ERROR: tree-sitter freshness input is missing: $abs" >&2
            echo "       Run scripts/tree-sitter-generate.sh first — every compiled input," >&2
            echo "       including the generated src/parser.c, must exist on disk before it" >&2
            echo "       can be fingerprinted." >&2
            return 1
        fi
        # compute_sha256 -> '<hash>  <path-as-given>'; keep the hash, substitute
        # the normalised relpath.
        hash=$(compute_sha256 "$abs" | awk '{print $1}')
        printf '%s  %s\n' "$hash" "$rel"
    done < <(ts_inputs)
}

# ts_archives
#
# Print every built libtree_sitter_reify.a under TARGET_DIR, de-duplicated.
# Both the plain (target/<profile>/build/...) and the --target-qualified
# (target/<triple>/<profile>/build/...) layouts are covered: there is no cheap
# way to tell from the shell WHICH fingerprint dir cargo will actually link, so
# all of them must be attested.
ts_archives() {
    local had_nullglob=0
    shopt -q nullglob && had_nullglob=1
    shopt -s nullglob

    local a
    local -a found=()
    for a in "$TARGET_DIR"/*/build/tree-sitter-reify-*/out/libtree_sitter_reify.a \
             "$TARGET_DIR"/*/*/build/tree-sitter-reify-*/out/libtree_sitter_reify.a; do
        found+=("$a")
    done

    [ "$had_nullglob" -eq 1 ] || shopt -u nullglob

    if [ "${#found[@]}" -gt 0 ]; then
        printf '%s\n' "${found[@]}" | LC_ALL=C sort -u
    fi
}

# ts_first_differing_input <stamp-file>
#
# Name the first input whose attested hash differs from the current one, so a
# stale verdict points at the actual culprit file rather than just at a hex
# fingerprint dir. Compares against $CURRENT_FP.
ts_first_differing_input() {
    local stamp_file="$1"
    local -A attested=()
    local line h p

    while IFS= read -r line; do
        [ -n "$line" ] || continue
        h="${line%% *}"
        p="${line##*  }"
        attested["$p"]="$h"
    done < "$stamp_file"

    while IFS= read -r line; do
        [ -n "$line" ] || continue
        h="${line%% *}"
        p="${line##*  }"
        if [ -z "${attested[$p]:-}" ]; then
            printf '%s (not present in the attested manifest)' "$p"
            return 0
        fi
        if [ "${attested[$p]}" != "$h" ]; then
            printf '%s' "$p"
            return 0
        fi
    done <<< "$CURRENT_FP"

    printf '(input set shrank: the attested manifest lists paths that are no longer compiled)'
}

# ts_scan
#
# The shared freshness computation behind both `check` and `ensure`.
# Sets, in the caller's shell:
#   CURRENT_FP           the current fingerprint manifest
#   TS_SCAN_STATUS       fresh | stale | skip | none
#   TS_SCAN_SKIP_REASON  set when status=skip
#   TS_STALE_REPORT      newline-joined, one actionable line per stale archive
ts_scan() {
    TS_SCAN_STATUS=""
    TS_SCAN_SKIP_REASON=""
    TS_STALE_REPORT=""

    local rc=0
    CURRENT_FP=$(ts_fingerprint) || rc=$?
    if [ "$rc" -eq "$TS_RC_UNAVAILABLE" ]; then
        TS_SCAN_STATUS="skip"
        TS_SCAN_SKIP_REASON="neither sha256sum nor shasum on PATH — archive inputs cannot be attested"
        return 0
    elif [ "$rc" -ne 0 ]; then
        return "$rc"
    fi

    local archives
    archives=$(ts_archives)
    if [ -z "$archives" ]; then
        TS_SCAN_STATUS="none"
        return 0
    fi

    local a dir stamp attested
    local -a stale=()
    while IFS= read -r a; do
        [ -n "$a" ] || continue
        # .../build/tree-sitter-reify-<fingerprint>/out/libtree_sitter_reify.a
        dir="$(basename "$(dirname "$(dirname "$a")")")"
        stamp="$(dirname "$a")/$STAMP_NAME"

        if [ ! -f "$stamp" ]; then
            stale+=("$dir — no $STAMP_NAME beside the archive; its provenance is UNPROVEN")
            continue
        fi

        attested=$(cat "$stamp")
        if [ "$attested" = "UNAVAILABLE" ]; then
            TS_SCAN_STATUS="skip"
            TS_SCAN_SKIP_REASON="$dir was built on a host without sha256sum/shasum — its inputs are unattestable"
            return 0
        fi
        if [ "$attested" != "$CURRENT_FP" ]; then
            stale+=("$dir — built from different bytes; first differing input: $(ts_first_differing_input "$stamp")")
        fi
    done <<< "$archives"

    if [ "${#stale[@]}" -gt 0 ]; then
        TS_SCAN_STATUS="stale"
        TS_STALE_REPORT=$(printf '%s\n' "${stale[@]}")
    else
        TS_SCAN_STATUS="fresh"
    fi
}

ts_report_stale() {
    local line
    echo "tree-sitter-freshness: STALE — these archives were NOT built from the sources now on disk:" >&2
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        echo "    $line" >&2
    done <<< "$TS_STALE_REPORT"
}

ts_mode_check() {
    echo "tree-sitter-freshness: check — attesting libtree_sitter_reify.a against $TS_DIR"
    ts_scan
    case "$TS_SCAN_STATUS" in
        skip)
            echo "tree-sitter-freshness: SKIP — $TS_SCAN_SKIP_REASON"
            return 0
            ;;
        none)
            echo "tree-sitter-freshness: no built archive under $TARGET_DIR; nothing to check"
            return 0
            ;;
        fresh)
            echo "tree-sitter-freshness: FRESH — every built archive matches the sources on disk"
            return 0
            ;;
        stale)
            ts_report_stale
            return 1
            ;;
        *)
            echo "ERROR: internal — unexpected scan status '$TS_SCAN_STATUS'" >&2
            return 1
            ;;
    esac
}

main() {
    local mode="${1:---help}"
    case "$mode" in
        --list-inputs)
            ts_inputs
            ;;
        --print-fingerprint)
            # An unhashable host still exits 0: it has successfully reported the
            # degradation, and the callers map the UNAVAILABLE line to a SKIP.
            local fp_rc=0
            ts_fingerprint || fp_rc=$?
            [ "$fp_rc" -eq "$TS_RC_UNAVAILABLE" ] && return 0
            return "$fp_rc"
            ;;
        check)
            ts_mode_check
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
