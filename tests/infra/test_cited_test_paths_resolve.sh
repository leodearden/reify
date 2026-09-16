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

# The shared lib under test.
#
# Sections A-F exercise it through `bash -c 'source ...'` CHILD SHELLS rather
# than the top-level source below. That is deliberate and survives the lib
# landing: those sections pin the lib's own INTERFACE, so each must fail on
# its own when a function is missing or renamed, rather than aborting the
# whole file under `set -e` (the test_harness_baseline_registration_gate.sh
# idiom). The top-level source exists for the gate's OWN checkers, which are
# ordinary functions in this file and call the lib directly.
LIB="$SCRIPT_DIR/cited-test-path-lib.sh"
[ -f "$LIB" ] || { echo "ERROR: cited-test-path-lib.sh not found at $LIB" >&2; exit 1; }
# shellcheck source=tests/infra/cited-test-path-lib.sh
source "$LIB"

# ---------------------------------------------------------------------------
# GENERATOR ENTRY POINTS. Argument-less invocation — the only form run_all.sh
# uses — runs the GATE; these two are opt-in side doors.
#
#   --emit-baseline   print the baseline file (header + rows) on stdout;
#   --list            print the human-readable scan records on stdout.
#
# Both are THIN WRAPPERS over the lib. Neither contains a second scan or a
# second fingerprint implementation: derivation lives ONLY in
# cited-test-path-lib.sh, the same invariant
# tests/infra/test_reify_audit_ptodo.sh states for ptodo. That is what makes
# the committed baseline and the check that reads it structurally incapable
# of drifting.
# ---------------------------------------------------------------------------
if [ "${1:-}" = "--emit-baseline" ] || [ "${1:-}" = "--list" ]; then
    if [ "${1:-}" = "--list" ]; then
        cited_test_path_scan "$REPO_ROOT"
        exit 0
    fi
    cat <<'HDR'
# tests/infra/cited-test-path-baseline.manifest
#
# GRANDFATHER BASELINE for the cited-test-path resolution contract —
# task #7095. Read by tests/infra/test_cited_test_paths_resolve.sh; every row
# is derived by tests/infra/cited-test-path-lib.sh, never by hand.
#
# WHAT A ROW MEANS: "<containing-file> cites <cited-path>, which no longer
# resolves, and is grandfathered." The cited path's basename DOES resolve
# elsewhere under the same crate's tests tree, so the citation is repointable
# — a row here is a deferred fix, not a permanent exemption.
#
# FINGERPRINT GRAMMAR: `<containing-file> :: <cited-path>` — two ` :: `
# separated fields. Line numbers and the suggested target are deliberately
# ERASED, so moving a citation within its file, or a later change to where
# its basename resolves, does not spuriously red the ratchet.
#
# REGENERATE WITH:
#     bash tests/infra/test_cited_test_paths_resolve.sh --emit-baseline \
#         > tests/infra/cited-test-path-baseline.manifest
#
# THE RATCHET IS ONE-DIRECTIONAL. The gate asserts live ⊆ baseline and
# nothing more. Rows may be removed freely as citations are repointed, and
# removing a row NEVER reds the gate — this file is a SHRINKING grandfather
# list, not a lockstep mirror of the tree. The converse assertion is
# deliberately absent: it would turn every citation fix into a red build,
# punishing exactly the cleanup this gate exists to encourage (the same
# ruling tests/infra/test_reify_audit_ptodo.sh records for ptodo).
#
# ADDING a row is therefore the deliberate act: do it only when a citation is
# being knowingly grandfathered, by regenerating with the command above.
#
# Comment lines (^\s*#) and blank lines are ignored (same stripping style as
# run-all-classification-lib.sh).
#
HDR
    cited_test_path_scan "$REPO_ROOT" | cited_test_path_fingerprint | LC_ALL=C sort -u
    exit 0
fi

