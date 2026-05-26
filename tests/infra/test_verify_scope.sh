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
assert "reify-eval: gated OCCT pass present" plan_has 'cargo-test-occt-gated\.sh cargo test -p reify-kernel-occt'
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

test_summary
