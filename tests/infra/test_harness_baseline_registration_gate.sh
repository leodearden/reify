#!/usr/bin/env bash
# tests/infra/test_harness_baseline_registration_gate.sh
#
# Guards the DIFF-SCOPED harness-layout baseline-registration drift gate
# (task #5300). Companion to tests/infra/test_harness_kloc_cap.sh (task 5265,
# the WHOLE-TREE anti-re-accretion kLOC-cap guard) and structured like
# tests/infra/test_infra_classification_manifest_gate.sh (task 5252, the early
# classification-manifest gate).
#
# WHY A SECOND, DIFF-SCOPED GATE. test_harness_kloc_cap.sh Section 5 is a
# WHOLE-TREE live scan: once a new standalone crates/<c>/tests/<f>.rs lands on
# main WITHOUT a matching harness-layout-baseline.manifest row (the task 4370
# drift), that scan goes RED for EVERY unrelated downstream task whose
# post-merge verify merely rebased onto the drifted tip — repeated merge thrash
# (5260/5266/5288), each re-diagnosing the same root cause. A whole-tree guard
# cannot tell "your diff introduced the drift" from "you rebased onto an
# already-drifted main". This gate closes the gap AT ITS SOURCE: it considers
# ONLY files ADDED in THIS diff, so a rebaser (whose diff adds no test file)
# stays GREEN while the offending diff (adds file, omits row) is blocked.
#
# WHAT IS TESTED (built up across the task's TDD steps):
#   - the shared lib tests/infra/harness-layout-lib.sh: the 5 consolidatable
#     crates, 7 override stems, the in-scope-standalone predicate, and the
#     baseline-membership predicate (this file, step-1);
#   - the gate scripts/check-harness-baseline-registration.sh in args/stdin
#     input mode (step-3) and --from-git self-derivation mode (step-5);
#   - verify.sh plan-shape: the gate is emitted early under RUN_RUST=1 (step-7).
#
# Hermetic: pure bash + filesystem (+ throwaway `git init` temp repos for the
# --from-git cases); never runs cargo/npm; never mutates the real baseline or
# the real manifest. Auto-discovered by tests/infra/run_all.sh via the
# test_*.sh glob; classified `pool` in run-all-classification.manifest.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# The shared lib + the gate script under test. Deliberately NOT sourced at the
# top level: this file is RED (lib absent) before the impl step, and a top-level
# `source` of a missing file would abort under `set -e` before any assert runs.
# Every lib interaction below therefore happens inside a `bash -c 'source ...'`
# child shell, so a missing lib fails just THAT assert (mirrors
# test_infra_classification_manifest_gate.sh section (a)).
LIB="$SCRIPT_DIR/harness-layout-lib.sh"
GATE="$REPO_ROOT/scripts/check-harness-baseline-registration.sh"

echo "=== harness-layout baseline-registration drift gate tests (task 5300) ==="

# Single EXIT-trap over an array of fixtures (the test_harness_kloc_cap.sh
# idiom): individual `trap ... EXIT` calls replace one another, so one handler
# over an array removes every fixture regardless of which section adds the last.
_TMPDIRS=()
trap '[ "${#_TMPDIRS[@]}" -gt 0 ] && rm -rf "${_TMPDIRS[@]}"' EXIT

# ===========================================================================
# Section A: the shared lib exists and is sourceable.
# ===========================================================================
echo ""
echo "--- Section A: harness-layout-lib.sh exists and is sourceable ---"

assert "A: tests/infra/harness-layout-lib.sh exists" \
    test -f "$LIB"
assert "A: harness-layout-lib.sh sources cleanly (rc 0)" \
    bash -c 'source "$1"' _ "$LIB"

# ===========================================================================
# Section B: harness_layout_consolidatable_crates prints EXACTLY the 5 crates.
# ===========================================================================
echo ""
echo "--- Section B: consolidatable crates (exactly the 5) ---"

