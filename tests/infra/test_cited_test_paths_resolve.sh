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
# STRICT DISPATCH. An unrecognised argument is REJECTED rather than falling
# through to the full suite: Section J re-invokes this very file, so a
# fall-through on a misspelt flag would recurse instead of failing.
GATE_SELF="$SCRIPT_DIR/test_cited_test_paths_resolve.sh"

case "${1:-}" in
    ''|--gate-only|--emit-baseline|--list) ;;
    *)
        echo "usage: $(basename "$GATE_SELF") [--gate-only|--emit-baseline|--list]" >&2
        echo "  (no argument)     run the full suite: scenarios + the gate" >&2
        echo "  --gate-only       run ONLY the whole-tree gate the merge gate consumes" >&2
        echo "  --emit-baseline   regenerate tests/infra/cited-test-path-baseline.manifest" >&2
        echo "  --list            print the live scan records, human-readable" >&2
        echo "ERROR: unrecognised argument '$1'" >&2
        exit 2
        ;;
esac


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

# ---------------------------------------------------------------------------
# VACUITY FLOOR BOUNDS.
#
# MEASURED BASIS (this tree, when the gate was written):
#   index units          1304   549 flat + 755 nested under crates/*/tests/
#   citation occurrences 1860   spanning 790 distinct cited paths
#
# The floors sit far below those: roughly a sixth and a quarter. They are
# lower bounds on THE INSTRUMENT WORKING, not targets for the tree — ordinary
# churn, even deleting a whole crate's tests, cannot reach them, while a scan
# that collapses toward zero does. Section I asserts the live corpus keeps at
# least 2x headroom over both, so these can never quietly drift up to meet
# the tree and become a second, accidental content assertion.
# ---------------------------------------------------------------------------
FLOOR_MIN_INDEX_UNITS=200
FLOOR_MIN_CITATIONS=500

# ---------------------------------------------------------------------------
# _floor_check_corpus <index-units> <citation-occurrences>
#
# THE VACUITY FLOOR. Takes the two observed corpus sizes as EXPLICIT
# ARGUMENTS rather than re-deriving them, so a degenerate case can be driven
# without fabricating a fake repository, and so the gate's main body can feed
# it the same numbers it already computed.
#
# READS THE BASELINE NOWHERE. That independence is the property that makes
# this a real second signal: a floor that consulted the baseline would go
# green exactly when the baseline was lost, which is one of the states it
# exists to catch.
#
# rc0 and byte-for-byte silent when both floors are cleared; otherwise rc1
# with a diagnostic naming WHICH floor broke and WHAT was observed. Both are
# reported when both break — a diagnostic that stops at the first breach
# hides half the picture.
# ---------------------------------------------------------------------------
_floor_check_corpus() {
    local idx_n="$1" cit_n="$2" breached=0

    # A non-numeric count is a breach, not a crash: it means the caller's own
    # measurement failed, which is exactly the blindness this floor detects.
    case "$idx_n" in (''|*[!0-9]*) idx_n=-1 ;; esac
    case "$cit_n" in (''|*[!0-9]*) cit_n=-1 ;; esac

    if [ "$idx_n" -lt "$FLOOR_MIN_INDEX_UNITS" ]; then
        printf 'VACUITY FLOOR BREACHED — tracked-test index: observed %s, floor %s.\n' \
            "$idx_n" "$FLOOR_MIN_INDEX_UNITS" >&2
        printf '  An index this small cannot resolve anything, so every citation is\n' >&2
        printf '  dropped and the ratchet goes quiet regardless of the tree.\n' >&2
        breached=1
    fi
    if [ "$cit_n" -lt "$FLOOR_MIN_CITATIONS" ]; then
        printf 'VACUITY FLOOR BREACHED — citation corpus: observed %s, floor %s.\n' \
            "$cit_n" "$FLOOR_MIN_CITATIONS" >&2
        printf '  The scan found almost nothing to resolve; the subset check below it\n' >&2
        printf '  is then trivially satisfied by the empty set.\n' >&2
        breached=1
    fi
    [ "$breached" -eq 0 ] || return 1
    return 0
}

