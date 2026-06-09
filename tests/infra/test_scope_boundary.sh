#!/usr/bin/env bash
# Infrastructure test for task 4061 (PRD §7 T5 — B+H integration gate).
#
# Proves the verify-scope contract in two directions:
#   B4 (contract C3): per-task narrowed verification still catches downstream
#       breakage.  PRD-cited example edge: X=reify-ir → Y=reify-eval (§4/§7).
#   B5 (contract C1): the merge gate (--scope all) never narrows; it always
#       issues --workspace clippy/check regardless of any override in the env.
#
# PRAGMATIC FORM USED (documented per task instruction — strong form rejected):
#   The strong form of B4 would introduce a real breaking change in reify-ir
#   and compile it through reify-eval's narrowed verification, requiring a
#   full cargo+OCCT compile (minutes).  That is incompatible with the hermetic
#   tests/infra/ suite that runs in seconds under run_all.sh.
#
#   Instead B4 is proved via a two-part composition:
#     Part 1 — call the real affected_crates() against the live workspace to
#              prove reify-eval ∈ closure(reify-ir) and the set is bounded
#              (not the ALL sentinel).  This is the C3 ground-truth.
#     Part 2 — feed that REAL computed set as REIFY_AFFECTED_CRATES_OVERRIDE
#              into a hermetic verify.sh --print-plan (--scope branch, mirroring
#              the orchestrator per-task path) and assert -p reify-eval appears
#              in the test (gated), clippy, and cargo-check passes.
#   The override is required because the hermetic fixture has no cargo workspace
#   (cargo metadata fails → affected_crates() returns ALL); replaying the REAL
#   Part-1 output keeps the proof grounded in the live graph, distinct from the
#   synthetic-override scenarios in test_verify_scope.sh.
#
# REUSE:
#   make_branch_fixture  — ported from tests/infra/test_verify_scope.sh
#   assert / test_summary — tests/infra/test_helpers.sh
#   affected_crates()    — scripts/affected-crates-lib.sh (dep 4058)
#   REIFY_AFFECTED_CRATES_OVERRIDE + --print-plan — scripts/verify.sh (dep 4060)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

source "$REPO_ROOT/scripts/affected-crates-lib.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== B+H integration gate: boundary narrowing + merge-gate C1/C3 ==="

# make_branch_fixture VARNAME — create an isolated git repo with the scripts
# verify.sh needs, with a base commit on `main`, writing its path to the
# named variable.  Ported from tests/infra/test_verify_scope.sh.
make_branch_fixture() {
    local _var="$1" dir
    dir="$(mktemp -d)"
    _TMPDIRS+=("$dir")
    mkdir -p "$dir/scripts"
    cp "$REPO_ROOT/scripts/verify.sh"                    "$dir/scripts/verify.sh"
    cp "$REPO_ROOT/scripts/occt-scope-lib.sh"            "$dir/scripts/occt-scope-lib.sh"
    cp "$REPO_ROOT/scripts/occt-touching-crates.txt"     "$dir/scripts/occt-touching-crates.txt"
    cp "$REPO_ROOT/scripts/release-scope-lib.sh"         "$dir/scripts/release-scope-lib.sh"
    cp "$REPO_ROOT/scripts/release-sensitive-crates.txt" "$dir/scripts/release-sensitive-crates.txt"
    cp "$REPO_ROOT/scripts/affected-crates-lib.sh"       "$dir/scripts/affected-crates-lib.sh"
    chmod +x "$dir/scripts/verify.sh"
    git -C "$dir" init -q
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    git -C "$dir" add scripts
    git -C "$dir" commit -q -m "base"
    git -C "$dir" branch -M main
    printf -v "$_var" '%s' "$dir"
}

# PRD §4/§7 cited example edge:
#   X=reify-ir (low-level crate)
#   Y=reify-eval (downstream OCCT crate that depends on reify-ir)
# reify-eval depends on reify-ir (reify-ir.workspace=true in its Cargo.toml)
# and is an OCCT crate — its -p exercises the affected∩OCCT gated-pass split,
# which is the most error-prone narrowing path under C3.
X_CRATE="reify-ir"
X_FILE="crates/reify-ir/src/lib.rs"
Y_CRATE="reify-eval"