# Expected set, sorted+comma-joined for an order-independent EXACT comparison.
_B_EXPECT="reify-cli,reify-compiler,reify-eval,reify-kernel-occt,reify-syntax,"

assert "B: consolidatable crates set is EXACTLY the 5" \
    bash -c '
        source "$1"
        got="$(harness_layout_consolidatable_crates | sort | tr "\n" ",")"
        [ "$got" = "$2" ]
    ' _ "$LIB" "$_B_EXPECT"
assert "B: consolidatable crates count is exactly 5" \
    bash -c 'source "$1"; [ "$(harness_layout_consolidatable_crates | grep -c .)" -eq 5 ]' _ "$LIB"

# ===========================================================================
# Section C: harness_layout_override_stems prints EXACTLY the 7 override stems.
# ===========================================================================
echo ""
echo "--- Section C: override stems (exactly the 7) ---"

_C_EXPECT="analytical_validation,buckling_smoke,determinism,fea_diagnostics_e2e,modal_benchmarks,representation_within_assertion,tensegrity_t0a,"

assert "C: override stems set is EXACTLY the 7" \
    bash -c '
        source "$1"
        got="$(harness_layout_override_stems | sort | tr "\n" ",")"
        [ "$got" = "$2" ]
    ' _ "$LIB" "$_C_EXPECT"
assert "C: override stems count is exactly 7" \
    bash -c 'source "$1"; [ "$(harness_layout_override_stems | grep -c .)" -eq 7 ]' _ "$LIB"

# ===========================================================================
# Section D: harness_layout_in_scope_standalone predicate.
# In-scope iff crates/<one-of-5>/tests/<base>.rs, top-level, base not harness_*
# and stem not an override.
# ===========================================================================
echo ""
echo "--- Section D: in-scope-standalone predicate ---"

assert "D: a plain standalone in a consolidatable crate is IN scope (rc 0)" \
    bash -c 'source "$1"; harness_layout_in_scope_standalone "crates/reify-eval/tests/foo.rs"' _ "$LIB"
assert "D: a harness_*.rs unit is NOT in scope (sanctioned harness)" \
    bash -c '! { source "$1"; harness_layout_in_scope_standalone "crates/reify-eval/tests/harness_x.rs"; }' _ "$LIB"
assert "D: an override stem (tensegrity_t0a) is NOT in scope" \
    bash -c '! { source "$1"; harness_layout_in_scope_standalone "crates/reify-eval/tests/tensegrity_t0a.rs"; }' _ "$LIB"
assert "D: a non-consolidatable crate (reify-solver-elastic) is NOT in scope" \
    bash -c '! { source "$1"; harness_layout_in_scope_standalone "crates/reify-solver-elastic/tests/foo.rs"; }' _ "$LIB"
assert "D: a NESTED (non-top-level) tests path is NOT in scope" \
    bash -c '! { source "$1"; harness_layout_in_scope_standalone "crates/reify-eval/tests/sub/foo.rs"; }' _ "$LIB"
assert "D: a non-tests path (src/) is NOT in scope" \
    bash -c '! { source "$1"; harness_layout_in_scope_standalone "crates/reify-eval/src/foo.rs"; }' _ "$LIB"

# ===========================================================================
# Section E: harness_layout_baseline_contains predicate (comment/blank stripped
# exact-line membership against a fixture baseline).
# ===========================================================================
echo ""
echo "--- Section E: baseline-membership predicate ---"

_E_BASELINE="$(mktemp)"; _TMPDIRS+=("$_E_BASELINE")
{
    echo "# a comment line — must be ignored"
    echo "#crates/reify-eval/tests/commented.rs"   # a commented-OUT registration row
    echo ""
    echo "crates/reify-eval/tests/listed.rs"
    echo "crates/reify-cli/tests/other.rs"
} > "$_E_BASELINE"

assert "E: a listed path is a member (rc 0)" \
    bash -c 'source "$1"; harness_layout_baseline_contains "crates/reify-eval/tests/listed.rs" "$2"' _ "$LIB" "$_E_BASELINE"
