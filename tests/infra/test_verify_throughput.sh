#!/usr/bin/env bash
# Infrastructure test for task 4062.
# Evidence harness for per-task verify throughput: --scope branch+narrowing vs --scope all.
#
# This test is hermetic (--print-plan only, never runs real cargo) and auto-discovered
# by tests/infra/run_all.sh and verify.sh --include-infra.
#
# What it tests:
#   1. Structural invariants (G6-safe, contract-derived — never a guessed threshold):
#      - branch_count <= all_count for every shape (narrowed plan is a subset)
#      - docs-only branch_count == 0 (B1)
#      - non-OCCT branch plan: LACKS cargo-test-occt-gated.sh, HAS -p reify-doc, LACKS --workspace (B2)
#      - gui-only branch plan: LACKS cargo, HAS cd gui && (B3)
#      - OCCT branch plan: HAS gated pass with -p reify-eval (narrowing mechanism)
#   2. Note completeness: docs/notes/verify-scope-throughput.md exists, references all 4
#      shape labels under both scopes, has >=1 wall-clock actual line and a machine/load caveat.
#
# S1 is intentionally RED until docs/notes/verify-scope-throughput.md is committed (S2).
# The structural invariants (test group 1) already hold on landed main and pass immediately;
# only the note-completeness assertions (test group 2) are RED.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== verify.sh throughput evidence harness ==="

# ---------------------------------------------------------------------------
# make_branch_fixture VARNAME — create an isolated throwaway git repo with a
# 'main' branch containing just the scripts verify.sh needs.
# Reuses the same technique as test_verify_scope.sh.
# ---------------------------------------------------------------------------
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
    chmod +x "$dir/scripts/verify.sh"
    git -C "$dir" init -q
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    git -C "$dir" add scripts
    git -C "$dir" commit -q -m "base"
    git -C "$dir" branch -M main
    printf -v "$_var" '%s' "$dir"
}

FIX=""
make_branch_fixture FIX

# Shared output variable populated by plan_for_shape / plan_for_shape_narrowed.
PLAN_ALL_OUT=""
PLAN_BR_OUT=""

# plan_for_shape <file> — commit the file on a task branch, derive plan step
# counts for scope=all and scope=branch, store plans in PLAN_ALL_OUT / PLAN_BR_OUT.
plan_for_shape() {
    local f="$1"
    git -C "$FIX" checkout -q -b task-branch
    mkdir -p "$FIX/$(dirname "$f")"
    printf 'x\n' > "$FIX/$f"
    git -C "$FIX" add "$f"
    git -C "$FIX" commit -q -m "task changes"
    PLAN_ALL_OUT="$(cd "$FIX" && bash scripts/verify.sh all --profile debug --scope all    --include-infra --print-plan 2>/dev/null)" || true
    PLAN_BR_OUT="$( cd "$FIX" && bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan 2>/dev/null)" || true
    git -C "$FIX" checkout -q main
    git -C "$FIX" branch -q -D task-branch
}

# plan_for_shape_narrowed <override> <file> — like plan_for_shape but exports
# REIFY_AFFECTED_CRATES_OVERRIDE for the hermetic narrowing counts.
plan_for_shape_narrowed() {
    local _override="$1" f="$2"
    git -C "$FIX" checkout -q -b task-branch
    mkdir -p "$FIX/$(dirname "$f")"
    printf 'x\n' > "$FIX/$f"
    git -C "$FIX" add "$f"
    git -C "$FIX" commit -q -m "task changes"
    PLAN_ALL_OUT="$(cd "$FIX" && REIFY_AFFECTED_CRATES_OVERRIDE="$_override" bash scripts/verify.sh all --profile debug --scope all    --include-infra --print-plan 2>/dev/null)" || true
    PLAN_BR_OUT="$( cd "$FIX" && REIFY_AFFECTED_CRATES_OVERRIDE="$_override" bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan 2>/dev/null)" || true
    git -C "$FIX" checkout -q main
    git -C "$FIX" branch -q -D task-branch
}

# Convenience predicates over PLAN_BR_OUT.
plan_br_has()    { printf '%s\n' "$PLAN_BR_OUT" | grep -qE "$1"; }
plan_br_lacks()  { ! printf '%s\n' "$PLAN_BR_OUT" | grep -qE "$1"; }
# plan_cmdcount counts non-comment lines in the plan output.
# grep -c exits 1 when count is 0, so we add || true to prevent set -e from
# firing on a zero-count plan (which is the expected result for docs-only branch).
plan_cmdcount()  { printf '%s\n' "$1" | grep -cE '^[^#]' || true; }

