#!/usr/bin/env bash
# tests/infra/test_verify_confirm_failed.sh — boundary test for task 7423,
# implementing §9.1 (B1-B11) of docs/prds/verify-confirm-failed-self-discovery.md.
#
# WHAT IS UNDER TEST — the offline lane's failure-confirmation seam:
#   RECORDING run  `run-offline-deep.sh --test-threads=N`
#                  → offline-role nextest with --no-fail-fast + JUnit capture
#                  → a profile-qualified confirm MANIFEST of failing bare test IDs.
#   CONFIRM run    `run-offline-deep.sh --test-threads=N --confirm-failed`
#                  → re-runs exactly that manifest's subset
#                  → prints ONLY the still-failing bare IDs, one per line.
#
# WHY THE ASSERTIONS LOOK THE WAY THEY DO. dark-factory's already-landed
# consumer spawns the confirm run with stdout=PIPE, stderr=STDOUT — the two
# streams are MERGED — and treats every non-blank line of the result as one
# confirmed-still-failing test ID. So the merged capture IS the wire contract,
# and every confirm-path assertion here captures `2>&1` into ONE buffer for
# exactly that reason. A stray diagnostic on either stream is not cosmetic: it
# becomes a bogus "test ID" that DF fingerprints and files a fix task against.
# B9 is the standing regression guard for that invariant.
#
# Every assertion EXECUTES a real entry point (verify.sh, run-offline-deep.sh,
# gen-nextest-config.sh, confirm-failed-manifest.sh). None asserts on source
# text — a grep for a flag in a script proves nothing about the command the
# plan actually emits.
#
# Mirrors tests/infra/test_run_offline_deep.sh for the wrapper/plan idioms and
# tests/infra/test_verify_retry_failed_only.sh for the retry-pipeline ones.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY_SH="$REPO_ROOT/scripts/verify.sh"
GEN_NEXTEST_CONFIG="$REPO_ROOT/scripts/gen-nextest-config.sh"

# Run-private scratch root; removed on EXIT. Holds the generated nextest
# configs and the JUnit fixtures, so nothing here touches the lane's target/.
WORK="$(mktemp -d "${TMPDIR:-/tmp}/reify-confirm-failed-test.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

echo "=== verify.sh --confirm-failed boundary tests (task 7423, PRD §9.1) ==="

# ---------------------------------------------------------------------------
# Shared helper: capture one role's `--print-plan` output.
#
# Stderr is discarded so the capture is the pure plan; rc is taken via
# `|| rc=$?` so a RED-phase non-zero exit reports as a clean assertion FAIL
# rather than tripping this suite's own `set -e` inside a command
# substitution.
# ---------------------------------------------------------------------------
plan_for_role() {
    local _role="$1" _rc=0
    DF_VERIFY_ROLE="$_role" bash "$VERIFY_SH" test --print-plan 2>/dev/null || _rc=$?
    return "$_rc"
}

PLAN_TASK="$(plan_for_role task || true)"
PLAN_MERGE="$(plan_for_role merge || true)"
PLAN_OFFLINE="$(plan_for_role offline || true)"
PLAN_BACKGROUND="$(plan_for_role background || true)"

# ===========================================================================
# Section A (leaf α) — offline-role `--no-fail-fast` + unconditional JUnit
# capture in the GENERATED nextest config.
#
# `--no-fail-fast` is the precondition for manifest COMPLETENESS (PRD §5.3):
# without it a recording pass with an early failure never runs the later
# tests at all — they are not even attributed as skipped — so the recorded
# failed-set silently under-captures. It is scoped to the offline role so
# task/merge/background keep their deliberate fail-fast posture.
# ===========================================================================
echo ""
echo "--- Section A (leaf α): offline --no-fail-fast + unconditional JUnit capture ---"

assert "A1: role=offline plan emits --no-fail-fast on its cargo-nextest line" \
    bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -q -- "--no-fail-fast"' \
    _ "$PLAN_OFFLINE"