assert "E: an unlisted path is NOT a member (rc non-zero)" \
    bash -c '! { source "$1"; harness_layout_baseline_contains "crates/reify-eval/tests/missing.rs" "$2"; }' _ "$LIB" "$_E_BASELINE"
# Comment stripping (grep -vE '^[[:space:]]*#') must be PROVEN, not assumed.
# Because membership is an exact full-line match (grep -xF), an UNPREFIXED query
# can never equal a '# …'-prefixed line whether or not stripping runs — so a
# query like "a comment line …" would pass even if the stripping stage were
# deleted, testing nothing. To actually exercise stripping, query the FULL
# comment line INCLUDING its leading '# ': stripping ON removes that line, so the
# exact-line match finds nothing -> non-member (this assertion PASSES); stripping
# OFF would leave the line for grep -xF to match verbatim -> member (this
# assertion would then FAIL, catching the regression).
assert "E: a full comment line (including its leading #) is NOT a member — proves comment stripping" \
    bash -c '! { source "$1"; harness_layout_baseline_contains "# a comment line — must be ignored" "$2"; }' _ "$LIB" "$_E_BASELINE"
# A commented-OUT registration row (a real footgun: a row is disabled with a
# leading '#' but expected to still count) must NOT register its path as a
# member — the '#'-prefixed line is stripped before the exact-line match.
assert "E: a commented-out registration row does NOT make its path a member" \
    bash -c '! { source "$1"; harness_layout_baseline_contains "crates/reify-eval/tests/commented.rs" "$2"; }' _ "$LIB" "$_E_BASELINE"

# ===========================================================================
# Section F: the gate script exists and is executable.
# ===========================================================================
echo ""
echo "--- Section F: check-harness-baseline-registration.sh exists + executable ---"

assert "F: scripts/check-harness-baseline-registration.sh exists" \
    test -f "$GATE"
assert "F: scripts/check-harness-baseline-registration.sh is executable" \
    test -x "$GATE"

# ---------------------------------------------------------------------------
# Shared input-mode fixture tree: a tmpdir whose repo-relative layout lets the
# gate's on-disk existence check resolve candidate paths (run with CWD=tree).
# ---------------------------------------------------------------------------
_G_ROOT="$(mktemp -d)"; _TMPDIRS+=("$_G_ROOT")
mkdir -p "$_G_ROOT/crates/reify-eval/tests"
mkdir -p "$_G_ROOT/crates/reify-solver-elastic/tests"
: > "$_G_ROOT/crates/reify-eval/tests/newthing.rs"          # in-scope, added
: > "$_G_ROOT/crates/reify-eval/tests/harness_foo.rs"       # harness_* -> ignored
: > "$_G_ROOT/crates/reify-eval/tests/tensegrity_t0a.rs"    # override   -> ignored
: > "$_G_ROOT/crates/reify-solver-elastic/tests/foo.rs"     # non-consolidatable -> ignored
# crates/reify-eval/tests/ghost.rs is deliberately NEVER created -> ignored (non-existent).

# Baseline WITHOUT newthing (unregistered -> the gate must FIRE on it).
_G_BASELINE_MISSING="$(mktemp)"; _TMPDIRS+=("$_G_BASELINE_MISSING")
printf 'crates/reify-eval/tests/some_existing_other.rs\n' > "$_G_BASELINE_MISSING"
# Baseline WITH newthing (registered in the same diff -> the gate must PASS).
_G_BASELINE_PRESENT="$(mktemp)"; _TMPDIRS+=("$_G_BASELINE_PRESENT")
printf 'crates/reify-eval/tests/newthing.rs\n' > "$_G_BASELINE_PRESENT"

# ===========================================================================
# Section G: an ADDED in-scope standalone NOT in the baseline FIRES (rc 1),
# naming it; the harness_*/override/non-consolidatable/non-existent candidates
# in the same run are each IGNORED (no FAIL).
# ===========================================================================
echo ""
echo "--- Section G: unregistered in-scope standalone fires; others ignored ---"