# ---------------------------------------------------------------------------
# _run_whole_tree_gate — THE GATE the merge pipeline consumes.
#
# Scans the real repository ONCE and feeds that single scan to BOTH signals:
#
#   the RATCHET        live ⊆ committed baseline (are there NEW stale citations?)
#   the VACUITY FLOOR  did the scan observe a real corpus at all?
#
# They are reported as two separate `assert` lines, deliberately. A combined
# line cannot distinguish "the ratchet is satisfied" from "the scan never
# ran" — and the second is what the floor exists to detect, so collapsing
# them would delete the signal. The floor is asserted FIRST, so the
# precondition is reported before the thing it conditions.
# ---------------------------------------------------------------------------
_run_whole_tree_gate() {
    local scan idx_n cit_n

    scan="$(_scan_file "$REPO_ROOT")"
    idx_n="$(cited_test_path_index "$REPO_ROOT" | grep -c . || true)"
    cit_n="$(cited_test_path_citations "$REPO_ROOT" | grep -c . || true)"

    echo "=== cited test-path resolution gate — whole tree ==="
    echo "    index units: $idx_n   citation occurrences: $cit_n"

    assert "vacuity floor: the scan observed a real corpus" \
        _floor_check_corpus "$idx_n" "$cit_n"
    assert "ratchet: no stale citation outside the committed baseline" \
        _ratchet_check_subset "$scan"

    test_summary
}

# --gate-only DISPATCHES HERE, before any scenario runs, and always exits.
#
# It must never fall through to the scenario sections: Section J re-invokes
# this file with exactly this flag, so a fall-through would re-enter Section J
# and recurse without bound. The `exit` below is the structural guarantee that
# it cannot — not a convention, a control-flow fact.
if [ "${1:-}" = "--gate-only" ]; then
    _run_whole_tree_gate
    exit $?
fi

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

echo "=== cited test-path resolution gate (task 7095) ==="

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

# ===========================================================================
# Section I: the VACUITY FLOOR.
#
# WHY IT EXISTS. `comm -23` can only ever report "no NEW fingerprints", and
# the empty set is a subset of everything. A regex typo, a `git grep` that
# silently matches nothing, a wrong repo root, or an exclusion pathspec
# widened until it swallows the tree would all leave the ratchet
# PERMANENTLY AND INVISIBLY GREEN — the gate would report success precisely
# when it had stopped looking. This is the exact failure mode task #6241
# added _ratchet_check_scan_evidence to tests/infra/test_reify_audit_ptodo.sh
# to close.
#
# The floor observes the CORPUS (how much the scan looked at), never the
# FINDINGS (how much it disliked), and never the baseline. That independence
# is what makes it a real second signal rather than a restatement of the
# ratchet: a burn-down commit that legitimately drives the finding count to
# zero must not trip it, while a scan that collapses to near-zero must.
# ===========================================================================
echo ""
echo "--- Section I: the baseline-independent vacuity floor ---"

_floor_rc() {
    local out rc=0
    out="$(_floor_check_corpus "$1" "$2" 2>&1)" || rc=$?
    printf '%s' "$out"
    return "$rc"
}

# (1) DEGENERATE: an empty tracked-test index. Nothing could ever resolve, so
# every citation would be dropped by the zero-basename-hit rule and the
# ratchet would go quiet.
_floor_rejects_empty_index() {
    local out rc=0
    out="$(_floor_rc 0 1860)" || rc=$?
    if [ "$rc" -eq 0 ]; then echo "expected non-zero for an empty index, got rc0"; return 1; fi
    printf '%s\n' "$out" | grep -qi 'index' && return 0
    echo "diagnostic does not name the index floor:"; printf '%s\n' "$out"; return 1
}

