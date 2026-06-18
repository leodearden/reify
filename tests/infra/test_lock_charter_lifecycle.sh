#!/usr/bin/env bash
# tests/infra/test_lock_charter_lifecycle.sh — integration gate for the
# task module-lock charter lifecycle end-to-end behavior.
#
# §8 boundary-test table from docs/prds/task-lock-charter-lifecycle.md:
#   Rows 1-2   — guard predicate (always-on, drives real α scripts/lock-charter-guard.sh)
#   Row 3      — C-P3 predicate determinism / no-drift (shared α/γ test vector)
#   Rows 4-5   — set-to-plan release + waiter dispatch (C-S1, curl-stub hermetic)
#   Rows 6-7   — BRE acquire-before-edit + no-release-pre-acquire (C-S2/C-K1)
#   Row 8      — staleness re-pend + revalidate preserved (C-K1)
#   Rows 9-10  — anti-anchored first architect + revalidation-exempt (C-A1/C-A2)
#   Row 13     — live submit-site dir-reject smoke (opt-in REIFY_LOCK_CHARTER_LIVE=1)
#
# Architecture: two-mode harness.
#   HERMETIC (always-on, merge-gate GREEN): rows 1-3 drive the real predicate;
#     rows 4-13 use PATH-stubbed curl with canned JSON-RPC MCP responses.
#   OPT-IN LIVE (REIFY_LOCK_CHARTER_LIVE=1): scheduler/submit rows drive the
#     real fused-memory HTTP MCP.  Without the flag the scheduler scenarios
#     SKIP cleanly (exit 0 + clear message) — never auto-enabled by reachability.
#
# Auto-discovered by tests/infra/run_all.sh (test_*.sh glob).
# Lib (lock_charter_harness_lib.sh) is *_lib.sh so it stays out of the glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: tests/infra/test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/lock_charter_harness_lib.sh" ] || {
    echo "ERROR: lock_charter_harness_lib.sh not found at $SCRIPT_DIR/lock_charter_harness_lib.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/lock_charter_harness_lib.sh
source "$SCRIPT_DIR/lock_charter_harness_lib.sh"

echo "=== Lock-charter lifecycle integration gate (task #4678) ==="

# ──────────────────────────────────────────────────────────────────────────────
# §8 rows 1-3: Guard surface — always-on, drives real α predicate
# (scripts/lock-charter-guard.sh — landed by task #4676)
# ──────────────────────────────────────────────────────────────────────────────
echo "--- §8 rows 1-3: guard surface (always-on, drives real α predicate) ---"

# Row 1 (OBSERVED firing — G6 non-vacuous mandate): dir-path is REJECT
lcl_run_guard classify "crates/reify-eval/src/"
assert "row 1: classify dir 'crates/reify-eval/src/' exits 1 (REJECT)" test "$LCL_GUARD_RC" -eq 1
assert "row 1: classify dir stdout contains REJECT" test "${LCL_GUARD_OUT#*REJECT}" != "$LCL_GUARD_OUT"

# Negative control: proves the positive assertion is non-vacuous
lcl_run_guard classify "crates/x/src/foo.rs"
assert "row 1 neg: classify file 'crates/x/src/foo.rs' exits 0 (ACCEPT)" test "$LCL_GUARD_RC" -eq 0
assert "row 1 neg: classify file stdout contains ACCEPT" test "${LCL_GUARD_OUT#*ACCEPT}" != "$LCL_GUARD_OUT"

# Row 2: check with empty stdin ([] defer-to-architect value) → ACCEPT
lcl_run_guard check </dev/null
assert "row 2: check empty stdin exits 0 (ACCEPT)" test "$LCL_GUARD_RC" -eq 0

# Row 3 (C-P3 determinism/no-drift):
# (a) --list-extensions equals the canonical α/γ shared test vector
_lcl_canonical="$(lcl_canonical_extensions)"
lcl_run_guard --list-extensions
assert "row 3a: --list-extensions exits 0" test "$LCL_GUARD_RC" -eq 0
assert "row 3a: --list-extensions matches canonical α/γ test vector" test "$LCL_GUARD_OUT" = "$_lcl_canonical"
# (b) same dir path yields byte-identical REJECT via both classify and check invocation styles
lcl_run_guard classify "crates/reify-eval/src/"
_lcl_classify_out="$LCL_GUARD_OUT"
_lcl_classify_rc="$LCL_GUARD_RC"
lcl_run_guard check "crates/reify-eval/src/"
assert "row 3b: classify+check agree on exit code for dir" test "$LCL_GUARD_RC" -eq "$_lcl_classify_rc"
assert "row 3b: classify+check produce byte-identical REJECT verdict" test "$LCL_GUARD_OUT" = "$_lcl_classify_out"