_G_OUT="$(mktemp)"; _TMPDIRS+=("$_G_OUT")
_G_RC=0
( cd "$_G_ROOT" && REIFY_HARNESS_LAYOUT_BASELINE="$_G_BASELINE_MISSING" bash "$GATE" \
    crates/reify-eval/tests/newthing.rs \
    crates/reify-eval/tests/harness_foo.rs \
    crates/reify-eval/tests/tensegrity_t0a.rs \
    crates/reify-solver-elastic/tests/foo.rs \
    crates/reify-eval/tests/ghost.rs </dev/null ) > "$_G_OUT" 2>/dev/null || _G_RC=$?

assert "G: gate exits 1 when an unregistered in-scope standalone is added" \
    test "$_G_RC" -eq 1
assert "G: FAIL line names the offending crate/file with reason=unregistered-standalone" \
    grep -Eq '^HARNESS_BASELINE_REG FAIL crate=reify-eval file=.*newthing\.rs reason=unregistered-standalone' "$_G_OUT"
assert "G: EXACTLY one FAIL line (harness_*/override/non-consolidatable/non-existent are ignored)" \
    bash -c '[ "$(grep -cE "^HARNESS_BASELINE_REG FAIL " "$1")" -eq 1 ]' _ "$_G_OUT"
assert "G: no ignored candidate leaks into a FAIL line" \
    bash -c '! grep -E "^HARNESS_BASELINE_REG FAIL " "$1" | grep -qE "harness_foo|tensegrity_t0a|reify-solver-elastic|ghost"' _ "$_G_OUT"

# ===========================================================================
# Section H: the SAME added path WITH a matching baseline row PASSES (rc 0).
# ===========================================================================
echo ""
echo "--- Section H: same standalone WITH its baseline row passes (rc 0) ---"

_H_OUT="$(mktemp)"; _TMPDIRS+=("$_H_OUT")
_H_RC=0
( cd "$_G_ROOT" && REIFY_HARNESS_LAYOUT_BASELINE="$_G_BASELINE_PRESENT" bash "$GATE" \
    crates/reify-eval/tests/newthing.rs </dev/null ) > "$_H_OUT" 2>/dev/null || _H_RC=$?

assert "H: gate exits 0 when the added standalone is registered in the baseline" \
    test "$_H_RC" -eq 0
assert "H: a clean run emits a structured PASS line" \
    grep -Eq '^HARNESS_BASELINE_REG PASS' "$_H_OUT"
assert "H: a clean run emits no FAIL line" \
    bash -c '! grep -qE "^HARNESS_BASELINE_REG FAIL" "$1"' _ "$_H_OUT"

# ===========================================================================
# Section I: rule-(c)-style structured verdict grammar — a machine-parseable
# SUMMARY line, and EVERY emitted non-blank line matches the canonical grammar.
# ===========================================================================
echo ""
echo "--- Section I: structured SUMMARY + canonical grammar (all lines) ---"

assert "I: Section G emits a structured SUMMARY added=5 violations=1 line" \
    grep -Eq '^HARNESS_BASELINE_REG SUMMARY added=5 violations=1$' "$_G_OUT"
assert "I: Section H emits a structured SUMMARY added=1 violations=0 line" \
    grep -Eq '^HARNESS_BASELINE_REG SUMMARY added=1 violations=0$' "$_H_OUT"

# Every non-blank emitted line (from BOTH captures) matches the grammar.
_I_BAD="$( { cat "$_G_OUT" "$_H_OUT"; } | grep -vE '^[[:space:]]*$' | grep -vE '^HARNESS_BASELINE_REG (FAIL|PASS|SUMMARY) ' || true)"
assert "I: every emitted non-empty line matches the canonical HARNESS_BASELINE_REG grammar" \
    test -z "$_I_BAD"

