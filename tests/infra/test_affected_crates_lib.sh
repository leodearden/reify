#!/usr/bin/env bash
# tests/infra/test_affected_crates_lib.sh — drift test for scripts/affected-crates-lib.sh
#
# Validates that affected_crates() correctly maps changed files to the
# affected workspace-crate set per docs/prds/verify-scope-contract.md §3 C3/C4/C5.
#
# Assertions (B8 battery):
#   1. C4: Cargo.lock (global) forces ALL
#   2. C1/§5: non-crate paths (docs/**, gui/src/**) contribute no crates (no ALL)
#   3. C5: unmappable path forces ALL
#   4. crates/<leaf-crate>/** maps to itself (reify-cli has no dependents)
#   5. gui/src-tauri/** maps to reify-gui
#   6. reverse closure of reify-core is NOT the ALL sentinel
#   7. reverse closure of reify-core contains sampled dependents (reify-ir, reify-eval, reify-cli, reify-gui)
#   8. reverse closure of reify-ir is NOT the ALL sentinel
#   9. cargo metadata failure -> ALL (C5)
#  10. global anywhere in arg list -> ALL
#  11. (task 4938) dev-dep non-transitivity: an OCCT-seed closure excludes a
#      dev-dep-of-a-dev-dep (reify-eval-fea-tests) whose own test binary links
#      no OCCT, while still including the seed (reify-kernel-occt) and a
#      direct dev-dependent (reify-eval) that does compile OCCT
#  12. (task 4938) tests/infra/* non-crate allowlist composes with crate
#      accumulation in a mixed diff (narrows; does not force ALL)
#  13. (task 4938 amendment; unified by task 5124) drift guard:
#      affected_crates(occt-seed) equals occt_touching_set — both now
#      delegate to the single shared _reify_compile_closure helper
#      (scripts/occt-scope-lib.sh), and this cross-check fails loudly if
#      that ever regresses
#  14. (task 5124) _reify_compile_closure — the shared helper both wrappers
#      delegate to — is exercised directly: its output equals
#      occt_touching_set and contains the seed crate
#  15. (task 6292) cold registry cache: with CARGO_HOME redirected to an
#      empty dir, affected_crates fails wide into the C5 ALL path and emits
#      the fallback diagnostic on stderr — i.e. _reverse_closure's
#      `cargo metadata` performs no network I/O — and does so inside a 5s
#      wall-clock bound that separately guards the pre-commit-hook stall
#      hazard (the ALL/diagnostic pair is what discriminates; see the
#      "WHAT PROVES WHAT" note on that block)
#  16. (task 6292) argv coverage: _reverse_closure invokes cargo metadata
#      with --offline alongside task 6277's --locked
#  17. (task 7427) INERT-SPOT: the inert class (docs/**, *.md, *.yaml, *.yml)
#      is ONE list shared with verify.sh's decide_scope, so a mixed
#      crate + top-level *.md diff narrows to the crate-alone closure
#      instead of C5-widening to ALL — while an unmappable NON-inert path
#      still widens, and a crate-OWNED *.md/*.yaml still maps to its owning
#      crate, because attribution outranks the inert class on both sides
#  18. (task 7427) EXAMPLES-CORPUS: examples/**/*.ri (flat and nested) maps
#      to the declared reader crates instead of C5-widening to ALL, while
#      non-.ri, non-inert content under examples/ still widens; plus
#      RI-CORPUS-DRIFT, a derived-⊆-declared guard that keeps the declared
#      reader list honest against the repo's real Rust sources
#  19. (task 7096) a per-crate manifest touch (crates/*/Cargo.toml,
#      gui/src-tauri/Cargo.toml) additionally contributes the crate that
#      hosts the workspace crate-DAG gate, a non-manifest source touch
#      does not, and the ALL sentinel is never unioned with it; plus a drift
#      guard pinning _REIFY_DAG_GATE_CRATE to a real workspace member that
#      hosts the gate

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$REPO_ROOT/scripts/affected-crates-lib.sh" ] || { echo "ERROR: affected-crates-lib.sh not found at $REPO_ROOT/scripts/affected-crates-lib.sh"; exit 1; }
source "$REPO_ROOT/scripts/affected-crates-lib.sh"

# Also source occt-scope-lib.sh (read-only usage — sourcing, not editing) so
# this file can cross-check _reverse_closure's ported compile-closure
# algorithm against occt_touching_set's independent implementation of the
# same model. See the drift-guard assertion below.
[ -f "$REPO_ROOT/scripts/occt-scope-lib.sh" ] || { echo "ERROR: occt-scope-lib.sh not found at $REPO_ROOT/scripts/occt-scope-lib.sh"; exit 1; }
source "$REPO_ROOT/scripts/occt-scope-lib.sh"

echo "=== affected-crates-lib drift tests ==="

# ---------------------------------------------------------------------------
# Preflight (review follow-up, task 6292): warm-registry-cache dependency.
#
# Every real-cargo closure assertion below (#4-#14) needs `cargo metadata
# --format-version 1 --locked --offline` — the exact invocation
# _reverse_closure makes — to SUCCEED. On a genuinely cold ~/.cargo (fresh
# container, new host, a setup-dev.sh that never ran) it cannot, so
# affected_crates fails wide to ALL and all ~10 of those assertions fail
# pointing at the closure logic rather than at the missing cache. Assertion
# #15 is the only one that EXPECTS ALL; it passes in that environment.
#
# Advisory, never fatal: this changes no pass/fail semantics, it only makes a
# cold-cache environment emit one legible CAUSE line instead of ten
# misleading SYMPTOMS. Re-emitted just before test_summary so the explanation
# lands next to the FAIL tally rather than scrolled off the top. Costs one
# extra `cargo metadata` (~2s warm) on top of a suite that already makes a
# dozen.
# ---------------------------------------------------------------------------
_COLD_REGISTRY_PREFLIGHT=""
if ! ( cd "$REPO_ROOT" && cargo metadata --format-version 1 --locked --offline ) >/dev/null 2>&1; then
    _COLD_REGISTRY_PREFLIGHT="COLD REGISTRY CACHE: \`cargo metadata --locked --offline\` fails in $REPO_ROOT, so every real-cargo closure assertion below fails wide to ALL. This is an environment problem, not a closure-logic bug — warm it with \`cargo fetch --locked\` (or run scripts/setup-dev.sh) and re-run."
    echo ""
    echo "!!! $_COLD_REGISTRY_PREFLIGHT"