# ===========================================================================
# Test group 1: structural invariants (G6-safe, contract-derived)
# These hold immediately on landed main and should ALWAYS PASS.
# ===========================================================================

# ---------------------------------------------------------------------------
# Shape (a): docs-only — docs/note.md
# ---------------------------------------------------------------------------
echo ""
echo "--- Shape (a): docs-only (docs/note.md) ---"
plan_for_shape "docs/note.md"

COUNT_ALL_A=$(plan_cmdcount "$PLAN_ALL_OUT")
COUNT_BR_A=$(plan_cmdcount "$PLAN_BR_OUT")

assert "docs-only: all plan non-empty (sanity)" \
    test "$COUNT_ALL_A" -gt 0

assert "docs-only: branch plan is empty (B1 — docs skip all heavy work)" \
    test "$COUNT_BR_A" -eq 0

assert "docs-only: branch_count <= all_count (narrowed subset invariant)" \
    test "$COUNT_BR_A" -le "$COUNT_ALL_A"

assert "docs-only: scope=branch in plan header" \
    bash -c 'printf "%s\n" "$1" | grep -q "scope=branch"' _ "$PLAN_BR_OUT"

# ---------------------------------------------------------------------------
# Shape (b): single non-OCCT crate — crates/reify-doc/src/lib.rs
# Override: "reify-doc" (deterministic representative affected set)
# ---------------------------------------------------------------------------
echo ""
echo "--- Shape (b): non-OCCT crate (reify-doc) with override=reify-doc ---"
plan_for_shape_narrowed "reify-doc" "crates/reify-doc/src/lib.rs"

COUNT_ALL_B=$(plan_cmdcount "$PLAN_ALL_OUT")
COUNT_BR_B=$(plan_cmdcount "$PLAN_BR_OUT")

assert "reify-doc: all plan non-empty (sanity)" \
    test "$COUNT_ALL_B" -gt 0

assert "reify-doc: branch plan non-empty (Rust+GUI shape has work)" \
    test "$COUNT_BR_B" -gt 0

assert "reify-doc: branch_count <= all_count (narrowed subset invariant)" \
    test "$COUNT_BR_B" -le "$COUNT_ALL_B"

assert "reify-doc: branch plan HAS -p reify-doc (narrowed -p flags, B2)" \
    plan_br_has 'cargo.*-p reify-doc'

assert "reify-doc: branch plan LACKS --workspace (narrowing active, B2)" \
    plan_br_lacks 'cargo (clippy|test|nextest run) --workspace'

assert "reify-doc: branch plan LACKS cargo-test-occt-gated.sh (non-OCCT, B2)" \
    plan_br_lacks 'cargo-test-occt-gated\.sh'

assert "reify-doc: scope=branch + narrowing in plan header" \
    bash -c 'printf "%s\n" "$1" | grep -q "NARROW_ACTIVE=1"' _ "$PLAN_BR_OUT"

# ---------------------------------------------------------------------------
# Shape (c): OCCT-touching crate — crates/reify-eval/src/lib.rs
# Override: "reify-eval"
# ---------------------------------------------------------------------------
echo ""
echo "--- Shape (c): OCCT-touching crate (reify-eval) with override=reify-eval ---"
plan_for_shape_narrowed "reify-eval" "crates/reify-eval/src/lib.rs"

COUNT_ALL_C=$(plan_cmdcount "$PLAN_ALL_OUT")
COUNT_BR_C=$(plan_cmdcount "$PLAN_BR_OUT")

assert "reify-eval: all plan non-empty (sanity)" \
    test "$COUNT_ALL_C" -gt 0

assert "reify-eval: branch plan non-empty (OCCT shape has work)" \
    test "$COUNT_BR_C" -gt 0

assert "reify-eval: branch_count <= all_count (narrowed subset invariant)" \
    test "$COUNT_BR_C" -le "$COUNT_ALL_C"

assert "reify-eval: branch plan HAS gated pass with -p reify-eval (narrowing mechanism)" \
    plan_br_has 'cargo-test-occt-gated\.sh.*-p reify-eval'

assert "reify-eval: branch plan LACKS --workspace in narrowed commands" \
    plan_br_lacks 'cargo (clippy|test|nextest run) --workspace'