# ──────────────────────────────────────────────────────────────────────────────
# Live skip-guard + MCP curl client contract
# lcl_live_enabled returns false (with SKIP reason to stderr) when:
#   - REIFY_LOCK_CHARTER_LIVE is unset/not-"1", OR
#   - curl or jq are absent (even with the flag set)
# lcl_mcp_call: bounded exit code (NOT 127) with unreachable URL, no hang.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- live skip-guard + MCP curl client contract ---"

_LCL_NOTOOL_DIR="$(mktemp -d /tmp/test-lcl-live-notool-XXXXXX)"
_lcl_full_cleanup() {
    rm -rf "${_LCL_NOTOOL_DIR:-}" 2>/dev/null || true
    lcl_cleanup_stubs 2>/dev/null || true
}
trap _lcl_full_cleanup EXIT

# (1) lcl_live_enabled: returns false when REIFY_LOCK_CHARTER_LIVE unset;
#     prints a clear SKIP reason to stderr (harness can test it via 2>&1 capture)
_lcl_live_msg="$(unset REIFY_LOCK_CHARTER_LIVE; lcl_live_enabled 2>&1)" \
    && _lcl_live_rc=0 || _lcl_live_rc=$?
assert "lcl_live_enabled: false when REIFY_LOCK_CHARTER_LIVE unset" \
    test "$_lcl_live_rc" -ne 0
assert "lcl_live_enabled: emits SKIP reason to stderr when flag unset" \
    test "${_lcl_live_msg#*SKIP}" != "$_lcl_live_msg"

# (2) lcl_live_enabled: false when curl or jq absent even with flag set
#     PATH stripped to empty dir so command -v curl/jq returns non-zero
_lcl_live_notool_msg="$(
    export REIFY_LOCK_CHARTER_LIVE=1
    PATH="$_LCL_NOTOOL_DIR" lcl_live_enabled 2>&1
)" && _lcl_live_notool_rc=0 || _lcl_live_notool_rc=$?
assert "lcl_live_enabled: false when tools absent (even with flag)" \
    test "$_lcl_live_notool_rc" -ne 0
assert "lcl_live_enabled: emits SKIP reason when tools absent" \
    test "${_lcl_live_notool_msg#*SKIP}" != "$_lcl_live_notool_msg"

# (3) Live mode NEVER auto-enabled by reachability — only by the explicit flag.
#     Already covered: test (1) shows that with flag unset, lcl_live_enabled
#     returns false regardless of what REIFY_FUSED_MEMORY_URL is pointed at.

# (4) lcl_mcp_call: bounded error with unreachable URL (no hang, no set-e abort)
#     Exit code must NOT be 127 (127 = undefined function).
#     127.0.0.1:1 is a closed port → curl connection-refused (fast, not a timeout)
_lcl_mcp_rc=0
_lcl_mcp_out="$(
    export REIFY_FUSED_MEMORY_URL='http://127.0.0.1:1'
    lcl_mcp_call get_scheduler_state '{}' 2>&1
)" && _lcl_mcp_rc=0 || _lcl_mcp_rc=$?
assert "lcl_mcp_call: bounded exit code (NOT 127=undefined)" test "$_lcl_mcp_rc" -ne 127

# ──────────────────────────────────────────────────────────────────────────────
# §8 rows 4-5: set-to-plan release + waiter dispatch (C-S1)
# Hermetic via PATH-stubbed curl returning canned JSON-RPC MCP responses.
# Canned shapes grounded in real observed fused-memory API:
#   get_scheduler_state: {parks:{<task>:{held:[...]}}}
#   get_scheduler_events: {events:[{event_type,task_id,data}]}
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- §8 rows 4-5: set-to-plan release + waiter dispatch (C-S1, curl-stub hermetic) ---"

_LCL_T1="task-1001"
_LCL_T2="task-1002"
_LCL_PLAN_FILES='["crates/reify-eval/src/foo.rs","crates/reify-eval/src/bar.rs"]'

