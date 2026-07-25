#!/usr/bin/env bash
# Infrastructure test for task 1992.
# Validates that scripts/cargo-test-occt-gated.sh exists with the correct
# structure, serializes OCCT-touching test processes via flock, and that
# dark-factory-orchestrator.yaml routes all cargo test --workspace invocations through it.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/occt_flock_gate_lib.sh" ] || { echo "ERROR: occt_flock_gate_lib.sh not found at $SCRIPT_DIR/occt_flock_gate_lib.sh"; exit 1; }
source "$SCRIPT_DIR/occt_flock_gate_lib.sh"

WRAPPER="$REPO_ROOT/scripts/cargo-test-occt-gated.sh"

echo "=== OCCT flock gate tests ==="

# -- Test 1: wrapper script exists ---------------------------------------------
echo ""
echo "--- Test 1: wrapper script exists ---"

assert "scripts/cargo-test-occt-gated.sh exists" \
    test -f "$WRAPPER"

# -- Test 2: wrapper script is executable --------------------------------------
echo ""
echo "--- Test 2: wrapper script is executable ---"

assert "scripts/cargo-test-occt-gated.sh is executable (mode +x)" \
    test -x "$WRAPPER"

# -- Test 3: shebang line ------------------------------------------------------
echo ""
echo "--- Test 3: wrapper has #!/usr/bin/env bash shebang ---"

assert "first line is '#!/usr/bin/env bash'" \
    bash -c "head -1 '$WRAPPER' | grep -qxF '#!/usr/bin/env bash'"

# -- Test 4: set -euo pipefail -------------------------------------------------
echo ""
echo "--- Test 4: wrapper sets strict error handling ---"

assert "wrapper contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$WRAPPER"

# -- Test 5: flock -x invocation -----------------------------------------------
echo ""
echo "--- Test 5: wrapper invokes flock -x ---"

assert "wrapper contains 'flock -x'" \
    grep -q 'flock -x' "$WRAPPER"

# -- Test 6: default lock path -------------------------------------------------
echo ""
echo "--- Test 6: default lock path is user-scoped ---"

assert "default lock path is user-scoped via 'id -u'" \
    grep -q 'reify-occt-$(id -u)' "$WRAPPER"

# -- Test 7: argument forwarding -----------------------------------------------
echo ""
echo "--- Test 7: wrapper forwards arguments with exec and \"\$@\" ---"

assert "wrapper contains 'exec'" \
    grep -q 'exec' "$WRAPPER"

assert 'wrapper contains "$@" for argument forwarding' \
    grep -qF '"$@"' "$WRAPPER"


# -- Test 8: serialization (REIFY_OCCT_LOCK override) --------------------------
echo ""
echo "--- Test 8: wrapper serializes two concurrent invocations ---"

_LOCK_FILE="$(mktemp)"
_START_NS="$(date +%s%N)"

# Spawn two concurrent invocations each sleeping 0.4s.
# REIFY_OCCT_CONCURRENCY=1 pins N=1 (exclusive mode) so this test remains
# valid after step-7 flips the default from N=1 to auto-detect.
REIFY_OCCT_LOCK="$_LOCK_FILE" REIFY_OCCT_CONCURRENCY=1 "$WRAPPER" bash -c 'sleep 0.4' &
_PID1=$!
REIFY_OCCT_LOCK="$_LOCK_FILE" REIFY_OCCT_CONCURRENCY=1 "$WRAPPER" bash -c 'sleep 0.4' &
_PID2=$!
wait "$_PID1" "$_PID2"

_END_NS="$(date +%s%N)"
_ELAPSED_MS=$(( (_END_NS - _START_NS) / 1000000 ))

rm -f "$_LOCK_FILE" "${_LOCK_FILE}.slot-1"

# Parallel would finish in ~400ms; serialized takes >=700ms.
assert "two 0.4s sleep invocations run serially (elapsed >= 700ms, got ${_ELAPSED_MS}ms)" \
    test "$_ELAPSED_MS" -ge 700

# -- Test 9: exit-code propagation ----------------------------------------------
echo ""
echo "--- Test 9: wrapper propagates exit code of wrapped command ---"

_TMP_LOCK="$(mktemp)"
_EC=0
REIFY_OCCT_LOCK="$_TMP_LOCK" "$WRAPPER" bash -c 'exit 42' || _EC=$?
rm -f "$_TMP_LOCK"

assert "wrapper exit code is 42 (got $_EC)" \
    test "$_EC" -eq 42


# -- verify.sh plan integration tests ------------------------------------------
# These formerly grepped dark-factory-orchestrator.yaml's test_command. Since task 3766 the
# orchestrator calls scripts/verify.sh, so the canonical command list is taken
# from verify.sh --print-plan (--scope all → full plan, index-independent; env
# lines stripped via `grep -v '^#'`). The gated passes are plain `cargo test`
# under the flock wrapper regardless of the nextest/cargo-test choice for the
# ungated tail, so the gated assertions below stay exact-match.
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --print-plan | grep -v '^#')"
export TEST_PLAN_SEGS

echo ""
echo "--- Test 10: plan has NO cargo-test-occt-gated.sh invocation (task 4451: OCCT folded into nextest pool) ---"

# Task 4451 folds OCCT crates into the nextest pool (occt test-group max-threads=24,
# env-driven, default 24 — raised from 4 by task 4503/γ).
# The flock-gated debug pass is gone; cargo-test-occt-gated.sh is retained only as
# a standalone/manual runner.
assert "plan contains NO cargo-test-occt-gated.sh invocation (gated pass dropped, task 4451)" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q './scripts/cargo-test-occt-gated\.sh'"

echo ""
echo "--- Test 11: nextest release pass includes -p reify-eval (sensitivity-scoped, folded, task 4451) ---"

# Release is sensitivity-scoped: only reify-eval (OCCT ∩ release-sensitive) appears
# in the release nextest pass. No flock wrapper — the nextest occt group handles it.
assert "plan contains nextest release pass with -p reify-eval (folded, no gated wrapper)" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -v 'cargo-test-occt-gated\.sh' | grep -qE 'cargo (test|nextest run).*-p reify-eval.*--release|cargo (test|nextest run).*--release.*-p reify-eval'"

echo ""
echo "--- Test 12: nextest --workspace pass has NO --exclude (OCCT folded in, task 4451) ---"

# After the fold the full-workspace debug nextest pass covers ALL crates including
# OCCT ones. The nextest occt test-group (max-threads=24, env-driven, default 24)
# bounds their concurrency. A bare --workspace pass WITHOUT --exclude is now the CORRECT form.
assert "full-workspace nextest pass does NOT have --exclude (OCCT no longer excluded, task 4451)" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -E 'cargo (test|nextest run) --workspace' | grep -q -- '--exclude'"

echo ""
echo "--- Test 13: nextest --workspace pass present and OCCT not excluded (coverage, task 4451) ---"

assert "plan contains 'cargo (test|nextest run) --workspace' (OCCT folded into the pool, not excluded)" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE 'cargo (test|nextest run) --workspace'"

# -- Test 14: bounded lock-wait exits non-zero with clear message ---------------
echo ""
echo "--- Test 14: REIFY_OCCT_LOCK_WAIT=1 fires within budget, exits non-zero with message ---"

_LOCK14="$(mktemp)"
_ERR14="$(mktemp)"