fi

# ---------------------------------------------------------------------------
# Step 1: C4 global-force — Cargo.lock forces ALL
# ---------------------------------------------------------------------------
echo ""
echo "--- C4: global files force ALL ---"

assert "Cargo.lock forces ALL" \
    test "$(affected_crates Cargo.lock)" = "ALL"

# ---------------------------------------------------------------------------
# Step 3: C1/§5 non-crate paths — no crates contributed, must NOT force ALL
# ---------------------------------------------------------------------------
echo ""
echo "--- §5 non-crate paths contribute nothing (not ALL) ---"

assert "docs path -> empty" \
    test -z "$(affected_crates docs/architecture/x.md)"

assert "gui frontend -> empty" \
    test -z "$(affected_crates gui/src/App.tsx)"

assert "tests/infra shell/python -> empty (no ALL)" \
    test -z "$(affected_crates tests/infra/test_cpu_load_governance.sh)"

# ---------------------------------------------------------------------------
# Step 5: C5 fail-wide — unmappable path forces ALL
# ---------------------------------------------------------------------------
echo ""
echo "--- C5: unmappable path forces ALL ---"

assert "unmappable path -> ALL" \
    test "$(affected_crates some/unknown/place.zzz)" = "ALL"

# ---------------------------------------------------------------------------
# INERT-SPOT (task 7427): the inert class is ONE list, shared with decide_scope.
# These assertions pin the two classifications together — the mixed
# crate + top-level *.md diff is the shape the old two-copy version widened to
# ALL. Contract and cost: §3 of docs/prds/verify-scope-contract.md.
# ---------------------------------------------------------------------------
echo ""
echo "--- INERT-SPOT: the inert class matches decide_scope's (docs/**, *.md, *.yaml, *.yml) ---"

_check_mixed_md_not_ALL() {
    local out
    out="$(affected_crates crates/reify-doc/src/lib.rs README.md)"
    [ "$out" != "ALL" ]
}
assert "crate + top-level README.md is NOT the ALL sentinel" _check_mixed_md_not_ALL

_check_mixed_md_equals_crate_alone() {
    local with_md without_md
    with_md="$(affected_crates crates/reify-doc/src/lib.rs README.md)"
    without_md="$(affected_crates crates/reify-doc/src/lib.rs)"
    echo "with README.md: [$with_md]"
    echo "crate alone:    [$without_md]"
    [ "$with_md" = "$without_md" ]
}
assert "an inert *.md contributes nothing: closure equals the crate-alone closure" \
    _check_mixed_md_equals_crate_alone

assert "top-level *.yaml -> empty (not ALL)" \
    test -z "$(affected_crates dark-factory-orchestrator.yaml)"

assert "top-level *.md -> empty (not ALL)" \
    test -z "$(affected_crates CLAUDE.md)"

# Fail-wide is narrowed, not weakened: an unmappable NON-inert path still
# C5-widens. scripts/verify.sh is the sharpest case — a verify-pipeline
# artifact whose blast radius is the whole workspace.
assert "unmappable non-inert path still forces ALL (C5 preserved)" \
    test "$(affected_crates scripts/verify.sh)" = "ALL"

# Crate ATTRIBUTION outranks the inert class (contract §5). The two fixtures
# below are chosen because they are real compile/test inputs rather than
# incidental docs — crates/reify-mcp/src/tools/chunks/*.md are `include_str!`-ed
# by crates/reify-mcp/src/tools/language_chunks.rs, and
# crates/reify-doc/tests/snapshots/*.md are compared by
# crates/reify-doc/tests/fmt_markdown_tests.rs; 32 of the 33 tracked
# crates/**/*.{md,yaml,yml} are of that kind. Classifying one as "no crate"
# drops the very crate whose tests read the edited bytes.
_check_crate_owned_doc_maps_to_owner() {
    # Usage: <expected-crate> <crate-owned path>
    local expected="$1" path="$2" out
    out="$(affected_crates "$path")"
    echo "$path -> [$(printf '%s' "$out" | tr '\n' ' ')]"
    printf '%s\n' "$out" | grep -qx "$expected"
}
assert "an include_str!-ed crate-owned *.md maps to its owning crate" \
    _check_crate_owned_doc_maps_to_owner reify-mcp crates/reify-mcp/src/tools/chunks/syntax.md
assert "a crate-owned snapshot *.md maps to its owning crate" \
    _check_crate_owned_doc_maps_to_owner reify-doc crates/reify-doc/tests/snapshots/integration_full_v01.single.md

# A crate-owned doc narrows like its crate — it neither vanishes nor widens
# to ALL. Compared against the UNION of the two crates' own closures rather
# than a hand-written crate list, so the assertion cannot rot as the
# dependency graph moves.
_check_crate_owned_doc_unions() {
    local mixed expected
    mixed="$(affected_crates crates/reify-doc/src/lib.rs crates/reify-mcp/src/tools/chunks/syntax.md | sort -u)"
    expected="$( { affected_crates crates/reify-doc/src/lib.rs
                   affected_crates crates/reify-mcp/src/lib.rs; } | sort -u)"
    echo "lib + crate-owned .md: [$(printf '%s' "$mixed"    | tr '\n' ' ')]"
    echo "union of both crates:  [$(printf '%s' "$expected" | tr '\n' ' ')]"
    [ "$mixed" != "ALL" ] && [ "$mixed" = "$expected" ]
}
assert "crate + another crate's owned *.md equals the union of the two closures" \
    _check_crate_owned_doc_unions

# ---------------------------------------------------------------------------
# Step 7: direct-set printing — crate-mapped paths emit the crate name
# ---------------------------------------------------------------------------
echo ""
echo "--- Direct-set printing (no closure yet) ---"

assert "leaf crate cli -> itself" \
    test "$(affected_crates crates/reify-cli/src/main.rs)" = "reify-cli"