# Positive: T1 holds exactly plan.files; lock_released(plan_refinement) + task_started T2
_LCL_STATE_POS='{"result":{"content":[{"text":"{\"parks\":{\"task-1001\":{\"held\":[\"crates/reify-eval/src/foo.rs\",\"crates/reify-eval/src/bar.rs\"]}}}"}]}}'
_LCL_EVENTS_POS='{"result":{"content":[{"text":"{\"events\":[{\"event_type\":\"lock_released\",\"task_id\":\"task-1001\",\"data\":{\"reason\":\"plan_refinement\",\"modules\":[\"crates/x/src/extra.rs\"]}},{\"event_type\":\"task_started\",\"task_id\":\"task-1002\",\"data\":{}}]}"}]}}'

# Negative: T1 over-declared (held ⊋ plan.files); no release; T2 never started
_LCL_STATE_NEG='{"result":{"content":[{"text":"{\"parks\":{\"task-1001\":{\"held\":[\"crates/reify-eval/src/foo.rs\",\"crates/reify-eval/src/bar.rs\",\"crates/x/src/extra.rs\"]}}}"}]}}'
_LCL_EVENTS_NEG='{"result":{"content":[{"text":"{\"events\":[]}"}]}}'

# Positive: set up stub → expect PASS
lcl_make_curl_stub "$_LCL_STATE_POS" "$_LCL_EVENTS_POS"
_lcl_stp_rc=0
lcl_assert_set_to_plan_release "$_LCL_T1" "$_LCL_PLAN_FILES" "$_LCL_T2" \
    && _lcl_stp_rc=0 || _lcl_stp_rc=$?
assert "row 4-5 pos: set-to-plan release PASS on positive canned" test "$_lcl_stp_rc" -eq 0

# Negative: set up stub → expect FAIL (held ⊋ plan.files)
lcl_make_curl_stub "$_LCL_STATE_NEG" "$_LCL_EVENTS_NEG"
_lcl_stn_rc=0
lcl_assert_set_to_plan_release "$_LCL_T1" "$_LCL_PLAN_FILES" "$_LCL_T2" \
    && _lcl_stn_rc=0 || _lcl_stn_rc=$?
assert "row 4-5 neg: set-to-plan release FAIL on negative canned (held over-declared)" \
    test "$_lcl_stn_rc" -ne 0

# ──────────────────────────────────────────────────────────────────────────────
# §8 rows 6-7: BRE acquire-before-edit + no-release-pre-acquire (C-S2/C-K1, OBSERVED)
# G6 non-vacuous mandate: both positive AND negative controls asserted.
# Event timestamps distinguish ordering (integer Unix-like values).
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- §8 rows 6-7: BRE acquire-before-edit + no-release-pre-acquire (C-S2/C-K1) ---"

_LCL_DUMMY_STATE='{"result":{"content":[{"text":"{\"parks\":{\"task-1001\":{\"held\":[\"crates/reify-eval/src/foo.rs\"]}}}"}]}}'

# Row 6 pos: lock_acquired timestamp PRECEDES implementation_started timestamp
_LCL_EVENTS_R6_POS='{"result":{"content":[{"text":"{\"events\":[{\"event_type\":\"lock_acquired\",\"task_id\":\"task-1001\",\"timestamp\":100,\"data\":{\"modules\":[\"crates/x/src/extra.rs\"]}},{\"event_type\":\"implementation_started\",\"task_id\":\"task-1001\",\"timestamp\":200,\"data\":{}}]}"}]}}'
# Row 6 neg: lock_acquired timestamp FOLLOWS implementation_started (wrong order)
_LCL_EVENTS_R6_NEG='{"result":{"content":[{"text":"{\"events\":[{\"event_type\":\"implementation_started\",\"task_id\":\"task-1001\",\"timestamp\":100,\"data\":{}},{\"event_type\":\"lock_acquired\",\"task_id\":\"task-1001\",\"timestamp\":200,\"data\":{\"modules\":[\"crates/x/src/extra.rs\"]}}]}"}]}}'

lcl_make_curl_stub "$_LCL_DUMMY_STATE" "$_LCL_EVENTS_R6_POS"
_lcl_ape_pos_rc=0
lcl_acquire_precedes_edit "$_LCL_T1" && _lcl_ape_pos_rc=0 || _lcl_ape_pos_rc=$?
assert "row 6 pos: BRE acquire-before-edit PASS (acquire ts < edit ts)" \
    test "$_lcl_ape_pos_rc" -eq 0