# Spawn a background holder that acquires slot-1 and holds it for 10s.
# The wrapper uses ${LOCK}.slot-1 (not $LOCK directly), so the holder must
# target the slot file to actually block the wrapper.
( flock -x 9; sleep 10 ) 9>>"${_LOCK14}.slot-1" &
_HOLDER14=$!
# Causal flock-probe barrier (task 5258, PRD merge-gate-health W4b): block until
# the holder actually holds slot-1, instead of a fixed `sleep 0.2` grace that a
# saturated host can outrun (holder unscheduled ⇒ wrapper finds slot FREE ⇒
# acquires instantly ⇒ got 0 instead of the expected exit 75).  Wrapped in
# `assert` so a transient confirm-failure yields a clear FAIL naming the root
# cause instead of tripping `set -e` and aborting the whole suite.
assert "Test 14: background holder confirmed holding slot-1 (causal flock-probe barrier)" \
    occt_wait_until_slot_held "${_LOCK14}.slot-1"

_START14="$(date +%s)"
_EXIT14=0
# REIFY_OCCT_CONCURRENCY=1 pins N=1 so the single holder on slot-1 blocks the wrapper.
# Backstop raised 5→15s: under load the legitimate exit-75 path can take >5s
# (loaded preamble + retry cycle); the old 5s backstop would trip → exit 124 →
# false RED on the exit==75 assert below.  The new 15s backstop gives headroom
# while still acting as a true anti-hang (a genuine hang produces exit≠75).
# (PRD docs/prds/infra-test-wallclock-deflake.md §2/T3 decision D4)
REIFY_OCCT_LOCK="$_LOCK14" REIFY_OCCT_CONCURRENCY=1 REIFY_OCCT_LOCK_WAIT=1 timeout 15 "$WRAPPER" true 2>"$_ERR14" || _EXIT14=$?
_END14="$(date +%s)"
_ELAPSED14=$(( _END14 - _START14 ))

kill "$_HOLDER14" 2>/dev/null || true
wait "$_HOLDER14" 2>/dev/null || true

# Discriminators: exit==75 (EX_TEMPFAIL) and stderr pattern are the NON-VACUOUS
# proof that the deadline logic fired correctly.  The elapsed guard is a generous
# anti-hang only (outer 15s > guard 10s > normal ~1.5s; a true hang still trips
# exit≠75 via the 15s backstop).
assert "Test 14: wrapper exits 75 (EX_TEMPFAIL) when lock-wait limit exceeded (got $_EXIT14)" \
    test "$_EXIT14" -eq 75

assert "Test 14: wrapper exits within 10s (generous anti-hang guard; outer 15s backstop catches true hang) (elapsed=${_ELAPSED14}s)" \
    test "$_ELAPSED14" -le 10 # wallclock:allow — generous anti-hang: discriminated by exit==75 and stderr pattern, not elapsed magnitude

assert "Test 14: stderr mentions 'acquire' and lock-wait duration (1s)" \
    grep -qE 'acquire.*1s|1s.*acquire' "$_ERR14"

rm -f "$_LOCK14" "${_LOCK14}.slot-1" "$_ERR14"

# -- Test 15: post-lock timer fires N seconds AFTER lock acquisition, not after start ---
echo ""
echo "--- Test 15: REIFY_OCCT_TEST_TIMEOUT measured post-lock, not from wrapper start ---"

_LOCK15="$(mktemp)"

# Spawn a holder that holds slot-1 for 6 seconds.
# The wrapper uses ${LOCK}.slot-1 (not $LOCK directly), so the holder must
# target the slot file to actually block the wrapper.
# The causal flock-probe barrier below records _START15 at HOLDER-CONFIRMED
# time, so the `elapsed >= 4` assert measures (remaining_hold − confirm_latency
# + 1s command).  Hold bumped 4s→6s for margin: with a bounded confirm latency
# (≤~2s under load) remaining_hold is ≥4s ⇒ elapsed ≥5s ⇒ truncates to ≥4 with
# ≥1s margin; 6s < LOCK_WAIT(10s) leaves headroom so no spurious exit-75.
( flock -x 9; sleep 6 ) 9>>"${_LOCK15}.slot-1" &
_HOLDER15=$!
# Causal flock-probe barrier (task 5258): block until the holder holds slot-1.
# Wrapped in `assert` so a transient confirm-failure cannot trip `set -e`.
assert "Test 15: background holder confirmed holding slot-1 (causal flock-probe barrier)" \
    occt_wait_until_slot_held "${_LOCK15}.slot-1"

_START15="$(date +%s)"
_EXIT15=0
# REIFY_OCCT_CONCURRENCY=1 pins N=1 so the single holder on slot-1 blocks the wrapper.
REIFY_OCCT_LOCK="$_LOCK15" REIFY_OCCT_CONCURRENCY=1 REIFY_OCCT_LOCK_WAIT=10 REIFY_OCCT_TEST_TIMEOUT=1 \
    "$WRAPPER" sleep 5 || _EXIT15=$?
_END15="$(date +%s)"
_ELAPSED15=$(( _END15 - _START15 ))

kill "$_HOLDER15" 2>/dev/null || true
wait "$_HOLDER15" 2>/dev/null || true
rm -f "$_LOCK15" "${_LOCK15}.slot-1"

# Expected: lock acquired after ~4s, then `timeout 1 sleep 5` kills after 1s → rc=124
# elapsed ≈ 5s total.  Without internal timeout: sleep 5 runs fully → rc=0, elapsed ≈ 9s.
# Lower bound ≥ 4 proves the timer could not have started at wrapper launch (~1s).
# Upper bound ≤ 8 DROPPED: under load the 4s holder can extend beyond 8s before the
# wrapper acquires, making the upper bound a RED-flake.  exit==124 already
# discriminates a broken internal timeout (exit 0 or exit≠124 would be the RED).
# (PRD docs/prds/infra-test-wallclock-deflake.md §2/T3 decision D4)
assert "Test 15: wrapper exits 124 (internal timeout killed the command, got $_EXIT15)" \
    test "$_EXIT15" -eq 124

assert "Test 15: elapsed ≥ 4s — timer started post-lock, not at wrapper launch (elapsed=${_ELAPSED15}s)" \
    test "$_ELAPSED15" -ge 4

# -- Test 16: wrapper logs wait duration on lock acquisition -------------------
echo ""
echo "--- Test 16: wrapper emits INFO log line with 'acquired' and numeric duration on stderr ---"

_LOCK16="$(mktemp)"
_ERR16="$(mktemp)"

_EXIT16=0
REIFY_OCCT_LOCK="$_LOCK16" "$WRAPPER" true 2>"$_ERR16" >/dev/null || _EXIT16=$?

assert "Test 16: wrapper exits 0 (sanity check, got $_EXIT16)" \
    test "$_EXIT16" -eq 0

assert "Test 16: stderr contains log line with 'acquired', 'OCCT lock', and numeric duration (Ns)" \
    grep -qiE 'acquired.*OCCT.*lock.*[0-9]+s' "$_ERR16"

rm -f "$_LOCK16" "$_ERR16"

# -- Test 17: no gated invocations in plan; both passes use unified 60m outer timeout --
echo ""
echo "--- Test 17: no REIFY_OCCT_TEST_TIMEOUT in plan; both nextest passes use unified 60m timeout (η/4521 floor 798.9s × 4.5 → 60m, task 4520) ---"

