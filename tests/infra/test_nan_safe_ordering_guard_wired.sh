#!/usr/bin/env bash
# Infrastructure test for task 5093 (INV-FEA-3 regression guard).
# Validates that scripts/verify.sh's lint plan includes a guarded, timeout-
# wrapped invocation of scripts/check-nan-safe-ordering.sh, that the script
# exists/executes and passes clean on the current tree, and — the task's
# user-observable signal — that the gate actually DETECTS a synthetic unguarded
# `partial_cmp(...).unwrap_or(Ordering::Equal)` and CLEARS it once annotated
# with the `// nan-safe:allow` escape. Mirrors test_event_inventory_wired.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== check-nan-safe-ordering.sh wiring tests ==="

# The orchestrator runs scripts/verify.sh, so wiring is asserted against the
# verify.sh lint plan. --include-infra so the lint-side infra leaf appears;
# --scope all for the full plan; env/comment lines stripped via `grep -v '^#'`.
LINT_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" lint --scope all --include-infra --print-plan | grep -v '^#')"
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --include-infra --print-plan | grep -v '^#')"
export LINT_PLAN_SEGS TEST_PLAN_SEGS

# -- (a): script is referenced in the lint plan --------------------------------
echo ""
echo "--- (a): scripts/check-nan-safe-ordering.sh is in the lint plan ---"
assert "lint plan contains 'scripts/check-nan-safe-ordering.sh'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'scripts/check-nan-safe-ordering.sh'"

# -- (b): if-test-f guard is used ---------------------------------------------
echo ""
echo "--- (b): if test -f guard is used in the lint plan ---"
assert "lint plan contains 'if test -f scripts/check-nan-safe-ordering.sh'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'if test -f scripts/check-nan-safe-ordering.sh'"

# -- (c): WARNING echo is present for guard-skip branch -----------------------
echo ""
echo "--- (c): WARNING echo for guard-skip branch in the lint plan ---"
assert "lint plan has WARNING echo for check-nan-safe-ordering.sh skip" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'WARNING.*check-nan-safe-ordering'"

# -- (d): invocation is wrapped with timeout --kill-after=60 ------------------
echo ""
echo "--- (d): timeout --kill-after=60 wraps the invocation in the lint plan ---"
# Exact-shape pattern: it cannot span &&-separated clauses because every segment
# between 'timeout --kill-after=60' and the script name is a short anchored class
# with no greedy '.*'.
TIMEOUT_PATTERN='timeout --kill-after=60 [0-9]+m bash scripts/check-nan-safe-ordering\.sh'
assert "lint plan wraps check-nan-safe-ordering.sh with 'timeout --kill-after=60'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -qE '$TIMEOUT_PATTERN'"

# Synthetic-negative: the tight pattern must reject a 'timeout' on a different
# clause than the script (a greedy '.*' regex would silently pass this).
assert "TIMEOUT_PATTERN rejects: timeout on a different clause than the script" \
    bash -c "! echo 'lint_command: timeout --kill-after=60 30m cargo clippy && bash scripts/check-nan-safe-ordering.sh' | grep -qE '$TIMEOUT_PATTERN'"

# -- (e): script exists and is executable on disk ------------------------------
echo ""
echo "--- (e): scripts/check-nan-safe-ordering.sh exists and is executable ---"
assert "scripts/check-nan-safe-ordering.sh exists" \
    test -f "$REPO_ROOT/scripts/check-nan-safe-ordering.sh"
assert "scripts/check-nan-safe-ordering.sh is executable" \
    test -x "$REPO_ROOT/scripts/check-nan-safe-ordering.sh"

# -- (f): script is NOT in the test plan (placement in lint only) --------------
echo ""
echo "--- (f): scripts/check-nan-safe-ordering.sh is NOT in the test plan ---"
assert "test plan does NOT reference scripts/check-nan-safe-ordering.sh" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'scripts/check-nan-safe-ordering.sh'"

# -- (g): script runs clean (exit 0) against the current tree ------------------
echo ""
echo "--- (g): check-nan-safe-ordering.sh exits 0 on the current tree ---"
assert "bash scripts/check-nan-safe-ordering.sh --repo-root REPO_ROOT exits 0" \
    bash "$REPO_ROOT/scripts/check-nan-safe-ordering.sh" --repo-root "$REPO_ROOT"
assert "bash scripts/check-nan-safe-ordering.sh exits 0 with CWD=repo root (mirrors lint_command)" \
    bash -c "cd '$REPO_ROOT' && bash scripts/check-nan-safe-ordering.sh"

# -- (h): DETECTION — the task's user-observable signal ------------------------
# Prove detection (not just presence-of-string-in-script) against a throwaway
# git repo: a synthetic unguarded site in a covered crate is FLAGGED; the same
# site CLEARS once annotated with `// nan-safe:allow`; a doc-comment mention is
# NOT flagged.
echo ""
echo "--- (h): gate detects a synthetic unguarded pattern and honors the escape ---"
GATE="$REPO_ROOT/scripts/check-nan-safe-ordering.sh"
DET_TMP="$(mktemp -d)"
cleanup_det() { rm -rf "$DET_TMP"; }
trap cleanup_det EXIT
git -C "$DET_TMP" init -q
git -C "$DET_TMP" config user.email test@invalid.local
git -C "$DET_TMP" config user.name test
mkdir -p "$DET_TMP/crates/reify-fdm/src"

# unguarded → flagged (gate exits non-zero)
printf 'fn s(v: &mut Vec<f64>) { v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)); }\n' \
    > "$DET_TMP/crates/reify-fdm/src/synthetic.rs"
git -C "$DET_TMP" add -A
assert "gate FLAGS a synthetic unguarded partial_cmp().unwrap_or(Ordering::Equal) in a covered crate" \
    bash -c "! bash '$GATE' --repo-root '$DET_TMP'"

# annotated → cleared (gate exits 0)
printf 'fn s(v: &mut Vec<f64>) { v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)); } // nan-safe:allow — guarded upstream\n' \
    > "$DET_TMP/crates/reify-fdm/src/synthetic.rs"
git -C "$DET_TMP" add -A
assert "gate CLEARS the same site once annotated with // nan-safe:allow" \
    bash "$GATE" --repo-root "$DET_TMP"

# doc-comment mention → not flagged
printf '/// avoid partial_cmp(...).unwrap_or(Ordering::Equal) here; use total_cmp\nfn s() {}\n' \
    > "$DET_TMP/crates/reify-fdm/src/synthetic.rs"
git -C "$DET_TMP" add -A
assert "gate does NOT flag a doc-comment mention of the hazard" \
    bash "$GATE" --repo-root "$DET_TMP"

test_summary