# (2) DEGENERATE: a zero-citation corpus. The scan found nothing to resolve,
# so there is nothing for the resolver to filter and the ratchet is vacuous.
_floor_rejects_zero_citations() {
    local out rc=0
    out="$(_floor_rc 1304 0)" || rc=$?
    if [ "$rc" -eq 0 ]; then echo "expected non-zero for a zero-citation corpus, got rc0"; return 1; fi
    printf '%s\n' "$out" | grep -qi 'citation' && return 0
    echo "diagnostic does not name the citation floor:"; printf '%s\n' "$out"; return 1
}

assert "I: an empty tracked-test index breaches the floor and is named" \
    _floor_rejects_empty_index
assert "I: a zero-citation corpus breaches the floor and is named" \
    _floor_rejects_zero_citations

# (3) The floor passes — and is silent — against the REAL repository's own
# observed corpus. Measured on this tree while writing the gate: 1304 index
# units and 1860 citation occurrences.
_floor_accepts_real_tree() {
    local idx_n cit_n out rc=0
    idx_n="$(cited_test_path_index "$REPO_ROOT" | grep -c . || true)"
    cit_n="$(cited_test_path_citations "$REPO_ROOT" | grep -c . || true)"
    out="$(_floor_rc "$idx_n" "$cit_n")" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "floor breached against the real tree (index=$idx_n citations=$cit_n):"
        printf '%s\n' "$out"; return 1
    fi
    [ -z "$out" ] && return 0
    echo "expected silence on the passing path, got:"; printf '%s\n' "$out"; return 1
}

assert "I: the real repository's observed corpus clears both floors, silently" \
    _floor_accepts_real_tree

# (4) The bounds themselves. Asserted against the LIVE corpus rather than
# against the constants, so this stays a statement about the tree and not a
# tautology restating the two literals.
#
# MEASURED BASIS (this tree, while writing the gate):
#   index units            1304   (549 flat + 755 nested under crates/*/tests/)
#   citation occurrences   1860   (790 distinct cited paths)
# Both floors sit at roughly a quarter to a sixth of the measured value, so
# ordinary churn — even deleting a whole crate's tests — cannot trip them,
# while a scan that collapses toward zero does. They are lower bounds on the
# instrument working, NOT targets for the tree.
_floor_bounds_are_conservative_against_live() {
    local idx_n cit_n
    # Read defensively: the constants are defined alongside the checker, and
    # under `set -u` a bare reference to a not-yet-defined one would abort the
    # whole file instead of failing this one assert.
    local min_idx="${FLOOR_MIN_INDEX_UNITS:-}" min_cit="${FLOOR_MIN_CITATIONS:-}"
    if [ -z "$min_idx" ] || [ -z "$min_cit" ]; then
        echo "FLOOR_MIN_INDEX_UNITS / FLOOR_MIN_CITATIONS are not defined"; return 1
    fi
    idx_n="$(cited_test_path_index "$REPO_ROOT" | grep -c . || true)"
    cit_n="$(cited_test_path_citations "$REPO_ROOT" | grep -c . || true)"
    if [ "$idx_n" -lt "$min_idx" ]; then
        echo "live index ($idx_n) is below the floor ($min_idx)"; return 1
    fi
    if [ "$cit_n" -lt "$min_cit" ]; then
        echo "live citations ($cit_n) is below the floor ($min_cit)"; return 1
    fi
    # Headroom: a floor that sat just under the live value would flake on
    # ordinary churn. Require the live corpus to be at least double each floor.
    if [ "$idx_n" -lt $(( min_idx * 2 )) ]; then
        echo "index floor ($min_idx) has less than 2x headroom under live ($idx_n)"; return 1
    fi
    if [ "$cit_n" -lt $(( min_cit * 2 )) ]; then
        echo "citation floor ($min_cit) has less than 2x headroom under live ($cit_n)"; return 1
    fi
    return 0
}