# Task 4451 drops the gated passes: REIFY_OCCT_TEST_TIMEOUT= no longer appears.
# Both nextest passes (debug --workspace and release --release) use the single unified
# 60m outer timeout re-derived from η/4521's authoritative measured floor: 798.9s
# (worst-observed cold real-load) × 4.5 production-weighted margin = ceil(3595.05s) →
# rounded up to 60m (3600s). Bound (3600s) > floor (798.9s) by construction.
# Asserted below for BOTH the debug pass (Test 17) and release pass (Test 17b).
# See verify.sh add_test_passes and docs/prds/jobserver-merge-priority-balancer.acceptance-report.md.
# Task 4862 revert: build+test are one unbroken slot-held block; no --no-run compile pass
# exists outside the slot. Assertions target the single combined pass directly.
assert "Test 17: REIFY_OCCT_TEST_TIMEOUT= does NOT appear in the plan (no gated pass, task 4451)" \
    bash -c "[ \"\$(printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -oF 'REIFY_OCCT_TEST_TIMEOUT=' | wc -l | tr -d ' ')\" -eq 0 ]"

assert "Test 17: no ./scripts/cargo-test-occt-gated in the plan at all (folded into nextest, task 4451)" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q './scripts/cargo-test-occt-gated'"

assert "Test 17: NO 'cargo nextest run ... --no-run' line in the plan (task 4862 revert: build inside slot)" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'cargo nextest run.*--no-run'"

assert "Test 17: debug full-workspace nextest pass uses unified 60m outer timeout (η/4521 floor 798.9s × 4.5, task 4520; build inside slot, task 4862)" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE 'timeout --kill-after=60 60m .*cargo nextest run --workspace'"

# -- Test 17b: release pass uses the cold-aware 90m release inner timeout (task 5382) ------
echo ""
echo "--- Test 17b: release nextest pass uses cold-aware 90m release inner timeout (task 5382; debug stays 60m) ---"

# The release pass (--release) carries its OWN cold-aware inner budget, DECOUPLED from the
# debug pass. The prior unified 60m (task 4520/ζ′) was derived from η/4521's floor (798.9s)
# measured cold-TARGET / WARM-sccache — it NEVER included a cold-SCCACHE native-kernel relink
# of OCCT/OpenVDB/gmsh/manifold, which pushes the combined release build+exec pass past 60m
# and SIGTERM'd it at the inner timeout (esc-5370-2, false "integration_skew"). Task 5382
# reintroduces a release-specific inner budget REIFY_VERIFY_TEST_TIMEOUT_RELEASE (default 90m);
# the debug --workspace pass keeps REIFY_VERIFY_TEST_TIMEOUT (60m, asserted by Test 17).
# The merge OUTER wall that must not preempt these two inner budgets is
# merge_verify_cold_command_timeout_secs (sibling task 5383) — its comment block in
# dark-factory-orchestrator.yaml is the SINGLE SOURCE for what that relationship does and
# does NOT model, and T10 below mechanises it. Deliberately no arithmetic restated here:
# the duplicated copy drifted once already (esc-5382-1 amendment review). The --release
# token in the regex discriminates this assertion from Test 17's --workspace assertion.
assert "Test 17b: release nextest pass uses cold-aware 90m release inner timeout (task 5382; not 60m)" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE 'timeout --kill-after=60 90m .*cargo nextest run .*--release'"

# -- Tests T1–T7: host-relative compile timeout knobs (task 4621) ---------------
# Pure hermetic plan-string assertions via `--print-plan` oracle + grep.
# Each test runs verify.sh with a specific REIFY_VERIFY_*_TIMEOUT env and
# checks that the rendered timeout token in the plan output matches.
#
# T1–T3: REIFY_VERIFY_TEST_TIMEOUT (debug/base nextest budget, default 60m)
# T4–T5: REIFY_VERIFY_CLIPPY_TIMEOUT (clippy+gui-feature check budget, default 45m)
# T6–T7: REIFY_VERIFY_CHECK_TIMEOUT (cargo check budget, default 30m)
# T8–T9: REIFY_VERIFY_TEST_TIMEOUT_RELEASE (release-pass inner budget, default 90m; task 5382)
# T10:   merge OUTER wall ≥ debug_inner + release_inner relationship guard (task 5382)
#
# Task 4621 knobs: RED (T1, T4, T6) until step-6 impl; GREEN guards (T2, T3, T5, T7).
# Task 5382: T1 (now decoupling), Test 17b, T2 (release token), T8, T9 are RED until the
# release knob lands; T10 is a GREEN relationship guard (sibling 5383 already sized the
# outer wall to 10800 ≥ 9000).
echo ""
echo "--- Tests T1–T7 (task 4621): host-relative compile timeout knobs ---"

# T1 (updated for task 5382 DECOUPLING): REIFY_VERIFY_TEST_TIMEOUT=95m drives ONLY the DEBUG
#     (--workspace) nextest pass → `timeout --kill-after=60 95m`, while the RELEASE (--release)
#     pass keeps its OWN default 90m (REIFY_VERIFY_TEST_TIMEOUT_RELEASE explicitly unset, so it
#     falls to its own default regardless of ambient env). This proves the base knob no longer
#     drives the release pass. 95m ≠ the release default 90m so the two tokens are
#     distinguishable. RED against current code: the release pass currently follows the unified
#     base knob → would render 95m.
_T1_ERR="$(mktemp)"
_T1_PLAN="$(env -u REIFY_VERIFY_TEST_TIMEOUT_RELEASE REIFY_VERIFY_TEST_TIMEOUT=95m \
    bash "$REPO_ROOT/scripts/verify.sh" test \
    --profile both --scope all --print-plan 2>"$_T1_ERR" | grep -v '^#')"
export _T1_PLAN
assert "T1: REIFY_VERIFY_TEST_TIMEOUT=95m: debug nextest pass uses 95m outer timeout" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 95m .*cargo nextest run --workspace' "$_T1_PLAN" "$_T1_ERR"
assert "T1: REIFY_VERIFY_TEST_TIMEOUT=95m: release nextest pass keeps its OWN default 90m (decoupled, task 5382)" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 90m .*cargo nextest run .*--release' "$_T1_PLAN" "$_T1_ERR"
rm -f "$_T1_ERR"

# T2: REIFY_VERIFY_TEST_TIMEOUT unset → debug pass uses 60m; release pass uses its OWN
#     default 90m (task 5382 decoupled the release inner budget from the base knob).
#     Guard: Tests 17/17b already check this via TEST_PLAN_SEGS; this is a direct re-check.
assert "T2: REIFY_VERIFY_TEST_TIMEOUT unset: debug nextest pass uses default 60m" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE 'timeout --kill-after=60 60m .*cargo nextest run --workspace'"
assert "T2: REIFY_VERIFY_TEST_TIMEOUT unset: release nextest pass uses its own default 90m (task 5382)" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE 'timeout --kill-after=60 90m .*cargo nextest run .*--release'"

# T3: Malformed REIFY_VERIFY_TEST_TIMEOUT=banana → falls back to 60m (validation guard).
_T3_ERR="$(mktemp)"
_T3_PLAN="$(REIFY_VERIFY_TEST_TIMEOUT=banana bash "$REPO_ROOT/scripts/verify.sh" test \
    --profile both --scope all --print-plan 2>"$_T3_ERR" | grep -v '^#')"
export _T3_PLAN
assert "T3: REIFY_VERIFY_TEST_TIMEOUT=banana (malformed): falls back to 60m default" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 60m .*cargo nextest run --workspace' "$_T3_PLAN" "$_T3_ERR"
rm -f "$_T3_ERR"