# A2-A4: the negative half of the role scoping. Asserted per role rather than
# as one combined check so a regression names the role that leaked.
assert "A2: role=task plan does NOT contain --no-fail-fast (fail-fast posture preserved)" \
    bash -c '! printf "%s\n" "$1" | grep -q -- "--no-fail-fast"' \
    _ "$PLAN_TASK"

assert "A3: role=merge plan does NOT contain --no-fail-fast" \
    bash -c '! printf "%s\n" "$1" | grep -q -- "--no-fail-fast"' \
    _ "$PLAN_MERGE"

assert "A4: role=background plan does NOT contain --no-fail-fast" \
    bash -c '! printf "%s\n" "$1" | grep -q -- "--no-fail-fast"' \
    _ "$PLAN_BACKGROUND"

# A5-A6: JUnit capture is UNCONDITIONAL — it lives in the GENERATED per-run
# config copy, not in the tracked .config/nextest.toml, so a bare developer
# `cargo nextest` is unaffected while every verify.sh-driven pass records one.
# Executed for real (the generator is run, not grepped) under two roles, to
# pin that it does not become role-gated by accident.
gen_config_for_role() {
    local _role="$1" _out
    _out="$WORK/nextest-$_role.toml"
    local _path
    _path="$(DF_VERIFY_ROLE="$_role" bash "$GEN_NEXTEST_CONFIG")" || return 1
    [ -f "$_path" ] || return 1
    cp "$_path" "$_out"
    rm -f "$_path"
    printf '%s\n' "$_out"
}

CFG_TASK="$(gen_config_for_role task || true)"
CFG_OFFLINE="$(gen_config_for_role offline || true)"

assert "A5: gen-nextest-config.sh (role=task) emits a [profile.default.junit] table with a path= value" \
    bash -c '[ -n "$1" ] && [ -f "$1" ] && grep -q "^\[profile\.default\.junit\]$" "$1" && sed -n "/^\[profile\.default\.junit\]$/,/^\[/p" "$1" | grep -qE "^path = \".+\"$"' \
    _ "$CFG_TASK"

assert "A6: gen-nextest-config.sh (role=offline) emits the same [profile.default.junit] table (capture is unconditional, not role-gated)" \
    bash -c '[ -n "$1" ] && [ -f "$1" ] && grep -q "^\[profile\.default\.junit\]$" "$1" && sed -n "/^\[profile\.default\.junit\]$/,/^\[/p" "$1" | grep -qE "^path = \".+\"$"' \
    _ "$CFG_OFFLINE"

# A7: the generator's `test-threads` sed is line-anchored but NOT
# section-anchored (its own header warning; pinned by test_occt_gated_scope.sh
# Test 17k). Adding a SECOND named profile would put a second `test-threads`
# line in reach of that one sed and silently clobber it. Only a sub-table of
# the existing profile.default may be appended — assert exactly that.
assert "A7: the generated config declares NO profile other than 'default' (a second named profile would be clobbered by the line-anchored test-threads sed)" \
    bash -c 'set -e; [ -n "$1" ]; [ -f "$1" ]; [ "$(grep -oE "^\[+profile\.[a-zA-Z0-9_-]+" "$1" | sed -E "s/^\[+profile\.//" | sort -u)" = "default" ]' \
    _ "$CFG_OFFLINE"

# ===========================================================================
# Section B (B10) — byte-identical when inactive.
#
# The confirm machinery is a side channel: with `--confirm-failed` unset, the
# non-offline roles' plans must carry no trace of it. The offline plan DOES
# change (it gains --no-fail-fast, and later the fused manifest write) — that
# is the intended, deliberate change to the offline golden, so B10 is stated
# per role rather than globally.
# ===========================================================================
echo ""
echo "--- Section B (B10): confirm machinery is invisible in every non-offline plan ---"

for _role_plan in "task:$PLAN_TASK" "merge:$PLAN_MERGE" "background:$PLAN_BACKGROUND"; do
    _role="${_role_plan%%:*}"
    _plan="${_role_plan#*:}"
    assert "B10/$_role: plan contains no 'confirm' token (the confirm path emits no plan line for this role)" \
        bash -c '! printf "%s\n" "$1" | grep -qi "confirm"' \
        _ "$_plan"
done

test_summary