# ===========================================================================
# Section J: green-on-truth — an empty candidate list exits 0 (added=0).
# ===========================================================================
echo ""
echo "--- Section J: empty candidate list is green (rc 0, added=0) ---"

_J_OUT="$(mktemp)"; _TMPDIRS+=("$_J_OUT")
_J_RC=0
( cd "$_G_ROOT" && REIFY_HARNESS_LAYOUT_BASELINE="$_G_BASELINE_MISSING" bash "$GATE" </dev/null ) \
    > "$_J_OUT" 2>/dev/null || _J_RC=$?

assert "J: gate exits 0 on an empty candidate list" \
    test "$_J_RC" -eq 0
assert "J: empty run emits SUMMARY added=0 violations=0" \
    grep -Eq '^HARNESS_BASELINE_REG SUMMARY added=0 violations=0$' "$_J_OUT"

# ===========================================================================
# --from-git self-derivation (hermetic `git init` temp-repo fixtures).
#
# Fully isolated from user/system git config (GIT_CONFIG_GLOBAL/SYSTEM=/dev/null)
# so no ambient hooksPath / signing / template can perturb the fixtures. The
# baseline the gate reads is pointed at the fixture repo's own working-tree
# manifest via REIFY_HARNESS_LAYOUT_BASELINE (after a completed merge the working
# tree equals HEAD, so it reflects exactly what the merge registered).
# ===========================================================================
_gitf() { GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null git "$@"; }

# _build_merge_fixture <dir> <register:0|1> — a repo whose HEAD is a real
# 2-parent merge that ADDS crates/reify-eval/tests/newthing.rs. register=1 also
# appends newthing's baseline row on the SAME topic branch (so the merged
# working tree registers it).
_build_merge_fixture() {
    local dir="$1" register="$2"
    _gitf init -q -b main "$dir"
    _gitf -C "$dir" config user.email t@e.x
    _gitf -C "$dir" config user.name t
    mkdir -p "$dir/tests/infra" "$dir/crates/reify-eval/tests"
    printf 'crates/reify-eval/tests/pre_existing.rs\n' > "$dir/tests/infra/harness-layout-baseline.manifest"
    : > "$dir/crates/reify-eval/tests/pre_existing.rs"
    _gitf -C "$dir" add -A
    _gitf -C "$dir" commit -q -m base
    _gitf -C "$dir" checkout -q -b topic
    : > "$dir/crates/reify-eval/tests/newthing.rs"
    if [ "$register" -eq 1 ]; then
        printf 'crates/reify-eval/tests/newthing.rs\n' >> "$dir/tests/infra/harness-layout-baseline.manifest"
    fi
    _gitf -C "$dir" add -A
    _gitf -C "$dir" commit -q -m topic
    _gitf -C "$dir" checkout -q main
    _gitf -C "$dir" merge -q --no-ff --no-edit topic
}

