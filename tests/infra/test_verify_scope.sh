#!/usr/bin/env bash
# Infrastructure test for task 3766.
# Drift catcher for scripts/verify.sh's --scope staged picker.
#
# verify.sh's scope decision reads only `git diff --cached` + the declared
# OCCT-touching crate list (scripts/occt-touching-crates.txt) — it does NOT
# need a real cargo workspace. So each scenario runs in an isolated throwaway
# git repo containing just the three scripts, and we drive it with
# `--print-plan` (which runs nothing). This keeps the test hermetic and, in
# particular, never mutates the real repository index.
#
# Assertions encode the scope contract:
#   docs/md/yaml only      -> nothing heavy (RUN_RUST=0 RUN_GUI=0)
#   gui/src (frontend TS)  -> GUI only, no cargo (RUN_RUST=0 RUN_GUI=1)
#   non-OCCT crate         -> Rust+GUI, NO gated pass (RUN_OCCT_GATE=0)
#   OCCT-touching crate    -> gated + ungated (RUN_OCCT_GATE=1)
#   gui/src-tauri          -> Rust+GUI, OCCT-clean (RUN_OCCT_GATE=0)
#   Cargo.lock / unknown   -> conservative gate (RUN_OCCT_GATE=1)
#   MERGE_HEAD present     -> forces --scope all regardless of stage

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== verify.sh scope picker tests ==="

# make_fixture VARNAME — create an isolated git repo with the three scripts
# verify.sh needs, writing its path to the named variable.
make_fixture() {
    local _var="$1" dir
    dir="$(mktemp -d)"
    _TMPDIRS+=("$dir")
    mkdir -p "$dir/scripts"
    cp "$REPO_ROOT/scripts/verify.sh" "$dir/scripts/verify.sh"
    cp "$REPO_ROOT/scripts/occt-scope-lib.sh" "$dir/scripts/occt-scope-lib.sh"
    cp "$REPO_ROOT/scripts/occt-touching-crates.txt" "$dir/scripts/occt-touching-crates.txt"
    cp "$REPO_ROOT/scripts/release-scope-lib.sh" "$dir/scripts/release-scope-lib.sh"
    cp "$REPO_ROOT/scripts/release-sensitive-crates.txt" "$dir/scripts/release-sensitive-crates.txt"
    chmod +x "$dir/scripts/verify.sh"
    git -C "$dir" init -q
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    printf -v "$_var" '%s' "$dir"
}

FIX=""
make_fixture FIX

# plan_for <scope> <file...> — stage the given (new) files in the fixture, emit
# the verify.sh plan for `all --scope <scope> --include-infra`, then unstage and
# delete them. Output is captured into the global PLAN_OUT.
PLAN_OUT=""
plan_for() {
    local scope="$1"; shift
    local f
    for f in "$@"; do
        mkdir -p "$FIX/$(dirname "$f")"
        printf 'x\n' > "$FIX/$f"
        git -C "$FIX" add "$f"
    done
    PLAN_OUT="$(cd "$FIX" && bash scripts/verify.sh all --profile debug --scope "$scope" --include-infra --print-plan)"
    git -C "$FIX" reset -q -- . 2>/dev/null || true
    for f in "$@"; do rm -f "$FIX/$f"; done
}

# Convenience predicates over PLAN_OUT.
plan_has()    { printf '%s\n' "$PLAN_OUT" | grep -qE "$1"; }
plan_lacks()  { ! printf '%s\n' "$PLAN_OUT" | grep -qE "$1"; }
plan_cmdcount() { printf '%s\n' "$PLAN_OUT" | grep -cE '^[^#]'; }