# T4: REIFY_VERIFY_CLIPPY_TIMEOUT=70m → cargo clippy AND gui-feature cargo check
#     both render `timeout --kill-after=60 70m` in verify.sh lint --print-plan.
#     RED: current code always emits 45m.
_T4_ERR="$(mktemp)"
_T4_PLAN="$(REIFY_VERIFY_CLIPPY_TIMEOUT=70m bash "$REPO_ROOT/scripts/verify.sh" lint \
    --print-plan 2>"$_T4_ERR" | grep -v '^#')"
export _T4_PLAN
assert "T4: REIFY_VERIFY_CLIPPY_TIMEOUT=70m: clippy pass uses 70m outer timeout" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 70m .*cargo clippy' "$_T4_PLAN" "$_T4_ERR"
assert "T4: REIFY_VERIFY_CLIPPY_TIMEOUT=70m: gui-feature cargo check uses 70m outer timeout" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 70m .*cargo check -p reify-gui' "$_T4_PLAN" "$_T4_ERR"
rm -f "$_T4_ERR"

# T5: REIFY_VERIFY_CLIPPY_TIMEOUT unset → clippy uses 45m (workstation default preserved).
_T5_ERR="$(mktemp)"
_T5_PLAN="$(env -u REIFY_VERIFY_CLIPPY_TIMEOUT bash "$REPO_ROOT/scripts/verify.sh" lint \
    --print-plan 2>"$_T5_ERR" | grep -v '^#')"
export _T5_PLAN
assert "T5: REIFY_VERIFY_CLIPPY_TIMEOUT unset: clippy pass uses default 45m" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 45m .*cargo clippy' "$_T5_PLAN" "$_T5_ERR"
rm -f "$_T5_ERR"

# T6: REIFY_VERIFY_CHECK_TIMEOUT=50m → cargo check --workspace --tests renders
#     `timeout --kill-after=60 50m` in verify.sh typecheck --print-plan.
#     RED: current code always emits 30m.
_T6_ERR="$(mktemp)"
_T6_PLAN="$(REIFY_VERIFY_CHECK_TIMEOUT=50m bash "$REPO_ROOT/scripts/verify.sh" typecheck \
    --print-plan 2>"$_T6_ERR" | grep -v '^#')"
export _T6_PLAN
assert "T6: REIFY_VERIFY_CHECK_TIMEOUT=50m: cargo check --workspace --tests uses 50m outer timeout" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 50m .*cargo check --workspace' "$_T6_PLAN" "$_T6_ERR"
rm -f "$_T6_ERR"

# T7: REIFY_VERIFY_CHECK_TIMEOUT unset → check uses 30m (workstation default preserved).
_T7_ERR="$(mktemp)"
_T7_PLAN="$(env -u REIFY_VERIFY_CHECK_TIMEOUT bash "$REPO_ROOT/scripts/verify.sh" typecheck \
    --print-plan 2>"$_T7_ERR" | grep -v '^#')"
export _T7_PLAN
assert "T7: REIFY_VERIFY_CHECK_TIMEOUT unset: cargo check --workspace --tests uses default 30m" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 30m .*cargo check --workspace' "$_T7_PLAN" "$_T7_ERR"
rm -f "$_T7_ERR"

# -- Tests T8–T10 (task 5382): release-pass cold-aware inner timeout knob --------
# The release nextest pass carries its OWN inner budget REIFY_VERIFY_TEST_TIMEOUT_RELEASE
# (default 90m), decoupled from the debug/base REIFY_VERIFY_TEST_TIMEOUT (60m). The 60m
# unified budget was measured cold-TARGET / WARM-sccache and SIGTERM'd the release pass on
# a cold-SCCACHE native-kernel relink (esc-5370-2). T8/T9 exercise the new knob via the
# same --print-plan oracle as T1–T7; T10 is a GREEN relationship guard tying the two inner
# budgets to sibling task 5383's merge OUTER wall.
echo ""
echo "--- Tests T8–T10 (task 5382): release-pass cold-aware inner timeout knob ---"

# T8: REIFY_VERIFY_TEST_TIMEOUT_RELEASE=100m drives ONLY the RELEASE (--release) nextest
#     pass → `timeout --kill-after=60 100m`, while the DEBUG (--workspace) pass keeps its
#     default 60m (REIFY_VERIFY_TEST_TIMEOUT explicitly unset). Proves the release knob is
#     release-only. RED against current code: no release knob exists, so release renders 60m.
_T8_ERR="$(mktemp)"
_T8_PLAN="$(env -u REIFY_VERIFY_TEST_TIMEOUT REIFY_VERIFY_TEST_TIMEOUT_RELEASE=100m \
    bash "$REPO_ROOT/scripts/verify.sh" test \
    --profile both --scope all --print-plan 2>"$_T8_ERR" | grep -v '^#')"
export _T8_PLAN
assert "T8: REIFY_VERIFY_TEST_TIMEOUT_RELEASE=100m: release nextest pass uses 100m outer timeout" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 100m .*cargo nextest run .*--release' "$_T8_PLAN" "$_T8_ERR"
assert "T8: REIFY_VERIFY_TEST_TIMEOUT_RELEASE=100m: debug nextest pass stays default 60m (release knob is release-only)" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 60m .*cargo nextest run --workspace' "$_T8_PLAN" "$_T8_ERR"
rm -f "$_T8_ERR"

# T9: malformed REIFY_VERIFY_TEST_TIMEOUT_RELEASE=banana → the release pass falls back to its
#     90m default (mirrors T3's malformed-fallback guard through the shared
#     _resolve_timeout_knob validator). RED against current code: release renders 60m.
_T9_ERR="$(mktemp)"
_T9_PLAN="$(env -u REIFY_VERIFY_TEST_TIMEOUT REIFY_VERIFY_TEST_TIMEOUT_RELEASE=banana \
    bash "$REPO_ROOT/scripts/verify.sh" test \
    --profile both --scope all --print-plan 2>"$_T9_ERR" | grep -v '^#')"
export _T9_PLAN
assert "T9: REIFY_VERIFY_TEST_TIMEOUT_RELEASE=banana (malformed): release pass falls back to 90m default" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 90m .*cargo nextest run .*--release' "$_T9_PLAN" "$_T9_ERR"
rm -f "$_T9_ERR"

# T10: GREEN relationship guard — the merge OUTER wall (merge_verify_cold_command_timeout_secs,
#      sibling task 5383) must be ≥ the sum of the two TERMINAL nextest inner budgets (debug
#      60m=3600s + release 90m=5400s = 9000s), so the outer merge wall can never SIGTERM the
#      release native-kernel pass before its own inner budget is reached.
#      THIS IS NOT A WORST-CASE SUM. The wall bounds EXPECTED cold merge-verify duration; the
#      full set of inner ceilings for the command it bounds sums to ~2x the wall and always has
#      (clippy 45m x2, check 30m, the release pre-builds, ...). What is asserted here is a
#      NECESSARY-CONDITION lower bound over a deliberately chosen SUBSET — the two passes that
#      run back-to-back at the END of the plan, where a wall below their sum would make the
#      release inner budget unreachable BY CONSTRUCTION. The release PRE-BUILD is deliberately
#      excluded: its cost is transferred FROM the release nextest pass (which then runs against
#      a warm release cone), so folding it in would double-count the same cold native-kernel
#      compile — and an earlier revision that did fold it in produced a bogus "300s margin"
#      that read as a binding cap on REIFY_VERIFY_PREBUILD_TIMEOUT (esc-5382-1 amendment
#      review). The pre-build's own budget is asserted by T11–T13 instead.
#      The authoritative prose for this model lives in the merge_verify_cold_command_timeout_secs
#      comment block in dark-factory-orchestrator.yaml — keep this comment a summary of it.
#      DISTINCT from 5383's test_merge_verify_cold_outer_timeout.sh (which pins the outer value
#      ≥ 10800 as an absolute floor), so it is not lockstep duplication.
#      WALLCLOCK-GUARD SAFETY: the comparison uses the lower-bound `-ge` (never -le/-lt) and the
#      operand vars carry NO ELAPSED/_S/_MS/_NS/SECONDS suffix, so
#      tests/infra/test_no_new_wallclock_upper_bounds.sh does not fire.
echo ""
echo "--- Test T10 (task 5382): merge outer wall >= debug_inner + release_inner (relationship guard) ---"