lcl_make_curl_stub "$_LCL_DUMMY_STATE" "$_LCL_EVENTS_R6_NEG"
_lcl_ape_neg_rc=0
lcl_acquire_precedes_edit "$_LCL_T1" && _lcl_ape_neg_rc=0 || _lcl_ape_neg_rc=$?
assert "row 6 neg: BRE acquire-after-edit FAIL (acquire ts > edit ts)" \
    test "$_lcl_ape_neg_rc" -ne 0

# Row 7 pos: REQUEUED present AND no lock_released → charter intact
_LCL_EVENTS_R7_POS='{"result":{"content":[{"text":"{\"events\":[{\"event_type\":\"REQUEUED\",\"task_id\":\"task-1001\",\"timestamp\":100,\"data\":{\"_last_block_reason\":\"plan_blast_radius_lock_conflict\"}}]}"}]}}'
# Row 7 neg: REQUEUED present BUT lock_released also fired (charter violated)
_LCL_EVENTS_R7_NEG='{"result":{"content":[{"text":"{\"events\":[{\"event_type\":\"REQUEUED\",\"task_id\":\"task-1001\",\"timestamp\":100,\"data\":{}},{\"event_type\":\"lock_released\",\"task_id\":\"task-1001\",\"timestamp\":200,\"data\":{}}]}"}]}}'

lcl_make_curl_stub "$_LCL_DUMMY_STATE" "$_LCL_EVENTS_R7_POS"
_lcl_nrr_pos_rc=0
lcl_no_release_when_repended "$_LCL_T1" && _lcl_nrr_pos_rc=0 || _lcl_nrr_pos_rc=$?
assert "row 7 pos: no-release-pre-acquire PASS (REQUEUED + no lock_released)" \
    test "$_lcl_nrr_pos_rc" -eq 0

lcl_make_curl_stub "$_LCL_DUMMY_STATE" "$_LCL_EVENTS_R7_NEG"
_lcl_nrr_neg_rc=0
lcl_no_release_when_repended "$_LCL_T1" && _lcl_nrr_neg_rc=0 || _lcl_nrr_neg_rc=$?
assert "row 7 neg: lock_released despite REQUEUED FAIL (charter violated)" \
    test "$_lcl_nrr_neg_rc" -ne 0

# ──────────────────────────────────────────────────────────────────────────────
# §8 row 8: staleness re-pend + revalidate preserved (C-K1, OBSERVED, G6)
# Event strings grounded in manifest/PRD ζ block:
#   REQUEUED + data._last_block_reason='plan_blast_radius_lock_conflict'
#   revalidation_passed (architect re-checks existing plan after re-pend)
# G6 teeth: both "no REQUEUED" and "REQUEUED but no revalidation" fail.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- §8 row 8: staleness re-pend + revalidate preserved (C-K1, OBSERVED) ---"

# Positive: REQUEUED(plan_blast_radius_lock_conflict) followed by revalidation_passed
_LCL_EVENTS_R8_POS='{"result":{"content":[{"text":"{\"events\":[{\"event_type\":\"REQUEUED\",\"task_id\":\"task-1001\",\"timestamp\":100,\"data\":{\"_last_block_reason\":\"plan_blast_radius_lock_conflict\"}},{\"event_type\":\"revalidation_passed\",\"task_id\":\"task-1001\",\"timestamp\":200,\"data\":{}}]}"}]}}'
# Negative 1: no REQUEUED at all (revalidation_passed alone is not enough)
_LCL_EVENTS_R8_NEG1='{"result":{"content":[{"text":"{\"events\":[{\"event_type\":\"revalidation_passed\",\"task_id\":\"task-1001\",\"timestamp\":200,\"data\":{}}]}"}]}}'
# Negative 2: REQUEUED with conflict reason but no revalidation marker
_LCL_EVENTS_R8_NEG2='{"result":{"content":[{"text":"{\"events\":[{\"event_type\":\"REQUEUED\",\"task_id\":\"task-1001\",\"timestamp\":100,\"data\":{\"_last_block_reason\":\"plan_blast_radius_lock_conflict\"}}]}"}]}}'