# _build_rename_fixture <dir> <register:0|1> — a repo whose HEAD is a real
# 2-parent merge that RENAMES a pre-existing out-of-scope file (in a
# NON-consolidatable crate) INTO an in-scope standalone slot
# (crates/reify-eval/tests/renamed_in.rs) via `git mv`. The source keeps
# identical content so git's default rename detection (forced on via
# `diff.renames=true` in the fixture's own config, which the gate's bare CWD git
# reads) classifies it as R, not A — the exact edge that a plain --diff-filter=A
# would miss and the gate's --no-renames decomposition must still catch.
# register=1 also appends the destination's baseline row on the same branch.
_build_rename_fixture() {
    local dir="$1" register="$2"
    _gitf init -q -b main "$dir"
    _gitf -C "$dir" config user.email t@e.x
    _gitf -C "$dir" config user.name t
    # Force rename detection ON locally so this fixture reproduces the
    # diff.renames=true default regardless of the ambient environment; the gate
    # runs bare `git` with CWD=<dir>, so it reads THIS repo-local config.
    _gitf -C "$dir" config diff.renames true
    mkdir -p "$dir/tests/infra" "$dir/crates/reify-eval/tests" \
             "$dir/crates/reify-solver-elastic/tests"
    printf 'crates/reify-eval/tests/pre_existing.rs\n' > "$dir/tests/infra/harness-layout-baseline.manifest"
    : > "$dir/crates/reify-eval/tests/pre_existing.rs"
    # A standalone in a NON-consolidatable crate (out of the gate's scope, hence
    # legitimately unregistered). Distinctive non-empty content so `git mv`
    # yields a 100%-similar rename that default detection reports as R.
    printf '// movable integration test body — do not consolidate\nfn t() {}\n' \
        > "$dir/crates/reify-solver-elastic/tests/movable.rs"
    _gitf -C "$dir" add -A
    _gitf -C "$dir" commit -q -m base
    _gitf -C "$dir" checkout -q -b topic
    # Rename INTO a consolidatable crate's top-level tests/ — now an in-scope
    # standalone whose canonical path is unregistered unless the row is added.
    _gitf -C "$dir" mv crates/reify-solver-elastic/tests/movable.rs \
                       crates/reify-eval/tests/renamed_in.rs
    if [ "$register" -eq 1 ]; then
        printf 'crates/reify-eval/tests/renamed_in.rs\n' >> "$dir/tests/infra/harness-layout-baseline.manifest"
    fi
    _gitf -C "$dir" add -A
    _gitf -C "$dir" commit -q -m topic
    _gitf -C "$dir" checkout -q main
    _gitf -C "$dir" merge -q --no-ff --no-edit topic
}

# _run_from_git <dir> — run the gate in --from-git mode with CWD=<dir>, its
# baseline pointed at the fixture's own working-tree manifest. Echoes the exit
# code; writes structured output to the file named by $2.
_run_from_git() {
    local dir="$1" out="$2" rc=0
    ( cd "$dir" \
        && REIFY_HARNESS_LAYOUT_BASELINE="$dir/tests/infra/harness-layout-baseline.manifest" \
           bash "$GATE" --from-git </dev/null ) > "$out" 2>/dev/null || rc=$?
    echo "$rc"
}

# ---------------------------------------------------------------------------
# Section K: a 2-parent merge that ADDS an unregistered standalone -> --from-git
# fires (rc 1) and names the file.
# ---------------------------------------------------------------------------
echo ""
echo "--- Section K: --from-git fires on a merge adding an unregistered standalone ---"

_K_DIR="$(mktemp -d)"; _TMPDIRS+=("$_K_DIR")
_build_merge_fixture "$_K_DIR" 0
_K_OUT="$(mktemp)"; _TMPDIRS+=("$_K_OUT")
_K_RC="$(_run_from_git "$_K_DIR" "$_K_OUT")"

assert "K: --from-git exits 1 on a merge that adds an unregistered standalone" \
    test "$_K_RC" -eq 1
assert "K: --from-git names the offending added file (newthing.rs)" \
    grep -Eq '^HARNESS_BASELINE_REG FAIL crate=reify-eval file=.*newthing\.rs reason=unregistered-standalone' "$_K_OUT"

# ---------------------------------------------------------------------------
# Section L: a sibling merge that adds the file AND its baseline row -> green.
# ---------------------------------------------------------------------------
echo ""
echo "--- Section L: --from-git green when the same merge also registers the row ---"

_L_DIR="$(mktemp -d)"; _TMPDIRS+=("$_L_DIR")
_build_merge_fixture "$_L_DIR" 1
_L_OUT="$(mktemp)"; _TMPDIRS+=("$_L_OUT")
_L_RC="$(_run_from_git "$_L_DIR" "$_L_OUT")"

assert "L: --from-git exits 0 when the merge adds the file AND its baseline row" \
    test "$_L_RC" -eq 0
assert "L: the registered run emits no FAIL line" \
    bash -c '! grep -qE "^HARNESS_BASELINE_REG FAIL" "$1"' _ "$_L_OUT"

