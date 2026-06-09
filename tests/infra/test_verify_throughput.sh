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
#   2. Note existence: docs/notes/verify-scope-throughput.md exists (bare file check).
#      Prose-completeness assertions were removed (S5) — cosmetic rewording would break CI
#      with zero functional regression.  Group 3's numeric sync guard is the real coverage.

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
    # Preflight: fail loudly if verify.sh sources a lib that was not copied to the
    # fixture.  Without this check a new 'source "$SCRIPT_DIR/foo.sh"' line in
    # verify.sh would be silently swallowed by the 2>/dev/null on the --print-plan
    # invocations, surfacing only as an opaque all-plan-non-empty sanity failure.
    while IFS= read -r _lib; do
        [ -f "$dir/scripts/$_lib" ] || {
            echo "ERROR: make_branch_fixture: '$_lib' is source'd by verify.sh" \
                 "but was not copied to the fixture." >&2
            echo "       Fix: add cp \"\$REPO_ROOT/scripts/$_lib\" \"\$dir/scripts/$_lib\"" \
                 "in make_branch_fixture." >&2
            exit 1
        }
    done < <(grep -E 'source "\$SCRIPT_DIR/' "$dir/scripts/verify.sh" \
                 | sed 's|.*source "\$SCRIPT_DIR/\([^"]*\)".*|\1|' || true)
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

assert "reify-eval: branch plan HAS nextest pass with -p reify-eval (task 4451: OCCT folded into nextest pool)" \
    plan_br_has 'cargo nextest run.*-p reify-eval'
assert "reify-eval: branch plan has NO cargo-test-occt-gated.sh (gated pass dropped, task 4451)" \
    plan_br_lacks 'cargo-test-occt-gated\.sh'

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
# Test group 2: note existence check
# ===========================================================================
echo ""
echo "--- Note existence (docs/notes/verify-scope-throughput.md) ---"

NOTE="$REPO_ROOT/docs/notes/verify-scope-throughput.md"

assert "note exists: docs/notes/verify-scope-throughput.md" \
    test -f "$NOTE"

# ===========================================================================
# Test group 3: note↔oracle sync drift guard (RED until S4 adds sentinel block)
#
# Parses the per-shape recorded counts from a machine-parseable sentinel block
# in docs/notes/verify-scope-throughput.md, then re-derives the same counts
# live from the --print-plan oracle and asserts equality.  Modeled on the
# declared==derived idiom in tests/infra/test_occt_gated_scope.sh.
#
# The sentinel block format (added to the note in S4):
#   <!-- THROUGHPUT-COUNTS:BEGIN -->
#   | shape | all | branch |
#   |-------|-----|--------|
#   | docs-only  | 14 |  0 |
#   | reify-doc  | 14 | 13 |
#   | reify-eval | 14 | 13 |
#   | gui-only   | 14 |  3 |
#   <!-- THROUGHPUT-COUNTS:END -->
#
# RED until the note emits counts in this exact format with values matching
# the live oracle (S2 wrote counts in a human-readable table only).
# ===========================================================================
echo ""
echo "--- Note↔oracle sync drift guard (requires THROUGHPUT-COUNTS sentinel block) ---"

assert "sync: note contains THROUGHPUT-COUNTS:BEGIN sentinel" \
    bash -c '[ -f "$1" ] && grep -q "THROUGHPUT-COUNTS:BEGIN" "$1"' _ "$NOTE"

assert "sync: note contains THROUGHPUT-COUNTS:END sentinel" \
    bash -c '[ -f "$1" ] && grep -q "THROUGHPUT-COUNTS:END" "$1"' _ "$NOTE"

# note_count_for <shape-grep-key> <all|branch>
# Extracts the recorded count from the sentinel block for a shape.
# Returns empty string when the sentinel block or shape row is absent.
note_count_for() {
    local _shape="$1" _col
    case "$2" in
        all)    _col=3 ;;
        branch) _col=4 ;;
        *)      printf ''; return ;;
    esac
    awk '/THROUGHPUT-COUNTS:BEGIN/,/THROUGHPUT-COUNTS:END/' "$NOTE" \
        | grep "$_shape" \
        | awk -F'|' -v c="$_col" 'NR==1{ val=$c; gsub(/ /,"",val); print val }' || true
}

# Re-derive live counts for each shape using the same fixture and oracle as
# test group 1.  FIX is still at main after the structural tests above.

# Shape (a): docs-only
plan_for_shape "docs/note.md"
LIVE_ALL_A=$(plan_cmdcount "$PLAN_ALL_OUT")
LIVE_BR_A=$(plan_cmdcount "$PLAN_BR_OUT")
REC_ALL_A=$(note_count_for "docs-only" "all")
REC_BR_A=$(note_count_for "docs-only" "branch")

assert "sync: docs-only scope=all: note($REC_ALL_A) == live($LIVE_ALL_A)" \
    test "$REC_ALL_A" = "$LIVE_ALL_A"
assert "sync: docs-only scope=branch: note($REC_BR_A) == live($LIVE_BR_A)" \
    test "$REC_BR_A" = "$LIVE_BR_A"

# Shape (b): reify-doc (non-OCCT)
plan_for_shape_narrowed "reify-doc" "crates/reify-doc/src/lib.rs"
LIVE_ALL_B=$(plan_cmdcount "$PLAN_ALL_OUT")
LIVE_BR_B=$(plan_cmdcount "$PLAN_BR_OUT")
REC_ALL_B=$(note_count_for "reify-doc" "all")
REC_BR_B=$(note_count_for "reify-doc" "branch")

assert "sync: reify-doc scope=all: note($REC_ALL_B) == live($LIVE_ALL_B)" \
    test "$REC_ALL_B" = "$LIVE_ALL_B"
assert "sync: reify-doc scope=branch: note($REC_BR_B) == live($LIVE_BR_B)" \
    test "$REC_BR_B" = "$LIVE_BR_B"

# Shape (c): reify-eval (OCCT)
plan_for_shape_narrowed "reify-eval" "crates/reify-eval/src/lib.rs"
LIVE_ALL_C=$(plan_cmdcount "$PLAN_ALL_OUT")
LIVE_BR_C=$(plan_cmdcount "$PLAN_BR_OUT")
REC_ALL_C=$(note_count_for "reify-eval" "all")
REC_BR_C=$(note_count_for "reify-eval" "branch")

assert "sync: reify-eval scope=all: note($REC_ALL_C) == live($LIVE_ALL_C)" \
    test "$REC_ALL_C" = "$LIVE_ALL_C"
assert "sync: reify-eval scope=branch: note($REC_BR_C) == live($LIVE_BR_C)" \
    test "$REC_BR_C" = "$LIVE_BR_C"

# Shape (d): gui-only
plan_for_shape "gui/src/editor/foo.ts"
LIVE_ALL_D=$(plan_cmdcount "$PLAN_ALL_OUT")
LIVE_BR_D=$(plan_cmdcount "$PLAN_BR_OUT")
REC_ALL_D=$(note_count_for "gui-only" "all")
REC_BR_D=$(note_count_for "gui-only" "branch")

assert "sync: gui-only scope=all: note($REC_ALL_D) == live($LIVE_ALL_D)" \
    test "$REC_ALL_D" = "$LIVE_ALL_D"
assert "sync: gui-only scope=branch: note($REC_BR_D) == live($LIVE_BR_D)" \
    test "$REC_BR_D" = "$LIVE_BR_D"

test_summary
