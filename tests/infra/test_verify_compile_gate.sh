#!/usr/bin/env bash
# tests/infra/test_verify_compile_gate.sh — hermetic --print-plan oracle tests
# for the compile-phase PSI admission gate (task 4618, extended task 4853).
#
# Asserts:
#   W1: lint/typecheck/all plans each contain the compile-gate line
#   W2: in each, compile-gate precedes the first cargo check/clippy line
#   W3: in the 'all' plan, compile-gate precedes both clippy and psi-gate
#   W4: pure 'test' plan DOES contain compile-gate (task 4853: test-path
#       backstop for the heavy --no-run nextest link outside the held slot);
#       ordering: compile-gate < first --no-run line < @@SEMAPHORE_ACQUIRE@@
#   W4b (W9): merge-role 'test' plan still contains compile-gate line
#       (plan-shape is role-invariant; runtime merge-bypass is in compile_gate())
#   W5: merge-role 'all' plan still contains the compile-gate line
#       (runtime-bypassed, plan-shape is role-invariant — CAVEAT 1)
#   W6: structural — verify.sh defines compile_gate() and contains the
#       DF_VERIFY_ROLE=merge bypass inside it (source grep)
#   W7: preservation — clippy, gui-feature cargo check, psi-gate, and nextest
#       run still present in the 'all' plan; compile-gate carries no
#       nice/ionice prefix and no 'cargo' token
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh) and auto-selected
# by the infra map (scripts/verify.sh -> tests/infra/test_verify_*.sh).
# No cargo/npm/tree-sitter builds — pure shell/grep hermetic oracle.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
VERIFY="$REPO_ROOT/scripts/verify.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== verify.sh compile-gate wiring tests (task 4618) ==="

# ---------------------------------------------------------------------------
# Capture plans (hermetic: --print-plan, no cargo/npm execution)
# ---------------------------------------------------------------------------
PLAN_LINT="$(bash "$VERIFY" lint --scope all --print-plan 2>/dev/null | grep -v '^#')"
PLAN_TC="$(bash "$VERIFY"   typecheck --scope all --print-plan 2>/dev/null | grep -v '^#')"
PLAN_ALL="$(bash "$VERIFY"  all --scope all --print-plan 2>/dev/null | grep -v '^#')"
PLAN_TEST="$(bash "$VERIFY" test --scope all --print-plan 2>/dev/null | grep -v '^#')"
# PLAN_TEST_FULL: includes # comment lines so the ACQUIRE marker is visible for
# index-based ordering assertions (W4 ordering; mirrors capture_plans() in
# test_verify_semaphore_e2e.sh).
PLAN_TEST_FULL="$(bash "$VERIFY" test --scope all --print-plan 2>/dev/null)"
PLAN_MERGE="$(env DF_VERIFY_ROLE=merge bash "$VERIFY" all --scope all --print-plan 2>/dev/null | grep -v '^#')"
# PLAN_MERGE_TEST: merge-role test plan (commands-only) for W4b role-invariance assertion.
PLAN_MERGE_TEST="$(env DF_VERIFY_ROLE=merge bash "$VERIFY" test --scope all --print-plan 2>/dev/null | grep -v '^#')"

# ---------------------------------------------------------------------------
# W1: compile-gate present in lint / typecheck / all plans
# ---------------------------------------------------------------------------
echo ""
echo "--- W1: compile-gate present in lint/typecheck/all plans ---"

assert "W1-lint: lint plan contains ./scripts/verify.sh compile-gate" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_LINT"

assert "W1-tc: typecheck plan contains ./scripts/verify.sh compile-gate" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_TC"

assert "W1-all: all plan contains ./scripts/verify.sh compile-gate" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_ALL"

# ---------------------------------------------------------------------------
# W2: compile-gate index < first cargo check/clippy line in each plan
# ---------------------------------------------------------------------------
echo ""
echo "--- W2: compile-gate precedes first cargo check/clippy ---"

# Helper: echo "1" if compile-gate index < first cargo (check OR clippy) index in $1
_compile_gate_before_cargo() {
    local cg_ln cargo_ln
    cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
    cargo_ln=$(printf "%s\n" "$1" | grep -nE "(^| )(cargo check|cargo clippy)" | head -1 | cut -d: -f1)
    [ -n "$cg_ln" ] && [ -n "$cargo_ln" ] && [ "$cg_ln" -lt "$cargo_ln" ]
}

assert "W2-lint: compile-gate index < first cargo check/clippy in lint plan" \
    bash -c '_compile_gate_before_cargo() {
        local cg_ln cargo_ln
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        cargo_ln=$(printf "%s\n" "$1" | grep -nE "(^| )(cargo check|cargo clippy)" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$cargo_ln" ] && [ "$cg_ln" -lt "$cargo_ln" ]
    }; _compile_gate_before_cargo "$1"' _ "$PLAN_LINT"