# ---------------------------------------------------------------------------
# _ratchet_check_subset <scan-records-file>
#
# THE RATCHET. Reads the baseline through cited_test_path_baseline_path (so
# REIFY_CITED_TEST_PATH_BASELINE is honored) and asserts
#
#     live ⊆ baseline
#
# via `comm -23`, the SUBSET DIRECTION ONLY.
#
# The converse (`comm -13`, baseline ⊆ live) is DELIBERATELY ABSENT, the same
# ruling tests/infra/test_reify_audit_ptodo.sh records for ptodo. Asserting it
# would turn every citation fix into a red build: repoint a citation, and its
# now-dead baseline row becomes a violation until someone edits the manifest
# in lockstep. The known cost is accepted in exchange — a grandfathered row
# may sit in the baseline forever, with no forcing function to drain it. The
# baseline is a SHRINKING grandfather list, and shrinking it must always be
# free.
#
# On a non-empty difference: print the offenders on STDOUT with the suggested
# target each one resolves to, joined from the scan records the caller already
# produced (no second scan), and return 1. On success: byte-for-byte silent,
# so an all-green suite stays quiet.
# ---------------------------------------------------------------------------
_ratchet_check_subset() {
    local scan="$1"
    local baseline live_fp base_fp new

    baseline="$(cited_test_path_baseline_path)"
    live_fp="$(mktemp)"; base_fp="$(mktemp)"
    _TMPFILES+=("$live_fp" "$base_fp")

    cited_test_path_fingerprint < "$scan" | LC_ALL=C sort -u > "$live_fp"
    cited_test_path_baseline_rows "$baseline" | LC_ALL=C sort -u > "$base_fp"

    new="$(LC_ALL=C comm -23 "$live_fp" "$base_fp")"
    [ -n "$new" ] || return 0

    printf 'RATCHET REGRESSION — %s live citation(s) NOT in the committed baseline:\n' \
        "$(printf '%s\n' "$new" | grep -c .)"
    # Join each offending fingerprint back to its scan record so the target is
    # printed alongside it. The scan is keyed "<file>\t<cited>\t<verdict>\t<targets>"
    # and a fingerprint is "<file> :: <cited>", so the record is recoverable
    # without re-resolving anything.
    printf '%s\n' "$new" | while IFS= read -r fp; do
        [ -n "$fp" ] || continue
        local rec verdict target
        rec="$(awk -F'\t' -v k="$fp" '$1 " :: " $2 == k { print; exit }' "$scan")"
        verdict="$(printf '%s' "$rec" | cut -f3)"
        target="$(printf '%s' "$rec" | cut -f4)"
        printf '  + %s\n' "$fp"
        printf '      -> %s: %s\n' "${verdict:-unresolved}" "${target:-<no candidate>}"
    done
    return 1
}

echo "=== cited test-path resolution gate (task 7095) ==="

# Single EXIT trap over an array of fixtures: individual `trap ... EXIT` calls
# replace one another, so one handler over an array removes every fixture
# regardless of which section adds the last.
_TMPDIRS=()
_TMPFILES=()
_cleanup() {
    [ "${#_TMPDIRS[@]}" -gt 0 ] && rm -rf "${_TMPDIRS[@]}"
    [ "${#_TMPFILES[@]}" -gt 0 ] && rm -f "${_TMPFILES[@]}"
    return 0
}
trap _cleanup EXIT

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

# ===========================================================================
# Section G: the committed baseline's contract.
#
# The baseline is the grandfather list the ratchet reads. It is STRUCTURED
# DATA, so it gets a grammar check rather than being trusted as prose nobody
# validates; and it is self-describing, so a reader who opens only the
# manifest can act on it without hunting for this file.
# ===========================================================================
echo ""
echo "--- Section G: the committed baseline's contract ---"

_baseline_default_is_committed_manifest() {
    local got want
    want="$SCRIPT_DIR/cited-test-path-baseline.manifest"
    got="$(env -u REIFY_CITED_TEST_PATH_BASELINE bash -c \
        'set -euo pipefail; source "$1"; cited_test_path_baseline_path' _ "$LIB")" || return 1
    [ "$got" = "$want" ] && return 0
    echo "default baseline path mismatch:"; echo "  want: $want"; echo "  got:  $got"; return 1
}

# Both directions of the testability seam are asserted, not just the default:
# every ratchet scenario below depends on the override actually taking effect,
# so an override that silently fell back to the committed manifest would make
# those scenarios assert against the real tree's baseline without saying so.
_baseline_override_is_honored() {
    local got want='/tmp/some-fixture-baseline.manifest'
    got="$(REIFY_CITED_TEST_PATH_BASELINE="$want" bash -c \
        'set -euo pipefail; source "$1"; cited_test_path_baseline_path' _ "$LIB")" || return 1
    [ "$got" = "$want" ] && return 0
    echo "REIFY_CITED_TEST_PATH_BASELINE was not honored:"; echo "  want: $want"; echo "  got:  $got"; return 1
}