_check_gui_maps_reify_gui() {
    affected_crates gui/src-tauri/src/main.rs | grep -qx reify-gui
}
assert "gui/src-tauri maps to reify-gui" _check_gui_maps_reify_gui

# ---------------------------------------------------------------------------
# Step 9: C3 reverse-closure — low-level crate expands to dependents
# Ground-truth assertions encode independently-known dependency facts rather
# than comparing against a clone of the implementation (which would make any
# shared logic bug silently pass on both sides).
# ---------------------------------------------------------------------------
echo ""
echo "--- C3: reverse-dependency closure ---"

_check_reify_core_not_ALL() {
    local out
    out="$(affected_crates crates/reify-core/src/lib.rs)"
    [ "$out" != "ALL" ]
}
assert "reify-core closure is NOT the ALL sentinel" _check_reify_core_not_ALL

_check_contains() {
    # Usage: _check_contains <expected-crate> <input-file-path>
    local expected="$1" input_path="$2"
    affected_crates "$input_path" | grep -qx "$expected"
}
assert "reify-core closure contains reify-ir"    _check_contains reify-ir    crates/reify-core/src/lib.rs
assert "reify-core closure contains reify-eval"  _check_contains reify-eval  crates/reify-core/src/lib.rs
assert "reify-core closure contains reify-cli"   _check_contains reify-cli   crates/reify-core/src/lib.rs
assert "reify-core closure contains reify-gui"   _check_contains reify-gui   crates/reify-core/src/lib.rs

_check_reify_ir_not_ALL() {
    local out
    out="$(affected_crates crates/reify-ir/src/lib.rs)"
    [ "$out" != "ALL" ]
}
assert "reify-ir closure is NOT the ALL sentinel" _check_reify_ir_not_ALL

# ---------------------------------------------------------------------------
# Task 4938 Fix #1: dev-dep non-transitivity (cargo-accurate compile closure)
#
# cargo's compile semantics are NOT transitive over dev-deps: testing crate X
# compiles normal/build-closure(X) plus normal/build-closure(each DIRECT
# dev-dep of X); dev-deps of X's transitive deps never compile. A reverse
# closure that walks ALL dep kinds (null/build/dev) transitively
# over-approximates this.
#
# Ground-truth facts (independently known, not a clone of _reverse_closure's
# algorithm — verified against scripts/occt-scope-lib.sh:occt_touching_set,
# which computes the OCCT-touching set from the FORWARD direction):
#   - reify-eval dev-deps reify-kernel-occt (crates/reify-eval/Cargo.toml) ->
#     reify-eval's own tests DO compile OCCT.
#   - reify-eval-fea-tests dev-deps reify-eval (its [dependencies] is empty) ->
#     a two-hop dev chain (occt <- reify-eval <- reify-eval-fea-tests) exists,
#     but reify-eval-fea-tests's test binary links no OCCT: normal-closure of
#     its direct dev-dep reify-eval does not carry reify-eval's OWN dev-dep on
#     OCCT. It must be EXCLUDED from an OCCT-seeded affected set.
#
# Placed BEFORE the cargo-metadata-failure stub below: that stub's `cargo()`
# shell function is defined in the current shell (assert invokes checkers
# directly, not in a subshell, per esc-4959-57) and is never unset, so it
# would otherwise leak into every assertion that follows it.
# ---------------------------------------------------------------------------
echo ""
echo "--- Task 4938 Fix #1: dev-dep non-transitivity ---"

_check_not_contains() {
    # Usage: _check_not_contains <unexpected-crate> <input-file-path>
    local unexpected="$1" input_path="$2"
    ! affected_crates "$input_path" | grep -qx "$unexpected"
}

_check_occt_seed_not_ALL() {
    local out
    out="$(affected_crates crates/reify-kernel-occt/src/lib.rs)"
    [ "$out" != "ALL" ]
}
assert "OCCT-seed closure is NOT the ALL sentinel" _check_occt_seed_not_ALL

assert "OCCT-seed closure does NOT contain reify-eval-fea-tests (dev-dep of a dev-dep; its test binary links no OCCT)" \
    _check_not_contains reify-eval-fea-tests crates/reify-kernel-occt/src/lib.rs

assert "OCCT-seed closure contains reify-kernel-occt (the seed itself)" \
    _check_contains reify-kernel-occt crates/reify-kernel-occt/src/lib.rs

assert "OCCT-seed closure contains reify-eval (direct dev-dependent whose own tests compile OCCT)" \
    _check_contains reify-eval crates/reify-kernel-occt/src/lib.rs

# ---------------------------------------------------------------------------
# Amendment (code-review follow-up, task 4938; unified by task 5124): drift
# guard between the two callers of the shared compile-closure model.
#
# _reverse_closure (scripts/affected-crates-lib.sh) and occt_touching_set
# (scripts/occt-scope-lib.sh) both delegate to the single shared
# `_reify_compile_closure` helper (scripts/occt-scope-lib.sh) for the
# adj_normal/adj_dev + normal_closure algorithm (the PRD calls this "reused
# verbatim and parameterized" — docs/prds/verify-scope-contract.md §3),
# rather than each carrying its own copy. This equality check remains as a
# regression guard: it fails loudly if either wrapper's seed handling, or
# the shared helper itself, ever regresses.
# ---------------------------------------------------------------------------
echo ""
echo "--- Drift guard: affected_crates(occt-seed) == occt_touching_set ---"

_check_occt_seed_matches_touching_set() {
    local from_affected from_occt
    from_affected="$(affected_crates crates/reify-kernel-occt/src/lib.rs | sort -u)"
    from_occt="$(occt_touching_set | sort -u)"
    [ "$from_affected" = "$from_occt" ]
}
assert "affected_crates(occt-seed) equals occt_touching_set (both delegate to the shared helper)" \
    _check_occt_seed_matches_touching_set

# ---------------------------------------------------------------------------
# Task 5124: direct exercise of the extracted shared helper.
#
# _reify_compile_closure (scripts/occt-scope-lib.sh) is the single
# implementation of the adj_normal/adj_dev + normal_closure model that both
# occt_touching_set and _reverse_closure delegate to. This calls it directly
# (bypassing both wrappers) to guard against a helper that silently returns
# nothing — a regression the drift-guard assert above would miss if both
# wrappers happened to be broken identically.
# ---------------------------------------------------------------------------
echo ""
echo "--- Task 5124: _reify_compile_closure direct exercise ---"

