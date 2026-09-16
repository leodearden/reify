#!/usr/bin/env bash
# tests/infra/test_cited_test_paths_resolve.sh
#
# Guards the CITED-TEST-PATH resolution contract (task #7095): prose that
# names a `crates/<crate>/tests/**/*.rs` file must name a path that still
# resolves, so a harness-consolidation `git mv` cannot silently orphan the
# doc comments, .ri prose, corpus fixtures and READMEs that cite the moved
# test.
#
# WHY THIS GATE EXISTS. The harness_<subsystem>/ consolidation moved test
# units wholesale (commit 276d32f025, "Refresh stale crates/*/tests/<stem>.rs
# prose path references", 123 files) and every citation of a moved unit went
# stale at once. That sweep was a MANUAL cleanup with no gate behind it, so
# the next consolidation re-opens the same hole. This file closes the root
# cause: a citation whose basename resolves elsewhere under the same crate's
# tests tree is reported, with the suggested target, against a committed
# grandfather baseline.
#
# WHAT IS TESTED (built up across the task's TDD steps):
#   - the shared lib tests/infra/cited-test-path-lib.sh: the tracked-test
#     index, the citation scan, the resolver and the fingerprint reduction
#     (this file, step-1);
#   - the committed baseline's existence, grammar, sortedness and self-
#     describing header (step-3);
#   - the one-directional (subset) ratchet checker (step-5);
#   - the baseline-independent vacuity floor (step-7);
#   - whole-tree wiring against the real repository (step-9);
#   - the real-`git mv` acceptance scenario and the stdout/stderr
#     remediation-hint separation (step-11).
#
# SCOPE LIMIT, stated rather than left to inference: a cited path is reported
# ONLY when its basename resolves to some other tracked path under the SAME
# crate's tests tree. A citation of a genuinely DELETED test, or a synthetic
# fixture path that never existed, resolves to nothing and is deliberately
# NOT reported — repointing it is impossible and guessing a target would be
# noise.
#
# Hermetic: pure bash + git plumbing (+ throwaway `git init` temp repos);
# never runs cargo/npm; never mutates the real tree or the real baseline.
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; classified
# `pool` in run-all-classification.manifest.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# The shared lib under test. Deliberately NOT sourced at the top level: this
# file is RED (lib absent) before the impl step, and a top-level `source` of a
# missing file would abort under `set -e` before any assert runs. Every lib
# interaction below therefore happens inside a `bash -c 'source ...'` child
# shell, so a missing lib fails just THAT assert (the
# test_harness_baseline_registration_gate.sh idiom).
LIB="$SCRIPT_DIR/cited-test-path-lib.sh"

echo "=== cited test-path resolution gate (task 7095) ==="

# Single EXIT trap over an array of fixtures: individual `trap ... EXIT` calls
# replace one another, so one handler over an array removes every fixture
# regardless of which section adds the last.
_TMPDIRS=()
trap '[ "${#_TMPDIRS[@]}" -gt 0 ] && rm -rf "${_TMPDIRS[@]}"' EXIT

_mktmpd() {
    local d
    d="$(mktemp -d "${TMPDIR:-/tmp}/reify-cited-path.XXXXXX")"
    _TMPDIRS+=("$d")
    printf '%s\n' "$d"
}

# Fully isolated from user/system git config so no ambient hooksPath, signing
# key or init template can perturb a fixture repo.
_gitf() { GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null git "$@"; }

# _fixture_init <dir> — a fresh repo with a deterministic identity.
_fixture_init() {
    local dir="$1"
    _gitf init -q -b main "$dir"
    _gitf -C "$dir" config user.email t@e.x
    _gitf -C "$dir" config user.name t
}

# _fixture_write <dir> <repo-rel-path> <content> — write (creating parents).
_fixture_write() {
    local dir="$1" rel="$2" content="$3"
    mkdir -p "$dir/$(dirname "$rel")"
    printf '%s\n' "$content" > "$dir/$rel"
}

_fixture_commit() {
    local dir="$1"
    _gitf -C "$dir" add -A
    _gitf -C "$dir" commit -qm fixture
}

# _scan <repo-root> — run the lib's resolver in a child shell.
_scan() {
    bash -c 'set -euo pipefail; source "$1"; cited_test_path_scan "$2"' _ "$LIB" "$1"
}

