#!/usr/bin/env bash
# scripts/test_psi_gate.sh — integration tests for the PSI-gated dispatch in verify.sh.
#
# Drives `verify.sh psi-gate` in isolation with injected PSI fixtures and
# isolated dispatch files — no cargo/tree-sitter/npm builds.
#
# Skip guard: exits 0 (skip) on hosts without /proc/pressure/cpu.
# Fail-open (missing PSI source) is still exercised via PROC_PATH override.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VERIFY="$REPO_ROOT/scripts/verify.sh"

[ -f "$REPO_ROOT/tests/infra/test_helpers.sh" ] || {
    echo "ERROR: tests/infra/test_helpers.sh not found at $REPO_ROOT/tests/infra/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$REPO_ROOT/tests/infra/test_helpers.sh"

if [ ! -r /proc/pressure/cpu ]; then
    echo "SKIP: kernel lacks /proc/pressure/cpu (PSI gate is Linux-only)"
    exit 0
fi

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

# make_psi_fixture <avg10>
# Writes a /proc/pressure/cpu-formatted fixture to a temp file and echoes its path.
make_psi_fixture() {
    local avg10="$1"
    local fixture
    fixture="$(mktemp -p "$WORKDIR" psi-fixture.XXXXXX)"
    printf 'some avg10=%s avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        "$avg10" > "$fixture"
    echo "$fixture"
}

# run_gate <dispatch_file> <proc_path> [VAR=val ...]
# Invokes `verify.sh psi-gate` with the given dispatch file and PSI proc path,
# plus any additional env overrides.  After returning:
#   GATE_RC     — exit code of the invocation
#   GATE_STDERR — captured stderr text
GATE_RC=0
GATE_STDERR=""
run_gate() {
    local dispatch="$1" proc="$2"
    shift 2
    local _stderr_file
    _stderr_file="$(mktemp -p "$WORKDIR" gate-stderr.XXXXXX)"
    GATE_RC=0
    GATE_STDERR=""
    env "$@" \
        REIFY_PSI_GATE_DISPATCH_FILE="$dispatch" \
        REIFY_PSI_GATE_PROC_PATH="$proc" \
        bash "$VERIFY" psi-gate \
        2>"$_stderr_file" \
        || GATE_RC=$?
    GATE_STDERR="$(cat "$_stderr_file")"
    rm -f "$_stderr_file"
}

echo "=== psi-gate tests ==="

# ---------------------------------------------------------------------------
# Cycle 1: core PSI gate — avg10 vs threshold, MAX_WAIT timeout, exit codes
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 1: core PSI gate ---"

# (a) avg10=40 < threshold=50 (default), no dispatch file → exit 0 + dispatch touched
PSI_1A="$(make_psi_fixture 40)"
DISPATCH_1A="$(mktemp -u -p "$WORKDIR" dispatch.XXXXXX)"   # -u: name only, file absent
run_gate "$DISPATCH_1A" "$PSI_1A"
assert "core-pass: avg10=40 < threshold=50 → exit 0" \
    test "$GATE_RC" -eq 0
assert "core-pass: dispatch file was touched" \
    test -e "$DISPATCH_1A"

# (b) avg10=60 >= threshold=50 → times out (exit 75), stderr contains give-up message,
#     dispatch file NOT created
PSI_1B="$(make_psi_fixture 60)"
DISPATCH_1B="$(mktemp -u -p "$WORKDIR" dispatch.XXXXXX)"
run_gate "$DISPATCH_1B" "$PSI_1B" \
    REIFY_PSI_GATE_MAX_WAIT=2 REIFY_PSI_GATE_POLL=1
assert "core-timeout: avg10=60 >= threshold=50, max_wait=2 → exit 75" \
    test "$GATE_RC" -eq 75
assert "core-timeout: stderr contains give-up message (cpu headroom/gave up/psi)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "cpu headroom|gave up|psi"' _ "$GATE_STDERR"
assert "core-timeout: dispatch file was NOT created" \
    test ! -e "$DISPATCH_1B"

# (c) avg10=40, THRESHOLD=30 → 40 >= 30 blocks; max_wait=2 → exit 75
#     (exercises threshold env override parsing)
PSI_1C="$(make_psi_fixture 40)"
DISPATCH_1C="$(mktemp -u -p "$WORKDIR" dispatch.XXXXXX)"
run_gate "$DISPATCH_1C" "$PSI_1C" \
    REIFY_PSI_GATE_THRESHOLD=30 REIFY_PSI_GATE_MAX_WAIT=2 REIFY_PSI_GATE_POLL=1
assert "threshold-override: avg10=40 >= threshold=30, max_wait=2 → exit 75" \
    test "$GATE_RC" -eq 75

# ---------------------------------------------------------------------------
# Cycle 2: WINDOW throttle — inter-dispatch spacing
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 2: WINDOW throttle ---"

# (a) block-wait — dispatch file pre-touched to "now", PSI never blocks (avg10=0),
#     WINDOW=2s; gate must wait out the window before passing.
PSI_ZERO="$(make_psi_fixture 0)"
DISPATCH_2A="$(mktemp -p "$WORKDIR" dispatch-2a.XXXXXX)"   # creates the file
touch "$DISPATCH_2A"                                         # set mtime to now

T2A_0=$(date +%s)
run_gate "$DISPATCH_2A" "$PSI_ZERO" \
    REIFY_PSI_GATE_WINDOW=2 REIFY_PSI_GATE_POLL=1 REIFY_PSI_GATE_MAX_WAIT=30
T2A_1=$(date +%s)
ELAPSED_2A=$(( T2A_1 - T2A_0 ))

assert "window-block: exit 0 after waiting" \
    test "$GATE_RC" -eq 0
assert "window-block: elapsed >= WINDOW=2s" \
    test "$ELAPSED_2A" -ge 2

# (b) concurrent burst — 5 background invocations sharing one dispatch file;
#     assert all pass AND consecutive timestamps are >= WINDOW=2s apart
PSI_BURST="$(make_psi_fixture 0)"
DISPATCH_2B="$(mktemp -u -p "$WORKDIR" dispatch-2b.XXXXXX)"  # absent initially
RESULTS_2B="$(mktemp -p "$WORKDIR" results.XXXXXX)"

for _i in $(seq 1 5); do
    (
        _d="$DISPATCH_2B" _p="$PSI_BURST" _r="$RESULTS_2B" _v="$VERIFY"
        GATE_RC=0
        env REIFY_PSI_GATE_DISPATCH_FILE="$_d" \
            REIFY_PSI_GATE_PROC_PATH="$_p" \
            REIFY_PSI_GATE_WINDOW=2 \
            REIFY_PSI_GATE_POLL=1 \
            REIFY_PSI_GATE_MAX_WAIT=60 \
            bash "$_v" psi-gate >/dev/null 2>&1 \
            && date +%s >> "$_r"
    ) &
done
wait

assert "concurrent-burst: all 5 invocations passed" \
    bash -c '[ "$(wc -l < "$1")" -eq 5 ]' _ "$RESULTS_2B"

SORTED_2B="$(sort -n "$RESULTS_2B")"
assert "concurrent-burst: consecutive pass timestamps >= WINDOW=2s apart" \
    bash -c '
        prev=""
        while IFS= read -r ts; do
            [ -z "$ts" ] && continue
            if [ -n "$prev" ]; then
                delta=$(( ts - prev ))
                [ "$delta" -ge 2 ] || { echo "  delta $delta < 2 between $prev and $ts" >&2; exit 1; }
            fi
            prev="$ts"
        done <<< "$1"
    ' _ "$SORTED_2B"

# ---------------------------------------------------------------------------
# Cycle 3: role=merge bypass — skips wait but still bumps the timestamp
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 3: merge bypass ---"

# Setup: dispatch pre-touched to "now"; PSI=99 (both WINDOW and PSI would block task)
PSI_HIGH="$(make_psi_fixture 99)"
PSI_ZERO_M="$(make_psi_fixture 0)"
DISPATCH_3="$(mktemp -p "$WORKDIR" dispatch-3.XXXXXX)"
touch "$DISPATCH_3"
sleep 1  # ensure a fresh mtime is distinguishable from "now"
MTIME_3_BEFORE=$(stat -c %Y "$DISPATCH_3")

# (a) merge bypass: avg10=99, WINDOW=2 → must exit 0 fast AND bump dispatch mtime
#     (MAX_WAIT=5 safety cap: without the bypass, the gate would block >1800s)
T3_0=$(date +%s)
run_gate "$DISPATCH_3" "$PSI_HIGH" \
    DF_VERIFY_ROLE=merge REIFY_PSI_GATE_WINDOW=2 REIFY_PSI_GATE_MAX_WAIT=5 REIFY_PSI_GATE_POLL=1
T3_1=$(date +%s)
MERGE_ELAPSED=$(( T3_1 - T3_0 ))
MTIME_3_AFTER=$(stat -c %Y "$DISPATCH_3")

assert "merge-bypass: exit 0" \
    test "$GATE_RC" -eq 0
assert "merge-bypass: returned fast (no window wait)" \
    test "$MERGE_ELAPSED" -lt 2
# Assert against T3_0 (wall-clock captured right before run_gate), not MTIME_3_BEFORE.
# If the bypass drops its 'touch', MTIME_3_AFTER stays at the pre-sleep value which is
# < T3_0 (sleep 1 elapsed between initial touch and T3_0), so the assertion fails.
# A strict -ge MTIME_3_BEFORE would be non-discriminating (AFTER == BEFORE satisfies it).
assert "merge-bypass: dispatch mtime was bumped (>= gate-start)" \
    test "$MTIME_3_AFTER" -ge "$T3_0"

# (b) immediately after merge: a task verify must back off >= WINDOW from merge's touch.
# Measure elapsed from MTIME_3_AFTER (the actual merge-touch mtime) rather than from
# T3B_0: with integer-second date +%s, T3B_0 can be up to 1s after MTIME_3_AFTER,
# so the gate may start with age=1 and only wait ~1s, making T3B_1-T3B_0 = 1 < 2.
# TASK_COMPLETE_OFFSET = T3B_1 - MTIME_3_AFTER is >= WINDOW whenever the gate passes
# correctly (the gate's pass condition is age >= WINDOW, i.e. now - mtime >= WINDOW).
T3B_0=$(date +%s)
run_gate "$DISPATCH_3" "$PSI_ZERO_M" \
    DF_VERIFY_ROLE=task REIFY_PSI_GATE_WINDOW=2 REIFY_PSI_GATE_POLL=1 REIFY_PSI_GATE_MAX_WAIT=30
T3B_1=$(date +%s)
TASK_COMPLETE_OFFSET=$(( T3B_1 - MTIME_3_AFTER ))

assert "merge-bypass: subsequent task completes >= WINDOW=2s after merge touch" \
    test "$TASK_COMPLETE_OFFSET" -ge 2

# ---------------------------------------------------------------------------
# Cycle 4: REIFY_PSI_GATE_DISABLE=1 break-glass — exits 0 fast, NO touch
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 4: DISABLE break-glass ---"

PSI_HIGH2="$(make_psi_fixture 99)"

# (a) dispatch file absent: gate should exit 0 fast AND not create the file
#     (MAX_WAIT=5/POLL=1 safety: without DISABLE, the gate would block on avg10=99)
DISPATCH_4A="$(mktemp -u -p "$WORKDIR" dispatch-4a.XXXXXX)"
T4A_0=$(date +%s)
run_gate "$DISPATCH_4A" "$PSI_HIGH2" REIFY_PSI_GATE_DISABLE=1 \
    REIFY_PSI_GATE_MAX_WAIT=5 REIFY_PSI_GATE_POLL=1
T4A_1=$(date +%s)
ELAPSED_4A=$(( T4A_1 - T4A_0 ))

assert "disable: exit 0 (absent dispatch)" \
    test "$GATE_RC" -eq 0
assert "disable: returned fast" \
    test "$ELAPSED_4A" -lt 2
assert "disable: dispatch file NOT created" \
    test ! -e "$DISPATCH_4A"

# (b) dispatch file pre-existing: gate exits 0 fast AND mtime must be unchanged
DISPATCH_4B="$(mktemp -p "$WORKDIR" dispatch-4b.XXXXXX)"
touch "$DISPATCH_4B"
sleep 1  # ensure clock-second boundary for a stable mtime
MTIME_4B_BEFORE=$(stat -c %Y "$DISPATCH_4B")

run_gate "$DISPATCH_4B" "$PSI_HIGH2" REIFY_PSI_GATE_DISABLE=1 \
    REIFY_PSI_GATE_MAX_WAIT=5 REIFY_PSI_GATE_POLL=1
MTIME_4B_AFTER=$(stat -c %Y "$DISPATCH_4B")

assert "disable: pre-existing dispatch file mtime unchanged" \
    test "$MTIME_4B_AFTER" -eq "$MTIME_4B_BEFORE"

# ---------------------------------------------------------------------------
# Cycle 5: fail-open on missing PSI source (older/non-Linux kernels)
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 5: fail-open on missing PSI source ---"

NONEXISTENT_PSI="$WORKDIR/nope/pressure-cpu"   # guaranteed absent
DISPATCH_5="$(mktemp -u -p "$WORKDIR" dispatch-5.XXXXXX)"

# MAX_WAIT=5/POLL=1 safety: without fail-open, the gate would loop until MAX_WAIT=1800s
run_gate "$DISPATCH_5" "$NONEXISTENT_PSI" \
    REIFY_PSI_GATE_MAX_WAIT=5 REIFY_PSI_GATE_POLL=1

assert "fail-open: exit 0 when PSI source is missing" \
    test "$GATE_RC" -eq 0
assert "fail-open: stderr contains 'PSI gate disabled' warning" \
    bash -c 'printf "%s\n" "$1" | grep -q "PSI gate disabled"' _ "$GATE_STDERR"
assert "fail-open: stderr mentions 'kernel lacks' and the path" \
    bash -c 'printf "%s\n" "$1" | grep -q "kernel lacks"' _ "$GATE_STDERR"

# ---------------------------------------------------------------------------
# Cycle 6: production wiring — gate appears in test/all plan, not in lint/typecheck
# (hermetic: --print-plan, never executes cargo)
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 6: production wiring (--print-plan) ---"

PLAN_TEST="$(bash "$VERIFY" test  --scope all --print-plan 2>/dev/null | grep -v '^#')"
PLAN_LINT="$(bash "$VERIFY" lint  --scope all --print-plan 2>/dev/null | grep -v '^#')"
PLAN_TC="$(bash "$VERIFY"   typecheck --scope all --print-plan 2>/dev/null | grep -v '^#')"

# (a) test plan contains the psi-gate line
assert "wiring: 'test --print-plan' contains verify.sh psi-gate" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh psi-gate"' _ "$PLAN_TEST"

# (b) gate line appears BEFORE first cargo test/nextest line in the test plan
assert "wiring: psi-gate line is before first cargo test line" \
    bash -c '
        gate_ln=$(printf "%s\n" "$1" | grep -n "verify\.sh psi-gate" | head -1 | cut -d: -f1)
        cargo_ln=$(printf "%s\n" "$1" | grep -nE "(^| )cargo (test|nextest)" | head -1 | cut -d: -f1)
        [ -n "$gate_ln" ] && [ -n "$cargo_ln" ] && [ "$gate_ln" -lt "$cargo_ln" ]
    ' _ "$PLAN_TEST"

# (c) lint plan does NOT contain psi-gate
assert "wiring: 'lint --print-plan' does NOT contain verify.sh psi-gate" \
    bash -c '! printf "%s\n" "$1" | grep -q "verify\.sh psi-gate"' _ "$PLAN_LINT"

# (d) typecheck plan does NOT contain psi-gate
assert "wiring: 'typecheck --print-plan' does NOT contain verify.sh psi-gate" \
    bash -c '! printf "%s\n" "$1" | grep -q "verify\.sh psi-gate"' _ "$PLAN_TC"

# (e) regression guard: test plan still has >= 2 cargo lines (gate line has no 'cargo' token)
_cargo_count=$(printf "%s\n" "$PLAN_TEST" | grep -cE "(^| )cargo " || true)
assert "wiring: test plan still contains >= 2 cargo lines (no regression)" \
    test "$_cargo_count" -ge 2

# ---------------------------------------------------------------------------
# Cycle 7: compile-gate behavioral tests (task #4618)
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 7: compile-gate ---"

# run_compile_gate <proc_path> [VAR=val ...]
# Invokes `verify.sh compile-gate` with the given PSI proc path and any
# additional env overrides.  After returning:
#   GATE_RC     — exit code of the invocation
#   GATE_STDERR — captured stderr text
# No dispatch file arg: compile_gate has no WINDOW/coordination flock.
run_compile_gate() {
    local proc="$1"
    shift
    local _stderr_file
    _stderr_file="$(mktemp -p "$WORKDIR" cg-stderr.XXXXXX)"
    GATE_RC=0
    GATE_STDERR=""
    env "$@" \
        REIFY_COMPILE_GATE_PROC_PATH="$proc" \
        bash "$VERIFY" compile-gate \
        2>"$_stderr_file" \
        || GATE_RC=$?
    GATE_STDERR="$(cat "$_stderr_file")"
    rm -f "$_stderr_file"
}

# (7a) avg10=40 < default threshold 85 -> exit 0, returns fast (< 2s)
PSI_7A="$(make_psi_fixture 40)"
T7A_0=$(date +%s)
run_compile_gate "$PSI_7A"
T7A_1=$(date +%s)
ELAPSED_7A=$(( T7A_1 - T7A_0 ))
assert "7a: avg10=40 < threshold=85 (default) → exit 0" \
    test "$GATE_RC" -eq 0
assert "7a: returned fast (< 2s)" \
    test "$ELAPSED_7A" -lt 2

# (7b) avg10=99 >= threshold, short MAX_WAIT -> ADMITS (exit 0), elapsed >= MAX_WAIT,
#      stderr contains an admit/fairness message. NOT exit 75.
PSI_7B="$(make_psi_fixture 99)"
T7B_0=$(date +%s)
run_compile_gate "$PSI_7B" \
    REIFY_COMPILE_GATE_MAX_WAIT=2 REIFY_COMPILE_GATE_POLL=1
T7B_1=$(date +%s)
ELAPSED_7B=$(( T7B_1 - T7B_0 ))
assert "7b: avg10=99 >= threshold, MAX_WAIT=2 → exit 0 (admit, NOT exit 75)" \
    test "$GATE_RC" -eq 0
assert "7b: NOT exit 75 (never requeues)" \
    test "$GATE_RC" -ne 75
assert "7b: elapsed >= MAX_WAIT=2s (waited before admitting)" \
    test "$ELAPSED_7B" -ge 2
assert "7b: stderr contains admit/fairness floor message" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "admit|fairness|proceeding under load|sustained pressure"' _ "$GATE_STDERR"

# (7c) DF_VERIFY_ROLE=merge + avg10=99 → exit 0 fast (CAVEAT 1: merge never waits)
PSI_7C="$(make_psi_fixture 99)"
T7C_0=$(date +%s)
run_compile_gate "$PSI_7C" \
    DF_VERIFY_ROLE=merge REIFY_COMPILE_GATE_MAX_WAIT=5 REIFY_COMPILE_GATE_POLL=1
T7C_1=$(date +%s)
ELAPSED_7C=$(( T7C_1 - T7C_0 ))
assert "7c: merge bypass: exit 0" \
    test "$GATE_RC" -eq 0
assert "7c: merge bypass: returned fast (< 2s)" \
    test "$ELAPSED_7C" -lt 2

# (7d) REIFY_COMPILE_GATE_DISABLE=1 + avg10=99 -> exit 0 fast
PSI_7D="$(make_psi_fixture 99)"
T7D_0=$(date +%s)
run_compile_gate "$PSI_7D" \
    REIFY_COMPILE_GATE_DISABLE=1 REIFY_COMPILE_GATE_MAX_WAIT=5 REIFY_COMPILE_GATE_POLL=1
T7D_1=$(date +%s)
ELAPSED_7D=$(( T7D_1 - T7D_0 ))
assert "7d: DISABLE=1 + avg10=99 → exit 0 fast" \
    test "$GATE_RC" -eq 0
assert "7d: DISABLE: returned fast (< 2s)" \
    test "$ELAPSED_7D" -lt 2

# (7e) missing PROC_PATH -> exit 0 + fail-open warning
NONEXISTENT_PSI_7E="$WORKDIR/nope/compile-pressure-cpu"  # guaranteed absent
run_compile_gate "$NONEXISTENT_PSI_7E"
assert "7e: missing PROC_PATH → exit 0 (fail-open)" \
    test "$GATE_RC" -eq 0
assert "7e: fail-open warning in stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "PSI|pressure|missing|warn|fail.open|kernel lacks"' _ "$GATE_STDERR"

# (7f) Leniency cross-check at avg10=70 (between compile-threshold=85 and test-threshold=50):
#   - compile-gate (threshold 85) admits fast (exit 0, < 2s) — 70 < 85
#   - SAME fixture through verify.sh psi-gate (test threshold 50) blocks → exit 75 — 70 >= 50
#   This is a structural threshold-ordering check proving compile_gate is strictly
#   more lenient than psi_gate (CAVEAT 2: a single merge's pressure level won't trip it).
PSI_7F="$(make_psi_fixture 70)"
DISPATCH_7F="$(mktemp -u -p "$WORKDIR" dispatch-7f.XXXXXX)"

# compile-gate with avg10=70 (default threshold 85 → 70 < 85 → admit fast)
T7F_0=$(date +%s)
run_compile_gate "$PSI_7F" REIFY_COMPILE_GATE_MAX_WAIT=5 REIFY_COMPILE_GATE_POLL=1
T7F_1=$(date +%s)
ELAPSED_7F=$(( T7F_1 - T7F_0 ))
assert "7f-compile: avg10=70 < threshold=85 → compile-gate exits 0 fast" \
    test "$GATE_RC" -eq 0
assert "7f-compile: compile-gate admits fast (< 2s)" \
    test "$ELAPSED_7F" -lt 2

# psi-gate (test gate, threshold 50) with avg10=70 → 70 >= 50 → blocks → exit 75
run_gate "$DISPATCH_7F" "$PSI_7F" \
    REIFY_PSI_GATE_THRESHOLD=50 REIFY_PSI_GATE_MAX_WAIT=2 REIFY_PSI_GATE_POLL=1
assert "7f-test: avg10=70 >= test-threshold=50 → psi-gate exit 75 (would block)" \
    test "$GATE_RC" -eq 75

test_summary
