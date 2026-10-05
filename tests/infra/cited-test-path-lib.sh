#!/usr/bin/env bash
# tests/infra/cited-test-path-lib.sh — shared cited-test-path scan/resolve data.
#
# SINGLE SOURCE OF TRUTH for the derivation behind the cited-test-path
# resolution contract (task #7095). Consumers:
#   - tests/infra/test_cited_test_paths_resolve.sh — the merge gate (the
#     ratchet + the vacuity floor) AND, via its `--emit-baseline` entry point,
#     the generator of tests/infra/cited-test-path-baseline.manifest.
# No consumer may re-derive any of this. The baseline is produced by the very
# functions the gate asserts against, so the file on disk and the check that
# reads it cannot drift: a second hand-rolled pipeline is exactly the failure
# this lib exists to prevent.
#
# Designed to be sourced, not executed directly:
#   source "$(dirname "${BASH_SOURCE[0]}")/cited-test-path-lib.sh"
#
# EVERY function takes the repo root as its FIRST argument and never reads a
# global REPO_ROOT. That is the testability seam: the gate can point the same
# derivation at a throwaway `git init` fixture repo, which is what makes its
# scenarios hermetic.
#
# Provides:
#   cited_test_path_exclusions <repo-root>
#       print the git pathspecs the citation scan must exclude, one per line.
#   cited_test_path_index <repo-root>
#       print "<crate>/<basename>\t<repo-relative-path>" for every tracked
#       test unit under crates/*/tests/.
#   cited_test_path_citations <repo-root>
#       print "<containing-file>\t<cited-path>" for every citation occurrence
#       across ALL tracked files.
#   cited_test_path_scan <repo-root>
#       print "<containing-file>\t<cited-path>\t<verdict>\t<targets>" for every
#       citation that does NOT resolve but whose basename does.
#   cited_test_path_fingerprint            (stdin -> stdout)
#       reduce scan records to "<containing-file> :: <cited-path>".
#   cited_test_path_baseline_path
#       the grandfather-baseline manifest path (honors
#       REIFY_CITED_TEST_PATH_BASELINE).
#   cited_test_path_baseline_rows [baseline]
#       the DATA ROWS of [baseline] — every line neither comment nor blank.

_CITED_TEST_PATH_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---------------------------------------------------------------------------
# THE CITATION SHAPE. One definition, consumed by the scan and quoted by the
# baseline's grammar check.
#
# `crates/<crate>/tests/<anything>.rs`, where <crate> is a cargo-style
# lowercase name and the tail may be nested (harness_<subsystem>/unit.rs) or
# flat (unit.rs). Deliberately permissive in the tail: the resolver, not the
# regex, decides whether a match is interesting.
# ---------------------------------------------------------------------------
CITED_TEST_PATH_REGEX='crates/[a-z0-9-]+/tests/[A-Za-z0-9_./-]+\.rs'