_check_compile_closure_matches_occt() {
    local from_helper from_occt
    from_helper="$(cargo metadata --format-version 1 2>/dev/null | _reify_compile_closure reify-kernel-occt | sort -u)"
    from_occt="$(occt_touching_set | sort -u)"
    [ "$from_helper" = "$from_occt" ]
}
assert "_reify_compile_closure(reify-kernel-occt) equals occt_touching_set" \
    _check_compile_closure_matches_occt

_check_compile_closure_contains_seed() {
    cargo metadata --format-version 1 2>/dev/null | _reify_compile_closure reify-kernel-occt | grep -qx reify-kernel-occt
}
assert "_reify_compile_closure(reify-kernel-occt) contains reify-kernel-occt (seed itself)" \
    _check_compile_closure_contains_seed

# ---------------------------------------------------------------------------
# Task 4938 Fix #2 composition guard: the tests/infra/* non-crate allowlist
# (already landed, ab021821fa) must compose with crate accumulation in a
# mixed diff — a tests/infra/* path contributes no crates but must not force
# ALL nor prevent a co-changed crate path from narrowing the set normally.
# ---------------------------------------------------------------------------
echo ""
echo "--- Task 4938 Fix #2 composition: tests/infra/* + crate path narrows together ---"

_check_mixed_not_ALL() {
    local out
    out="$(affected_crates tests/infra/test_cpu_load_governance.sh crates/reify-doc/src/lib.rs)"
    [ "$out" != "ALL" ]
}
assert "mixed tests/infra + crate diff is NOT the ALL sentinel" _check_mixed_not_ALL

_check_mixed_contains_reify_doc() {
    affected_crates tests/infra/test_cpu_load_governance.sh crates/reify-doc/src/lib.rs | grep -qx reify-doc
}
assert "mixed tests/infra + crate diff contains reify-doc" _check_mixed_contains_reify_doc

# ---------------------------------------------------------------------------
# EXAMPLES-CORPUS (task 7427): examples/**/*.ri maps to its reader crates.
#
# The examples/ tree is a test CORPUS: compiled Rust test targets walk it and
# open .ri leaves by path. With no _file_to_crate rule for it, every one of the
# 264 tracked .ri files was an unmappable path — so the C5 arm fired and an
# .ri-only edit became the most expensive diff shape in the repo (a full
# workspace verify for a corpus edit). Mapping it to the declared reader set
# feeds those seeds through the normal reverse closure instead.
#
# Non-.ri, non-inert content under examples/ still takes C5: the mapping is a
# claim about .ri corpus leaves specifically, not about the directory.
# ---------------------------------------------------------------------------
echo ""
echo "--- EXAMPLES-CORPUS: examples/**/*.ri maps to the declared reader crates ---"

_check_examples_ri_not_ALL() {
    local out
    out="$(affected_crates examples/foo.ri)"
    echo "closure: [$out]"
    [ "$out" != "ALL" ] && [ -n "$out" ]
}
assert "examples/*.ri is NOT the ALL sentinel" _check_examples_ri_not_ALL

_check_examples_ri_contains() {
    # Usage: _check_examples_ri_contains <expected-crate>
    affected_crates examples/foo.ri | grep -qx "$1"
}
assert "examples/*.ri closure contains reify-eval"           _check_examples_ri_contains reify-eval
assert "examples/*.ri closure contains reify-compiler"       _check_examples_ri_contains reify-compiler
assert "examples/*.ri closure contains reify-cli"            _check_examples_ri_contains reify-cli
assert "examples/*.ri closure contains reify-eval-fea-tests" _check_examples_ri_contains reify-eval-fea-tests

# NESTED is the common real shape (examples/auto/, examples/ambient_default_
# material/, …), and a bash `case` glob's `*` matches `/`, so one arm covers
# both depths. Asserted as EQUALITY with the flat case so a future rule that
# accidentally keys on depth cannot pass.
_check_examples_nested_same_as_flat() {
    local nested flat
    nested="$(affected_crates examples/auto/bearing_unsat.ri)"
    flat="$(affected_crates examples/foo.ri)"
    echo "nested: [$nested]"
    echo "flat:   [$flat]"
    [ "$nested" = "$flat" ]
}
assert "nested examples/<dir>/*.ri closure equals the flat one" _check_examples_nested_same_as_flat

assert "examples/README.md -> empty (inert, not ALL)" \
    test -z "$(affected_crates examples/README.md)"

# Fail-wide PRESERVED for the two real non-.ri, non-inert tracked shapes.
assert "examples/**/*.gcode still forces ALL (C5 preserved)" \
    test "$(affected_crates examples/trajectory/test_data/printer_print_envelope.gcode)" = "ALL"

assert "examples/**/.gitkeep still forces ALL (C5 preserved)" \
    test "$(affected_crates examples/generics/.gitkeep)" = "ALL"

# ---------------------------------------------------------------------------
# RI-CORPUS-DRIFT: the declared reader set is derived from the repo, not
# hand-maintained (house pattern; mirrors PG-DRIFT and
# test_release_scoped_scope.sh).
#
# DERIVED ⊆ DECLARED, deliberately a subset and not an equality: an extra
# DECLARED crate only ever WIDENS the closure, which is the direction of error
# C5 already blesses. A new corpus reader that nobody declared is the real
# regression — that crate's tests would be narrowed AWAY by an edit to the
# very fixture they read.
# ---------------------------------------------------------------------------
echo ""
echo "--- RI-CORPUS-DRIFT: every workspace member whose Rust sources read the examples/ corpus is declared ---"

