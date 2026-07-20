#!/usr/bin/env bash
# Plan-shape guard for task 5287 (PRD verify-retry-failed-only, task α) —
# scripts/verify.sh must honor a dark-factory-supplied "failed-only" retry
# subset so a merge-gate retry re-runs ONLY the did-not-pass tests against the
# warm _merge-verify target/, instead of a full recompile + full ~20,280-test
# run.
#
# Seam (CLAUDE.md): "reify ships the primitive, dark-factory wires the
# invocation." verify.sh BUILDS the exact-match nextest filterset expression
# and owns the tree-OID guard / loud full-fallback / size ceiling; DF (D2) owns
# the subset CONTENT (the newline-delimited exact test-id file) and sets the
# consumed envs. This test locks in the primitive, at PLAN-BUILD time.
#
# What α delivers (this test's scope):
#   - subset consumption: REIFY_VERIFY_RETRY_SCOPE=failed_only + a matching
#     attempt-0 sidecar tree_oid ⇒ append ` -E 'test(=id1) | test(=id2) | …'`
#     (EXACT `test(=…)` only — never substring — PRD §6.3 soundness) to the
#     NEXTEST=1 cargo line, built once from the DF-written filter file (INV-5).
#   - tree-OID eligibility gate: a mismatched / absent sidecar tree_oid ⇒ no
#     subset fragment (full verify).
#   - byte-identical default: no REIFY_VERIFY_RETRY_* envs ⇒ plan unchanged.
# (The three LOUD full-fallback reasons — tree drift / no subset / subset too
# large — the per-profile filter precedence, and the attempt-0 sidecar stamp
# are added by the later steps of this same test file.)
#
# SCOPE BOUNDARY: α owns this fast hermetic plan-shape unit test. Sibling task δ
# (tests/infra/test_verify_retry_failed_only.sh) owns the @@REIFY_RETRY_SCOPE@@
# honest marker and the B1-B6 e2e boundary test that exercises the REAL nextest
# run — complementary levels, NOT G7 lock-step duplication.
#
# Hermetic: drives ONLY `verify.sh … --print-plan` (verify.sh builds the plan
# and exits 0 — no cargo build, no tests executed). The subset-vs-fallback
# decision is made at build time, so --print-plan is a faithful oracle of
# whether the `-E` subset applied. Command-shape assertions that depend on
# nextest being installed are guarded on a NEXTEST_AVAILABLE probe of the plan
# header's `nextest=` token (sibling idiom, test_verify_test_threads.sh);
# host-independent invariants (byte-identical default; the loud stderr lines,
# added by later steps) are asserted unconditionally. Temp sidecar / filter
# files are pointed at via the REIFY_VERIFY_ATTEMPT_SIDECAR /
# REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE* envs for full hermeticity.
#
# Mirrors:
#   - tests/infra/test_verify_test_threads.sh (task 5264) — the direct
#     structural precedent (env→nextest fragment in emit_nextest_pass,
#     --print-plan oracle + NEXTEST availability probe).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY="$REPO_ROOT/scripts/verify.sh"

echo "=== verify.sh retry-subset plan-shape tests (task 5287, PRD verify-retry-failed-only α) ==="

# --- Hermetic fixtures -------------------------------------------------------
# A temp attempt-0 sidecar (tree_oid=deadbeef) and a 2-line exact-id filter
# file, both outside target/ and pointed at via the testability-knob envs so
# nothing touches the real repo state.
_TMP="$(mktemp -d "${TMPDIR:-/tmp}/reify-retry-subset.XXXXXX")"
trap 'rm -rf "$_TMP"' EXIT

SIDECAR="$_TMP/attempt.json"
printf '{"tree_oid":"deadbeef","profiles":"debug","timestamp":"2026-07-20T00:00:00Z"}\n' > "$SIDECAR"

FILTER="$_TMP/filter.txt"
ID1="reify_core::foo::test_alpha"
ID2="reify_core::bar::test_beta"
printf '%s\n%s\n' "$ID1" "$ID2" > "$FILTER"

# --- nextest availability probe (off the byte-identical default plan) --------
PLAN_DEFAULT="$(bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true
_HEADER="$(printf '%s\n' "$PLAN_DEFAULT" | grep '^# verify.sh plan' || true)"
NEXTEST_AVAILABLE=0
case "$_HEADER" in
    *"nextest=1"*) NEXTEST_AVAILABLE=1 ;;
esac
echo "(nextest available on this host: $NEXTEST_AVAILABLE)"

# ---------------------------------------------------------------------------
# Test 1: SUBSET APPLICATION + TREE-OID ELIGIBILITY GATE + DEFAULT-INVARIANT.
#   (a) TREE_OID matches the sidecar (deadbeef) ⇒ the debug `cargo nextest run`
#       line carries the exact-match filterset `test(=id1)` / `test(=id2)`, and
#       the bare-substring form `test(<id1>)` (no `=`) is ABSENT (soundness).
#   (b) TREE_OID mismatches (cafef00d) ⇒ the nextest line has NO `test(=` retry
#       fragment (the eligibility gate blocks the subset → full verify).
#   (c) DEFAULT plan (no REIFY_VERIFY_RETRY_* envs) ⇒ NO `test(=` fragment on
#       any cargo line (byte-identical to today).
# RED at base: emit_nextest_pass has no retry handling, so (a) finds no fragment.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: failed_only subset applies under a matching tree_oid; gate blocks a mismatch; default unchanged ---"

PLAN_MATCH="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true

PLAN_MISMATCH="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=cafef00d \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "match: debug nextest line carries exact-match filterset test(=$ID1)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test(=$2)"' \
        _ "$PLAN_MATCH" "$ID1"

    assert "match: debug nextest line carries exact-match filterset test(=$ID2)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test(=$2)"' \
        _ "$PLAN_MATCH" "$ID2"

    assert "match: bare-substring form test($ID1) (no '='') is ABSENT (exact-match soundness, PRD §6.3)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test($2)"' \
        _ "$PLAN_MATCH" "$ID1"

    assert "mismatch (tree drift): debug nextest line has NO test(= retry fragment (eligibility gate blocks)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test(="' \
        _ "$PLAN_MISMATCH"
fi

assert "default: NO test(= retry fragment on any cargo line (byte-identical default)" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -qF -- "test(="' \
    _ "$PLAN_DEFAULT"

test_summary