# plan_has <plan_str> <pattern>  — true if plan_str has a line matching pattern.
# plan_lacks <plan_str> <pattern> — true if plan_str has NO line matching pattern.
plan_has()   { printf '%s\n' "$1" | grep -qE "$2"; }
plan_lacks() { ! printf '%s\n' "$1" | grep -qE "$2"; }

# ---------------------------------------------------------------------------
# B4 Part 1: real reverse-closure includes downstream dependent Y (C3 ground-truth)
# ---------------------------------------------------------------------------
# Calls the real affected_crates() against the live workspace to prove
# reify-eval ∈ closure(reify-ir) and the set is bounded (≠ ALL sentinel).
echo ""
echo "--- B4 Part 1: real reverse closure includes dependent Y ---"

AFFECTED_SET="$(cd "$REPO_ROOT" && affected_crates "$X_FILE")"

assert "B4P1: affected set is NOT the ALL sentinel (genuinely narrowed)" \
    bash -c '[ "$1" != "ALL" ]' _ "$AFFECTED_SET"
assert "B4P1: affected set is non-empty" \
    bash -c '[ -n "$1" ]' _ "$AFFECTED_SET"
assert "B4P1: changed crate X ($X_CRATE) is in affected set" \
    bash -c 'grep -qx "$2" <<< "$1"' _ "$AFFECTED_SET" "$X_CRATE"
assert "B4P1: downstream dependent Y ($Y_CRATE) is in affected set (C3)" \
    bash -c 'grep -qx "$2" <<< "$1"' _ "$AFFECTED_SET" "$Y_CRATE"

test_summary

# ---------------------------------------------------------------------------
# B4 Part 2: the affected set -> -p wiring across test+clippy+check passes
# ---------------------------------------------------------------------------
# Exercises the REIFY_AFFECTED_CRATES_OVERRIDE -> NARROW_ACTIVE=1 -> flag-
# emission path hermetically.  The override replays the REAL Part-1 set into
# a fixture that has no cargo workspace (where affected_crates() would return
# ALL), keeping the proof grounded in the live graph.
#
# reify-eval is OCCT, so its -p lands in the gated pass (AFFECTED_OCCT_FLAGS);
# non-OCCT dependents (reify-ir, reify-compiler, …) land in the ungated tail
# (AFFECTED_UNGATED_FLAGS); clippy/cargo-check get the full set (AFFECTED_ALL).
echo ""
echo "--- B4 Part 2: narrowed per-task plan carries -p Y across test+clippy+check ---"

PLAN_B4_ALL=""
PLAN_B4_TC=""

assert "B4P2: NARROW_ACTIVE=1 in narrowed plan header" \
    plan_has "$PLAN_B4_ALL" 'NARROW_ACTIVE=1'
assert "B4P2/all: clippy carries -p $Y_CRATE" \
    plan_has "$PLAN_B4_ALL" "cargo clippy.*-p $Y_CRATE"
assert "B4P2/all: clippy lacks --workspace (narrowed)" \
    plan_lacks "$PLAN_B4_ALL" 'cargo clippy --workspace'
assert "B4P2/all: gated OCCT test pass carries -p $Y_CRATE (reify-eval is OCCT)" \
    plan_has "$PLAN_B4_ALL" "cargo-test-occt-gated\\.sh.*cargo test .*-p $Y_CRATE"
assert "B4P2/all: ungated test tail lacks --workspace (narrowed to affected set)" \
    plan_lacks "$PLAN_B4_ALL" 'cargo (test|nextest run) --workspace'
assert "B4P2/typecheck: cargo check carries -p $Y_CRATE" \
    plan_has "$PLAN_B4_TC" "cargo check .*-p $Y_CRATE"
assert "B4P2/typecheck: cargo check lacks --workspace (narrowed)" \
    plan_lacks "$PLAN_B4_TC" 'cargo check --workspace'