# ---------------------------------------------------------------------------
# SCAN EXCLUSIONS — load-bearing, not cosmetic. NOT a self-exclusion list: it
# carries a third-party data file too, on the principle below.
#
# THE PRINCIPLE — MENTION, NOT USE. A file is excluded when the citation shape
# appears in it as this tool's own SUBJECT MATTER rather than as a reference a
# reader is meant to follow. Repointing a mention falsifies the record that
# carries it, so a finding against one is never actionable. Same principle,
# and the same shape of judgement, as the ALLOWLIST_PREFIXES in
# crates/reify-audit/src/ptodo.rs, which exempts files carrying the TODO
# pattern as the data they operate on. Decide the next case against THAT, not
# against "the scan is noisy here".
#
# ROWS 1-3 ARE STRUCTURAL. This gate's own three artifacts quote the citation
# shape: the baseline is ~300 rows each ENDING in a stale cited path, and the
# lib and the gate carry the regex plus worked examples. Without exclusion the
# scan would harvest its own baseline as ~300 fresh citations, every one of
# them stale, and regenerating the baseline would fold it into itself. Their
# mention-shape is not a property of what they happen to contain; it follows
# from what the files ARE, so rows 1-3 need no re-audit, ever.
#
# ROW 4 IS DIFFERENT IN KIND. docs/legibility/confusion-codebook.yaml is
# dark-factory's agent-confusion registry, written by its
# scripts/legibility/codebook.py merger out of the nightly legibility trickle.
# Its `cause:` / `evidence_quote:` / sighting `note:` fields carry LLM-authored
# prose recording agents that were handed a path which did not exist — so in a
# sighting the STALE PATH IS THE PAYLOAD, and repointing it would destroy the
# finding it is evidence for. Its exclusion therefore rests on a MEASURED,
# CONTINGENT property of the file's content, NOT on a structural guarantee the
# way rows 1-3 do: measured at the time of writing, all 16 citation
# occurrences in that file are mention and not use (10 in evidence_quote, 3 in
# sighting note, 3 in cause), every one naming the path as the thing that was
# missing rather than as a reference to open. Zero live references are blinded.
#
# RE-AUDIT TRIGGER for row 4. That property can stop holding. The codebook's
# schema is OPEN-WORLD — codebook.py constrains only the v2 structural fields
# and leaves additionalProperties permissive — and the v1 vocabulary it still
# admits includes REMEDIATION-shaped fields, `fix` and `fix_where`, which are
# unpopulated today but WOULD carry live references a reader is meant to
# follow. If either starts being written, revisit this exclusion: part of the
# file becomes use rather than mention, and a whole-file exclusion is then too
# coarse.
#
# AND THE VACUITY FLOOR CANNOT POLICE THAT. Do not assume Section I has it
# covered. The codebook is ~16 of ~1925 citation occurrences — 0.83%, three
# orders of magnitude below anything the floor's bounds can resolve — so
# dropping it moves no observable the floor watches, and NOTHING will
# automatically signal when this exemption goes stale. The trigger above is a
# human obligation, not a check.
#
# Defined ONCE here so the gate and the generator cannot drift apart — the
# `:(exclude)` pathspec idiom tests/infra/test_orchestrator_config_canonical_path.sh
# uses to exclude itself.
#
# The <repo-root> argument is accepted for interface uniformity with every
# other function here (and so a future root-dependent exclusion needs no
# caller change); the list itself is static.
# ---------------------------------------------------------------------------
cited_test_path_exclusions() {
    printf '%s\n' \
        ':(exclude)tests/infra/cited-test-path-baseline.manifest' \
        ':(exclude)tests/infra/cited-test-path-lib.sh' \
        ':(exclude)tests/infra/test_cited_test_paths_resolve.sh' \
        ':(exclude)docs/legibility/confusion-codebook.yaml'
}

# ---------------------------------------------------------------------------
# cited_test_path_index <repo-root>
#
# Keyed by <crate>/<basename> because that is the resolution key: a test unit
# that moves keeps its basename and its crate, which is what makes a stale
# citation repointable at all.
#
# `git ls-files -- 'crates/*/tests/*.rs'` matches BOTH the flat
# crates/<c>/tests/<f>.rs and the nested crates/<c>/tests/harness_<s>/<f>.rs
# forms: a git pathspec wildcard crosses `/` unless :(glob) magic is used, so
# one pattern covers the whole tests tree (measured on the live tree: 1304
# units = 549 flat + 755 nested). Adding a second `**` pattern would
# DOUBLE-COUNT every nested unit, not widen coverage.
#
# Sourcing from `git ls-files` rather than the filesystem is deliberate: an
# untracked scratch file in a working tree must never change the verdict.
# ---------------------------------------------------------------------------
cited_test_path_index() {
    local root="$1"
    git -C "$root" ls-files -- 'crates/*/tests/*.rs' \
        | awk -F/ 'NF >= 4 { print $2 "/" $NF "\t" $0 }' \
        | sort -u
}

# ---------------------------------------------------------------------------
# cited_test_path_citations <repo-root>
#
# The RAW corpus: every occurrence, pre-resolution. Filtering is the
# resolver's job, not this function's — keeping them separate is what lets the
# vacuity floor observe corpus size independently of how many findings the
# resolver produced.
#
# NO EXTENSION FILTER. Extension-agnosticism is the ABSENCE of a filter, not
# an allowlist: the stale citations measured on the live tree span 8
# extensions (.ri, .md, .rs, .yaml, .txt, .sh, .js, .grammar) across crates/,
# examples/, docs/, tests/, tree-sitter-reify/, gui/ and .claude/, and a
# ninth would silently escape an allowlist.
#
# `-z` puts a NUL between the filename and the match instead of a `:`, so a
# path legally containing a colon cannot be mis-split. `|| true` because
# git grep exits 1 on no match, which would abort a `set -e` caller.
# ---------------------------------------------------------------------------
cited_test_path_citations() {
    local root="$1"
    local -a excl
    mapfile -t excl < <(cited_test_path_exclusions "$root")
    git -C "$root" grep -z -I -Eo "$CITED_TEST_PATH_REGEX" -- . "${excl[@]}" 2>/dev/null \
        | tr '\0' '\t' \
        || true
}