_T10_VERIFY_SH="$REPO_ROOT/scripts/verify.sh"
_T10_ORCH_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"

# Extract the debug inner default (minutes) from the base-knob resolution line. The trailing
# space after REIFY_VERIFY_TEST_TIMEOUT ensures this does NOT match the _RELEASE line.
# `|| true` keeps a NO-MATCH extraction from tripping `set -o pipefail`/`set -e` (the release
# knob is absent until step-2 lands); emptiness is caught by the `test -n` asserts below.
_debug_inner_min="$(grep -oE '_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT [0-9]+m' "$_T10_VERIFY_SH" | grep -oE '[0-9]+' | head -n1 || true)"
# Extract the release inner default (minutes) from the release-knob resolution line.
_release_inner_min="$(grep -oE '_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT_RELEASE [0-9]+m' "$_T10_VERIFY_SH" | grep -oE '[0-9]+' | head -n1 || true)"
# NB the merge-path RELEASE pre-build budget (REIFY_VERIFY_PREBUILD_TIMEOUT) is intentionally
# NOT extracted or summed here — see the subset rationale in this test's header comment.
# Extract the merge outer wall (seconds) from the yaml key.
_outer_budget="$(grep -oE '^merge_verify_cold_command_timeout_secs:[[:space:]]*[0-9]+' "$_T10_ORCH_YAML" | grep -oE '[0-9]+' | head -n1 || true)"

assert "T10: debug inner default extracted from verify.sh (non-empty minutes)" \
    test -n "$_debug_inner_min"
assert "T10: release inner default extracted from verify.sh (non-empty minutes)" \
    test -n "$_release_inner_min"
assert "T10: merge outer wall extracted from dark-factory-orchestrator.yaml (non-empty seconds)" \
    test -n "$_outer_budget"

# Default any absent extraction to 0 so the arithmetic below cannot abort the suite under
# set -e during the RED window (the release knob is absent until the step-2 impl lands).
# Non-emptiness is asserted above; this is purely an anti-abort backstop.
_inner_budget_total=$(( ${_debug_inner_min:-0} * 60 + ${_release_inner_min:-0} * 60 ))
_outer_budget="${_outer_budget:-0}"

assert "T10: merge outer wall (${_outer_budget}s) >= debug_inner + release_inner (${_inner_budget_total}s from ${_debug_inner_min:-?}m + ${_release_inner_min:-?}m) so the wall cannot preempt either terminal nextest pass before its own inner budget is reached" \
    test "$_outer_budget" -ge "$_inner_budget_total"

# -- Tests T11–T13 (task 5382, esc-5382-1): merge-path RELEASE pre-build budget -----
# The two `cargo build --release -p {reify-audit,reify-cli}` pre-builds render ONLY on the
# merge/background path and only when not already inside an infra suite, so the plan oracle
# needs DF_VERIFY_ROLE=merge AND `env -u REIFY_INFRA_SUITE_ACTIVE` (this suite itself sets
# that variable, which would otherwise suppress the whole block and make T11–T13 vacuous).
# esc-5382-1: reify-cli was SIGTERM'd mid-`Compiling reify-runtime` by the former fixed 10m
# ceiling on a cold merge lane, with zero failing assertions — same class as esc-5370-2.
# REIFY_RELEASE_DELTA_SKIP is unset alongside REIFY_INFRA_SUITE_ACTIVE in each capture below:
# that knob is consulted ONLY when DF_VERIFY_ROLE=merge (scripts/verify.sh, the
# _RELEASE_DELTA_SKIP decision block), and when it is 1 on a delta-clean tree the release
# nextest pass is replaced by the frozen `echo 'RELEASE-PASS: skipped (delta-clean)'` marker —
# which would spuriously FAIL T12's "release nextest pass stays default 90m" assertion. It is
# default-OFF today but is slated for activation in the orchestrator's verify_env by the
# sibling sweep task (#5280), so pinning the plan shape here is a live concern, not a
# hypothetical. T1/T2/T8/T9 need no such pin: they do not set the merge role.
echo ""
echo "--- Tests T11–T13 (task 5382): merge-path release pre-build cold-aware timeout ---"

# T11: default → both release pre-builds render the 45m pre-build budget (was a fixed 10m).
_T11_ERR="$(mktemp)"
_T11_PLAN="$(env -u REIFY_INFRA_SUITE_ACTIVE -u REIFY_RELEASE_DELTA_SKIP -u REIFY_VERIFY_PREBUILD_TIMEOUT \
    DF_VERIFY_ROLE=merge \
    bash "$REPO_ROOT/scripts/verify.sh" test \
    --profile both --scope all --print-plan 2>"$_T11_ERR" | grep -v '^#')"
export _T11_PLAN
assert "T11: default: reify-cli release pre-build uses the 45m pre-build budget (not the former fixed 10m)" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 45m .*cargo build --release -p reify-cli' "$_T11_PLAN" "$_T11_ERR"
assert "T11: default: reify-audit release pre-build uses the same 45m pre-build budget" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 45m .*cargo build --release -p reify-audit' "$_T11_PLAN" "$_T11_ERR"
rm -f "$_T11_ERR"

# T12: REIFY_VERIFY_PREBUILD_TIMEOUT=40m drives the pre-builds ONLY — the two nextest passes
#      keep their own 60m/90m defaults. Proves the pre-build knob is pre-build-scoped.
_T12_ERR="$(mktemp)"
_T12_PLAN="$(env -u REIFY_INFRA_SUITE_ACTIVE -u REIFY_RELEASE_DELTA_SKIP \
    -u REIFY_VERIFY_TEST_TIMEOUT -u REIFY_VERIFY_TEST_TIMEOUT_RELEASE \
    DF_VERIFY_ROLE=merge REIFY_VERIFY_PREBUILD_TIMEOUT=40m \
    bash "$REPO_ROOT/scripts/verify.sh" test \
    --profile both --scope all --print-plan 2>"$_T12_ERR" | grep -v '^#')"
export _T12_PLAN
assert "T12: REIFY_VERIFY_PREBUILD_TIMEOUT=40m: reify-cli pre-build uses 40m" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 40m .*cargo build --release -p reify-cli' "$_T12_PLAN" "$_T12_ERR"
assert "T12: REIFY_VERIFY_PREBUILD_TIMEOUT=40m: debug nextest pass stays default 60m (pre-build knob is pre-build-only)" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 60m .*cargo nextest run --workspace' "$_T12_PLAN" "$_T12_ERR"
assert "T12: REIFY_VERIFY_PREBUILD_TIMEOUT=40m: release nextest pass stays default 90m (pre-build knob is pre-build-only)" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 90m .*cargo nextest run .*--release' "$_T12_PLAN" "$_T12_ERR"
rm -f "$_T12_ERR"