# _workspace_member_dirs — every workspace member's repo-relative directory and
# package name, TAB-separated, longest directory first so the prefix match below
# resolves a nested member to the nearest one.
#
# Read from `cargo metadata`, never hand-listed: a hand-kept pathspec stops
# meaning "every member" the moment one is added outside `crates/`, which is how
# this derivation first missed gui/src-tauri (reify-gui `include_str!`s a corpus
# leaf at compile time) and then tree-sitter-reify. Empty output means
# `cargo metadata` failed — see the cold-registry preflight above.
_workspace_member_dirs() {
    local meta
    meta="$( cd "$REPO_ROOT" && cargo metadata --format-version 1 --locked --offline 2>/dev/null )" || return 0
    printf '%s\n' "$meta" | python3 -c '
import json, os, sys
meta = json.load(sys.stdin)
root = os.path.realpath(sys.argv[1])
members = set(meta["workspace_members"])
rows = [
    (os.path.relpath(os.path.dirname(os.path.realpath(p["manifest_path"])), root), p["name"])
    for p in meta["packages"] if p["id"] in members
]
for d, name in sorted(rows, key=lambda r: (-len(r[0]), r[0])):
    print(f"{d}\t{name}")
' "$REPO_ROOT"
}

# _derived_ri_corpus_crates — every workspace member with a non-comment Rust
# source line naming the examples/ corpus, one package name per line.
#
# TWO reader shapes are matched, because the literal one alone left this guard
# green by coincidence rather than by construction: a named leaf
# (`include_str!(".../examples/foo.ri")`) AND a directory walk
# (`collect_files(&root.join("examples"), "ri", …)`, or a path assembled with
# `format!("examples/{}.ri", …)`, which no `\.ri` pattern can see). A crate that
# only walked the directory would otherwise be an undeclared reader.
# OVER-derivation is the safe direction: it can only force a DECLARATION, and an
# extra declared crate merely widens — the direction of error C5 already blesses.
#
# A member with no §5 mapping rule of its own (tree-sitter-reify, which
# _is_global widens to ALL on its own edits) is still swept and still has to be
# declared if it ever reads the corpus: an examples/*.ri edit must reach the
# tests of every crate that opens those bytes, whatever that crate's own edits do.
_RI_CORPUS_READ_RE='examples/|"examples"'