# ---------------------------------------------------------------------------
# cited_test_path_scan <repo-root>
#
# The resolver. For each citation:
#   - cited path IS a tracked file          -> emit nothing (it resolves);
#   - basename hits exactly one other path  -> STALE, with the suggested target;
#   - basename hits more than one path      -> STALE, `ambiguous:<N>`, listing
#                                              every candidate (naming an
#                                              arbitrary one would be a guess
#                                              presented as an answer);
#   - basename hits nothing                 -> emit nothing.
#
# That last rule is what holds the false-positive count at zero: of the 257
# unresolved cited paths measured on the live tree, 107 are deleted files or
# synthetic fixture paths (`crates/foo/tests/bar.rs`) with no basename hit,
# and repointing them is impossible. Out of charter, so not reported.
#
# Output is STRUCTURED — four tab-separated fields — so no consumer has to
# re-parse a prose line.
#
# ONE awk pass over the corpus, joined against two in-memory tables, rather
# than a lookup per citation: the per-citation shape forks ~257 subshells and
# measured 14.9s against the live tree, versus 0.57s for this one (budget:
# under 1s, since the gate runs in run_all.sh's `pool`).
# ---------------------------------------------------------------------------
cited_test_path_scan() {
    local root="$1"
    local idx tracked
    idx="$(mktemp)" || return 1
    tracked="$(mktemp)" || { rm -f "$idx"; return 1; }

    cited_test_path_index "$root" > "$idx"
    git -C "$root" ls-files > "$tracked"

    cited_test_path_citations "$root" | awk -F'\t' -v IDXF="$idx" -v TRF="$tracked" '
        BEGIN {
            FS = "\t"; OFS = "\t"
            while ((getline line < TRF) > 0) is_tracked[line] = 1
            while ((getline line < IDXF) > 0) {
                split(line, f, "\t")
                if (f[1] in hits) { hits[f[1]] = hits[f[1]] "," f[2]; n[f[1]]++ }
                else              { hits[f[1]] = f[2];                n[f[1]] = 1 }
            }
        }
        {
            file = $1; cited = $2
            if (file == "" || cited == "") next
            if (cited in is_tracked) next           # resolves: nothing to say
            parts = split(cited, p, "/")
            if (parts < 4) next
            key = p[2] "/" p[parts]
            if (!(key in hits)) next                # deleted or synthetic
            if (n[key] == 1) print file, cited, "stale", hits[key]
            else             print file, cited, "ambiguous:" n[key], hits[key]
        }
    ' | sort -u
    local rc=$?

    # Explicit rc capture so the temp files are removed even when the caller
    # runs under `set -e` and the pipeline fails: an early `return` past the
    # cleanup would leak one pair of files per aborted run.
    rm -f "$idx" "$tracked"
    return "$rc"
}

# ---------------------------------------------------------------------------
# cited_test_path_fingerprint   (scan records on stdin -> fingerprints on stdout)
#
# Reduces a scan record to "<containing-file> :: <cited-path>", deliberately
# erasing BOTH the line number (never captured in the first place) and the
# suggested target. Moving a citation within its file, or a later change to
# where its basename resolves, must not spuriously red the ratchet — the
# grandfathered fact is "this file cites this stale path", nothing finer.
#
# Mirrors the `path :: kind :: text` shape of crates/reify-audit/ptodo-baseline.txt.
# ---------------------------------------------------------------------------
cited_test_path_fingerprint() {
    awk -F'\t' 'NF >= 2 && $1 != "" && $2 != "" { print $1 " :: " $2 }' | sort -u
}

# ---------------------------------------------------------------------------
# cited_test_path_baseline_path
#
# Honors REIFY_CITED_TEST_PATH_BASELINE so the ratchet scenarios can drive
# fixture baselines without ever mutating the committed one — the
# harness_layout_baseline_path convention.
# ---------------------------------------------------------------------------
cited_test_path_baseline_path() {
    printf '%s\n' \
        "${REIFY_CITED_TEST_PATH_BASELINE:-$_CITED_TEST_PATH_LIB_DIR/cited-test-path-baseline.manifest}"
}

# ---------------------------------------------------------------------------
# cited_test_path_baseline_rows [baseline]
#
# THE single definition of "a data row of the baseline": every line that is
# neither a comment (^\s*#) nor blank. Same stripping style as
# run-all-classification-lib.sh and harness-layout-lib.sh.
#
# A MISSING baseline prints nothing and returns 0 (each caller decides what
# that means); an UNREADABLE one returns grep's error status (>= 2), never a
# vacuous "zero rows".
# ---------------------------------------------------------------------------
cited_test_path_baseline_rows() {
    local baseline="${1:-$(cited_test_path_baseline_path)}"
    [ -f "$baseline" ] || return 0
    grep -Ev '^[[:space:]]*(#|$)' "$baseline" || {
        local rc=$?
        [ "$rc" -eq 1 ] && return 0   # no data rows is not an error
        return "$rc"
    }
}