# T13: malformed REIFY_VERIFY_PREBUILD_TIMEOUT=banana → falls back to the 45m default via the
#      shared _resolve_timeout_knob validator (mirrors T3/T9's malformed-fallback guard).
_T13_ERR="$(mktemp)"
_T13_PLAN="$(env -u REIFY_INFRA_SUITE_ACTIVE -u REIFY_RELEASE_DELTA_SKIP \
    DF_VERIFY_ROLE=merge REIFY_VERIFY_PREBUILD_TIMEOUT=banana \
    bash "$REPO_ROOT/scripts/verify.sh" test \
    --profile both --scope all --print-plan 2>"$_T13_ERR" | grep -v '^#')"
export _T13_PLAN
assert "T13: REIFY_VERIFY_PREBUILD_TIMEOUT=banana (malformed): pre-builds fall back to the 45m default" \
    occt_plan_grep_or_dump 'timeout --kill-after=60 45m .*cargo build --release -p reify-cli' "$_T13_PLAN" "$_T13_ERR"
rm -f "$_T13_ERR"

# -- Test 18: wrapper does not leak the lock fd into background daemons --------
# Regression test for the 2026-04-20 merge-queue wedge: sccache (spawned as a
# detached daemon by cargo via RUSTC_WRAPPER) inherited FD 9 and outlived
# cargo, pinning the flock forever.  The wrapper must run its child with FD 9
# closed so no descendant can leak the open file description.
echo ""
echo "--- Test 18: wrapper closes fd 9 on the child; daemons do not leak the lock ---"

_LOCK18="$(mktemp)"
_DAEMON_PID_FILE="$(mktemp)"
_EXIT18=0

# Run the wrapper on a command that forks a detached daemon that survives the
# wrapper's exit.  setsid + & + disown produces exactly the sccache-style
# inheritance pattern: the daemon's only link to the lock fd is inheritance
# from its parent, so a correctly-patched wrapper closes fd 9 before the
# child exec and the daemon starts life without fd 9.
# REIFY_OCCT_CONCURRENCY=1 pins N=1 so the slot file is ${_LOCK18}.slot-1.
REIFY_OCCT_LOCK="$_LOCK18" REIFY_OCCT_CONCURRENCY=1 "$WRAPPER" bash -c '
    setsid bash -c "sleep 30" </dev/null >/dev/null 2>&1 &
    echo $! > "'"$_DAEMON_PID_FILE"'"
    disown
    exit 0
' || _EXIT18=$?

_DAEMON_PID="$(cat "$_DAEMON_PID_FILE" 2>/dev/null || echo "")"

# The daemon must still be alive (otherwise the test is vacuous — we'd be
# verifying the lock is free because nothing inherited it at all).
assert "Test 18: daemon spawned inside wrapper is still alive after wrapper exits (pid=$_DAEMON_PID)" \
    bash -c "[ -n '$_DAEMON_PID' ] && kill -0 '$_DAEMON_PID' 2>/dev/null"

# After the wrapper returns, the slot file flock must be free: a non-blocking
# flock attempt on ${_LOCK18}.slot-1 must succeed immediately.  If fd 9 had
# leaked into the surviving daemon, this would fail (slot lock still held).
_LOCK_FREE18=1
( flock -n -x 9 || exit 1 ) 9>>"${_LOCK18}.slot-1" || _LOCK_FREE18=0

assert "Test 18: slot-1 lock released after wrapper exit despite surviving daemon (fd 9 not inherited)" \
    test "$_LOCK_FREE18" -eq 1

# Cleanup the surviving daemon.
if [ -n "$_DAEMON_PID" ]; then
    kill "$_DAEMON_PID" 2>/dev/null || true
fi
rm -f "$_LOCK18" "${_LOCK18}.slot-1" "$_DAEMON_PID_FILE"

assert "Test 18: wrapper exited 0 on successful spawn (got $_EXIT18)" \
    test "$_EXIT18" -eq 0

# -- Test 19: REIFY_OCCT_CONCURRENCY=2 runs two invocations in parallel ----------
# R-technique (PRD docs/prds/infra-test-wallclock-deflake.md §2/T3): causal
# event-log proof that ≥2 slots were held simultaneously.  Replaces the vacuous
# <2000ms wall-clock ceiling (fully-serial N=1 also finishes <2000ms, so it
# could not distinguish parallel from serial — the assertion's own comment
# admitted this; see occt_flock_gate_lib.sh COVERAGE GAP note).
#
# Spawn-skew mitigation: touch-file barrier (each wrapper touches ready-$$ after
# ACQUIRE; test waits for BOTH ready files before touching go).  This guarantees
# concurrent holding regardless of spawn timing.  Under an N→1 regression only
# 1 ready file appears; bounded wait elapses; go is touched; both run serially →
# event log A/R/A/R → max=1 → clean RED (no infinite hang).
echo ""
echo "--- Test 19: REIFY_OCCT_CONCURRENCY=2 allows two concurrent invocations to run in parallel ---"

_LOCK19="$(mktemp)"
_LOG19="$(mktemp)"
_BARRIER19="$(mktemp -d)"

# Each wrapper: acquire slot → emit ACQUIRE to log → touch ready-$$ → wait
# (bounded ~20s) for go signal → exit → emit RELEASE to log.
REIFY_OCCT_LOCK="$_LOCK19" REIFY_OCCT_CONCURRENCY=2 \
    REIFY_SLOT_EVENT_LOG="$_LOG19" \
    "$WRAPPER" bash -c '
        touch "'"$_BARRIER19"'/ready-$$"
        _w=0; while [ ! -f "'"$_BARRIER19"'/go" ] && [ "$_w" -lt 100 ]; do
            sleep 0.2; _w=$(( _w + 1 )); done
    ' &
_PID19A=$!
REIFY_OCCT_LOCK="$_LOCK19" REIFY_OCCT_CONCURRENCY=2 \
    REIFY_SLOT_EVENT_LOG="$_LOG19" \
    "$WRAPPER" bash -c '
        touch "'"$_BARRIER19"'/ready-$$"
        _w=0; while [ ! -f "'"$_BARRIER19"'/go" ] && [ "$_w" -lt 100 ]; do
            sleep 0.2; _w=$(( _w + 1 )); done
    ' &
_PID19B=$!

# Wait (bounded ~20s) for BOTH ready files — proves both ACQUIREd concurrently.
# Under N→1 regression only 1 ready file appears; wait elapses → go → serial.
_w19=0
while [ "$(ls "$_BARRIER19"/ready-* 2>/dev/null | wc -l)" -lt 2 ] && [ "$_w19" -lt 100 ]; do
    sleep 0.2; _w19=$(( _w19 + 1 ))
done
touch "$_BARRIER19/go"
wait "$_PID19A" "$_PID19B"

_MAX19="$(occt_max_concurrent_holders "$_LOG19")"
rm -rf "$_BARRIER19"
rm -f "$_LOCK19" "${_LOCK19}.slot-1" "${_LOCK19}.slot-2" "$_LOG19"

assert "Test 19: ≥2 slots held simultaneously with N=2 (causal R-proof; max_concurrent=${_MAX19})" \
    test "$_MAX19" -ge 2