# _expect_scan <repo-root> [<expected tab-separated record> ...]
#
# Compares the resolver's WHOLE output (order-insensitively) against the
# expected record set; with no expected records, asserts the output is empty.
# Every fixture below that asserts an empty result also carries a record that
# MUST be reported, so a lib that emits nothing at all can never pass
# vacuously.
_expect_scan() {
    local root="$1"; shift
    local exp act rc=0
    exp="$(mktemp)"; act="$(mktemp)"
    if [ "$#" -gt 0 ]; then printf '%s\n' "$@" | sort > "$exp"; fi
    _scan "$root" 2>/dev/null | sort > "$act" || rc=$?
    if ! diff -u "$exp" "$act"; then
        echo "(resolver output mismatch for fixture $root; scan rc=$rc)"
        rm -f "$exp" "$act"
        return 1
    fi
    rm -f "$exp" "$act"
    return 0
}

_rec() { printf '%s\t%s\t%s\t%s' "$1" "$2" "$3" "$4"; }

# ===========================================================================
# Section A: positive control + both negative rules, in ONE fixture.
#
# Co-locating them is deliberate: a resolver that emits nothing would pass
# the two negative rules vacuously, so the fixture pairs them with a stale
# citation that MUST be reported.
# ===========================================================================
echo ""
echo "--- Section A: stale reported; resolving and unresolvable citations ignored ---"

FIX_BASIC="$(_mktmpd)/repo"
_fixture_init "$FIX_BASIC"
_fixture_write "$FIX_BASIC" crates/mycrate/tests/harness_sub/moved_test.rs '// moved here'
_fixture_write "$FIX_BASIC" crates/mycrate/tests/present_test.rs '// still flat'
# STALE: cites the pre-move flat path.
_fixture_write "$FIX_BASIC" docs/note.md 'see crates/mycrate/tests/moved_test.rs for coverage'
# RESOLVES: cites a path that is a tracked file.
_fixture_write "$FIX_BASIC" docs/ok.md 'see crates/mycrate/tests/present_test.rs for coverage'
# DELETED: basename matches nothing in that crate's tests tree.
_fixture_write "$FIX_BASIC" docs/gone.md 'see crates/mycrate/tests/deleted_forever.rs for coverage'
# SYNTHETIC: the hermetic-fixture shape; crate `foo` does not exist at all.
_fixture_write "$FIX_BASIC" docs/synthetic.md 'e.g. crates/foo/tests/bar.rs'
_fixture_commit "$FIX_BASIC"

assert "A: stale citation reported with its suggested target; resolving, deleted and synthetic citations emit nothing" \
    _expect_scan "$FIX_BASIC" \
    "$(_rec docs/note.md crates/mycrate/tests/moved_test.rs stale crates/mycrate/tests/harness_sub/moved_test.rs)"

# ===========================================================================
# Section B: extension-agnosticism.
#
# The SAME stale citation in five containing-file shapes, including one with
# no extension at all. Extension-agnosticism is the ABSENCE of a filter, not
# an allowlist, and this is what pins it: the 149 stale paths measured on the
# live tree span 8 extensions across crates/, examples/, docs/, tests/,
# tree-sitter-reify/, gui/ and .claude/.
# ===========================================================================
echo ""
echo "--- Section B: detection is extension-agnostic ---"

FIX_EXT="$(_mktmpd)/repo"
_fixture_init "$FIX_EXT"
_fixture_write "$FIX_EXT" crates/mycrate/tests/harness_sub/moved_test.rs '// moved here'
CITE='crates/mycrate/tests/moved_test.rs'
_fixture_write "$FIX_EXT" docs/note.md "prose cites $CITE"
_fixture_write "$FIX_EXT" examples/model.ri "// design note: $CITE"
_fixture_write "$FIX_EXT" ci/conf.yaml "comment: $CITE"
_fixture_write "$FIX_EXT" crates/mycrate/src/lib.rs "//! covered by $CITE"
_fixture_write "$FIX_EXT" NOTICE "see $CITE"
_fixture_commit "$FIX_EXT"

assert "B: the same stale citation is detected in .md, .ri, .yaml, .rs and an extensionless file" \
    _expect_scan "$FIX_EXT" \
    "$(_rec docs/note.md "$CITE" stale crates/mycrate/tests/harness_sub/moved_test.rs)" \
    "$(_rec examples/model.ri "$CITE" stale crates/mycrate/tests/harness_sub/moved_test.rs)" \
    "$(_rec ci/conf.yaml "$CITE" stale crates/mycrate/tests/harness_sub/moved_test.rs)" \
    "$(_rec crates/mycrate/src/lib.rs "$CITE" stale crates/mycrate/tests/harness_sub/moved_test.rs)" \
    "$(_rec NOTICE "$CITE" stale crates/mycrate/tests/harness_sub/moved_test.rs)"