assert "G: cited_test_path_baseline_path defaults to the committed manifest" \
    _baseline_default_is_committed_manifest
assert "G: REIFY_CITED_TEST_PATH_BASELINE overrides the baseline path" \
    _baseline_override_is_honored

BASELINE="$SCRIPT_DIR/cited-test-path-baseline.manifest"

assert "G: the committed baseline exists" \
    test -f "$BASELINE"

_baseline_is_non_empty() {
    local n
    n="$(env -u REIFY_CITED_TEST_PATH_BASELINE bash -c \
        'set -euo pipefail; source "$1"; cited_test_path_baseline_rows | grep -c .' _ "$LIB")" || true
    [ -n "$n" ] && [ "$n" -gt 0 ] 2>/dev/null && return 0
    echo "committed baseline has no data rows (got: '${n:-<none>}')"; return 1
}

assert "G: the committed baseline has at least one data row" \
    _baseline_is_non_empty

# GRAMMAR: "<containing-file> :: <cited-path>" — two ` :: `-separated
# non-empty fields, the second matching the citation regex. A baseline that
# is not machine-checkable is a meaningful string, not structured data.
_baseline_rows_well_formed() {
    local rows bad
    rows="$(env -u REIFY_CITED_TEST_PATH_BASELINE bash -c \
        'set -euo pipefail; source "$1"; cited_test_path_baseline_rows' _ "$LIB")" || return 1
    bad="$(printf '%s\n' "$rows" \
        | grep -Ev "^[^ ].* :: crates/[a-z0-9-]+/tests/[A-Za-z0-9_./-]+\.rs$" || true)"
    if [ -n "$bad" ]; then
        echo "malformed baseline row(s) — expected '<containing-file> :: <cited-path>':"
        printf '%s\n' "$bad" | sed 's/^/  ! /'
        return 1
    fi
    return 0
}

assert "G: every baseline data row matches the '<file> :: <cited-path>' grammar" \
    _baseline_rows_well_formed

# SORTED AND DEDUPED: the ratchet's `comm -23` requires sorted input and would
# silently misbehave on an unsorted or duplicated baseline, reporting phantom
# regressions or missing real ones.
_baseline_sorted_and_deduped() {
    local rows sorted dups
    rows="$(env -u REIFY_CITED_TEST_PATH_BASELINE bash -c \
        'set -euo pipefail; source "$1"; cited_test_path_baseline_rows' _ "$LIB")" || return 1
    sorted="$(printf '%s\n' "$rows" | LC_ALL=C sort)"
    if [ "$rows" != "$sorted" ]; then
        echo "baseline data rows are not in LC_ALL=C sorted order; first divergence:"
        diff <(printf '%s\n' "$rows") <(printf '%s\n' "$sorted") | head -6
        return 1
    fi
    dups="$(printf '%s\n' "$rows" | LC_ALL=C sort | uniq -d)"
    if [ -n "$dups" ]; then
        echo "duplicate baseline row(s):"; printf '%s\n' "$dups" | sed 's/^/  = /'; return 1
    fi
    return 0
}

assert "G: the baseline is sorted (LC_ALL=C) and free of duplicate rows" \
    _baseline_sorted_and_deduped

# SELF-DESCRIBING HEADER, in the style of run-all-classification.manifest,
# harness-layout-baseline.manifest and scripts/verify-pipeline-paths.txt. The
# regeneration command is asserted LITERALLY: a reader who opens only this
# manifest must be able to act on it.
_baseline_header_is_self_describing() {
    local header missing=""
    header="$(grep -E '^[[:space:]]*#' "$BASELINE" || true)"
    [ -n "$header" ] || { echo "baseline carries no comment header at all"; return 1; }
    printf '%s\n' "$header" | grep -qF 'tests/infra/test_cited_test_paths_resolve.sh --emit-baseline' \
        || missing="$missing\n  - the literal regeneration command"
    printf '%s\n' "$header" | grep -qiE 'one-directional|shrink' \
        || missing="$missing\n  - the one-directional / shrink-friendly ratchet semantics"
    printf '%s\n' "$header" | grep -qF ' :: ' \
        || missing="$missing\n  - the fingerprint grammar"
    if [ -n "$missing" ]; then
        echo "baseline header does not describe itself; missing:"
        printf "%b\n" "$missing"
        return 1
    fi
    return 0
}