# ---------------------------------------------------------------------------
# Section M: fail-open — a LINEAR repo with no `main` (unresolvable base, no
# MERGE_HEAD) exits 0 even though it contains an unregistered added file.
# ---------------------------------------------------------------------------
echo ""
echo "--- Section M: --from-git fail-open on a linear/unresolvable-base repo ---"

_M_DIR="$(mktemp -d)"; _TMPDIRS+=("$_M_DIR")
_gitf init -q -b feature "$_M_DIR"
_gitf -C "$_M_DIR" config user.email t@e.x
_gitf -C "$_M_DIR" config user.name t
mkdir -p "$_M_DIR/tests/infra" "$_M_DIR/crates/reify-eval/tests"
printf 'crates/reify-eval/tests/pre_existing.rs\n' > "$_M_DIR/tests/infra/harness-layout-baseline.manifest"
: > "$_M_DIR/crates/reify-eval/tests/pre_existing.rs"
_gitf -C "$_M_DIR" add -A; _gitf -C "$_M_DIR" commit -q -m base
: > "$_M_DIR/crates/reify-eval/tests/newthing.rs"   # unregistered, but base is unresolvable (no main)
_gitf -C "$_M_DIR" add -A; _gitf -C "$_M_DIR" commit -q -m add-newthing
_M_OUT="$(mktemp)"; _TMPDIRS+=("$_M_OUT")
_M_RC="$(_run_from_git "$_M_DIR" "$_M_OUT")"

assert "M: --from-git exits 0 (fail-open) when the merge base is unresolvable" \
    test "$_M_RC" -eq 0
assert "M: fail-open run emits no FAIL line (never a false RED)" \
    bash -c '! grep -qE "^HARNESS_BASELINE_REG FAIL" "$1"' _ "$_M_OUT"

# ---------------------------------------------------------------------------
# Section N: --from-git in the REAL repo root exits 0 (the current tree adds no
# unregistered standalone — green on truth).
# ---------------------------------------------------------------------------
echo ""
echo "--- Section N: --from-git in the real repo root is green ---"

_N_OUT="$(mktemp)"; _TMPDIRS+=("$_N_OUT")
_N_RC=0
( cd "$REPO_ROOT" && bash "$GATE" --from-git </dev/null ) > "$_N_OUT" 2>/dev/null || _N_RC=$?

assert "N: --from-git exits 0 in the real repo (no unregistered added standalone)" \
    test "$_N_RC" -eq 0

# ---------------------------------------------------------------------------
# Section N2: a RENAME into an in-scope standalone slot. Git's default rename
# detection reports it as R (not A), so a plain --diff-filter=A would miss it;
# the gate's --no-renames decomposition (add+delete, delete dropped by
# --diff-filter=A) must still surface the destination as an add and FIRE.
# ---------------------------------------------------------------------------
echo ""
echo "--- Section N2: --from-git fires on a rename INTO an in-scope standalone slot ---"

_P_DIR="$(mktemp -d)"; _TMPDIRS+=("$_P_DIR")
_build_rename_fixture "$_P_DIR" 0
_P_OUT="$(mktemp)"; _TMPDIRS+=("$_P_OUT")
_P_RC="$(_run_from_git "$_P_DIR" "$_P_OUT")"

assert "N2: --from-git exits 1 on a rename into an unregistered in-scope standalone" \
    test "$_P_RC" -eq 1
assert "N2: --from-git names the rename destination (renamed_in.rs)" \
    grep -Eq '^HARNESS_BASELINE_REG FAIL crate=reify-eval file=.*renamed_in\.rs reason=unregistered-standalone' "$_P_OUT"

# The sibling merge that ALSO registers the destination row in the same diff -> green.
_P2_DIR="$(mktemp -d)"; _TMPDIRS+=("$_P2_DIR")
_build_rename_fixture "$_P2_DIR" 1
_P2_OUT="$(mktemp)"; _TMPDIRS+=("$_P2_OUT")
_P2_RC="$(_run_from_git "$_P2_DIR" "$_P2_OUT")"