assert "W2-tc: compile-gate index < first cargo check/clippy in typecheck plan" \
    bash -c '_compile_gate_before_cargo() {
        local cg_ln cargo_ln
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        cargo_ln=$(printf "%s\n" "$1" | grep -nE "(^| )(cargo check|cargo clippy)" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$cargo_ln" ] && [ "$cg_ln" -lt "$cargo_ln" ]
    }; _compile_gate_before_cargo "$1"' _ "$PLAN_TC"

assert "W2-all: compile-gate index < first cargo check/clippy in all plan" \
    bash -c '_compile_gate_before_cargo() {
        local cg_ln cargo_ln
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        cargo_ln=$(printf "%s\n" "$1" | grep -nE "(^| )(cargo check|cargo clippy)" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$cargo_ln" ] && [ "$cg_ln" -lt "$cargo_ln" ]
    }; _compile_gate_before_cargo "$1"' _ "$PLAN_ALL"

# ---------------------------------------------------------------------------
# W3: in 'all' plan, compile-gate < clippy AND compile-gate < psi-gate
# ---------------------------------------------------------------------------
echo ""
echo "--- W3: compile-gate precedes clippy AND psi-gate in all plan ---"

assert "W3-all: compile-gate index < cargo clippy index" \
    bash -c '
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        clippy_ln=$(printf "%s\n" "$1" | grep -n "cargo clippy" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$clippy_ln" ] && [ "$cg_ln" -lt "$clippy_ln" ]
    ' _ "$PLAN_ALL"

assert "W3-all: compile-gate index < psi-gate index" \
    bash -c '
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        psi_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$psi_ln" ] && [ "$cg_ln" -lt "$psi_ln" ]
    ' _ "$PLAN_ALL"

# ---------------------------------------------------------------------------
# W4: test plan DOES contain compile-gate (task 4853: test-path PSI backstop,
# repositioned as block-entry load gate before @@SEMAPHORE_ACQUIRE@@ by task 4862).
# Ordering: compile-gate < ACQUIRE marker (block-entry gate for unified build+test block).
# No --no-run line exists in the plan (task 4862 revert: build+test in one slot-held block).
# (PLAN_TEST_FULL includes comment lines so the ACQUIRE marker is visible)
# ---------------------------------------------------------------------------
echo ""
echo "--- W4: test plan contains compile-gate ordered before ACQUIRE (block-entry gate, tasks 4853/4862) ---"

assert "W4: 'test --print-plan' DOES contain compile-gate (task 4853)" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_TEST"

assert "W4: NO 'cargo nextest run ... --no-run' line in test plan (task 4862 revert: build inside slot)" \
    bash -c '! printf "%s\n" "$1" | grep -q "cargo nextest run.*--no-run"' _ "$PLAN_TEST_FULL"

assert "W4: compile-gate index < first 'cargo nextest run' (build+exec) line in test plan" \
    bash -c '
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        nextest_ln=$(printf "%s\n" "$1" | grep -n "cargo nextest run" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$nextest_ln" ] && [ "$cg_ln" -lt "$nextest_ln" ]
    ' _ "$PLAN_TEST_FULL"

assert "W4: compile-gate index < ACQUIRE marker index in test plan (block-entry load gate for unified build+test block)" \
    bash -c '
        cg_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        acq_ln=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        [ -n "$cg_ln" ] && [ -n "$acq_ln" ] && [ "$cg_ln" -lt "$acq_ln" ]
    ' _ "$PLAN_TEST_FULL"

# ---------------------------------------------------------------------------
# W4b (W9): merge-role test plan still contains compile-gate (role-invariant)
# Mirror of W5 for the test action: the plan line is emitted regardless of
# DF_VERIFY_ROLE; the merge bypass fires at RUNTIME inside compile_gate().
# ---------------------------------------------------------------------------
echo ""
echo "--- W4b (W9): merge-role test plan still contains compile-gate (role-invariant) ---"

assert "W4b (W9): merge-role test plan contains compile-gate line (plan-shape role-invariant)" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_MERGE_TEST"

# ---------------------------------------------------------------------------
# W5: merge-role 'all' plan still contains compile-gate (runtime-bypassed)
# ---------------------------------------------------------------------------
echo ""
echo "--- W5: merge-role all plan still contains compile-gate (runtime bypass) ---"

assert "W5: merge-role all plan contains compile-gate line (plan-shape role-invariant)" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$PLAN_MERGE"

# ---------------------------------------------------------------------------
# W6: structural — verify.sh source contains compile_gate() definition and
#     DF_VERIFY_ROLE=merge bypass inside compile_gate
# ---------------------------------------------------------------------------
echo ""
echo "--- W6: verify.sh structural checks ---"

assert "W6: verify.sh defines compile_gate()" \
    grep -q "^compile_gate()" "$VERIFY"

assert "W6: compile_gate() contains DF_VERIFY_ROLE=merge bypass" \
    bash -c 'awk "/^compile_gate\(\)/,/^\}/" "$1" | grep -q "DF_VERIFY_ROLE.*merge"' _ "$VERIFY"

# ---------------------------------------------------------------------------
# W7: preservation — all expected plan components still present in 'all' plan,
#     and the compile-gate line carries no nice/ionice prefix and no cargo token
# ---------------------------------------------------------------------------
echo ""
echo "--- W7: preservation and compile-gate line properties in all plan ---"

assert "W7: all plan still contains cargo clippy" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo clippy"' _ "$PLAN_ALL"

assert "W7: all plan still contains gui-feature cargo check -p reify-gui" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo check -p reify-gui"' _ "$PLAN_ALL"

assert "W7: all plan still contains psi-gate" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh psi-gate"' _ "$PLAN_ALL"

assert "W7: all plan still contains cargo nextest run (or cargo test fallback)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "cargo (nextest run|test)"' _ "$PLAN_ALL"

# The compile-gate line should be a bare ./scripts/verify.sh compile-gate with
# no nice/ionice priority prefix and no 'cargo' token.
CG_LINE="$(printf "%s\n" "$PLAN_ALL" | grep "verify\.sh compile-gate" | head -1)"

assert "W7: compile-gate line has no nice/ionice prefix" \
    bash -c '! printf "%s\n" "$1" | grep -qE "^(nice|ionice)"' _ "$CG_LINE"

assert "W7: compile-gate line contains no 'cargo' token" \
    bash -c '! printf "%s\n" "$1" | grep -qE "(^| )cargo( |$)"' _ "$CG_LINE"

# ---------------------------------------------------------------------------
# W8: operational-doc sync — the operative doc documents the compile-gate
# ---------------------------------------------------------------------------
# Keeps the doc that dispatched agents read in sync with the gate. The
# canonical operational digest moved from CLAUDE.md to
# docs/notes/verify-pipeline-knobs.md in the CLAUDE.md trim (96ce210ebd);
# CLAUDE.md now carries only a Pointers-table row to it. This is an
# existence/substring check on operative doc content, not a prose-wording pin.
echo ""
echo "--- W8: verify-pipeline-knobs.md doc-sync ---"

KNOBS_DOC="$REPO_ROOT/docs/notes/verify-pipeline-knobs.md"

assert "W8: knobs doc exists (docs/notes/verify-pipeline-knobs.md)" \
    test -f "$KNOBS_DOC"

assert "W8: knobs doc mentions REIFY_COMPILE_GATE_THRESHOLD knob" \
    grep -q "REIFY_COMPILE_GATE_THRESHOLD" "$KNOBS_DOC"

assert "W8: knobs doc mentions compile-gate merge-exempt or admit-on-timeout" \
    bash -c 'grep -qiE "compile.gate|compile_gate" "$1"' _ "$KNOBS_DOC"

# ---------------------------------------------------------------------------
# Cycle X: compile-gate clock-stop marker emission (execution; task 4920)
# The W-sections above are a hermetic --print-plan oracle (no execution). This
# section additionally DRIVES `bash verify.sh compile-gate` (still cargo-free —
# compile-gate is dispatched before the cargo/npm pipeline) under injected PSI
# fixtures to prove the admit-mode gate is now IN clock-stop scope (task 4920:
# compile_gate() sets _ca_clock_reason=psi_pressure; admit mode no longer
# admits-on-timeout, it HOLDS until PSI drops).
#
# X1 (hold): stuck avg10=99 fixture wrapped in `timeout 8` -> rc==124 (killed
#     while still holding, well past the old MAX_WAIT=2s), stderr has
#     @@REIFY_CLOCK_STOP@@ reason=psi_pressure + @@REIFY_CLOCK_HEARTBEAT@@, and
#     NEVER an admit/fairness-floor message (removed) nor @@REIFY_CLOCK_START@@.
# X2 (admit-on-drop): same stuck start, but a background updater clears the
#     fixture to a low avg10 after ~2s -> exit 0, STOP+HEARTBEAT+START all
#     present (balanced), no fairness message.
# RED today: compile_gate() sets no _ca_clock_reason -> admits-on-timeout at
# MAX_WAIT=2s -> `timeout 8` never fires, no STOP marker emitted at all.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle X: compile-gate clock-stop marker emission (execution) ---"

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

# Quiet-memory fixture: neutralize compile_gate's default-ON memfull dimension
# (task 4911) so these CPU-only cases are deterministic regardless of host
# memory load (mirrors tests/infra/test_cpu_admit.sh's hermeticity block).
MEM_QUIET_X="$(mktemp -p "$WORKDIR" mem-quiet.XXXXXX)"
printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
    > "$MEM_QUIET_X"

make_psi_fixture_x() {
    local avg10="$1"
    local fixture
    fixture="$(mktemp -p "$WORKDIR" psi-fixture.XXXXXX)"
    printf 'some avg10=%s avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        "$avg10" > "$fixture"
    echo "$fixture"
}

# X1: the hold — fixture stays stuck at avg10=99 for the whole 8s window.
PSI_X1="$(make_psi_fixture_x 99)"
_X1_STDERR="$(mktemp -p "$WORKDIR" x1-stderr.XXXXXX)"
_X1_RC=0
timeout 8 \
    env REIFY_COMPILE_GATE_PROC_PATH="$PSI_X1" \
        REIFY_COMPILE_GATE_MEM_PROC_PATH="$MEM_QUIET_X" \
        REIFY_COMPILE_GATE_MAX_WAIT=2 \
        REIFY_COMPILE_GATE_POLL=1 \
        REIFY_CLOCK_HEARTBEAT_SECS=1 \
        bash "$VERIFY" compile-gate \
    2>"$_X1_STDERR" || _X1_RC=$?

assert "X1: avg10=99 sustained → timeout 8 kills it (rc==124; HELD past old MAX_WAIT=2s)" \
    test "$_X1_RC" -eq 124
assert "X1: stderr contains @@REIFY_CLOCK_STOP@@ reason=psi_pressure (compile-gate hold entered)" \
    bash -c 'grep -q "@@REIFY_CLOCK_STOP@@ reason=psi_pressure" "$1"' _ "$_X1_STDERR"
assert "X1: stderr contains @@REIFY_CLOCK_HEARTBEAT@@ (still holding, liveness)" \
    bash -c 'grep -q "@@REIFY_CLOCK_HEARTBEAT@@" "$1"' _ "$_X1_STDERR"
assert "X1: stderr does NOT match admit/fairness-floor message (removed — never admits-on-timeout)" \
    bash -c '! grep -qiE "admit|fairness|proceeding under load|sustained pressure" "$1"' _ "$_X1_STDERR"
assert "X1: stderr does NOT contain the CLOCK_START marker (never admitted)" \
    bash -c '! grep -qF "@@REIFY_CLOCK_START@@" "$1"' _ "$_X1_STDERR"

# X2: same stuck-fixture start, but a background updater clears it to
# avg10=10 after ~2s → the hold admits on the PSI drop, not on a timeout.
PSI_X2="$(make_psi_fixture_x 99)"
(
    sleep 2
    printf 'some avg10=10.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        > "$PSI_X2"
) &
_X2_UPDATER=$!

_X2_STDERR="$(mktemp -p "$WORKDIR" x2-stderr.XXXXXX)"
_X2_RC=0
timeout 30 \
    env REIFY_COMPILE_GATE_PROC_PATH="$PSI_X2" \
        REIFY_COMPILE_GATE_MEM_PROC_PATH="$MEM_QUIET_X" \
        REIFY_COMPILE_GATE_MAX_WAIT=2 \
        REIFY_COMPILE_GATE_POLL=1 \
        REIFY_CLOCK_HEARTBEAT_SECS=1 \
        bash "$VERIFY" compile-gate \
    2>"$_X2_STDERR" || _X2_RC=$?

kill "$_X2_UPDATER" 2>/dev/null || true
wait "$_X2_UPDATER" 2>/dev/null || true

assert "X2: admits once PSI drops (exit 0; got $_X2_RC)" \
    test "$_X2_RC" -eq 0
assert "X2: stderr contains @@REIFY_CLOCK_STOP@@ reason=psi_pressure" \
    bash -c 'grep -q "@@REIFY_CLOCK_STOP@@ reason=psi_pressure" "$1"' _ "$_X2_STDERR"
assert "X2: stderr contains @@REIFY_CLOCK_HEARTBEAT@@" \
    bash -c 'grep -q "@@REIFY_CLOCK_HEARTBEAT@@" "$1"' _ "$_X2_STDERR"
assert "X2: stderr contains the CLOCK_START marker (STOP/START balanced)" \
    bash -c 'grep -qF "@@REIFY_CLOCK_START@@" "$1"' _ "$_X2_STDERR"
assert "X2: stderr does NOT match fairness-floor admit-on-timeout message" \
    bash -c '! grep -qiE "admit|fairness|proceeding under load|sustained pressure" "$1"' _ "$_X2_STDERR"

test_summary