assert "G: the baseline header names its purpose, the literal regeneration command, the grammar and the one-directional semantics" \
    _baseline_header_is_self_describing

# ===========================================================================
# Section H: the one-directional ratchet checker.
#
# Driven entirely against FIXTURE baselines via REIFY_CITED_TEST_PATH_BASELINE
# and fixture repos, so no scenario here depends on the committed baseline's
# contents — which change every time a citation is repointed.
#
# ORACLE DIRECTION: subset-of, BY RULING. The gate asserts live ⊆ baseline and
# nothing more. The accepted limitation is that a grandfathered row may sit in
# the baseline forever with no forcing function to drain it. Adding the
# converse (baseline ⊆ live) does not fix that and costs the property this
# gate most needs: that repointing a citation is never punished.
# ===========================================================================
echo ""
echo "--- Section H: the one-directional (subset) ratchet ---"

# A fixture repo carrying exactly two stale citations, so a baseline can
# cover both, one, or neither.
FIX_RATCHET="$(_mktmpd)/repo"
_fixture_init "$FIX_RATCHET"
_fixture_write "$FIX_RATCHET" crates/mycrate/tests/harness_sub/alpha.rs '// moved'
_fixture_write "$FIX_RATCHET" crates/mycrate/tests/harness_sub/beta.rs '// moved'
_fixture_write "$FIX_RATCHET" docs/a.md 'see crates/mycrate/tests/alpha.rs'
_fixture_write "$FIX_RATCHET" docs/b.md 'see crates/mycrate/tests/beta.rs'
_fixture_commit "$FIX_RATCHET"

FP_A='docs/a.md :: crates/mycrate/tests/alpha.rs'
FP_B='docs/b.md :: crates/mycrate/tests/beta.rs'

# _write_baseline <row...> -> prints the fixture baseline path. A leading
# comment line is always written, so the comment-stripping path is exercised
# even by the "empty baseline" fixtures.
_write_baseline() {
    local f
    f="$(mktemp "${TMPDIR:-/tmp}/reify-cited-baseline.XXXXXX")"
    _TMPFILES+=("$f")
    printf '# fixture baseline\n' > "$f"
    [ "$#" -gt 0 ] && printf '%s\n' "$@" >> "$f"
    printf '%s\n' "$f"
}

# _scan_file <repo-root> -> path to a file of that root's scan records.
#
# The checker consumes a SCAN FILE rather than a repo root, so the gate's main
# body can feed ONE scan to both the ratchet and the vacuity floor without
# scanning the tree twice.
_scan_file() {
    local f
    f="$(mktemp "${TMPDIR:-/tmp}/reify-cited-scan.XXXXXX")"
    _TMPFILES+=("$f")
    cited_test_path_scan "$1" > "$f"
    printf '%s\n' "$f"
}

# _ratchet_rc <repo-root> <baseline-path> — run the very checker the gate's
# main body uses, against an arbitrary root + baseline. Prints the checker's
# combined stdout+stderr; returns its rc. The baseline override is exported
# inside a SUBSHELL so it cannot leak into a later scenario.
_ratchet_rc() {
    local root="$1" baseline="$2" scan out rc=0
    scan="$(_scan_file "$root")"
    out="$( ( export REIFY_CITED_TEST_PATH_BASELINE="$baseline"
              _ratchet_check_subset "$scan" ) 2>&1 )" || rc=$?
    printf '%s' "$out"
    return "$rc"
}

# (1) EXACT COVER: live == baseline -> green.
_ratchet_exact_cover_is_green() {
    local b out rc=0
    b="$(_write_baseline "$FP_A" "$FP_B")"
    out="$(_ratchet_rc "$FIX_RATCHET" "$b")" || rc=$?
    [ "$rc" -eq 0 ] && return 0
    echo "expected rc0 for an exactly-covering baseline, got rc$rc:"; printf '%s\n' "$out"; return 1
}

assert "H: a baseline that exactly covers the live set returns 0" \
    _ratchet_exact_cover_is_green