assert "reify-eval: branch plan HAS -p reify-eval in clippy (narrowed -p flags)" \
    plan_br_has 'cargo.*-p reify-eval'

assert "reify-eval: scope=branch + narrowing in plan header" \
    bash -c 'printf "%s\n" "$1" | grep -q "NARROW_ACTIVE=1"' _ "$PLAN_BR_OUT"

# ---------------------------------------------------------------------------
# Shape (d): gui-only — gui/src/editor/foo.ts
# ---------------------------------------------------------------------------
echo ""
echo "--- Shape (d): gui-only (gui/src/editor/foo.ts) ---"
plan_for_shape "gui/src/editor/foo.ts"

COUNT_ALL_D=$(plan_cmdcount "$PLAN_ALL_OUT")
COUNT_BR_D=$(plan_cmdcount "$PLAN_BR_OUT")

assert "gui-only: all plan non-empty (sanity)" \
    test "$COUNT_ALL_D" -gt 0

assert "gui-only: branch plan non-empty (GUI has npm work)" \
    test "$COUNT_BR_D" -gt 0

assert "gui-only: branch_count <= all_count (narrowed subset invariant)" \
    test "$COUNT_BR_D" -le "$COUNT_ALL_D"

assert "gui-only: branch plan HAS cd gui && (GUI npm block present, B3)" \
    plan_br_has 'cd gui &&'

assert "gui-only: branch plan LACKS cargo clippy (no Rust, B3)" \
    plan_br_lacks 'cargo clippy'

assert "gui-only: branch plan LACKS cargo test/nextest (no Rust, B3)" \
    plan_br_lacks 'cargo (test|nextest run)'

assert "gui-only: branch plan LACKS cargo-test-occt-gated.sh (no Rust, B3)" \
    plan_br_lacks 'cargo-test-occt-gated\.sh'

assert "gui-only: scope=branch in plan header" \
    bash -c 'printf "%s\n" "$1" | grep -q "scope=branch"' _ "$PLAN_BR_OUT"

# ===========================================================================
# Test group 2: note completeness (RED until S2 commits the note)
# docs/notes/verify-scope-throughput.md must exist and contain the required
# content: all 4 shape labels, both scope labels, >=1 wall-clock actual line,
# and a machine/load caveat line.
# ===========================================================================
echo ""
echo "--- Note completeness (requires docs/notes/verify-scope-throughput.md) ---"

NOTE="$REPO_ROOT/docs/notes/verify-scope-throughput.md"

assert "note exists: docs/notes/verify-scope-throughput.md" \
    test -f "$NOTE"

# Check all 4 shape labels appear under scope=all context
assert "note: references docs-only shape label" \
    bash -c '[ -f "$1" ] && grep -qi "docs-only\|docs.only\|docs/note" "$1"' _ "$NOTE"

assert "note: references reify-doc (non-OCCT) shape label" \
    bash -c '[ -f "$1" ] && grep -q "reify-doc" "$1"' _ "$NOTE"

assert "note: references reify-eval (OCCT) shape label" \
    bash -c '[ -f "$1" ] && grep -q "reify-eval" "$1"' _ "$NOTE"

assert "note: references gui-only shape label" \
    bash -c '[ -f "$1" ] && grep -qi "gui-only\|gui.only\|gui/src" "$1"' _ "$NOTE"

# Both scope labels must appear
assert "note: references scope=all" \
    bash -c '[ -f "$1" ] && grep -qE "scope=all|scope: all|\bscope-all\b|--scope all" "$1"' _ "$NOTE"

assert "note: references scope=branch" \
    bash -c '[ -f "$1" ] && grep -qE "scope=branch|scope: branch|\bscope-branch\b|--scope branch" "$1"' _ "$NOTE"

# Wall-clock actual (must have at least one line with a time measurement — e.g. "1m 23s" or "83s" or "83.4s")
assert "note: contains >=1 wall-clock actual (time measurement line)" \
    bash -c '[ -f "$1" ] && grep -qE "[0-9]+(\.[0-9]+)?[[:space:]]*(s\b|sec|seconds|m[[:space:]]|min|minutes)" "$1"' _ "$NOTE"

# Machine/load caveat (must mention host or load or machine or CPU)
assert "note: contains machine/load caveat line" \
    bash -c '[ -f "$1" ] && grep -qiE "(host|machine|cpu|load|sccache warm|warm sccache|hardware)" "$1"' _ "$NOTE"

test_summary