assert "N2: --from-git exits 0 when the rename destination is registered in the same merge" \
    test "$_P2_RC" -eq 0
assert "N2: the registered-rename run emits no FAIL line" \
    bash -c '! grep -qE "^HARNESS_BASELINE_REG FAIL" "$1"' _ "$_P2_OUT"

# ===========================================================================
# Section O: plan-shape — verify.sh emits the gate EARLY under RUN_RUST=1, among
# the cheap fail-fast poles before check-manifold-deps.sh / psi-gate / run_all.sh.
# Hermetic --print-plan oracle + `grep -n` relative-index technique (mirrors
# test_infra_classification_manifest_gate.sh section (e); never runs cargo/npm).
#
# Two plan captures: the role=task `all --scope all` plan carries the psi-gate
# and check-manifold-deps.sh poles but NOT run_all.sh (the wholesale pool suite
# moved to the merge tier), while the merge-tier plan carries run_all.sh. Both
# tiers run at RUN_RUST=1, where the gate is emitted. The RUN_RUST=0 negative
# (docs-only / gui-src-only plans must NOT emit the line) is delegated to
# test_verify_scope.sh's existing zero-command-leaf assertions, preserved by the
# RUN_RUST=1 gating.
# ===========================================================================
echo ""
echo "--- Section O: verify.sh emits the gate early under RUN_RUST=1 (plan-shape) ---"

O_GATE_LINE='./scripts/check-harness-baseline-registration.sh --from-git'
O_TASK_ALL_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan | grep -v '^#')"
O_MERGE_ALL_PLAN="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan | grep -v '^#')"

assert "O: task all plan contains the gate line ($O_GATE_LINE)" \
    bash -c 'printf "%s\n" "$2" | grep -qF -- "$1"' _ "$O_GATE_LINE" "$O_TASK_ALL_PLAN"
assert "O: merge all plan contains the gate line ($O_GATE_LINE)" \
    bash -c 'printf "%s\n" "$2" | grep -qF -- "$1"' _ "$O_GATE_LINE" "$O_MERGE_ALL_PLAN"

# task all plan: gate index < check-manifold-deps.sh index (before the deps preflight).
assert "O: task all plan — gate index < check-manifold-deps.sh index (emitted early)" \
    bash -c '
        G=$(printf "%s\n" "$2" | grep -nF -- "$1" | head -1 | cut -d: -f1)
        M=$(printf "%s\n" "$2" | grep -n "check-manifold-deps\.sh" | head -1 | cut -d: -f1)
        [ -n "$G" ] && [ -n "$M" ] && [ "$G" -lt "$M" ]
    ' _ "$O_GATE_LINE" "$O_TASK_ALL_PLAN"

# task all plan: gate index < psi-gate index (before the expensive rust pole).
assert "O: task all plan — gate index < psi-gate index (before the expensive pole)" \
    bash -c '
        G=$(printf "%s\n" "$2" | grep -nF -- "$1" | head -1 | cut -d: -f1)
        P=$(printf "%s\n" "$2" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$G" ] && [ -n "$P" ] && [ "$G" -lt "$P" ]
    ' _ "$O_GATE_LINE" "$O_TASK_ALL_PLAN"

# merge all plan: gate index < run_all.sh index (before the wholesale pool suite).
assert "O: merge all plan — gate index < run_all.sh index (before the pool suite)" \
    bash -c '
        G=$(printf "%s\n" "$2" | grep -nF -- "$1" | head -1 | cut -d: -f1)
        R=$(printf "%s\n" "$2" | grep -n "run_all\.sh" | head -1 | cut -d: -f1)
        [ -n "$G" ] && [ -n "$R" ] && [ "$G" -lt "$R" ]
    ' _ "$O_GATE_LINE" "$O_MERGE_ALL_PLAN"

test_summary