_derived_ri_corpus_crates() {
    local members
    members="$(_workspace_member_dirs)"
    [ -n "$members" ] || return 0

    local -a pathspecs=()
    local -A member_pkg=()
    local dir pkg
    while IFS=$'\t' read -r dir pkg; do
        [ -n "$dir" ] || continue
        pathspecs+=("$dir/**.rs")
        member_pkg["$dir"]="$pkg"
    done <<< "$members"

    git -C "$REPO_ROOT" grep -nE "$_RI_CORPUS_READ_RE" -- "${pathspecs[@]}" \
        | while IFS= read -r line; do
            # `path:lineno:code` — split off the prefix to inspect the CODE.
            local path="${line%%:*}"
            local code="${line#*:}"; code="${code#*:}"
            # A pure-comment mention is not a corpus read: skip lines whose
            # first non-space characters are `//`.
            local trimmed="${code#"${code%%[![:space:]]*}"}"
            case "$trimmed" in //*) continue ;; esac
            # Project the source path onto its owning member: walk up parents
            # until one is a member directory (longest match wins).
            local owner="$path"
            while [ -n "$owner" ] && [ -z "${member_pkg[$owner]:-}" ]; do
                case "$owner" in */*) owner="${owner%/*}" ;; *) owner="" ;; esac
            done
            [ -n "$owner" ] && printf '%s\n' "${member_pkg[$owner]}"
        done | sort -u
}

_check_derived_subset_of_declared() {
    local derived missing=""
    derived="$(_derived_ri_corpus_crates)"
    echo "derived:  [$(printf '%s' "$derived" | tr '\n' ' ')]"
    echo "declared: [${_RI_CORPUS_CRATES:-<unset>}]"
    [ -n "$derived" ] || { echo "derivation produced NOTHING — cargo metadata, the grep or the projection broke"; return 1; }
    local c
    while IFS= read -r c; do
        [ -n "$c" ] || continue
        case " ${_RI_CORPUS_CRATES:-} " in
            *" $c "*) ;;
            *) missing+=" $c" ;;
        esac
    done <<< "$derived"
    [ -z "$missing" ] || { echo "UNDECLARED corpus readers:$missing"; return 1; }
    return 0
}
assert "derived examples/*.ri reader crates ⊆ declared _RI_CORPUS_CRATES" \
    _check_derived_subset_of_declared

# ---------------------------------------------------------------------------
# GV-PREMISE (task 7427): the two crates the vitest-gate fixtures are built on.
#
# tests/infra/test_verify_scope.sh's GV-1/GV-2 drive AFFECTED_CLOSURE through
# REIFY_AFFECTED_CRATES_OVERRIDE, because their throwaway fixture repo has no
# cargo workspace. Those hand-written override values are only meaningful while
# they describe reality — so pin the two facts they encode against the REAL
# repo here, where a real `cargo metadata` runs.
#
# The negative holds because the reify-doc -> reify-eval edge is a DEV-dep and
# the compile-closure model is dev-dep non-transitive — the property the task
# 4938 section above already covers. If that model ever changes, GV-1 becomes
# fiction silently; this assertion is what makes it fail loudly instead.
# ---------------------------------------------------------------------------
echo ""
echo "--- GV-PREMISE: the reify-doc / reify-eval closures the vitest-gate fixtures assume ---"

assert "reify-doc closure EXCLUDES reify-gui (GV-1's skip premise)" \
    _check_not_contains reify-gui crates/reify-doc/src/lib.rs

assert "reify-eval closure INCLUDES reify-gui (GV-2's run premise)" \
    _check_contains reify-gui crates/reify-eval/src/lib.rs

# ---------------------------------------------------------------------------
# Task 7096: a per-crate manifest touch pulls in the crate-DAG gate.
#
# Rule and rationale — why a per-crate Cargo.toml edit can restructure the
# crate DAG with no Cargo.lock delta (so C4 never fires), and why the gate's
# host crate is unioned into the closure's result rather than seeded into it:
# docs/prds/verify-scope-contract.md §3, "Per-crate manifest touches also
# contribute the crate-DAG gate".
#
# The non-manifest assertion is the discrimination half: it refuses a
# degenerate "always append the gate crate" implementation, which would
# permanently widen every narrowed verify.
#
# Real-cargo assertions, so they share the placement constraint the Fix #1
# block above documents: they must sit BEFORE the stub sections at the end of
# this file, each of which redefines cargo() for the rest of this shell and
# never unsets it.
# ---------------------------------------------------------------------------
echo ""
echo "--- Task 7096: per-crate manifest touch pulls in the crate-DAG gate ---"

assert "crates/*/Cargo.toml touch pulls in reify-build-utils, so the workspace DAG gate runs at task-verify time" \
    _check_contains reify-build-utils crates/reify-expr/Cargo.toml

assert "gui/src-tauri/Cargo.toml touch pulls in reify-build-utils (the second per-crate manifest location in this workspace)" \
    _check_contains reify-build-utils gui/src-tauri/Cargo.toml

assert "a non-manifest source touch does NOT pull in reify-build-utils — the rule keys on the manifest, not on the crate" \
    _check_not_contains reify-build-utils crates/reify-expr/src/lib.rs

_check_manifest_mixed_not_ALL() {
    local out
    out="$(affected_crates docs/x.md crates/reify-expr/Cargo.toml)"
    [ "$out" != "ALL" ]
}
assert "mixed docs + manifest diff is NOT the ALL sentinel (manifest rule composes with the non-crate allowlist)" \
    _check_manifest_mixed_not_ALL

_check_manifest_mixed_contains_gate() {
    affected_crates docs/x.md crates/reify-expr/Cargo.toml | grep -qx reify-build-utils
}
assert "mixed docs + manifest diff still contains reify-build-utils (the allowlist does not short-circuit the manifest rule)" \
    _check_manifest_mixed_contains_gate

# SPOT drift guard: scripts/affected-crates-lib.sh hardcodes the gate's host
# crate name in _REIFY_DAG_GATE_CRATE, and nothing downstream validates it —
# the name is appended OUTSIDE the reverse closure, so cargo never sees it
# until verify.sh expands it into `-p <crate>`. Both assertions below READ the
# constant rather than restating its value, so they pin it in the direction
# that has teeth: a stale constant (gate relocated, crate renamed) reds here
# instead of hard-failing every branch verify that touches a manifest with
# "package ID specification did not match any packages".
assert "_REIFY_DAG_GATE_CRATE names the crate that hosts the DAG gate (crates/<crate>/tests/crate_dag_assertion.rs)" \
    test -f "$REPO_ROOT/crates/$_REIFY_DAG_GATE_CRATE/tests/crate_dag_assertion.rs"

# Workspace membership is a fact distinct from directory layout: the root
# Cargo.toml lists members explicitly, and a package name need not equal its
# directory name. --no-deps restricts `packages` to the workspace members
# exactly — the set a `-p` selector resolves against. Matched on that package
# list rather than by grepping the raw JSON, because each member's nested
# `dependencies` array carries `name` keys too, so a name still referenced as
# a dependency would pass a plain grep after being dropped from `members`.
# python3 is already a hard dependency of this suite via _reify_compile_closure.
_check_gate_crate_is_workspace_member() {
    echo "_REIFY_DAG_GATE_CRATE=[$_REIFY_DAG_GATE_CRATE]"
    ( cd "$REPO_ROOT" && cargo metadata --format-version 1 --locked --offline --no-deps 2>/dev/null ) \
        | python3 -c 'import json,sys; meta=json.load(sys.stdin); sys.exit(0 if sys.argv[1] in {p["name"] for p in meta["packages"]} else 1)' \
            "$_REIFY_DAG_GATE_CRATE"
}
assert "_REIFY_DAG_GATE_CRATE is a real workspace member, so the -p selector verify.sh emits for it resolves" \
    _check_gate_crate_is_workspace_member

# ---------------------------------------------------------------------------
# Amendment (code-review follow-up, task 6277): --locked non-mutation check.
#
# --locked's whole purpose is refusing to rewrite Cargo.lock rather than
# silently updating it (scripts/affected-crates-lib.sh _reverse_closure).
# Must run against the REAL cargo, so it is placed here — before the
# cargo-failure stub section below redefines cargo() for the rest of this
# shell (see that section's own placement comment). This only holds while
# the repo's Cargo.lock is valid/in-sync, which every closure assertion
# above already depends on (each needs a real `cargo metadata` to succeed).
# ---------------------------------------------------------------------------
echo ""
echo "--- Amendment (task 6277): affected_crates does not mutate Cargo.lock ---"

_check_cargo_lock_unchanged() {
    local before after
    [ -f "$REPO_ROOT/Cargo.lock" ] || return 1
    before="$(cat "$REPO_ROOT/Cargo.lock")"
    affected_crates crates/reify-core/src/lib.rs >/dev/null
    after="$(cat "$REPO_ROOT/Cargo.lock")"
    [ "$before" = "$after" ]
}
assert "affected_crates leaves Cargo.lock byte-for-byte unchanged" _check_cargo_lock_unchanged

# ---------------------------------------------------------------------------
# Amendment (task 6292): cold-registry-cache containment.
#
# --offline, alongside task 6277's --locked, makes _reverse_closure's
# `cargo metadata` hermetic: --locked only refuses to REWRITE Cargo.lock, it
# does not forbid network I/O. With --offline, a cold registry cache must
# fail FAST into the pre-existing C5 fail-wide ALL path
# (docs/prds/verify-scope-contract.md §3 C5) instead of stalling on an
# unbounded index/manifest fetch — which matters because this call now runs
# on the pre-commit-hook tier and under verify.sh --print-plan.
#
# Mechanism: redirect CARGO_HOME to an empty temp dir, which is a genuinely
# cold registry. Deliberately does NOT set CARGO_NET_OFFLINE (or any offline
# cargo config, or pass --offline from here): that would force the behaviour
# under test from OUTSIDE the code and pass identically with or without the
# flag in _reverse_closure's argv — a tautological green. RUSTUP_HOME is
# left untouched (it is independent of CARGO_HOME), so toolchain resolution
# still works and cargo fails for the intended offline-resolution reason.
#
# WHAT PROVES WHAT (corrected in review). The ALL and stderr-diagnostic
# assertions below are the discriminators, and they are binary and
# timing-independent: with --offline a cold registry cannot resolve, so the
# closure fails wide to the ALL sentinel; without it, cargo fetches the index
# and returns the real narrowed closure instead.
#
# The elapsed bound is a SECONDARY guard on the stall hazard that motivated
# the flag — NOT the no-network proof it was originally documented as.
# Measured on this host, two runs each: 0.15-0.35s with --offline, versus
# 12.3-12.7s for the same cold probe with --offline stripped from
# _reverse_closure's argv and the network reachable. The original 20s bound
# sat above BOTH and so passed identically either way, i.e. it asserted
# nothing. 5s sits between them — ~14x over the slowest measured offline run,
# ~2.4x under the fastest measured networked one — so it now fails on a
# revert as well, and still catches an offline path that regresses into
# something slow enough to stall a pre-commit hook. Re-measure both endpoints
# before retuning it.
#
# `timeout` wraps the WHOLE affected_crates call, not just cargo: wrapping
# only cargo would let _reverse_closure's `|| { echo ALL; return 0; }`
# swallow the kill and FALSE-GREEN this assertion. Wrapping the whole call
# makes a timeout yield rc=124 with empty output, which fails it instead.
#
# Real-cargo assertion, so it shares _check_cargo_lock_unchanged's placement
# constraint above: it must sit BEFORE the stub sections below, each of
# which redefines cargo() for the rest of this shell.
# ---------------------------------------------------------------------------
echo ""
echo "--- Amendment (task 6292): cold registry cache fails fast into ALL ---"

# Probe result memoized in parent-shell globals: assert() invokes checkers
# directly in this shell (no subshell), so one probe feeds all four checks
# below rather than paying for four cold-cache runs.
_COLD_CACHE_RC=""
_COLD_CACHE_OUT=""
_COLD_CACHE_ERR=""
_COLD_CACHE_ELAPSED_MS=""

# Bound in milliseconds; see the "WHAT PROVES WHAT" note above for the two
# measured endpoints this sits between, and re-measure before changing it.
_COLD_CACHE_MAX_MS=5000

_cold_cache_probe_once() {
    [ -n "$_COLD_CACHE_RC" ] && return 0
    local cold errfile t0 t1
    cold="$(mktemp -d "${TMPDIR:-/tmp}/reify-cold-cargo-home.XXXXXX")" || return 1
    errfile="$(mktemp "${TMPDIR:-/tmp}/reify-cold-cargo-err.XXXXXX")" || { rm -rf "$cold"; return 1; }
    # Millisecond clock (date +%s%N, as tests/infra/test_cpu_admit.sh and
    # test_lane_x_flock.sh already use): integer `date +%s` cannot express a
    # bound anywhere near the sub-second offline path this is measuring.
    t0="$(date +%s%N)"
    _COLD_CACHE_OUT="$(CARGO_HOME="$cold" timeout 60 bash -c '
        cd "$1" || exit 1
        # shellcheck source=scripts/affected-crates-lib.sh
        source "$1/scripts/affected-crates-lib.sh" || exit 1
        affected_crates crates/reify-core/src/lib.rs
    ' _ "$REPO_ROOT" 2>"$errfile")" && _COLD_CACHE_RC=0 || _COLD_CACHE_RC=$?
    t1="$(date +%s%N)"
    _COLD_CACHE_ELAPSED_MS=$(( (t1 - t0) / 1000000 ))
    _COLD_CACHE_ERR="$(cat "$errfile" 2>/dev/null)"
    rm -rf "$cold" "$errfile"
    return 0
}

# Emitted by EVERY checker rather than once inside the memoized probe:
# assert() discards a passing checker's output, so evidence printed by the
# probe alone would be dropped whenever the first checker to trigger it
# happens to pass. Dumped only on FAIL; an all-green run stays silent.
_cold_cache_report() {
    echo "cold-cache probe: rc=$_COLD_CACHE_RC elapsed=${_COLD_CACHE_ELAPSED_MS}ms out=[$_COLD_CACHE_OUT]"
    echo "cold-cache probe stderr: $_COLD_CACHE_ERR"
}

_check_cold_cache_rc_zero() {
    _cold_cache_probe_once || return 1
    _cold_cache_report
    [ "$_COLD_CACHE_RC" = "0" ]
}
assert "cold CARGO_HOME: affected_crates still returns 0 (rc=124 would mean it hung)" \
    _check_cold_cache_rc_zero

_check_cold_cache_is_ALL() {
    _cold_cache_probe_once || return 1
    _cold_cache_report
    [ "$_COLD_CACHE_OUT" = "ALL" ]
}
assert "cold CARGO_HOME: closure fails wide to ALL (C5)" _check_cold_cache_is_ALL

_check_cold_cache_fast() {
    _cold_cache_probe_once || return 1
    _cold_cache_report
    [ -n "$_COLD_CACHE_ELAPSED_MS" ] && [ "$_COLD_CACHE_ELAPSED_MS" -lt "$_COLD_CACHE_MAX_MS" ]
}
assert "cold CARGO_HOME: fails fast (<${_COLD_CACHE_MAX_MS}ms) — bounds the hook-tier stall hazard" \
    _check_cold_cache_fast

# Pins only the stable `falling back to ALL` substring, NOT the parenthetical
# cause wording, which _reverse_closure's diagnostic rewrites alongside the
# flag. This proves the C5 fallback path is what fired.
_check_cold_cache_diagnostic() {
    _cold_cache_probe_once || return 1
    _cold_cache_report
    case "$_COLD_CACHE_ERR" in
        *"falling back to ALL"*) return 0 ;;
        *) return 1 ;;
    esac
}
assert "cold CARGO_HOME: emits the C5 fallback diagnostic on stderr" \
    _check_cold_cache_diagnostic