# ===========================================================================
# Section C: inter-harness moves (the resolver is layout-agnostic).
#
# The stale set measured on the live tree is NOT only flat -> harness_*: it
# includes harness_cli/cli_cache.rs -> harness_cli_surface/,
# harness_fea_solver_e2e/stress_*.rs -> harness_stress_scenarios/ and
# harness_syntax/*_lowering_tests.rs -> harness_syntax_lowering/. A resolver
# that special-cased the `harness_` prefix, or that only walked flat -> nested,
# would miss all of them.
# ===========================================================================
echo ""
echo "--- Section C: inter-harness move is resolved layout-agnostically ---"

FIX_INTER="$(_mktmpd)/repo"
_fixture_init "$FIX_INTER"
_fixture_write "$FIX_INTER" crates/mycrate/tests/harness_b/thing.rs '// now under harness_b'
_fixture_write "$FIX_INTER" docs/note.md 'see crates/mycrate/tests/harness_a/thing.rs'
_fixture_commit "$FIX_INTER"

assert "C: a citation under harness_a/ resolving to harness_b/ is flagged with the harness_b/ target" \
    _expect_scan "$FIX_INTER" \
    "$(_rec docs/note.md crates/mycrate/tests/harness_a/thing.rs stale crates/mycrate/tests/harness_b/thing.rs)"

# ===========================================================================
# Section D: ambiguity is an explicit outcome, not a silent pass.
#
# Every test basename is unique within its crate on the live tree today
# (`git ls-files 'crates/*/tests/*.rs' | awk -F/ '{print $2"/"$NF}' | sort |
# uniq -d` yields 0), so no live citation is ambiguous. The resolver still
# handles >1 explicitly rather than assuming that invariant holds forever: the
# citation is broken either way, and naming an arbitrary one of the candidates
# would be a guess presented as an answer.
# ===========================================================================
echo ""
echo "--- Section D: ambiguous resolution is reported with its candidate count ---"

FIX_AMBIG="$(_mktmpd)/repo"
_fixture_init "$FIX_AMBIG"
_fixture_write "$FIX_AMBIG" crates/mycrate/tests/harness_a/dup.rs '// candidate 1'
_fixture_write "$FIX_AMBIG" crates/mycrate/tests/harness_b/dup.rs '// candidate 2'
_fixture_write "$FIX_AMBIG" docs/note.md 'see crates/mycrate/tests/dup.rs'
_fixture_commit "$FIX_AMBIG"

assert "D: two same-basename candidates yield an ambiguous:2 record listing both, not an arbitrary target" \
    _expect_scan "$FIX_AMBIG" \
    "$(_rec docs/note.md crates/mycrate/tests/dup.rs ambiguous:2 \
        'crates/mycrate/tests/harness_a/dup.rs,crates/mycrate/tests/harness_b/dup.rs')"

# ===========================================================================
# Section E: SELF-EXCLUSION of the gate's own three artifacts.
#
# LOAD-BEARING, not cosmetic. The committed baseline is ~300 rows each ENDING
# in a stale cited path, and the lib and this file carry the citation regex
# plus worked examples. Without exclusion the scan would harvest its own
# baseline as ~300 fresh citations, every one of them stale, and regenerating
# the baseline would fold it into itself. The exclusion list is defined ONCE
# in the lib so the gate and the generator cannot drift apart.
# ===========================================================================
echo ""
echo "--- Section E: the gate's own artifacts are excluded from the scan ---"

FIX_SELF="$(_mktmpd)/repo"
_fixture_init "$FIX_SELF"
_fixture_write "$FIX_SELF" crates/mycrate/tests/harness_sub/moved_test.rs '// moved here'
_fixture_write "$FIX_SELF" tests/infra/cited-test-path-baseline.manifest \
    'docs/old.md :: crates/mycrate/tests/moved_test.rs'
_fixture_write "$FIX_SELF" tests/infra/cited-test-path-lib.sh \
    '# worked example: crates/mycrate/tests/moved_test.rs'
_fixture_write "$FIX_SELF" tests/infra/test_cited_test_paths_resolve.sh \
    '# worked example: crates/mycrate/tests/moved_test.rs'