assert "I: both floors sit at least 2x below the live corpus (conservative, not tuned to it)" \
    _floor_bounds_are_conservative_against_live

# (5) The floor NEVER reads the baseline. Its independence from baseline
# content is the whole reason it is a second signal; a floor that consulted
# the baseline would go green exactly when the baseline was lost.
_floor_ignores_the_baseline() {
    local with_real with_missing rc1=0 rc2=0
    with_real="$(_floor_rc 1304 1860)" || rc1=$?
    with_missing="$(REIFY_CITED_TEST_PATH_BASELINE=/nonexistent/baseline.manifest \
        _floor_rc 1304 1860)" || rc2=$?
    if [ "$rc1" -eq "$rc2" ] && [ "$with_real" = "$with_missing" ]; then return 0; fi
    echo "the floor's verdict changed when the baseline was pointed at a missing file:"
    echo "  with real baseline:    rc$rc1 '$with_real'"
    echo "  with missing baseline: rc$rc2 '$with_missing'"
    return 1
}

assert "I: the floor's verdict is unchanged by a missing baseline (baseline-independent)" \
    _floor_ignores_the_baseline

# ===========================================================================
# Section J: WHOLE-TREE WIRING.
#
# Sections A-I prove the helpers work. None of them proves the GATE fires
# against the real repository — a gate wired to the wrong root, or one whose
# main body never calls its own checkers, would pass every one of them. This
# section runs the real thing end to end, in a child process, exactly as the
# merge gate does.
# ===========================================================================
echo ""
echo "--- Section J: the gate fires against the real repository ---"

# (1) THE MERGE-GATE SIGNAL: the real tree against the real committed
# baseline exits 0.
_real_gate_is_green() {
    local out rc=0
    out="$(bash "$GATE_SELF" --gate-only 2>&1)" || rc=$?
    [ "$rc" -eq 0 ] && return 0
    echo "the gate is RED against the real tree with the committed baseline (rc$rc):"
    printf '%s\n' "$out"
    return 1
}

assert "J: the gate against the real tree + committed baseline exits 0" \
    _real_gate_is_green

# (2) WHOLE-TREE LIVENESS CONTROL, in two halves.
#
# The same gate, same real tree, pointed at an EMPTY baseline must go red and
# must be seeing the whole tree. Without this, a gate that scanned an empty
# directory would pass (1) just as happily.
#
# It is TWO assertions because assert() deliberately caps its captured-output
# dump at `tail -50`. The wired gate's offender listing therefore reaches this
# file already truncated to ~25 entries, so counting from it would measure
# that cap rather than the tree. The red-ness is taken from the wired child
# process; the COUNT is taken from the same checker called directly, where
# nothing truncates it.

# (2a) the wired gate itself goes red on the real tree.
_real_gate_is_red_against_empty_baseline() {
    local empty out rc=0
    empty="$(_write_baseline)"
    out="$(REIFY_CITED_TEST_PATH_BASELINE="$empty" bash "$GATE_SELF" --gate-only 2>&1)" || rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "the gate stayed GREEN against the real tree with an EMPTY baseline —"
        echo "the scan is not reaching the tree it claims to be checking:"
        printf '%s\n' "$out"
        return 1
    fi
    printf '%s\n' "$out" | grep -qE '\+ [^ ]+ :: crates/' && return 0
    echo "the gate went red but listed no offender:"; printf '%s\n' "$out"; return 1
}

assert "J: the wired gate against an EMPTY baseline goes red and lists offenders" \
    _real_gate_is_red_against_empty_baseline