# (2) STRICT SUBSET: baseline has rows with no live counterpart -> still green.
#
# THIS IS THE LOAD-BEARING ONE. It pins the one-directional semantics: an
# author who repoints a citation without pruning its now-dead baseline row
# must stay GREEN. Reddening here would make the gate punish exactly the
# cleanup it exists to encourage, and every burn-down commit would need a
# baseline edit in lockstep.
_ratchet_strict_subset_is_green() {
    local b out rc=0
    b="$(_write_baseline "$FP_A" "$FP_B" 'docs/gone.md :: crates/mycrate/tests/already_repointed.rs')"
    out="$(_ratchet_rc "$FIX_RATCHET" "$b")" || rc=$?
    [ "$rc" -eq 0 ] && return 0
    echo "expected rc0 for a baseline with a stale-but-harmless extra row, got rc$rc:"
    printf '%s\n' "$out"; return 1
}

assert "H: a baseline row with no live counterpart stays green (repointing is never punished)" \
    _ratchet_strict_subset_is_green

# (3) REGRESSION: a live fingerprint absent from the baseline -> red, and the
# offender is NAMED. An unactionable red is the failure mode the ptodo RCA
# found; the fingerprint must land in assert()'s captured-output dump.
_ratchet_regression_is_red_and_names_offender() {
    local b out rc=0
    b="$(_write_baseline "$FP_A")"          # beta deliberately ungrandfathered
    out="$(_ratchet_rc "$FIX_RATCHET" "$b")" || rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "expected non-zero for an uncovered live fingerprint, got rc0:"; printf '%s\n' "$out"; return 1
    fi
    printf '%s\n' "$out" | grep -qF "$FP_B" || {
        echo "checker did not NAME the offending fingerprint ($FP_B):"; printf '%s\n' "$out"; return 1
    }
    printf '%s\n' "$out" | grep -qF "$FP_A" && {
        echo "checker wrongly named a COVERED fingerprint ($FP_A):"; printf '%s\n' "$out"; return 1
    }
    return 0
}

assert "H: an uncovered live fingerprint returns non-zero and names exactly that offender" \
    _ratchet_regression_is_red_and_names_offender

# (4) The failure output carries the SUGGESTED TARGET, so the message is
# actionable without re-running anything by hand — the difference between
# "something is wrong" and "change this line to that path".
_ratchet_failure_carries_suggested_target() {
    local b out rc=0
    b="$(_write_baseline "$FP_A")"
    out="$(_ratchet_rc "$FIX_RATCHET" "$b")" || rc=$?
    [ "$rc" -ne 0 ] || { echo "expected a failure to inspect"; return 1; }
    printf '%s\n' "$out" | grep -qF 'crates/mycrate/tests/harness_sub/beta.rs' && return 0
    echo "failure output does not carry the suggested target for the offender:"
    printf '%s\n' "$out"; return 1
}

assert "H: the failure output carries each offender's suggested target path" \
    _ratchet_failure_carries_suggested_target

# (5) An EMPTY baseline against a repo with live findings must be red — the
# degenerate case that would otherwise let a lost/emptied baseline pass.
_ratchet_empty_baseline_is_red() {
    local b out rc=0
    b="$(_write_baseline)"
    out="$(_ratchet_rc "$FIX_RATCHET" "$b")" || rc=$?
    [ "$rc" -ne 0 ] && return 0
    echo "expected non-zero against an empty baseline with 2 live findings, got rc0:"
    printf '%s\n' "$out"; return 1
}

assert "H: an empty baseline against a repo with live findings returns non-zero" \
    _ratchet_empty_baseline_is_red

# (6) A CLEAN repo (no stale citations at all) is green under an empty
# baseline, and byte-for-byte silent: a green suite must stay quiet.
_ratchet_clean_repo_is_green_and_silent() {
    local b out rc=0
    b="$(_write_baseline)"
    out="$(_ratchet_rc "$FIX_INTER_CLEAN" "$b")" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "expected rc0 for a repo with no stale citations, got rc$rc:"; printf '%s\n' "$out"; return 1
    fi
    [ -z "$out" ] && return 0
    echo "expected byte-for-byte silence on the green path, got:"; printf '%s\n' "$out"; return 1
}

FIX_INTER_CLEAN="$(_mktmpd)/repo"
_fixture_init "$FIX_INTER_CLEAN"
_fixture_write "$FIX_INTER_CLEAN" crates/mycrate/tests/harness_sub/ok.rs '// here'
_fixture_write "$FIX_INTER_CLEAN" docs/n.md 'see crates/mycrate/tests/harness_sub/ok.rs'
_fixture_commit "$FIX_INTER_CLEAN"

assert "H: a repo with no stale citations is green and byte-for-byte silent" \
    _ratchet_clean_repo_is_green_and_silent

test_summary