# ---------------------------------------------------------------------------
# Step 11: C5 metadata-failure fail-wide + C4 global precedes crates in list
# ---------------------------------------------------------------------------
echo ""
echo "--- C5: cargo metadata failure -> ALL; C4: global anywhere -> ALL ---"

# Stub cargo as a shell function that returns 1 (failure).
# The stub is defined locally so it shadows the real cargo only within the
# subshell created by $(...), which is what _reverse_closure calls.
_check_cargo_fail_all() {
    cargo() { return 1; }
    local result
    result="$(affected_crates crates/reify-core/src/lib.rs)"
    [ "$result" = "ALL" ]
}
assert "cargo metadata failure -> ALL" _check_cargo_fail_all

assert "global anywhere in list -> ALL" \
    test "$(affected_crates crates/reify-cli/src/main.rs Cargo.lock)" = "ALL"

# Sentinel integrity (task 7096). The manifest rule must never union anything
# ONTO the ALL sentinel: ALL is a sentinel, not a crate name. verify.sh's
# NARROW_ACTIVE assignment reads any non-empty value other than exactly `ALL`
# as a narrowed -p list, so a two-line `ALL\nreify-build-utils` would expand to
# `-p ALL -p reify-build-utils` — converting a deliberate C5 fail-WIDE into a
# false NARROW carrying a bogus package selector, which is precisely the
# under-approximation docs/prds/verify-scope-contract.md §3 calls shipped
# breakage.
#
# Both checkers compare with `=` against the WHOLE captured value. A grep or
# substring check would pass on exactly the corrupt `ALL\nreify-build-utils`
# value and so would assert nothing.
#
# Each dumps the captured value from inside the checker, per the
# _cold_cache_report convention above: assert() discards a passing checker's
# output, so a corrupt sentinel stays legible from the FAIL line alone.