# -- Test 20: N=2, three concurrent invocations serializes the third ----------
# With only 2 slots, a third concurrent wrapper invocation must wait until one
# slot is released. Measured elapsed must be >= 700ms (two parallel rounds of
# ~400ms — proves serialization) and <= 2000ms (load-tolerant sanity ceiling,
# raised 1200->2000 per esc-3939-94: verify-pipeline load inflated the
# serialized 3rd invocation to 1473ms in one run with no logic defect).
# At 2000ms the upper bound no longer discriminates N=2 (~800ms) from fully-serial
# N=1 (~1200ms); the >=700ms lower bound guards against under-serialization only.
# COVERAGE GAP (accepted tradeoff per esc-3939-94): a fully-serial regression
# (N->1) produces ~1200ms for three invocations — inside [700,2000], undetected.
# Test 19 does NOT guard this: two fully-serial invocations complete in ~800ms,
# below Test 19's <900ms threshold (both pass under a fully-serial regression).
# This validates that the acquire-loop bounds N strictly (not ">=N" slots).
echo ""
echo "--- Test 20: REIFY_OCCT_CONCURRENCY=2 serializes the 3rd invocation when both slots are busy ---"

_LOCK20="$(mktemp)"
_START20_NS="$(date +%s%N)"

# Spawn three concurrent invocations each sleeping 0.4s with N=2 slots.
REIFY_OCCT_LOCK="$_LOCK20" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c 'sleep 0.4' &
_PID20A=$!
REIFY_OCCT_LOCK="$_LOCK20" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c 'sleep 0.4' &
_PID20B=$!
REIFY_OCCT_LOCK="$_LOCK20" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c 'sleep 0.4' &
_PID20C=$!
wait "$_PID20A" "$_PID20B" "$_PID20C"

_END20_NS="$(date +%s%N)"
_ELAPSED20_MS=$(( (_END20_NS - _START20_NS) / 1000000 ))

rm -f "$_LOCK20" "${_LOCK20}.slot-1" "${_LOCK20}.slot-2"

# Two slots: two run in parallel (~400ms), third waits and runs (~800ms total).
# Lower bound >= 700ms proves the third was serialized (all-parallel ~400ms).
# Upper bound <= 2000ms is a load-tolerant sanity ceiling (esc-3939-94).
assert "Test 20: 3 invocations with N=2 complete in [${OCCT_SERIAL3_N2_LOW_MS},${OCCT_SERIAL3_N2_HIGH_MS}]ms — 3rd is serialized (got ${_ELAPSED20_MS}ms)" \
    occt_serial3_n2_within_bounds "$_ELAPSED20_MS"

# -- Test 21: REIFY_OCCT_MAX_CONCURRENCY sets N when CONCURRENCY is unset ------
# With REIFY_OCCT_CONCURRENCY unset, N falls back to REIFY_OCCT_MAX_CONCURRENCY.
# Sub-test A: two concurrent wrappers → R-proof ≥2 slots simultaneously held
#   (causal event-log, same technique as Test 19; replaces vacuous <2000ms ceiling).
# Sub-test B: three concurrent wrappers → third serialized ([700,5000]ms,
#   load-tolerant ceiling per esc-3939-94; raised 2000→5000 after 3317ms observed
#   under task/3443 load; shared with Test 20 via occt_flock_gate_lib.sh).
#
# Historical note: a prior implementation auto-detected N as
# clamp(nproc - load_1m_int, 1, MAX_CAP) and was retired (esc-4000-39, 2026-05-28)
# because the load-based reduction created positive-feedback collapse to N=1
# under concurrent worktree load.  The MAX_CONCURRENCY env var is now N itself
# when CONCURRENCY is unset.
echo ""
echo "--- Test 21: REIFY_OCCT_MAX_CONCURRENCY=2 sets N=2 — 2 parallel, 3rd serialized ---"

_LOCK21A="$(mktemp)"
_LOG21A="$(mktemp)"
_BARRIER21A="$(mktemp -d)"
_LOCK21B="$(mktemp)"

# Sub-test A: 2 invocations with MAX_CONCURRENCY=2 → R-proof ≥2 slots simultaneously.
# Barrier: each wrapper touches ready-$$ after ACQUIRE, waits bounded for go signal.
# Test waits for both ready files (proves concurrent holding) then touches go.
REIFY_OCCT_MAX_CONCURRENCY=2 REIFY_OCCT_LOCK="$_LOCK21A" \
    REIFY_SLOT_EVENT_LOG="$_LOG21A" \
    "$WRAPPER" bash -c '
        touch "'"$_BARRIER21A"'/ready-$$"
        _w=0; while [ ! -f "'"$_BARRIER21A"'/go" ] && [ "$_w" -lt 100 ]; do
            sleep 0.2; _w=$(( _w + 1 )); done
    ' &
_PID21A1=$!
REIFY_OCCT_MAX_CONCURRENCY=2 REIFY_OCCT_LOCK="$_LOCK21A" \
    REIFY_SLOT_EVENT_LOG="$_LOG21A" \
    "$WRAPPER" bash -c '
        touch "'"$_BARRIER21A"'/ready-$$"
        _w=0; while [ ! -f "'"$_BARRIER21A"'/go" ] && [ "$_w" -lt 100 ]; do
            sleep 0.2; _w=$(( _w + 1 )); done
    ' &
_PID21A2=$!
_w21a=0
while [ "$(ls "$_BARRIER21A"/ready-* 2>/dev/null | wc -l)" -lt 2 ] && [ "$_w21a" -lt 100 ]; do
    sleep 0.2; _w21a=$(( _w21a + 1 ))
done
touch "$_BARRIER21A/go"
wait "$_PID21A1" "$_PID21A2"
_MAX21A="$(occt_max_concurrent_holders "$_LOG21A")"
rm -rf "$_BARRIER21A"
rm -f "$_LOCK21A" "${_LOCK21A}.slot-1" "${_LOCK21A}.slot-2" "$_LOG21A"

# Sub-test B: 3 invocations with MAX_CONCURRENCY=2 → third must wait.
_START21B_NS="$(date +%s%N)"
REIFY_OCCT_MAX_CONCURRENCY=2 REIFY_OCCT_LOCK="$_LOCK21B" \
    "$WRAPPER" bash -c 'sleep 0.4' &
_PID21B1=$!
REIFY_OCCT_MAX_CONCURRENCY=2 REIFY_OCCT_LOCK="$_LOCK21B" \
    "$WRAPPER" bash -c 'sleep 0.4' &
_PID21B2=$!
REIFY_OCCT_MAX_CONCURRENCY=2 REIFY_OCCT_LOCK="$_LOCK21B" \
    "$WRAPPER" bash -c 'sleep 0.4' &
_PID21B3=$!
wait "$_PID21B1" "$_PID21B2" "$_PID21B3"
_END21B_NS="$(date +%s%N)"
_ELAPSED21B_MS=$(( (_END21B_NS - _START21B_NS) / 1000000 ))
rm -f "$_LOCK21B" "${_LOCK21B}.slot-1" "${_LOCK21B}.slot-2"

assert "Test 21A: ≥2 slots held simultaneously with MAX_CONCURRENCY=2 (causal R-proof; max_concurrent=${_MAX21A})" \
    test "$_MAX21A" -ge 2

assert "Test 21B: 3 invocations with MAX_CONCURRENCY=2 have 3rd serialized ([${OCCT_SERIAL3_N2_LOW_MS},${OCCT_SERIAL3_N2_HIGH_MS}]ms, got ${_ELAPSED21B_MS}ms)" \
    occt_serial3_n2_within_bounds "$_ELAPSED21B_MS"