lcl_make_curl_stub "$_LCL_DUMMY_STATE" "$_LCL_EVENTS_R8_POS"
_lcl_r8_pos_rc=0
lcl_assert_repend_revalidate "$_LCL_T1" && _lcl_r8_pos_rc=0 || _lcl_r8_pos_rc=$?
assert "row 8 pos: re-pend+revalidate PASS (REQUEUED + revalidation_passed)" \
    test "$_lcl_r8_pos_rc" -eq 0

lcl_make_curl_stub "$_LCL_DUMMY_STATE" "$_LCL_EVENTS_R8_NEG1"
_lcl_r8_neg1_rc=0
lcl_assert_repend_revalidate "$_LCL_T1" && _lcl_r8_neg1_rc=0 || _lcl_r8_neg1_rc=$?
assert "row 8 neg1: no REQUEUED → FAIL" test "$_lcl_r8_neg1_rc" -ne 0

lcl_make_curl_stub "$_LCL_DUMMY_STATE" "$_LCL_EVENTS_R8_NEG2"
_lcl_r8_neg2_rc=0
lcl_assert_repend_revalidate "$_LCL_T1" && _lcl_r8_neg2_rc=0 || _lcl_r8_neg2_rc=$?
assert "row 8 neg2: REQUEUED but no revalidation marker → FAIL" test "$_lcl_r8_neg2_rc" -ne 0

# ──────────────────────────────────────────────────────────────────────────────
# §8 rows 9-10: anti-anchored first architect + revalidation-exempt (C-A1/C-A2)
# These helpers introspect canned plan-derivation-input JSON payloads (the
# structure ε controls); no MCP call needed.
# Row 9: first-architect input EXCLUDES metadata.files (anti-anchoring contract)
# Row 10: revalidation input INCLUDES metadata.files (sees the prior plan)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- §8 rows 9-10: anti-anchored first architect + revalidation-exempt (C-A1/C-A2) ---"

# Row 9 pos: description/intent present, metadata.files absent → anti-anchored
_LCL_FIRST_ARCH_POS='{"description":"implement widget feature","intent":"add widget support","metadata":{"task_id":"task-1001","title":"Widget"}}'
# Row 9 neg: metadata.files present in first-arch input (leaks queue-time charter)
_LCL_FIRST_ARCH_NEG='{"description":"implement widget feature","intent":"add widget support","metadata":{"task_id":"task-1001","title":"Widget","files":["crates/widget/src/lib.rs"]}}'

_lcl_aaa_pos_rc=0
lcl_assert_first_plan_anti_anchored "$_LCL_FIRST_ARCH_POS" \
    && _lcl_aaa_pos_rc=0 || _lcl_aaa_pos_rc=$?
assert "row 9 pos: first-plan anti-anchored PASS (no metadata.files)" \
    test "$_lcl_aaa_pos_rc" -eq 0

_lcl_aaa_neg_rc=0
lcl_assert_first_plan_anti_anchored "$_LCL_FIRST_ARCH_NEG" \
    && _lcl_aaa_neg_rc=0 || _lcl_aaa_neg_rc=$?
assert "row 9 neg: metadata.files leaked in first-plan → FAIL" \
    test "$_lcl_aaa_neg_rc" -ne 0

# Row 10 pos: metadata.files present in revalidation input (prior plan visible)
_LCL_REVAL_POS='{"description":"implement widget feature","metadata":{"task_id":"task-1001","files":["crates/widget/src/lib.rs"],"revalidation":true}}'
# Row 10 neg: metadata.files absent in revalidation input (prior plan hidden)
_LCL_REVAL_NEG='{"description":"implement widget feature","metadata":{"task_id":"task-1001","revalidation":true}}'

_lcl_rsp_pos_rc=0
lcl_assert_revalidation_sees_plan "$_LCL_REVAL_POS" \
    && _lcl_rsp_pos_rc=0 || _lcl_rsp_pos_rc=$?
assert "row 10 pos: revalidation sees plan PASS (metadata.files present)" \
    test "$_lcl_rsp_pos_rc" -eq 0

_lcl_rsp_neg_rc=0
lcl_assert_revalidation_sees_plan "$_LCL_REVAL_NEG" \
    && _lcl_rsp_neg_rc=0 || _lcl_rsp_neg_rc=$?
assert "row 10 neg: revalidation hides plan → FAIL (metadata.files absent)" \
    test "$_lcl_rsp_neg_rc" -ne 0

test_summary