_SENTINEL_MANIFEST_OUT=""
_sentinel_manifest_report() {
    echo "cargo-failure on a manifest path: affected_crates -> [$_SENTINEL_MANIFEST_OUT]"
}

# Same local failing-cargo stub idiom as _check_cargo_fail_all above, so this
# does not depend on that checker's stub having leaked into this shell.
_check_manifest_cargo_failure_is_exactly_ALL() {
    cargo() { return 1; }
    _SENTINEL_MANIFEST_OUT="$(affected_crates crates/reify-expr/Cargo.toml)"
    _sentinel_manifest_report
    [ "$_SENTINEL_MANIFEST_OUT" = "ALL" ]
}
assert "C5 integrity: cargo failure on a MANIFEST path -> exactly ALL, never ALL unioned with the DAG-gate crate" \
    _check_manifest_cargo_failure_is_exactly_ALL

_SENTINEL_C4_OUT=""
_sentinel_c4_report() {
    echo "global alongside a manifest path: affected_crates -> [$_SENTINEL_C4_OUT]"
}

_check_manifest_plus_global_is_exactly_ALL() {
    _SENTINEL_C4_OUT="$(affected_crates crates/reify-expr/Cargo.toml Cargo.lock)"
    _sentinel_c4_report
    [ "$_SENTINEL_C4_OUT" = "ALL" ]
}
assert "C4 precedence: a global path alongside a MANIFEST path -> exactly ALL (global short-circuits ahead of the manifest rule)" \
    _check_manifest_plus_global_is_exactly_ALL

# ---------------------------------------------------------------------------
# Amendment (code-review follow-up, task 6277; extended by task 6292): argv
# coverage for --locked and --offline.
#
# The C5 cargo-failure->ALL assertion above proves the fallback fires on any
# cargo error, but it passes identically whether or not --locked/--offline
# are on the invocation — a future revert of either flag would be silently
# green here. This records the literal argv _reverse_closure hands to cargo
# so a revert fails loudly and specifically.
#
# The --offline guard (task 6292) is the flag-level companion to the
# cold-registry-cache assertion further up: unlike that one it needs neither
# a real cargo nor a network, so it still catches a revert in an environment
# where the cold-cache probe cannot run.
#
# Placed LAST: like the stub above, cargo() gets redefined in the current
# shell (assert invokes checkers directly, not in a subshell) and is never
# unset, so nothing after this point may rely on the real cargo.
# ---------------------------------------------------------------------------
echo ""
echo "--- Amendment (tasks 6277/6292): cargo metadata invoked with --locked --offline ---"

# Recorded once and memoized: both flag checkers below assert against the
# same captured argv rather than re-stubbing and re-running per flag.
_RECORDED_CARGO_ARGV=""

_record_reverse_closure_cargo_argv() {
    [ -n "$_RECORDED_CARGO_ARGV" ] && return 0
    local argv_file
    argv_file="$(mktemp "${TMPDIR:-/tmp}/reify-cargo-argv.XXXXXX")" || return 1
    # shellcheck disable=SC2317  # invoked indirectly, via _reverse_closure
    cargo() { printf '%s\n' "$*" >"$argv_file"; return 1; }
    affected_crates crates/reify-core/src/lib.rs >/dev/null
    _RECORDED_CARGO_ARGV="$(cat "$argv_file" 2>/dev/null)"
    rm -f "$argv_file"
    return 0
}

# Emitted per checker, not once inside the memoized recorder — see
# _cold_cache_report above for why. Dumped only on FAIL.
_recorded_cargo_argv_report() {
    echo "recorded cargo argv: [$_RECORDED_CARGO_ARGV]"
}

_check_cargo_invoked_with_locked() {
    _record_reverse_closure_cargo_argv || return 1
    _recorded_cargo_argv_report
    case "$_RECORDED_CARGO_ARGV" in
        metadata*--locked*) return 0 ;;
        *) return 1 ;;
    esac
}
assert "_reverse_closure invokes cargo metadata with --locked" _check_cargo_invoked_with_locked

_check_cargo_invoked_with_offline() {
    _record_reverse_closure_cargo_argv || return 1
    _recorded_cargo_argv_report
    case "$_RECORDED_CARGO_ARGV" in
        metadata*--offline*) return 0 ;;
        *) return 1 ;;
    esac
}
assert "_reverse_closure invokes cargo metadata with --offline" _check_cargo_invoked_with_offline

# Re-emit the cold-cache cause line adjacent to the FAIL tally (see the
# preflight near the top of this file for why). Deliberately an `if` and not
# `[ -n "$x" ] && echo ...`: at top level under `set -e` the latter exits the
# script with status 1 on the healthy path, where the test is false.
if [ -n "$_COLD_REGISTRY_PREFLIGHT" ]; then
    echo ""
    echo "!!! $_COLD_REGISTRY_PREFLIGHT"
fi

test_summary