# (2b) at SCALE: a conservative lower bound on the offender count, not an
# exact number — the baseline is a shrinking list, so repointing citations
# over time must reduce this without flaking the assertion (measured 308).
_real_tree_reports_offenders_at_scale() {
    local empty out rc=0 n
    empty="$(_write_baseline)"
    out="$(_ratchet_rc "$REPO_ROOT" "$empty")" || rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "the ratchet was GREEN against the real tree with an EMPTY baseline"; return 1
    fi
    n="$(printf '%s\n' "$out" | grep -cE '^  \+ [^ ]+ :: crates/' || true)"
    if [ "$n" -lt 100 ]; then
        echo "expected >= 100 live citations against an empty baseline, got $n:"
        printf '%s\n' "$out" | head -20
        return 1
    fi
    return 0
}

assert "J: the real tree yields >= 100 live citations against an empty baseline" \
    _real_tree_reports_offenders_at_scale

# (3) THE RATCHET AND THE FLOOR ARE REPORTED SEPARATELY.
#
# Collapsing them into one assert would destroy the floor's whole purpose:
# a combined line cannot distinguish "the ratchet is satisfied" from "the
# scan never ran", which is the confusion the floor exists to resolve. Same
# reason test_helpers.sh pairs assert_no_shared_trash_litter with
# assert_shared_trash_litter_detector_live rather than merging them.
_gate_reports_two_independent_signals() {
    local out n_ratchet n_floor
    out="$(bash "$GATE_SELF" --gate-only 2>&1)" || {
        echo "gate failed; cannot inspect its reported signals:"; printf '%s\n' "$out"; return 1
    }
    n_ratchet="$(printf '%s\n' "$out" | grep -ci 'PASS:.*ratchet' || true)"
    n_floor="$(printf '%s\n' "$out" | grep -ci 'PASS:.*floor' || true)"
    if [ "$n_ratchet" -lt 1 ] || [ "$n_floor" -lt 1 ]; then
        echo "expected one PASS line for the ratchet and one for the floor"
        echo "  ratchet lines: $n_ratchet   floor lines: $n_floor"
        printf '%s\n' "$out"
        return 1
    fi
    return 0
}

assert "J: the gate reports the ratchet and the floor as two independent signals" \
    _gate_reports_two_independent_signals

# (4) The strict dispatch itself: an unrecognised flag is rejected, never run
# as the full suite. Section J re-invokes this file, so a fall-through would
# recurse rather than fail.
_unknown_argument_is_rejected() {
    local out rc=0
    out="$(bash "$GATE_SELF" --no-such-flag 2>&1)" || rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "an unrecognised argument was ACCEPTED (rc0); it must be rejected:"
        printf '%s\n' "$out" | head -5
        return 1
    fi
    printf '%s\n' "$out" | grep -qi 'usage' && return 0
    echo "rejection carried no usage line:"; printf '%s\n' "$out" | head -5; return 1
}

assert "J: an unrecognised argument is rejected with a usage line, not run as the suite" \
    _unknown_argument_is_rejected

# ===========================================================================
# Section K: ACCEPTANCE — the scenario this whole gate exists for.
#
# Replays the root cause literally: a test unit is committed together with
# prose citing it, then a real `git mv` relocates the unit into a
# harness_<subsystem>/ directory and the move is committed ALONE, leaving
# every citation untouched. That is exactly what the CMP-*/C-eval-* harness
# consolidation series did, and what commit 276d32f025 then had to clean up
# by hand across 123 files.
#
# The acceptance criterion for "a future harness-consolidation git mv gets
# caught automatically" is: the gate, run against that repository, goes red
# and names EVERY orphaned citation with the correct new target.
# ===========================================================================
echo ""
echo "--- Section K: acceptance — a real `git mv` orphans citations and is caught ---"

FIX_ACCEPT="$(_mktmpd)/repo"
_fixture_init "$FIX_ACCEPT"
# Commit 1: the unit, plus three citations of it in three different media —
# a README, a .ri design file, and a Rust doc comment.
_fixture_write "$FIX_ACCEPT" crates/mycrate/tests/examples_smoke.rs '// the unit'
_fixture_write "$FIX_ACCEPT" docs/testing.md \
    'Coverage lives in crates/mycrate/tests/examples_smoke.rs today.'