# -- Test 22: LOCK_WAIT bound fires when ALL N slots are externally held ------
# Mirrors Test 14's pattern but with N=2 (full contention across all slots).
# Spawn two background flock holders each pinning one slot file for 10s;
# invoke wrapper with REIFY_OCCT_CONCURRENCY=2 and REIFY_OCCT_LOCK_WAIT=1.
# The wrapper must exit 75 (EX_TEMPFAIL) within <= 5s (1s wait + one 0.5s
# retry cycle, plus slack for integer-second `date +%s` truncation and
# verify-pipeline load), confirming the deadline is checked after ALL N
# slots fail on a given pass — not only after a single slot fails.
echo ""
echo "--- Test 22: LOCK_WAIT fires within budget when ALL N=2 slots are externally held ---"

_LOCK22="$(mktemp)"
_ERR22="$(mktemp)"

# Spawn two background holders: one pins slot-1, one pins slot-2.
( flock -x 9; sleep 10 ) 9>>"${_LOCK22}.slot-1" &
_HOLDER22A=$!
( flock -x 9; sleep 10 ) 9>>"${_LOCK22}.slot-2" &
_HOLDER22B=$!
# Causal flock-probe barrier (task 5258, PRD merge-gate-health W4b): block until
# BOTH holders hold their slots, instead of a fixed `sleep 0.2` grace a saturated
# host can outrun.  Two calls reuse the single-slot helper (one per slot); each
# is wrapped in `assert` so a transient confirm-failure cannot trip `set -e`.
assert "Test 22: background holder confirmed holding slot-1 (causal flock-probe barrier)" \
    occt_wait_until_slot_held "${_LOCK22}.slot-1"
assert "Test 22: background holder confirmed holding slot-2 (causal flock-probe barrier)" \
    occt_wait_until_slot_held "${_LOCK22}.slot-2"

_START22="$(date +%s)"
_EXIT22=0
# Backstop raised 5→15s (same rationale as Test 14 twin): under load the preamble
# + retry cycle can push the legitimate exit-75 past the old 5s backstop → false RED.
# Discriminators remain exit==75 and stderr pattern; the elapsed guard is generous
# anti-hang only (outer 15s > guard 10s > normal ~1.5s).
# (PRD docs/prds/infra-test-wallclock-deflake.md §2/T3 decision D4)
REIFY_OCCT_LOCK="$_LOCK22" REIFY_OCCT_CONCURRENCY=2 REIFY_OCCT_LOCK_WAIT=1 \
    timeout 15 "$WRAPPER" true 2>"$_ERR22" || _EXIT22=$?
_END22="$(date +%s)"
_ELAPSED22=$(( _END22 - _START22 ))

kill "$_HOLDER22A" "$_HOLDER22B" 2>/dev/null || true
wait "$_HOLDER22A" "$_HOLDER22B" 2>/dev/null || true

assert "Test 22: wrapper exits 75 when all N=2 slots held and LOCK_WAIT=1 (got $_EXIT22)" \
    test "$_EXIT22" -eq 75

assert "Test 22: wrapper exits within 10s (generous anti-hang; outer 15s backstop catches true hang) (elapsed=${_ELAPSED22}s)" \
    test "$_ELAPSED22" -le 10 # wallclock:allow — generous anti-hang: discriminated by exit==75 and stderr pattern, not elapsed magnitude

assert "Test 22: stderr mentions acquire and lock-wait duration (1s)" \
    grep -qE 'acquire.*1s|1s.*acquire' "$_ERR22"

rm -f "$_LOCK22" "${_LOCK22}.slot-1" "${_LOCK22}.slot-2" "$_ERR22"

# -- Test 23: FD-non-leak invariant with N>1 -----------------------------------
# Multi-slot generalization of Test 18 (which tests N=1 / slot-1 only after
# step-6's migration).  Run two concurrent wrapper invocations under
# REIFY_OCCT_CONCURRENCY=2 so they each acquire a different slot file
# (slot-1 and slot-2).  Each wrapper spawns a detached daemon.
# After both wrappers exit, BOTH slot-1 AND slot-2 must be flock-acquirable,
# confirming that neither surviving daemon inherited the slot FD from its
# respective wrapper invocation.  The 9<&- redirection closes FD 9 before
# the child exec, so no descendant process can hold the slot open.
echo ""
echo "--- Test 23: N=2 wrappers close fd 9 on child; daemons do not leak either slot ---"

_LOCK23="$(mktemp)"
_DAEMON_PID_FILE23A="$(mktemp)"
_DAEMON_PID_FILE23B="$(mktemp)"
_EXIT23A=0
_EXIT23B=0

# Spawn both wrappers concurrently so they acquire different slots
# (slot-1 and slot-2 respectively).
REIFY_OCCT_LOCK="$_LOCK23" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c '
    setsid bash -c "sleep 30" </dev/null >/dev/null 2>&1 &
    echo $! > "'"$_DAEMON_PID_FILE23A"'"
    disown
    exit 0
' &
_W23A=$!
REIFY_OCCT_LOCK="$_LOCK23" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c '
    setsid bash -c "sleep 30" </dev/null >/dev/null 2>&1 &
    echo $! > "'"$_DAEMON_PID_FILE23B"'"
    disown
    exit 0
' &
_W23B=$!
wait "$_W23A" || _EXIT23A=$?
wait "$_W23B" || _EXIT23B=$?

_DAEMON23A="$(cat "$_DAEMON_PID_FILE23A" 2>/dev/null || echo "")"
_DAEMON23B="$(cat "$_DAEMON_PID_FILE23B" 2>/dev/null || echo "")"

# Both daemons must still be alive (otherwise the test is vacuous — no
# inherited FD to worry about).
assert "Test 23: daemon A spawned inside wrapper A is still alive (pid=$_DAEMON23A)" \
    bash -c "[ -n '$_DAEMON23A' ] && kill -0 '$_DAEMON23A' 2>/dev/null"

assert "Test 23: daemon B spawned inside wrapper B is still alive (pid=$_DAEMON23B)" \
    bash -c "[ -n '$_DAEMON23B' ] && kill -0 '$_DAEMON23B' 2>/dev/null"

# Both slot files must be flock-acquirable: surviving daemons must not have
# inherited FD 9 from either wrapper invocation.
_LOCK_FREE23A=1
( flock -n -x 9 || exit 1 ) 9>>"${_LOCK23}.slot-1" || _LOCK_FREE23A=0

_LOCK_FREE23B=1
( flock -n -x 9 || exit 1 ) 9>>"${_LOCK23}.slot-2" || _LOCK_FREE23B=0

assert "Test 23: slot-1 lock released after wrapper A exit (fd 9 not inherited by daemon A)" \
    test "$_LOCK_FREE23A" -eq 1

assert "Test 23: slot-2 lock released after wrapper B exit (fd 9 not inherited by daemon B)" \
    test "$_LOCK_FREE23B" -eq 1

# Both wrappers must have exited 0.
assert "Test 23: wrapper A exited 0 (got $_EXIT23A)" \
    test "$_EXIT23A" -eq 0

assert "Test 23: wrapper B exited 0 (got $_EXIT23B)" \
    test "$_EXIT23B" -eq 0

# Cleanup surviving daemons.
[ -n "$_DAEMON23A" ] && kill "$_DAEMON23A" 2>/dev/null || true
[ -n "$_DAEMON23B" ] && kill "$_DAEMON23B" 2>/dev/null || true
rm -f "$_LOCK23" "${_LOCK23}.slot-1" "${_LOCK23}.slot-2" \
    "$_DAEMON_PID_FILE23A" "$_DAEMON_PID_FILE23B"

test_summary
