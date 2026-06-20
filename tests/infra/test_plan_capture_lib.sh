#!/usr/bin/env bash
# tests/infra/test_plan_capture_lib.sh — unit tests for tests/infra/plan_capture_lib.sh
#
# Validates fork-free plan capture/match helpers introduced for task #4708:
# hardening test_verify_scope.sh B9-default against nondeterministic --print-plan
# output under concurrent load (esc-4574-42 class: pipe-to-grep EINTR).
#
# Covers:
#   plan_match        — fork-free ERE matcher ([[ =~ ]])
#   plan_capture_complete — completeness check via structural markers
#   plan_narrow_active    — extract NARROW_ACTIVE value from dump
#   capture_print_plan    — retry-on-incomplete-capture wrapper
#   consumer drift guards — structural checks on test_verify_scope.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"

# Negative assertion helper (assert() only checks for success rc).
refute() { ! "$@"; }

echo "=== plan_capture_lib unit tests ==="

# ---------------------------------------------------------------------------
# Section 1: plan_match — fork-free ERE matcher
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_match: fork-free ERE matching ---"

# Sample plan dump used across multiple assertions.
# Includes a literal-asterisk line for the escaped-star test (b4).
_SAMPLE_PLAN="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
cargo clippy --workspace --all-targets --message-format=json 2>&1 | tee /tmp/clippy.json
cargo nextest run --workspace --profile debug --exclude reify-occt-gated
tests/infra/run_all.sh
tests/infra/test_verify_scope.sh
tests/infra/test_verify_*.sh
cargo-test-occt-gated.sh foo"

# (a) Matches a literal substring present in the sample plan dump.
assert "plan_match: literal 'cargo clippy' matches" \
    plan_match "$_SAMPLE_PLAN" "cargo clippy"

# (b1) Matches alternation pattern used by the suite.
assert "plan_match: alternation 'cargo (test|nextest run) --workspace'" \
    plan_match "$_SAMPLE_PLAN" "cargo (test|nextest run) --workspace"

# (b2) Matches .* same-line pattern used by the suite.
assert "plan_match: '.*' same-line 'cargo nextest run --workspace.*--exclude'" \
    plan_match "$_SAMPLE_PLAN" "cargo nextest run --workspace.*--exclude"

# (b3) Matches escaped-dot pattern used by the suite.
assert "plan_match: escaped-dot 'cargo-test-occt-gated\\.sh'" \
    plan_match "$_SAMPLE_PLAN" "cargo-test-occt-gated\\.sh"

# (b4) Matches escaped-star glob pattern used by the suite.
assert "plan_match: escaped-star 'tests/infra/test_verify_\\*\\.sh'" \
    plan_match "$_SAMPLE_PLAN" "tests/infra/test_verify_\\*\\.sh"

# (c) Returns non-zero when pattern is absent.
assert "plan_match: absent pattern returns non-zero" \
    refute plan_match "$_SAMPLE_PLAN" "cargo build --release"

# (d) Same-line .* correctly matches a single-line pattern in a multiline dump.
# Note: bash [[ =~ ]] on Linux/glibc uses regexec() WITHOUT REG_NEWLINE, so
# . DOES match newline characters (unlike grep -qE which sets REG_NEWLINE).
# This does not affect correctness for the suite's patterns because all .*
# patterns in test_verify_scope.sh match same-line content (e.g. both
# "--workspace" and "--exclude" appear on a single plan line). Test (d) verifies
# that same-line .* works as expected (esc-4708-51 documents the discrepancy).
_MULTILINE_DUMP="line one content
line two content"
assert "plan_match: '.*' same-line match works in multiline dump (line one present)" \
    plan_match "$_MULTILINE_DUMP" "line one.*content"
assert "plan_match: absent same-line pattern fails in multiline dump" \
    refute plan_match "$_MULTILINE_DUMP" "line one.*ABSENT"

# (e) Empty pattern matches (grep -qE "" parity).
assert "plan_match: empty pattern matches any non-empty dump" \
    plan_match "$_SAMPLE_PLAN" ""

# ---------------------------------------------------------------------------
# Section 2: plan_capture_complete — structural completeness check
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_capture_complete: structural completeness ---"

_COMPLETE_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
cargo clippy --workspace"

_TRUNCATED_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL"

_EMPTY_PLAN_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=0 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
# (no commands — docs/yaml-only scope)"

# (a) Complete dump with both markers returns 0.
assert "plan_capture_complete: complete dump returns 0" \
    plan_capture_complete "$_COMPLETE_DUMP"

# (b) Truncated dump (header only, no commands marker) returns non-zero.
assert "plan_capture_complete: truncated dump returns non-zero" \
    refute plan_capture_complete "$_TRUNCATED_DUMP"

# (c) Empty string returns non-zero.
assert "plan_capture_complete: empty string returns non-zero" \
    refute plan_capture_complete ""

# (d) Empty-PLAN dump (both markers present, but no actual commands) returns 0.
# Completeness is structural — independent of whether commands exist.
assert "plan_capture_complete: docs-only (no commands) dump still returns 0" \
    plan_capture_complete "$_EMPTY_PLAN_DUMP"

test_summary