_fixture_write "$FIX_ACCEPT" examples/part.ri \
    '// exercised by crates/mycrate/tests/examples_smoke.rs'
_fixture_write "$FIX_ACCEPT" crates/mycrate/src/lib.rs \
    '//! See crates/mycrate/tests/examples_smoke.rs for the smoke coverage.'
_fixture_commit "$FIX_ACCEPT"

# Commit 2: a REAL `git mv` into the harness directory, committed ALONE. No
# citation is touched — precisely the omission the consolidation made.
mkdir -p "$FIX_ACCEPT/crates/mycrate/tests/harness_compilation_surface"
_gitf -C "$FIX_ACCEPT" mv crates/mycrate/tests/examples_smoke.rs \
    crates/mycrate/tests/harness_compilation_surface/examples_smoke.rs
_gitf -C "$FIX_ACCEPT" commit -qm 'consolidate examples_smoke into harness_compilation_surface'

_acceptance_catches_all_three_citations() {
    local empty out rc=0 f
    empty="$(_write_baseline)"
    out="$(_ratchet_rc "$FIX_ACCEPT" "$empty")" || rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "the gate stayed GREEN after a real git mv orphaned three citations:"
        printf '%s\n' "$out"
        return 1
    fi
    for f in docs/testing.md examples/part.ri crates/mycrate/src/lib.rs; do
        printf '%s\n' "$out" \
            | grep -qF "$f :: crates/mycrate/tests/examples_smoke.rs" || {
            echo "orphaned citation in $f was NOT reported:"; printf '%s\n' "$out"; return 1
        }
    done
    # And each is repointed at the real new home, not merely flagged.
    if [ "$(printf '%s\n' "$out" \
            | grep -cF 'crates/mycrate/tests/harness_compilation_surface/examples_smoke.rs')" -lt 3 ]; then
        echo "not every offender carried the harness_compilation_surface/ target:"
        printf '%s\n' "$out"; return 1
    fi
    return 0
}

assert "K: a real git mv orphaning three citations is caught, each with the harness_compilation_surface/ target" \
    _acceptance_catches_all_three_citations

# The move must be a genuine rename in git's eyes, not a delete+add that
# happens to look similar — otherwise the fixture is not replaying the root
# cause at all.
_acceptance_move_was_a_real_rename() {
    local st
    st="$(_gitf -C "$FIX_ACCEPT" show --name-status --find-renames HEAD | grep -E '^R[0-9]*' || true)"
    [ -n "$st" ] && return 0
    echo "HEAD is not a rename commit; the fixture does not replay the root cause:"
    _gitf -C "$FIX_ACCEPT" show --name-status HEAD
    return 1
}

assert "K: the fixture's HEAD is a genuine git rename, not a delete+add" \
    _acceptance_move_was_a_real_rename

# ===========================================================================
# Section L: the REMEDIATION CONTRACT — stream separation.
#
# Mirrors what tests/infra/test_harness_baseline_registration_gate.sh Sections
# P/P2 guard for scripts/check-harness-baseline-registration.sh (task #5381):
# the FINDINGS are the machine-readable product and belong on stdout, while
# the human-facing remediation hint belongs on stderr — so a reader piping
# stdout into a file still SEES the hint, and a script consuming stdout is not
# forced to parse prose out of its data.
#
# The hint must name BOTH available fixes, because they are genuinely
# different decisions: repoint the citation (the move was incidental), or
# regenerate the baseline (the move is intentional and the citation is being
# deliberately grandfathered). A hint naming only one silently steers every
# author toward it.
# ===========================================================================
echo ""
echo "--- Section L: remediation hint on STDERR, findings on STDOUT ---"