# ---------------------------------------------------------------------------
# Scenario 1: docs/markdown/yaml only -> nothing heavy
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 1: docs/*.md + *.yaml only -> no Rust, no GUI ---"
plan_for staged docs/note.md config/thing.yaml
assert "docs/yaml-only: scope decision RUN_RUST=0 RUN_GUI=0 RUN_OCCT_GATE=0" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=0 RUN_GUI=0 RUN_OCCT_GATE=0"' _ "$PLAN_OUT"
assert "docs/yaml-only: zero command leaves (preamble only)" \
    test "$(plan_cmdcount)" -eq 0

# ---------------------------------------------------------------------------
# Scenario 2: gui/src frontend TS -> GUI only, no cargo
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 2: gui/src/*.ts -> GUI only (Rust skipped) ---"
plan_for staged gui/src/editor/foo.ts
assert "gui/src: scope decision RUN_RUST=0 RUN_GUI=1 RUN_OCCT_GATE=0" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=0 RUN_GUI=1 RUN_OCCT_GATE=0"' _ "$PLAN_OUT"
assert "gui/src: GUI npm block present" plan_has 'cd gui &&'
assert "gui/src: no cargo clippy" plan_lacks 'cargo clippy'
assert "gui/src: no cargo test/nextest pass" plan_lacks 'cargo (test|nextest run) --workspace'
assert "gui/src: no gated OCCT pass" plan_lacks 'cargo-test-occt-gated\.sh'
assert "gui/src: no tree-sitter generate (Rust prereq)" plan_lacks 'tree-sitter-generate'

# ---------------------------------------------------------------------------
# Scenario 3: non-OCCT crate -> Rust+GUI, ungated tail only (no gated pass)
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 3: crates/reify-doc (non-OCCT) -> ungated tail, NO gated ---"
plan_for staged crates/reify-doc/src/lib.rs
assert "reify-doc: scope decision RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=0" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=0"' _ "$PLAN_OUT"
assert "reify-doc: clippy present" plan_has 'cargo clippy --workspace'
assert "reify-doc: ungated workspace tail present" plan_has 'cargo (test|nextest run) --workspace --exclude'
assert "reify-doc: gated OCCT pass ABSENT" plan_lacks 'cargo-test-occt-gated\.sh'

# ---------------------------------------------------------------------------
# Scenario 4: OCCT-touching crate -> gated + ungated
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 4: crates/reify-eval (OCCT-touching) -> gated + ungated ---"
plan_for staged crates/reify-eval/src/cache.rs
assert "reify-eval: scope decision RUN_OCCT_GATE=1" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1"' _ "$PLAN_OUT"
assert "reify-eval: gated OCCT pass present" plan_has 'cargo-test-occt-gated\.sh.*cargo test -p reify-kernel-occt'
assert "reify-eval: ungated tail present" plan_has 'cargo (test|nextest run) --workspace --exclude'

# ---------------------------------------------------------------------------
# Scenario 5: gui/src-tauri (Rust crate, OCCT-clean) -> Rust+GUI, no gate
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 5: gui/src-tauri -> Rust+GUI, OCCT-clean (no gated pass) ---"
plan_for staged gui/src-tauri/src/main.rs
assert "gui/src-tauri: scope decision RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=0" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=0"' _ "$PLAN_OUT"
assert "gui/src-tauri: clippy present" plan_has 'cargo clippy --workspace'
assert "gui/src-tauri: gated OCCT pass ABSENT" plan_lacks 'cargo-test-occt-gated\.sh'

# ---------------------------------------------------------------------------
# Scenario 6: workspace-global (Cargo.lock) -> conservative gate
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 6: Cargo.lock -> workspace-global, gated ---"
plan_for staged Cargo.lock
assert "Cargo.lock: scope decision RUN_OCCT_GATE=1" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1"' _ "$PLAN_OUT"
assert "Cargo.lock: gated OCCT pass present" plan_has 'cargo-test-occt-gated\.sh'

# ---------------------------------------------------------------------------
# Scenario 7: unrecognised path -> conservative rust+gui+gate
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 7: unknown top-level path -> conservative gate ---"
plan_for staged some_unrecognised_file
assert "unknown path: scope decision RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1"' _ "$PLAN_OUT"

# ---------------------------------------------------------------------------
# Scenario 8: MERGE_HEAD forces --scope all even with a docs-only stage
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 8: MERGE_HEAD present forces --scope all ---"
# Stage a docs-only change (would normally be RUN_RUST=0) but plant a MERGE_HEAD.
printf 'x\n' > "$FIX/docs/note.md" 2>/dev/null || { mkdir -p "$FIX/docs"; printf 'x\n' > "$FIX/docs/note.md"; }
git -C "$FIX" add docs/note.md
: > "$FIX/.git/MERGE_HEAD"
MERGE_OUT="$(cd "$FIX" && bash scripts/verify.sh all --profile debug --scope staged --include-infra --print-plan)"
rm -f "$FIX/.git/MERGE_HEAD"
git -C "$FIX" reset -q -- . 2>/dev/null || true
rm -f "$FIX/docs/note.md"
assert "MERGE_HEAD: docs-only stage is overridden to full scope (RUN_RUST=1 RUN_OCCT_GATE=1)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1"' _ "$MERGE_OUT"
assert "MERGE_HEAD: scope reported as all in plan header" \
    bash -c 'printf "%s\n" "$1" | grep -q "scope=all"' _ "$MERGE_OUT"

# ---------------------------------------------------------------------------
# Scenario 9: --print-plan is a pure dry run (exit 0, no index mutation)
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 9: --print-plan leaves the index untouched ---"
_before="$(git -C "$FIX" status --porcelain)"
(cd "$FIX" && bash scripts/verify.sh test --scope all --print-plan >/dev/null)
_after="$(git -C "$FIX" status --porcelain)"
assert "print-plan does not mutate the working tree/index" \
    test "$_before" = "$_after"

# ===========================================================================
# Branch-scope scenarios (--scope branch): verify.sh derives changed files
# from `git diff --name-only "$merge_base"` instead of `git diff --cached`.
# ===========================================================================

# make_branch_fixture VARNAME — like make_fixture but also commits the
# scripts on a branch named exactly `main` so `git merge-base main HEAD`
# can resolve inside the fixture.
make_branch_fixture() {
    local _var="$1" dir
    dir="$(mktemp -d)"
    _TMPDIRS+=("$dir")
    mkdir -p "$dir/scripts"
    cp "$REPO_ROOT/scripts/verify.sh" "$dir/scripts/verify.sh"
    cp "$REPO_ROOT/scripts/occt-scope-lib.sh" "$dir/scripts/occt-scope-lib.sh"
    cp "$REPO_ROOT/scripts/occt-touching-crates.txt" "$dir/scripts/occt-touching-crates.txt"
    cp "$REPO_ROOT/scripts/release-scope-lib.sh" "$dir/scripts/release-scope-lib.sh"
    cp "$REPO_ROOT/scripts/release-sensitive-crates.txt" "$dir/scripts/release-sensitive-crates.txt"
    chmod +x "$dir/scripts/verify.sh"
    git -C "$dir" init -q
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    git -C "$dir" add scripts
    git -C "$dir" commit -q -m "base"
    git -C "$dir" branch -M main
    printf -v "$_var" '%s' "$dir"
}

FIX_B=""
make_branch_fixture FIX_B

# plan_for_branch <file...> — checkout a task-branch off main, commit the
# given (new) files, capture verify.sh --scope branch --print-plan output
# into PLAN_OUT, then restore main and delete the branch.
plan_for_branch() {
    local f
    git -C "$FIX_B" checkout -q -b task-branch
    for f in "$@"; do
        mkdir -p "$FIX_B/$(dirname "$f")"
        printf 'x\n' > "$FIX_B/$f"
        git -C "$FIX_B" add "$f"
    done
    git -C "$FIX_B" commit -q -m "task changes"
    PLAN_OUT="$(cd "$FIX_B" && bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan 2>/dev/null)" || true
    git -C "$FIX_B" checkout -q main
    git -C "$FIX_B" branch -q -D task-branch
    for f in "$@"; do rm -f "$FIX_B/$f"; done
}

# ---------------------------------------------------------------------------
# Scenario B1: docs-only branch -> nothing heavy (empty plan)
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario B1: docs-only branch -> no Rust, no GUI, empty plan ---"
plan_for_branch docs/note.md
assert "branch/docs: scope decision RUN_RUST=0 RUN_GUI=0 RUN_OCCT_GATE=0" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=0 RUN_GUI=0 RUN_OCCT_GATE=0"' _ "$PLAN_OUT"
assert "branch/docs: scope=branch reported in plan header" \
    bash -c 'printf "%s\n" "$1" | grep -q "scope=branch"' _ "$PLAN_OUT"
assert "branch/docs: zero command leaves (empty plan)" \
    test "$(plan_cmdcount)" -eq 0

# ---------------------------------------------------------------------------
# Scenario B2: non-OCCT crate branch -> ungated Rust tail, no gated pass
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario B2: crates/reify-doc (non-OCCT) branch -> ungated tail, NO gated ---"
plan_for_branch crates/reify-doc/src/lib.rs
assert "branch/reify-doc: scope decision RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=0" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=0"' _ "$PLAN_OUT"
assert "branch/reify-doc: clippy present" plan_has 'cargo clippy --workspace'
assert "branch/reify-doc: ungated workspace tail present" plan_has 'cargo (test|nextest run) --workspace --exclude'
assert "branch/reify-doc: gated OCCT pass ABSENT" plan_lacks 'cargo-test-occt-gated\.sh'

# ---------------------------------------------------------------------------
# Scenario B3: gui/src frontend-only branch -> GUI only, no cargo
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario B3: gui/src/*.ts branch -> GUI only (Rust skipped) ---"
plan_for_branch gui/src/editor/foo.ts
assert "branch/gui/src: scope decision RUN_RUST=0 RUN_GUI=1 RUN_OCCT_GATE=0" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=0 RUN_GUI=1 RUN_OCCT_GATE=0"' _ "$PLAN_OUT"
assert "branch/gui/src: GUI npm block present" plan_has 'cd gui &&'
assert "branch/gui/src: no cargo clippy" plan_lacks 'cargo clippy'
assert "branch/gui/src: no cargo test/nextest pass" plan_lacks 'cargo (test|nextest run) --workspace'
assert "branch/gui/src: no gated OCCT pass" plan_lacks 'cargo-test-occt-gated\.sh'
assert "branch/gui/src: no tree-sitter generate" plan_lacks 'tree-sitter-generate'

# ---------------------------------------------------------------------------
# Scenario B5: staged-but-uncommitted working-tree change -> still classified
# ---------------------------------------------------------------------------
# plan_for_branch always commits changes, so the branch scenarios above only
# exercise the committed path of `git diff $MERGE_BASE`. This scenario
# exercises the uncommitted (staged) path: a file `git add`-ed but not yet
# committed is still visible to `git diff $MERGE_BASE` because that command
# compares the merge-base commit to the WORKING TREE (not to HEAD), and a
# staged file exists on disk with its new content.
echo ""
echo "--- Scenario B5: staged-but-not-committed change -> --scope branch still classifies it ---"
FIX_B5=""
make_branch_fixture FIX_B5
git -C "$FIX_B5" checkout -q -b task-branch
mkdir -p "$FIX_B5/crates/reify-doc/src"
printf 'x\n' > "$FIX_B5/crates/reify-doc/src/lib.rs"
git -C "$FIX_B5" add crates/reify-doc/src/lib.rs
# Intentionally NOT committed — the staged file is on disk, so
# `git diff "$_MERGE_BASE"` sees it via the working-tree comparison.
PLAN_B5="$(cd "$FIX_B5" && bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan 2>/dev/null)" || true
# FIX_B5 left dirty; cleaned up by the EXIT trap via _TMPDIRS.
assert "B5/staged-uncommitted: RUN_RUST=1 (staged file classified by --scope branch)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1"' _ "$PLAN_B5"
assert "B5/staged-uncommitted: gated OCCT pass ABSENT (reify-doc is non-OCCT)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo-test-occt-gated"' _ "$PLAN_B5"

# ---------------------------------------------------------------------------
# Scenario B6: OCCT-touching crate branch -> gated pass present
# ---------------------------------------------------------------------------
# The staged-scope suite covers the OCCT->gate path (Scenarios 4/5/6/9).
# This scenario verifies that --scope branch also correctly sets RUN_OCCT_GATE=1
# and includes cargo-test-occt-gated.sh when a declared OCCT crate is changed —
# i.e. the fail-wide paths (B4/C5) are not the only way to reach RUN_OCCT_GATE=1
# under branch scope.
echo ""
echo "--- Scenario B6: crates/reify-eval (OCCT-touching) branch -> gated pass present ---"
plan_for_branch crates/reify-eval/src/lib.rs
assert "branch/reify-eval: scope decision RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1"' _ "$PLAN_OUT"
assert "branch/reify-eval: gated OCCT pass present" plan_has 'cargo-test-occt-gated\.sh'
assert "branch/reify-eval: clippy present" plan_has 'cargo clippy --workspace'

# ---------------------------------------------------------------------------
# Scenario B4: MERGE_HEAD present + --scope branch -> forces scope=all
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario B4: MERGE_HEAD + --scope branch -> forced scope=all ---"
# Use a branch fixture, commit a task change, plant MERGE_HEAD, run branch scope.
FIX_B4=""
make_branch_fixture FIX_B4
git -C "$FIX_B4" checkout -q -b task-branch
mkdir -p "$FIX_B4/crates/reify-doc/src"
printf 'x\n' > "$FIX_B4/crates/reify-doc/src/lib.rs"
git -C "$FIX_B4" add crates
git -C "$FIX_B4" commit -q -m "task changes"
: > "$FIX_B4/.git/MERGE_HEAD"
PLAN_B4="$(cd "$FIX_B4" && bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan 2>/dev/null)" || true
rm -f "$FIX_B4/.git/MERGE_HEAD"
git -C "$FIX_B4" checkout -q main
git -C "$FIX_B4" branch -q -D task-branch
assert "B4/MERGE_HEAD+branch: scope=all in plan header (merge forces full scope)" \
    bash -c 'printf "%s\n" "$1" | grep -q "scope=all"' _ "$PLAN_B4"
assert "B4/MERGE_HEAD+branch: full scope (RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1"' _ "$PLAN_B4"

# ---------------------------------------------------------------------------
# Scenario C5: no local main ref -> fail-wide to scope=all (contract C5)
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario C5: no local main ref -> fail-wide to scope=all ---"
FIX_C5=""
make_branch_fixture FIX_C5
git -C "$FIX_C5" branch -m main work  # rename main so no local 'main' ref exists
PLAN_C5="$(cd "$FIX_C5" && bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan 2>/dev/null)" || true
assert "C5/no-main: scope=all in plan header (fail-wide, contract C5)" \
    bash -c 'printf "%s\n" "$1" | grep -q "scope=all"' _ "$PLAN_C5"
assert "C5/no-main: full scope forced (RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1"' _ "$PLAN_C5"

test_summary
