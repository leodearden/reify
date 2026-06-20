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

[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"

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
    cp "$REPO_ROOT/scripts/affected-crates-lib.sh" "$dir/scripts/affected-crates-lib.sh"
    cp "$REPO_ROOT/scripts/lib_test_semaphore.sh" "$dir/scripts/lib_test_semaphore.sh"
    cp "$REPO_ROOT/scripts/cpu-admit.sh" "$dir/scripts/cpu-admit.sh"
    cp "$REPO_ROOT/scripts/gen-nextest-config.sh" "$dir/scripts/gen-nextest-config.sh"
    cp "$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt" "$dir/scripts/verify-pipeline-infra-tests.txt"
    mkdir -p "$dir/.config"
    cp "$REPO_ROOT/.config/nextest.toml" "$dir/.config/nextest.toml"
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
#
# Uses capture_print_plan for retry-on-incomplete-capture defense-in-depth
# (task #4708): up to REIFY_PLAN_CAPTURE_RETRIES attempts (default 3) until
# plan_capture_complete certifies both structural markers are present.
# Calls with `|| true` so exhaustion surfaces as a failed assertion rather
# than aborting the suite via set -euo pipefail.
PLAN_OUT=""
plan_for() {
    local scope="$1"; shift
    local f
    for f in "$@"; do
        mkdir -p "$FIX/$(dirname "$f")"
        printf 'x\n' > "$FIX/$f"
        git -C "$FIX" add "$f"
    done
    capture_print_plan PLAN_OUT "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
        bash -c 'cd "$1" && exec bash scripts/verify.sh all --profile debug --scope "$2" --include-infra --print-plan' \
        _ "$FIX" "$scope" || true
    git -C "$FIX" reset -q -- . 2>/dev/null || true
    for f in "$@"; do rm -f "$FIX/$f"; done
}

# Convenience predicates over PLAN_OUT.
# Fork-free: delegate to plan_match ([[ =~ ]]) — eliminates the pipe-to-grep
# EINTR surface that caused B9-default spurious failures under load (task #4708,
# esc-4574-42).
plan_has()    { plan_match "$PLAN_OUT" "$1"; }
plan_lacks()  { ! plan_match "$PLAN_OUT" "$1"; }
plan_cmdcount() { plan_count_noncomment_lines "$PLAN_OUT"; }

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
assert "reify-doc: nextest workspace pass present (no --exclude, OCCT folded in, task 4451)" plan_has 'cargo nextest run --workspace'
assert "reify-doc: nextest workspace pass has NO --exclude (task 4451: OCCT in pool)" plan_lacks 'cargo nextest run --workspace.*--exclude'
assert "reify-doc: gated OCCT pass ABSENT" plan_lacks 'cargo-test-occt-gated\.sh'

# ---------------------------------------------------------------------------
# Scenario 4: OCCT-touching crate -> gated + ungated
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 4: crates/reify-eval (OCCT-touching) -> gated + ungated ---"
plan_for staged crates/reify-eval/src/cache.rs
assert "reify-eval: scope decision RUN_OCCT_GATE=1" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1"' _ "$PLAN_OUT"
assert "reify-eval: no gated OCCT pass (task 4451: OCCT folded into nextest pool)" plan_lacks 'cargo-test-occt-gated\.sh'
assert "reify-eval: nextest workspace pass present (OCCT folded in, task 4451)" plan_has 'cargo nextest run --workspace'
assert "reify-eval: nextest workspace pass has NO --exclude (task 4451)" plan_lacks 'cargo nextest run --workspace.*--exclude'

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
assert "Cargo.lock: no gated OCCT pass (task 4451: OCCT folded into nextest pool)" plan_lacks 'cargo-test-occt-gated\.sh'
assert "Cargo.lock: nextest workspace pass present with no --exclude (task 4451)" plan_has 'cargo nextest run --workspace'
assert "Cargo.lock: nextest workspace pass has NO --exclude (OCCT folded in)" plan_lacks 'cargo nextest run --workspace.*--exclude'

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
    cp "$REPO_ROOT/scripts/affected-crates-lib.sh" "$dir/scripts/affected-crates-lib.sh"
    cp "$REPO_ROOT/scripts/lib_test_semaphore.sh" "$dir/scripts/lib_test_semaphore.sh"
    cp "$REPO_ROOT/scripts/cpu-admit.sh" "$dir/scripts/cpu-admit.sh"
    cp "$REPO_ROOT/scripts/gen-nextest-config.sh" "$dir/scripts/gen-nextest-config.sh"
    cp "$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt" "$dir/scripts/verify-pipeline-infra-tests.txt"
    mkdir -p "$dir/.config"
    cp "$REPO_ROOT/.config/nextest.toml" "$dir/.config/nextest.toml"
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
assert "branch/reify-doc: nextest workspace pass present (no --exclude, task 4451)" plan_has 'cargo nextest run --workspace'
assert "branch/reify-doc: nextest workspace pass has NO --exclude (OCCT folded in, task 4451)" plan_lacks 'cargo nextest run --workspace.*--exclude'
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
assert "branch/reify-eval: no gated OCCT pass (task 4451: OCCT folded into nextest pool)" plan_lacks 'cargo-test-occt-gated\.sh'
assert "branch/reify-eval: nextest workspace pass present (OCCT in pool, task 4451)" plan_has 'cargo nextest run --workspace'
assert "branch/reify-eval: nextest workspace pass has NO --exclude (task 4451)" plan_lacks 'cargo nextest run --workspace.*--exclude'
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

# ===========================================================================
# Branch-scope narrowing scenarios: REIFY_AFFECTED_CRATES_OVERRIDE drives
# -p flag wiring for clippy, nextest tail, and cargo check (step-2/step-4).
# RED until step-2 lands (verify.sh has no narrowing yet).
# ===========================================================================

# plan_for_branch_narrowed <override> <file...>
# Like plan_for_branch but exports REIFY_AFFECTED_CRATES_OVERRIDE, and also
# captures the typecheck-action plan into PLAN_OUT_NARROW_TC.
PLAN_OUT_NARROW_TC=""
plan_for_branch_narrowed() {
    local _override="$1"; shift
    local f
    git -C "$FIX_B" checkout -q -b task-branch
    for f in "$@"; do
        mkdir -p "$FIX_B/$(dirname "$f")"
        printf 'x\n' > "$FIX_B/$f"
        git -C "$FIX_B" add "$f"
    done
    git -C "$FIX_B" commit -q -m "task changes"
    PLAN_OUT="$(cd "$FIX_B" && REIFY_AFFECTED_CRATES_OVERRIDE="$_override" bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan 2>/dev/null)" || true
    PLAN_OUT_NARROW_TC="$(cd "$FIX_B" && REIFY_AFFECTED_CRATES_OVERRIDE="$_override" bash scripts/verify.sh typecheck --profile debug --scope branch --print-plan 2>/dev/null)" || true
    git -C "$FIX_B" checkout -q main
    git -C "$FIX_B" branch -q -D task-branch
    for f in "$@"; do rm -f "$FIX_B/$f"; done
}

# ---------------------------------------------------------------------------
# Scenario B2-narrow: non-OCCT branch + override -> -p flags, no --workspace
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario B2-narrow: branch + override -> narrowed -p flags ---"
plan_for_branch_narrowed "reify-doc reify-ir" crates/reify-doc/src/lib.rs
assert "B2-narrow: PLAN_OUT non-empty (verify.sh exited OK)" \
    bash -c '[ -n "$1" ]' _ "$PLAN_OUT"
assert "B2-narrow: PLAN_OUT_NARROW_TC non-empty (typecheck plan exited OK)" \
    bash -c '[ -n "$1" ]' _ "$PLAN_OUT_NARROW_TC"
# action=all: clippy must use -p flags, NOT --workspace
assert "B2-narrow/all: clippy has -p reify-doc" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo clippy.*-p reify-doc"' _ "$PLAN_OUT"
assert "B2-narrow/all: clippy has -p reify-ir" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo clippy.*-p reify-ir"' _ "$PLAN_OUT"
assert "B2-narrow/all: clippy LACKS --workspace" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo clippy --workspace"' _ "$PLAN_OUT"
# action=all: nextest tail must use -p flag, NOT --workspace
assert "B2-narrow/all: nextest tail has -p reify-doc" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo (test|nextest run) .*-p reify-doc"' _ "$PLAN_OUT"
assert "B2-narrow/all: nextest tail LACKS --workspace" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo (test|nextest run) --workspace"' _ "$PLAN_OUT"
# action=typecheck: cargo check must use -p flag, NOT --workspace
assert "B2-narrow/typecheck: cargo check has -p reify-doc" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo check .*-p reify-doc"' _ "$PLAN_OUT_NARROW_TC"
assert "B2-narrow/typecheck: cargo check LACKS --workspace" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo check --workspace"' _ "$PLAN_OUT_NARROW_TC"

# ---------------------------------------------------------------------------
# Scenario Intersect/C3: non-OCCT change but OCCT in override -> gated fires
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario Intersect/C3: non-OCCT change, OCCT in override -> gated pass fires ---"
# changed=crates/reify-doc/src/lib.rs -> RUN_OCCT_GATE=0 from scope decision.
# But override includes reify-eval (OCCT) -> affected ∩ OCCT non-empty -> gate runs.
plan_for_branch_narrowed "reify-doc reify-eval" crates/reify-doc/src/lib.rs
assert "Intersect/C3: PLAN_OUT non-empty (verify.sh exited OK)" \
    bash -c '[ -n "$1" ]' _ "$PLAN_OUT"
assert "Intersect/C3: RUN_OCCT_GATE=0 from changed file (reify-doc is non-OCCT)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_OCCT_GATE=0"' _ "$PLAN_OUT"
assert "Intersect/C3: no cargo-test-occt-gated.sh in plan (task 4451: OCCT folded into nextest pool)" \
    bash -c '! printf "%s\n" "$1" | grep -q "cargo-test-occt-gated\.sh"' _ "$PLAN_OUT"
assert "Intersect/C3: nextest pass has -p reify-eval (OCCT crate in narrowed nextest pass, task 4451)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo nextest run .*-p reify-eval"' _ "$PLAN_OUT"
assert "Intersect/C3: nextest pass LACKS reify-kernel-occt (only affected ∩ OCCT, not full OCCT set)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo nextest run .*-p reify-kernel-occt"' _ "$PLAN_OUT"
assert "Intersect/C3: nextest tail has -p reify-doc" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo nextest run .*-p reify-doc"' _ "$PLAN_OUT"
assert "Intersect/C3: nextest tail LACKS --workspace (narrowed to affected set)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo nextest run --workspace"' _ "$PLAN_OUT"

# ---------------------------------------------------------------------------
# Scenario C1-guard: scope=all + override -> --workspace preserved, override ignored
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario C1-guard: scope=all + override -> --workspace kept (C1 contract) ---"
FIX_C1=""
make_branch_fixture FIX_C1
git -C "$FIX_C1" checkout -q -b task-branch
mkdir -p "$FIX_C1/crates/reify-doc/src"
printf 'x\n' > "$FIX_C1/crates/reify-doc/src/lib.rs"
git -C "$FIX_C1" add crates
git -C "$FIX_C1" commit -q -m "task changes"
PLAN_C1="$(cd "$FIX_C1" && REIFY_AFFECTED_CRATES_OVERRIDE="reify-doc reify-ir" bash scripts/verify.sh all --profile debug --scope all --include-infra --print-plan 2>/dev/null)" || true
git -C "$FIX_C1" checkout -q main
git -C "$FIX_C1" branch -q -D task-branch
assert "C1-guard: PLAN_C1 non-empty (verify.sh exited OK)" \
    bash -c '[ -n "$1" ]' _ "$PLAN_C1"
assert "C1-guard: clippy keeps --workspace (override ignored for scope=all)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo clippy --workspace"' _ "$PLAN_C1"
assert "C1-guard: ungated tail keeps --workspace (override ignored for scope=all)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo (test|nextest run) --workspace"' _ "$PLAN_C1"
assert "C1-guard: NO affected -p reify-doc in clippy (override ignored for scope=all)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo clippy.*-p reify-doc"' _ "$PLAN_C1"

# ===========================================================================
# B7 and B9 scenarios (step-3):
#   B7: branch, Cargo.lock changed -> affected_crates returns ALL (C4 path-match)
#       -> no narrowing, --workspace kept. GREEN now, stays green (regression guard).
#   B9-default: staged without --narrow -> --workspace preserved (current behavior).
#       GREEN now, stays green (regression guard).
#   B9-narrowed: staged + --narrow + override -> narrowed -p flags.
#       RED until step-4 adds --narrow parsing to verify.sh.
# ===========================================================================

# plan_for_staged_narrowed <override> <file...>
# Like plan_for but exports REIFY_AFFECTED_CRATES_OVERRIDE and passes --narrow.
# Captures PLAN_OUT (all action) and PLAN_OUT_NARROW_TC (typecheck action).
plan_for_staged_narrowed() {
    local _override="$1"; shift
    local f
    for f in "$@"; do
        mkdir -p "$FIX/$(dirname "$f")"
        printf 'x\n' > "$FIX/$f"
        git -C "$FIX" add "$f"
    done
    PLAN_OUT="$(cd "$FIX" && REIFY_AFFECTED_CRATES_OVERRIDE="$_override" bash scripts/verify.sh all --profile debug --scope staged --narrow --include-infra --print-plan 2>/dev/null)" || true
    PLAN_OUT_NARROW_TC="$(cd "$FIX" && REIFY_AFFECTED_CRATES_OVERRIDE="$_override" bash scripts/verify.sh typecheck --profile debug --scope staged --narrow --print-plan 2>/dev/null)" || true
    git -C "$FIX" reset -q -- . 2>/dev/null || true
    for f in "$@"; do rm -f "$FIX/$f"; done
}

# ---------------------------------------------------------------------------
# Scenario B7: branch, Cargo.lock -> ALL fallback, --workspace preserved
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario B7: branch Cargo.lock -> affected=ALL, --workspace preserved ---"
plan_for_branch Cargo.lock
assert "B7/Cargo.lock: PLAN_OUT non-empty (verify.sh exited OK)" \
    bash -c '[ -n "$1" ]' _ "$PLAN_OUT"
assert "B7/Cargo.lock: clippy keeps --workspace (affected=ALL, no narrowing)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo clippy --workspace"' _ "$PLAN_OUT"
assert "B7/Cargo.lock: ungated tail keeps --workspace (affected=ALL, no narrowing)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo (test|nextest run) --workspace"' _ "$PLAN_OUT"
assert "B7/Cargo.lock: NO affected -p narrowing (affected=ALL, no narrowing)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo clippy.*-p reify-"' _ "$PLAN_OUT"

# ---------------------------------------------------------------------------
# Scenario B9-default: staged without --narrow -> current --workspace behavior
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario B9-default: staged without --narrow -> --workspace preserved ---"
plan_for staged crates/reify-doc/src/lib.rs
assert "B9-default: NARROW_ACTIVE=0 (coupling invariant — clippy & nextest --workspace both derive from this single global)" \
    test "$(plan_narrow_active "$PLAN_OUT")" = "0"
assert "B9-default: nextest workspace pass keeps --workspace (staged, no --narrow flag, task 4451)" \
    plan_has 'cargo nextest run --workspace'
assert "B9-default: nextest workspace pass has NO --exclude (OCCT folded in, task 4451)" \
    plan_lacks 'cargo nextest run --workspace.*--exclude'
assert "B9-default: clippy keeps --workspace (staged, no --narrow flag)" \
    plan_has 'cargo clippy --workspace'

# ---------------------------------------------------------------------------
# Scenario B9-narrowed: staged + --narrow + override -> narrowed -p flags
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario B9-narrowed: staged --narrow + override -> narrowed -p flags (RED: --narrow not yet parsed) ---"
plan_for_staged_narrowed "reify-doc reify-ir" crates/reify-doc/src/lib.rs
assert "B9-narrowed: PLAN_OUT non-empty (verify.sh exited OK)" \
    bash -c '[ -n "$1" ]' _ "$PLAN_OUT"
assert "B9-narrowed: PLAN_OUT_NARROW_TC non-empty (typecheck plan exited OK)" \
    bash -c '[ -n "$1" ]' _ "$PLAN_OUT_NARROW_TC"
assert "B9-narrowed/all: clippy has -p reify-doc" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo clippy.*-p reify-doc"' _ "$PLAN_OUT"
assert "B9-narrowed/all: clippy LACKS --workspace" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo clippy --workspace"' _ "$PLAN_OUT"
assert "B9-narrowed/all: nextest tail has -p reify-doc" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo (test|nextest run) .*-p reify-doc"' _ "$PLAN_OUT"
assert "B9-narrowed/all: nextest tail LACKS --workspace" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo (test|nextest run) --workspace"' _ "$PLAN_OUT"
assert "B9-narrowed/typecheck: cargo check has -p reify-doc" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo check .*-p reify-doc"' _ "$PLAN_OUT_NARROW_TC"
assert "B9-narrowed/typecheck: cargo check LACKS --workspace" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo check --workspace"' _ "$PLAN_OUT_NARROW_TC"

# ===========================================================================
# Merge-gate contract guard (T2 / PRD §4 B5+B6, contract C2)
#
# MG-* labels follow PRD §4 table labels; they are DISTINCT from the file's
# pre-existing local B4/B5/B6 labels (which cover unrelated branch-scope
# cases and already diverge from the PRD §4 table labels).
# ===========================================================================
echo ""
echo "=== Merge-gate contract guard (T2 / contract C2) ==="

# ---------------------------------------------------------------------------
# Scenario MG-B6a: role=merge defeats active narrowing (RED until step-2 impl)
# ---------------------------------------------------------------------------
# Fixture: resolvable local main, no MERGE_HEAD — the ONLY path to scope=all
# is the new role-guard. REIFY_AFFECTED_CRATES_OVERRIDE is set to prove the
# guard defeats active branch-diff narrowing (the strongest form of the
# invariant: would-be-narrowed plan is forced back to full --workspace).
echo ""
echo "--- Scenario MG-B6a: role=merge + --scope branch + override -> scope=all forced, narrowing defeated (RED until guard) ---"
FIX_MG_B6A=""
make_branch_fixture FIX_MG_B6A
git -C "$FIX_MG_B6A" checkout -q -b task-branch
mkdir -p "$FIX_MG_B6A/crates/reify-doc/src"
printf 'x\n' > "$FIX_MG_B6A/crates/reify-doc/src/lib.rs"
git -C "$FIX_MG_B6A" add crates
git -C "$FIX_MG_B6A" commit -q -m "task changes"
PLAN_MG_B6A="$(cd "$FIX_MG_B6A" && DF_VERIFY_ROLE=merge REIFY_AFFECTED_CRATES_OVERRIDE="reify-doc reify-ir" bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan 2>/dev/null)" || true
git -C "$FIX_MG_B6A" checkout -q main
git -C "$FIX_MG_B6A" branch -q -D task-branch
assert "MG-B6a: scope=all in plan header (role=merge forces full scope)" \
    bash -c 'printf "%s\n" "$1" | grep -q "scope=all"' _ "$PLAN_MG_B6A"
assert "MG-B6a: RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1 (forced scope=all -> full workspace)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1"' _ "$PLAN_MG_B6A"
assert "MG-B6a: clippy keeps --workspace (override narrowing defeated by role-guard)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo clippy --workspace"' _ "$PLAN_MG_B6A"
assert "MG-B6a: ungated tail keeps --workspace (override narrowing defeated by role-guard)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo (test|nextest run) --workspace"' _ "$PLAN_MG_B6A"
assert "MG-B6a: NO -p reify-doc anywhere (override narrowing defeated by role-guard)" \
    bash -c '! printf "%s\n" "$1" | grep -qE " -p reify-doc"' _ "$PLAN_MG_B6A"

# ---------------------------------------------------------------------------
# Scenario MG-B6b: role=merge force is unconditional (RED until step-2 impl)
# ---------------------------------------------------------------------------
# Docs-only branch => scope=branch classifies RUN_RUST=0 RUN_GUI=0 -> empty
# plan. With the role-guard, SCOPE is forced to all before decide_scope,
# so the plan becomes the full workspace: RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1.
echo ""
echo "--- Scenario MG-B6b: role=merge + --scope branch on docs-only branch -> unconditional scope=all (RED until guard) ---"
FIX_MG_B6B=""
make_branch_fixture FIX_MG_B6B
git -C "$FIX_MG_B6B" checkout -q -b task-branch
mkdir -p "$FIX_MG_B6B/docs"
printf 'x\n' > "$FIX_MG_B6B/docs/note.md"
git -C "$FIX_MG_B6B" add docs
git -C "$FIX_MG_B6B" commit -q -m "task changes"
PLAN_MG_B6B="$(cd "$FIX_MG_B6B" && DF_VERIFY_ROLE=merge bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan 2>/dev/null)" || true
git -C "$FIX_MG_B6B" checkout -q main
git -C "$FIX_MG_B6B" branch -q -D task-branch
assert "MG-B6b: scope=all in plan header (docs-only branch forced to full scope)" \
    bash -c 'printf "%s\n" "$1" | grep -q "scope=all"' _ "$PLAN_MG_B6B"
assert "MG-B6b: RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1 (forced full scope, not empty docs plan)" \
    bash -c 'printf "%s\n" "$1" | grep -q "RUN_RUST=1 RUN_GUI=1 RUN_OCCT_GATE=1"' _ "$PLAN_MG_B6B"

# ---------------------------------------------------------------------------
# Scenario MG-B5: merge gate full; OCCT+release -p axes permitted (GREEN now)
# ---------------------------------------------------------------------------
# role=merge + --profile both + --scope all: scope=all ignores the override
# (C1 contract: no branch-diff narrowing). But --profile both LEGITIMATELY
# emits -p flags on the OCCT gated pass and the release-sensitivity pass.
# Assert: reify-doc / reify-ir (override sentinels, neither OCCT nor
# release-sensitive) never appear; POSITIVELY permit the OCCT gated -p and
# the release-sensitivity -p axes.
echo ""
echo "--- Scenario MG-B5: merge gate full (both profiles); OCCT+release -p permitted, no branch-diff narrowing (GREEN, regression guard) ---"
FIX_MG_B5=""
make_branch_fixture FIX_MG_B5
git -C "$FIX_MG_B5" checkout -q -b task-branch
mkdir -p "$FIX_MG_B5/crates/reify-doc/src"
printf 'x\n' > "$FIX_MG_B5/crates/reify-doc/src/lib.rs"
git -C "$FIX_MG_B5" add crates
git -C "$FIX_MG_B5" commit -q -m "task changes"
PLAN_MG_B5="$(cd "$FIX_MG_B5" && DF_VERIFY_ROLE=merge REIFY_AFFECTED_CRATES_OVERRIDE="reify-doc reify-ir" bash scripts/verify.sh all --profile both --scope all --include-infra --print-plan 2>/dev/null)" || true
git -C "$FIX_MG_B5" checkout -q main
git -C "$FIX_MG_B5" branch -q -D task-branch
assert "MG-B5: scope=all in plan header" \
    bash -c 'printf "%s\n" "$1" | grep -q "scope=all"' _ "$PLAN_MG_B5"
assert "MG-B5: clippy keeps --workspace (scope=all ignores override, C1 contract)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo clippy --workspace"' _ "$PLAN_MG_B5"
assert "MG-B5: nextest debug tail keeps --workspace (task 4451: OCCT folded in, no --exclude)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo nextest run --workspace"' _ "$PLAN_MG_B5"
assert "MG-B5: nextest --workspace pass has NO --exclude (task 4451: OCCT in pool)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo nextest run --workspace.*--exclude"' _ "$PLAN_MG_B5"
assert "MG-B5: NO -p reify-doc (no branch-diff narrowing in merge gate)" \
    bash -c '! printf "%s\n" "$1" | grep -qE " -p reify-doc"' _ "$PLAN_MG_B5"
assert "MG-B5: NO -p reify-ir (no branch-diff narrowing in merge gate)" \
    bash -c '! printf "%s\n" "$1" | grep -qE " -p reify-ir"' _ "$PLAN_MG_B5"
assert "MG-B5: no cargo-test-occt-gated.sh in plan (task 4451: OCCT folded into nextest pool)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo-test-occt-gated\.sh"' _ "$PLAN_MG_B5"
assert "MG-B5: release-sensitivity pass present with -p reify- (permitted axis: release scope)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo nextest run .*-p reify-.*--release"' _ "$PLAN_MG_B5"

# ---------------------------------------------------------------------------
# Scenario MG-hook: pre-merge-commit hook drift guard (GREEN now)
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario MG-hook: pre-merge-commit invokes verify.sh --scope all, not --scope branch/staged (GREEN, drift guard) ---"
assert "MG-hook: pre-merge-commit calls verify.sh with --scope all" \
    grep -qE 'verify\.sh.*--scope all' "$REPO_ROOT/hooks/pre-merge-commit"
assert "MG-hook: pre-merge-commit does NOT pass --scope branch or --scope staged" \
    bash -c '! grep -qE "verify\.sh.*--scope (branch|staged)" "$1"' _ "$REPO_ROOT/hooks/pre-merge-commit"

# ===========================================================================
# VS-* scenarios: selective infra test injection (task 4523)
#
# When a task-level verify (--scope branch, NO --include-infra) detects that a
# verify-pipeline artifact listed in scripts/verify-pipeline-infra-tests.txt
# was changed, build_plan() must emit a guarded for-loop that runs the
# artifact's infra test glob via `timeout ... bash` BEFORE cargo test poles.
#
# VS-pos   (RED until step-2): verify.sh change -> selective loop present
# VS-coverage (GREEN now):     glob covers both incident-named guards
# VS-neg   (GREEN now):        non-artifact change -> no selective loop
# ===========================================================================
echo ""
echo "=== Selective infra injection (task 4523 VS-* scenarios) ==="

# FIX_VS — branch fixture for VS-pos.  make_branch_fixture now copies the map.
FIX_VS=""
make_branch_fixture FIX_VS

# plan_for_vs_change — on a fresh task-branch, APPEND a harmless comment line
# to the fixture's scripts/verify.sh (NEVER overwrite — it is the SUT), commit,
# then capture the plan WITHOUT --include-infra (the real task-verify path).
# Restores main and deletes the branch when done.
plan_for_vs_change() {
    git -C "$FIX_VS" checkout -q -b task-branch
    echo "# task-4523 verify.sh-change simulation sentinel" >> "$FIX_VS/scripts/verify.sh"
    git -C "$FIX_VS" add scripts/verify.sh
    git -C "$FIX_VS" commit -q -m "task changes"
    PLAN_OUT="$(cd "$FIX_VS" && bash scripts/verify.sh all --profile debug --scope branch --print-plan 2>/dev/null)" || true
    git -C "$FIX_VS" checkout -q main
    git -C "$FIX_VS" branch -q -D task-branch
}

# ---------------------------------------------------------------------------
# Scenario VS-pos: verify.sh change (no --include-infra) -> selective infra loop
# RED until step-2: implementation doesn't exist yet.
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario VS-pos: scripts/verify.sh changed -> selective infra loop in plan (RED until step-2) ---"
plan_for_vs_change
assert "VS-pos: plan contains test_verify_*.sh glob literal" \
    plan_has 'tests/infra/test_verify_\*\.sh'
assert "VS-pos: plan contains timeout+bash invocation for infra tests" \
    plan_has 'test_verify.*timeout.*bash'

# ---------------------------------------------------------------------------
# Scenario VS-coverage: glob expands to include both incident-named guards
# GREEN now (pure filesystem lock; no implementation dependency).
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario VS-coverage: glob tests/infra/test_verify_*.sh covers incident guards (GREEN) ---"
assert "VS-coverage: test_verify_gui_feature_check.sh exists under glob" \
    test -f "$REPO_ROOT/tests/infra/test_verify_gui_feature_check.sh"
assert "VS-coverage: test_verify_throughput.sh exists under glob" \
    test -f "$REPO_ROOT/tests/infra/test_verify_throughput.sh"

# ---------------------------------------------------------------------------
# Scenario VS-neg: non-artifact change (no --include-infra) -> NO selective loop
# GREEN now (no implementation => no loop emitted; plan_lacks trivially passes).
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario VS-neg: crates/reify-doc change -> NO selective infra loop (no --include-infra; GREEN) ---"
FIX_VS_NEG=""
make_branch_fixture FIX_VS_NEG
git -C "$FIX_VS_NEG" checkout -q -b task-branch
mkdir -p "$FIX_VS_NEG/crates/reify-doc/src"
printf 'x\n' > "$FIX_VS_NEG/crates/reify-doc/src/lib.rs"
git -C "$FIX_VS_NEG" add crates
git -C "$FIX_VS_NEG" commit -q -m "task changes"
PLAN_VS_NEG="$(cd "$FIX_VS_NEG" && bash scripts/verify.sh all --profile debug --scope branch --print-plan 2>/dev/null)" || true
git -C "$FIX_VS_NEG" checkout -q main
git -C "$FIX_VS_NEG" branch -q -D task-branch
assert "VS-neg: plan lacks test_verify_*.sh glob (reify-doc not in artifact map)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "tests/infra/test_verify_\*\.sh"' _ "$PLAN_VS_NEG"

# ---------------------------------------------------------------------------
# Scenario VS-incl: verify.sh change WITH --include-infra -> wholesale run_all.sh
# present, selective test_verify_*.sh loop ABSENT (no double-run).
# RED until step-4: step-2 emits the selective loop regardless of INCLUDE_INFRA.
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario VS-incl: verify.sh change + --include-infra -> run_all.sh present, no selective loop (RED until step-4) ---"

# plan_for_vs_change_incl — like plan_for_vs_change but passes --include-infra.
# Reuses FIX_VS (restored to main by plan_for_vs_change above).
plan_for_vs_change_incl() {
    git -C "$FIX_VS" checkout -q -b task-branch
    echo "# task-4523 verify.sh-change simulation sentinel (incl)" >> "$FIX_VS/scripts/verify.sh"
    git -C "$FIX_VS" add scripts/verify.sh
    git -C "$FIX_VS" commit -q -m "task changes"
    PLAN_OUT="$(cd "$FIX_VS" && bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan 2>/dev/null)" || true
    git -C "$FIX_VS" checkout -q main
    git -C "$FIX_VS" branch -q -D task-branch
}

plan_for_vs_change_incl
assert "VS-incl: wholesale run_all.sh present (--include-infra fires)" \
    plan_has 'tests/infra/run_all\.sh'
assert "VS-incl: selective test_verify_*.sh loop ABSENT (no double-run under --include-infra)" \
    plan_lacks 'tests/infra/test_verify_\*\.sh'

test_summary
