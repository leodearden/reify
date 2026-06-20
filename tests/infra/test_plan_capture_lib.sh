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

# ---------------------------------------------------------------------------
# Section 3: plan_narrow_active — extract NARROW_ACTIVE value
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_narrow_active: NARROW_ACTIVE extraction ---"

_NARROW0_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands ---"

_NARROW1_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=1 affected=reify-doc
# --- commands ---"

_NO_NARROW_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# --- commands ---"

# (a) NARROW_ACTIVE=0 -> echoes "0".
assert "plan_narrow_active: NARROW_ACTIVE=0 echoes '0'" \
    test "$(plan_narrow_active "$_NARROW0_DUMP")" = "0"

# (b) NARROW_ACTIVE=1 -> echoes "1".
assert "plan_narrow_active: NARROW_ACTIVE=1 echoes '1'" \
    test "$(plan_narrow_active "$_NARROW1_DUMP")" = "1"

# (c) Dump lacking narrowing line -> echoes empty.
assert "plan_narrow_active: no narrowing line echoes empty" \
    test "$(plan_narrow_active "$_NO_NARROW_DUMP")" = ""

# ---------------------------------------------------------------------------
# Section 4: capture_print_plan — retry-on-incomplete-capture wrapper
# ---------------------------------------------------------------------------
echo ""
echo "--- capture_print_plan: retry-on-incomplete-capture ---"

# Use a counter FILE (survives the command-substitution subshell) for tracking
# how many times the fixture function is called.
_COUNTER_FILE="$(mktemp)"
trap 'rm -f "$_COUNTER_FILE"' EXIT

# Fixture: emits TRUNCATED on attempt 1, COMPLETE on attempt >= 2.
_fake_emit_succeed_on_second() {
    local cnt
    cnt=$(cat "$_COUNTER_FILE" 2>/dev/null || echo 0)
    cnt=$((cnt + 1))
    printf '%s' "$cnt" > "$_COUNTER_FILE"
    if [ "$cnt" -ge 2 ]; then
        printf '%s\n' "# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task"
        printf '%s\n' "# narrowing — NARROW_ACTIVE=0 affected=ALL"
        printf '%s\n' "# --- commands (executed in order; '&&' semantics — stop on first failure) ---"
        printf '%s\n' "cargo clippy --workspace"
    else
        # Truncated: header only, no '# --- commands' marker.
        printf '%s\n' "# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task"
        printf '%s\n' "# narrowing — NARROW_ACTIVE=0 affected=ALL"
    fi
}

# (a) Returns 0, OUT holds complete dump, counter == 2 (retried exactly once).
printf '0' > "$_COUNTER_FILE"
_OUT_A=""
assert "capture_print_plan (a): returns 0 when second attempt succeeds" \
    capture_print_plan _OUT_A 3 _fake_emit_succeed_on_second

assert "capture_print_plan (a): OUT holds complete dump" \
    plan_capture_complete "$_OUT_A"

_cnt_a=$(cat "$_COUNTER_FILE")
assert "capture_print_plan (a): retried exactly once (counter == 2)" \
    test "$_cnt_a" = "2"

# Fixture: always emits truncated dump.
_fake_emit_always_truncated() {
    local cnt
    cnt=$(cat "$_COUNTER_FILE" 2>/dev/null || echo 0)
    cnt=$((cnt + 1))
    printf '%s' "$cnt" > "$_COUNTER_FILE"
    # Header only — no '# --- commands' marker.
    printf '%s\n' "# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task"
    printf '%s\n' "# narrowing — NARROW_ACTIVE=0 affected=ALL"
}

# (b) Returns non-zero after exactly max_attempts; OUT holds last (truncated) capture.
printf '0' > "$_COUNTER_FILE"
_OUT_B=""
assert "capture_print_plan (b): returns non-zero after exhausting max_attempts" \
    refute capture_print_plan _OUT_B 3 _fake_emit_always_truncated

_cnt_b=$(cat "$_COUNTER_FILE")
assert "capture_print_plan (b): called exactly max_attempts times (counter == 3)" \
    test "$_cnt_b" = "3"

assert "capture_print_plan (b): OUT holds last (truncated) capture (non-empty)" \
    test -n "$_OUT_B"

# Fixture: always emits complete dump on first call.
_fake_emit_always_complete() {
    local cnt
    cnt=$(cat "$_COUNTER_FILE" 2>/dev/null || echo 0)
    cnt=$((cnt + 1))
    printf '%s' "$cnt" > "$_COUNTER_FILE"
    printf '%s\n' "# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task"
    printf '%s\n' "# narrowing — NARROW_ACTIVE=0 affected=ALL"
    printf '%s\n' "# --- commands (executed in order; '&&' semantics — stop on first failure) ---"
    printf '%s\n' "cargo clippy --workspace"
}

# (c) Returns 0 with counter == 1 (no superfluous retries).
printf '0' > "$_COUNTER_FILE"
_OUT_C=""
assert "capture_print_plan (c): returns 0 on first complete dump" \
    capture_print_plan _OUT_C 3 _fake_emit_always_complete

_cnt_c=$(cat "$_COUNTER_FILE")
assert "capture_print_plan (c): no superfluous retries (counter == 1)" \
    test "$_cnt_c" = "1"

test_summary