_fixture_write "$FIX_SELF" docs/elsewhere.md 'see crates/mycrate/tests/moved_test.rs'
_fixture_commit "$FIX_SELF"

assert "E: the same stale citation is ignored inside the baseline/lib/gate and reported everywhere else" \
    _expect_scan "$FIX_SELF" \
    "$(_rec docs/elsewhere.md crates/mycrate/tests/moved_test.rs stale crates/mycrate/tests/harness_sub/moved_test.rs)"

# ===========================================================================
# Section F: the index and the citation scan are sourced from git, and the
# fingerprint erases line numbers and the suggested target.
# ===========================================================================
echo ""
echo "--- Section F: index/citation sourcing and fingerprint reduction ---"

_index_of() { bash -c 'set -euo pipefail; source "$1"; cited_test_path_index "$2"' _ "$LIB" "$1"; }
_citations_of() { bash -c 'set -euo pipefail; source "$1"; cited_test_path_citations "$2"' _ "$LIB" "$1"; }
_fingerprints_of() {
    bash -c 'set -euo pipefail; source "$1"; cited_test_path_scan "$2" | cited_test_path_fingerprint' \
        _ "$LIB" "$1"
}

# An UNTRACKED scratch file must influence neither side: the index and the
# citation corpus both come from git, so a stray working-tree file can never
# change the verdict.
_index_excludes_untracked() {
    local out
    : > "$FIX_BASIC/crates/mycrate/tests/untracked_scratch.rs"
    out="$(_index_of "$FIX_BASIC")" || { echo "index failed"; rm -f "$FIX_BASIC/crates/mycrate/tests/untracked_scratch.rs"; return 1; }
    rm -f "$FIX_BASIC/crates/mycrate/tests/untracked_scratch.rs"
    if printf '%s\n' "$out" | grep -q untracked_scratch; then
        echo "index wrongly included an untracked file:"; printf '%s\n' "$out"; return 1
    fi
    printf '%s\n' "$out" | grep -qF "$(printf 'mycrate/moved_test.rs\tcrates/mycrate/tests/harness_sub/moved_test.rs')"
}

assert "F: the index keys tracked tests by <crate>/<basename> and excludes untracked files" \
    _index_excludes_untracked

_citations_include_every_occurrence() {
    local out
    out="$(_citations_of "$FIX_BASIC")" || { echo "citations failed"; return 1; }
    # All four citing files appear, including the ones the resolver later drops:
    # the citation scan is a raw corpus, the RESOLVER is what filters.
    local f
    for f in docs/note.md docs/ok.md docs/gone.md docs/synthetic.md; do
        printf '%s\n' "$out" | grep -qF "$f" || { echo "citation corpus missing $f:"; printf '%s\n' "$out"; return 1; }
    done
    return 0
}

assert "F: the citation corpus is raw (every occurrence, pre-resolution), filtering happens in the resolver" \
    _citations_include_every_occurrence

_fingerprint_erases_line_and_target() {
    local out
    out="$(_fingerprints_of "$FIX_BASIC")" || { echo "fingerprint failed"; return 1; }
    [ "$out" = 'docs/note.md :: crates/mycrate/tests/moved_test.rs' ] && return 0
    echo "unexpected fingerprint output:"; printf '%s\n' "$out"; return 1
}

assert "F: a scan record reduces to '<containing-file> :: <cited-path>', erasing the target" \
    _fingerprint_erases_line_and_target

# A citation that MOVES within its containing file must keep the same
# fingerprint — that is what makes the ratchet stable under ordinary edits.
_fingerprint_is_line_number_stable() {
    local before after
    before="$(_fingerprints_of "$FIX_BASIC")" || return 1
    _fixture_write "$FIX_BASIC" docs/note.md "$(printf 'padding\npadding\nsee crates/mycrate/tests/moved_test.rs for coverage')"
    _gitf -C "$FIX_BASIC" add -A >/dev/null
    _gitf -C "$FIX_BASIC" commit -qm shift >/dev/null
    after="$(_fingerprints_of "$FIX_BASIC")" || return 1
    [ "$before" = "$after" ] && return 0
    echo "fingerprint changed when the citation moved within its file:"
    echo "  before: $before"
    echo "  after:  $after"
    return 1
}

assert "F: moving a citation within its file does not change its fingerprint" \
    _fingerprint_is_line_number_stable

test_summary