# Capture the two streams INDEPENDENTLY — the only way to prove they are
# genuinely separate rather than interleaved into one.
_HINT_OUT="$(mktemp)"; _TMPFILES+=("$_HINT_OUT")
_HINT_ERR="$(mktemp)"; _TMPFILES+=("$_HINT_ERR")
_HINT_RC=0
_HINT_SCAN="$(_scan_file "$FIX_ACCEPT")"
_HINT_BASELINE="$(_write_baseline)"
( export REIFY_CITED_TEST_PATH_BASELINE="$_HINT_BASELINE"
  _ratchet_check_subset "$_HINT_SCAN" ) > "$_HINT_OUT" 2> "$_HINT_ERR" || _HINT_RC=$?

assert "L: the checker still exits non-zero on a violation (the hint must not perturb the exit code)" \
    test "$_HINT_RC" -ne 0

assert "L: STDOUT carries the findings" \
    grep -qF 'docs/testing.md :: crates/mycrate/tests/examples_smoke.rs' "$_HINT_OUT"

assert "L: STDERR carries a remediation hint" \
    grep -qiE 'hint|remedy|fix' "$_HINT_ERR"

# BOTH fixes named, on stderr.
assert "L: the hint names fix 1 — repoint the citation to the suggested target" \
    grep -qiE 'repoint|update the citation' "$_HINT_ERR"
assert "L: the hint names fix 2 — regenerate the baseline to grandfather the citation deliberately" \
    grep -qF -- '--emit-baseline' "$_HINT_ERR"

# The streams are genuinely separate: the hint must NOT also appear on stdout,
# and the findings must NOT also appear on stderr. Either leak would defeat
# the separation even though both texts were technically emitted.
_streams_do_not_leak() {
    if grep -qF -- '--emit-baseline' "$_HINT_OUT"; then
        echo "the remediation hint leaked onto STDOUT:"; cat "$_HINT_OUT"; return 1
    fi
    if grep -qF 'docs/testing.md :: ' "$_HINT_ERR"; then
        echo "the findings leaked onto STDERR:"; cat "$_HINT_ERR"; return 1
    fi
    return 0
}

assert "L: the two streams do not leak into each other" \
    _streams_do_not_leak

# On a GREEN run neither stream may carry anything: an all-green suite stays
# quiet, and a hint emitted on success would train readers to ignore it.
_no_hint_on_a_clean_run() {
    local out err rc=0 covered
    out="$(mktemp)"; err="$(mktemp)"; _TMPFILES+=("$out" "$err")
    covered="$(_write_baseline \
        'docs/testing.md :: crates/mycrate/tests/examples_smoke.rs' \
        'examples/part.ri :: crates/mycrate/tests/examples_smoke.rs' \
        'crates/mycrate/src/lib.rs :: crates/mycrate/tests/examples_smoke.rs')"
    ( export REIFY_CITED_TEST_PATH_BASELINE="$covered"
      _ratchet_check_subset "$_HINT_SCAN" ) > "$out" 2> "$err" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "expected a green run against a fully-covering baseline, got rc$rc:"; cat "$out" "$err"; return 1
    fi
    if [ -s "$out" ] || [ -s "$err" ]; then
        echo "a green run emitted output; it must be byte-for-byte silent:"
        echo "--- stdout ---"; cat "$out"; echo "--- stderr ---"; cat "$err"; return 1
    fi
    return 0
}

assert "L: a green run emits no hint and no findings on either stream" \
    _no_hint_on_a_clean_run

# ===========================================================================
# The gate itself, last: the scenarios above having passed, run the real
# whole-tree check. This is the SAME function --gate-only dispatches to — the
# full suite does not carry a second copy of the main body.
#
# test_summary is called by _run_whole_tree_gate and exits non-zero if any
# assert in this whole file failed (the PASS/FAIL counters are file-global).
# ===========================================================================
echo ""
_run_whole_tree_gate
