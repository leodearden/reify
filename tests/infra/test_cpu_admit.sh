#!/usr/bin/env bash
# tests/infra/test_cpu_admit.sh — integration tests for scripts/cpu-admit.sh.
#
# Drives `cpu-admit.sh <mode>` in isolation with injected PSI fixtures and
# verifies the α-layer wiring contract (verify.sh sources cpu-admit.sh; guard
# classifies it as load-bearing).  Modeled on scripts/test_psi_gate.sh.
#
# Skip guard: exits 0 (skip) on hosts without /proc/pressure/cpu.
# Fail-open (missing PSI source) is still exercised via PROC_PATH override.
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).
#
# COVERAGE NOTE: this file drives cpu-admit.sh only via the direct-exec (CLI)
# path, where _ca_window/_ca_dispatch are empty.  The flock-coordinated critical
# section (_cpu_admit_psi_should_pass, WINDOW spacing, concurrent-burst
# atomicity) is NOT exercised here — it is covered transitively by
# scripts/test_psi_gate.sh Cycles 2–4 (which call `verify.sh psi-gate`, the
# thin wrapper that enables the window+dispatch path).  A maintainer reading
# this file alone should consult test_psi_gate.sh for the flock/window contract.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CPU_ADMIT="$REPO_ROOT/scripts/cpu-admit.sh"
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

# ---------------------------------------------------------------------------
# Harness helpers
# ---------------------------------------------------------------------------

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

# run_cpu_admit <mode> <proc_path> [VAR=val ...]
# Invokes `bash scripts/cpu-admit.sh <mode>` with the given PSI proc path,
# plus any additional env overrides.  After returning:
#   ADMIT_RC     — exit code of the invocation
#   ADMIT_STDERR — captured stderr text
ADMIT_RC=0
ADMIT_STDERR=""
run_cpu_admit() {
    local mode="$1" proc="$2"
    shift 2
    local _stderr_file
    _stderr_file="$(mktemp -p "$WORKDIR" admit-stderr.XXXXXX)"
    ADMIT_RC=0
    ADMIT_STDERR=""
    env "$@" \
        REIFY_CPU_ADMIT_PROC_PATH="$proc" \
        bash "$CPU_ADMIT" "$mode" \
        2>"$_stderr_file" \
        || ADMIT_RC=$?
    ADMIT_STDERR="$(cat "$_stderr_file")"
    rm -f "$_stderr_file"
}

echo "=== cpu-admit tests ==="

# ---------------------------------------------------------------------------
# Cycle A: low PSI admits instantly — both modes exit 0 (structural; no wall-clock)
# (G6-safe: PSI % comparisons mirroring the landed gates, no guessed thresholds)
# avg10=40 < default THRESHOLD=50 → BOTH admit and requeue exit 0 silently;
# absence of timeout markers proves the silent fast-admit path was taken.
# MAX_WAIT=1/POLL=1 bounds the regression failure window to ~1s: if the fast-path
# breaks and the poll loop runs, the timeout marker is emitted within seconds
# (not the default 300s), so the negated-grep assertions trip promptly.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle A: low PSI admits instantly ---"

PSI_A="$(make_psi_fixture 40)"

run_cpu_admit admit "$PSI_A" REIFY_CPU_ADMIT_MAX_WAIT=1 REIFY_CPU_ADMIT_POLL=1
assert "A-admit: avg10=40 < THRESHOLD=50 → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "A-admit: instant admit — no wait/timeout marker (fast-path taken)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "sustained pressure|fairness floor"' _ "$ADMIT_STDERR"

run_cpu_admit requeue "$PSI_A" REIFY_CPU_ADMIT_MAX_WAIT=1 REIFY_CPU_ADMIT_POLL=1
assert "A-requeue: avg10=40 < THRESHOLD=50 → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "A-requeue: instant admit — no wait/timeout marker (fast-path taken)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "gave up|cpu headroom"' _ "$ADMIT_STDERR"

# ---------------------------------------------------------------------------
# Cycle B: admit-on-timeout — avg10=99, REIFY_CPU_ADMIT_MAX_WAIT=2, mode=admit
# → exit 0 (NOT 75), elapsed >= 2s, stderr matches admit/fairness/sustained-pressure
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle B: admit-on-timeout ---"

PSI_B="$(make_psi_fixture 99)"
TB_0=$(date +%s)
run_cpu_admit admit "$PSI_B" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1
TB_1=$(date +%s)
ELAPSED_B=$(( TB_1 - TB_0 ))

assert "B: avg10=99, MAX_WAIT=2, mode=admit → exit 0 (NOT 75)" \
    test "$ADMIT_RC" -eq 0
assert "B: NOT exit 75 (admit never requeues)" \
    test "$ADMIT_RC" -ne 75
assert "B: elapsed >= MAX_WAIT=2s (waited before admitting)" \
    test "$ELAPSED_B" -ge 2
assert "B: stderr matches admit/fairness/sustained-pressure" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "admit|fairness|sustained pressure"' _ "$ADMIT_STDERR"

# ---------------------------------------------------------------------------
# Cycle C: requeue-on-timeout — avg10=99, REIFY_CPU_ADMIT_MAX_WAIT=2, mode=requeue
# → exit 75, elapsed >= 2s, stderr matches cpu headroom/gave up/psi
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle C: requeue-on-timeout ---"

PSI_C="$(make_psi_fixture 99)"
TC_0=$(date +%s)
run_cpu_admit requeue "$PSI_C" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1
TC_1=$(date +%s)
ELAPSED_C=$(( TC_1 - TC_0 ))

assert "C: avg10=99, MAX_WAIT=2, mode=requeue → exit 75 (EX_TEMPFAIL)" \
    test "$ADMIT_RC" -eq 75
assert "C: elapsed >= MAX_WAIT=2s" \
    test "$ELAPSED_C" -ge 2
assert "C: stderr matches cpu headroom/gave up/psi" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "cpu headroom|gave up|psi"' _ "$ADMIT_STDERR"

# ---------------------------------------------------------------------------
# Cycle D: merge bypass — DF_VERIFY_ROLE=merge + avg10=99 → exit 0 + stderr marks
# 'bypass (role=merge)' (structural; no wall-clock); MAX_WAIT=5/POLL=1 safety cap:
# without bypass would block on 99
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle D: merge bypass ---"

PSI_D="$(make_psi_fixture 99)"

run_cpu_admit admit "$PSI_D" \
    DF_VERIFY_ROLE=merge REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1
assert "D-admit: merge bypass → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "D-admit: merge bypass → stderr marks 'bypass (role=merge)'" \
    bash -c 'printf "%s\n" "$1" | grep -qF "bypass (role=merge)"' _ "$ADMIT_STDERR"

run_cpu_admit requeue "$PSI_D" \
    DF_VERIFY_ROLE=merge REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1
assert "D-requeue: merge bypass → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "D-requeue: merge bypass → stderr marks 'bypass (role=merge)'" \
    bash -c 'printf "%s\n" "$1" | grep -qF "bypass (role=merge)"' _ "$ADMIT_STDERR"

# ---------------------------------------------------------------------------
# Cycle E: fail-open — nonexistent PROC_PATH → exit 0 + stderr warning
# MAX_WAIT=5/POLL=1 safety: without fail-open would loop until timeout
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle E: fail-open ---"

NONEXISTENT_PSI="$WORKDIR/nope/pressure-cpu"   # guaranteed absent

run_cpu_admit admit "$NONEXISTENT_PSI" \
    REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1
assert "E-admit: nonexistent PROC_PATH → exit 0 (fail-open)" \
    test "$ADMIT_RC" -eq 0
assert "E-admit: stderr matches kernel lacks/fail-open/pressure/warn" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "kernel lacks|fail.open|pressure|warn"' _ "$ADMIT_STDERR"

run_cpu_admit requeue "$NONEXISTENT_PSI" \
    REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1
assert "E-requeue: nonexistent PROC_PATH → exit 0 (fail-open)" \
    test "$ADMIT_RC" -eq 0
assert "E-requeue: stderr matches kernel lacks/fail-open/pressure/warn" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "kernel lacks|fail.open|pressure|warn"' _ "$ADMIT_STDERR"

# ---------------------------------------------------------------------------
# Cycle F: DISABLE break-glass — REIFY_CPU_ADMIT_DISABLE=1 + avg10=99 → exit 0 +
# stderr marks 'disabled' (structural; no wall-clock);
# MAX_WAIT=5/POLL=1 safety: without DISABLE would block on avg10=99
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle F: DISABLE break-glass ---"

PSI_F="$(make_psi_fixture 99)"

run_cpu_admit admit "$PSI_F" \
    REIFY_CPU_ADMIT_DISABLE=1 REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1
assert "F-admit: DISABLE=1 + avg10=99 → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "F-admit: DISABLE break-glass → stderr marks 'disabled'" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "disabled"' _ "$ADMIT_STDERR"

run_cpu_admit requeue "$PSI_F" \
    REIFY_CPU_ADMIT_DISABLE=1 REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1
assert "F-requeue: DISABLE=1 + avg10=99 → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "F-requeue: DISABLE break-glass → stderr marks 'disabled'" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "disabled"' _ "$ADMIT_STDERR"

# ---------------------------------------------------------------------------
# Cycle G: bad mode (e.g. `cpu-admit.sh bogus`) → nonzero usage exit (64)
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle G: bad mode ---"

PSI_G="$(make_psi_fixture 0)"
run_cpu_admit "bogus" "$PSI_G"
assert "G: bogus mode → nonzero exit" \
    test "$ADMIT_RC" -ne 0
assert "G: bogus mode → exit 64 (usage error)" \
    test "$ADMIT_RC" -eq 64

# ---------------------------------------------------------------------------
# Cycle CS: PSI-gate (cpu_admit requeue) clock-stop cycle (step-5 / task 4837)
# Tests the @@REIFY_CLOCK_*@@ marker emission + MAX_WAIT=unlimited on the PSI path.
# RED today: cpu-admit.sh does not yet source lib_clock_stop.sh nor support
# MAX_WAIT=unlimited (step-6 will implement it).
#
# (CS-a) requeue MAX_WAIT=unlimited: high-PSI fixture cleared after ~2s by a
#         backgrounded updater → exit 0 (never 75), elapsed >= 1500ms, stderr has
#         @@REIFY_CLOCK_STOP@@ reason=psi_pressure + @@REIFY_CLOCK_START@@, and
#         with REIFY_CLOCK_HEARTBEAT_SECS=1 also @@REIFY_CLOCK_HEARTBEAT@@.
# (CS-b) admit mode under sustained pressure with short MAX_WAIT → exit 0,
#         stderr does NOT contain @@REIFY_CLOCK_STOP@@ (PRD D2 out-of-scope guard:
#         compile_gate admits-on-timeout, not a starvation source).
# (CS-c) requeue immediate-pass (low avg10) → exit 0, no STOP marker (balance).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle CS: PSI-gate clock-stop markers ---"

# CS-a: requeue MAX_WAIT=unlimited, fixture clears after ~2s → exit 0 + markers
PSI_CS_A="$(make_psi_fixture 99)"
_CS_A_STDERR="$(mktemp -p "$WORKDIR" cs-a-stderr.XXXXXX)"

# Background updater: overwrite fixture with low avg10 after 2s.
(
    sleep 2
    printf 'some avg10=10.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        > "$PSI_CS_A"
) &
_CS_A_UPDATER=$!

_CS_A_START_NS="$(date +%s%N)"
_CS_A_RC=0
timeout 30 \
    env REIFY_CPU_ADMIT_PROC_PATH="$PSI_CS_A" \
        REIFY_CPU_ADMIT_MAX_WAIT=unlimited \
        REIFY_CPU_ADMIT_POLL=1 \
        REIFY_CLOCK_HEARTBEAT_SECS=1 \
        bash "$CPU_ADMIT" requeue \
    2>"$_CS_A_STDERR" || _CS_A_RC=$?

_CS_A_END_NS="$(date +%s%N)"
_CS_A_ELAPSED_MS=$(( (_CS_A_END_NS - _CS_A_START_NS) / 1000000 ))

kill "$_CS_A_UPDATER" 2>/dev/null || true
wait "$_CS_A_UPDATER" 2>/dev/null || true

assert "CS-a: requeue MAX_WAIT=unlimited exits 0 (never 75; got $_CS_A_RC)" \
    test "$_CS_A_RC" -eq 0
assert "CS-a: elapsed >= 1500ms (blocked by high PSI until fixture cleared; got ${_CS_A_ELAPSED_MS}ms)" \
    test "$_CS_A_ELAPSED_MS" -ge 1500
assert "CS-a: stderr contains @@REIFY_CLOCK_STOP@@ reason=psi_pressure" \
    grep -q '@@REIFY_CLOCK_STOP@@ reason=psi_pressure' "$_CS_A_STDERR"
assert "CS-a: stderr contains @@REIFY_CLOCK_START@@" \
    grep -q '@@REIFY_CLOCK_START@@' "$_CS_A_STDERR"
assert "CS-a: stderr contains @@REIFY_CLOCK_HEARTBEAT@@ (HEARTBEAT_SECS=1 + ~2s hold)" \
    grep -q '@@REIFY_CLOCK_HEARTBEAT@@' "$_CS_A_STDERR"

# CS-b: admit mode (compile_gate path) under sustained pressure + short MAX_WAIT
# → exit 0 (admits-on-timeout), stderr does NOT contain @@REIFY_CLOCK_STOP@@
# (PRD D2: compile_gate is out-of-scope for clock-stop; bounded admits-on-timeout)
PSI_CS_B="$(make_psi_fixture 99)"
_CS_B_STDERR="$(mktemp -p "$WORKDIR" cs-b-stderr.XXXXXX)"
_CS_B_RC=0
env REIFY_CPU_ADMIT_PROC_PATH="$PSI_CS_B" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1 \
    bash "$CPU_ADMIT" admit \
    2>"$_CS_B_STDERR" || _CS_B_RC=$?

assert "CS-b: admit (compile_gate) under pressure exits 0 (admits-on-timeout; got $_CS_B_RC)" \
    test "$_CS_B_RC" -eq 0
assert "CS-b: admit mode does NOT emit @@REIFY_CLOCK_STOP@@ (PRD D2 out-of-scope)" \
    bash -c '! grep -q "@@REIFY_CLOCK_STOP@@" "$1"' _ "$_CS_B_STDERR"

# CS-c: requeue immediate-pass (low avg10 < threshold) → exit 0, no STOP marker (balance)
PSI_CS_C="$(make_psi_fixture 10)"
_CS_C_STDERR="$(mktemp -p "$WORKDIR" cs-c-stderr.XXXXXX)"
_CS_C_RC=0
env REIFY_CPU_ADMIT_PROC_PATH="$PSI_CS_C" \
    REIFY_CPU_ADMIT_MAX_WAIT=unlimited \
    REIFY_CPU_ADMIT_POLL=1 \
    bash "$CPU_ADMIT" requeue \
    2>"$_CS_C_STDERR" || _CS_C_RC=$?

assert "CS-c: requeue immediate-pass exits 0 (got $_CS_C_RC)" \
    test "$_CS_C_RC" -eq 0
assert "CS-c: immediate-pass emits NO @@REIFY_CLOCK_STOP@@ (fast path is silent)" \
    bash -c '! grep -q "@@REIFY_CLOCK_STOP@@" "$1"' _ "$_CS_C_STDERR"

# ---------------------------------------------------------------------------
# Cycle W: α wiring contract — verify.sh sources cpu-admit.sh; guard classifies
# it as load-bearing; plan shape is unchanged
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle W: α wiring (verify.sh ↔ cpu-admit.sh) ---"

# W1: verify.sh contains a real `source "$SCRIPT_DIR/cpu-admit.sh"` statement
# (anchored to start-of-line to exclude comment lines, mirrors test_verify_throughput.sh
# preflight at L75)
assert "W1: verify.sh contains source \"\$SCRIPT_DIR/cpu-admit.sh\"" \
    bash -c 'grep -qE "^[[:space:]]*source \"\\\$SCRIPT_DIR/cpu-admit\.sh\"" "$1"' _ "$VERIFY"

# W2: cpu-admit.sh is auto-classified as load-bearing by verify-pipeline-guard.sh
# (guard auto-derives sourced libs live from verify.sh's source lines)
assert "W2: verify-pipeline-guard.sh classifies cpu-admit.sh as load-bearing (exit 0)" \
    bash "$REPO_ROOT/scripts/verify-pipeline-guard.sh" requires-full-gate scripts/cpu-admit.sh

# W3: plan shape regression — all --scope all --print-plan still emits both
# verify.sh psi-gate and verify.sh compile-gate lines (role-invariant)
_PLAN_W3="$(bash "$VERIFY" all --scope all --print-plan 2>/dev/null | grep -v '^#')"
assert "W3: verify.sh all --print-plan still emits 'verify.sh psi-gate'" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh psi-gate"' _ "$_PLAN_W3"
assert "W3: verify.sh all --print-plan still emits 'verify.sh compile-gate'" \
    bash -c 'printf "%s\n" "$1" | grep -q "verify\.sh compile-gate"' _ "$_PLAN_W3"

# ---------------------------------------------------------------------------
# Cycle V: psi_gate() wrapper wiring — @@REIFY_CLOCK_*@@ markers via verify.sh psi-gate
# (step-12 / task 4837; drives the real verify.sh psi-gate entry, NOT cpu-admit.sh directly)
#
# The CS cycle confirmed markers on the direct cpu-admit.sh path.  These tests confirm that
# verify.sh psi_gate() properly sets _ca_clock_reason so cpu_admit's unlimited-mode detection
# fires and markers are emitted on the real verify.sh path.
#
# run_psi_gate_wrapper(): models on test_psi_gate.sh run_gate — REIFY_PSI_GATE_* env overrides
# + bash "$VERIFY" psi-gate; make_psi_fixture is already defined above.
#
# (V-a) WINDOW-forced wait — pre-touched dispatch (mtime=now), avg10=0, WINDOW=3,
#         POLL=1, MAX_WAIT=unlimited, HEARTBEAT_SECS=1:
#         → exit 0 (NOT 75, NOT a set-u abort), elapsed >= 2000ms,
#           stderr has @@REIFY_CLOCK_STOP@@ reason=psi_pressure + HEARTBEAT + START.
#         RED today: psi_gate() omits _ca_clock_reason → _ca_unlimited=0 → deadline
#         arithmetic treats "unlimited" as var=0 → _deadline=_ca_start → first poll fails
#         the WINDOW check → deadline already elapsed → returns 75 (not 0).
# (V-b) Unbound-variable regression guard — MAX_WAIT=unlimited, immediately-passing
#         fixture (avg10=40 < threshold=50, absent dispatch → age >> window):
#         → exit 0 fast, NO @@REIFY_CLOCK_STOP@@ (uncontended balance).
#         Regression guard: currently passes (immediate admit before deadline check),
#         but would expose the deadline-arithmetic crash if the pass logic changes.
# (V-c) compile_gate confirmation — verify.sh compile-gate under avg10=99, MAX_WAIT=2:
#         → admits exit 0, NO @@REIFY_CLOCK_STOP@@.
#         Confirms compile_gate() intentionally leaves _ca_clock_reason unset (PRD D2).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle V: psi_gate() wrapper wiring (verify.sh psi-gate clock-stop markers) ---"

# V-a: WINDOW-forced wait with unlimited + clock-stop markers.
# Pre-touch dispatch to "now" so age=0, WINDOW=3 → gate blocks ~3s before passing.
DISPATCH_V_A="$(mktemp -p "$WORKDIR" dispatch-va.XXXXXX)"
touch "$DISPATCH_V_A"
PSI_V_A="$(make_psi_fixture 0)"   # avg10=0: PSI is clear, only WINDOW forces the wait
_V_A_STDERR="$(mktemp -p "$WORKDIR" va-stderr.XXXXXX)"
_V_A_RC=0
_V_A_START_NS="$(date +%s%N)"
timeout 30 \
    env REIFY_PSI_GATE_DISPATCH_FILE="$DISPATCH_V_A" \
        REIFY_PSI_GATE_PROC_PATH="$PSI_V_A" \
        REIFY_PSI_GATE_WINDOW=3 \
        REIFY_PSI_GATE_POLL=1 \
        REIFY_PSI_GATE_MAX_WAIT=unlimited \
        REIFY_CLOCK_HEARTBEAT_SECS=1 \
        bash "$VERIFY" psi-gate \
    2>"$_V_A_STDERR" || _V_A_RC=$?
_V_A_END_NS="$(date +%s%N)"
_V_A_ELAPSED_MS=$(( (_V_A_END_NS - _V_A_START_NS) / 1000000 ))

assert "V-a: WINDOW-forced wait + unlimited → exit 0 (not 75, not set-u abort; got $_V_A_RC)" \
    test "$_V_A_RC" -eq 0
assert "V-a: elapsed >= 2000ms (WINDOW=3s forces wait; got ${_V_A_ELAPSED_MS}ms)" \
    test "$_V_A_ELAPSED_MS" -ge 2000
assert "V-a: stderr contains @@REIFY_CLOCK_STOP@@ reason=psi_pressure" \
    grep -q '@@REIFY_CLOCK_STOP@@ reason=psi_pressure' "$_V_A_STDERR"
assert "V-a: stderr contains @@REIFY_CLOCK_HEARTBEAT@@" \
    grep -q '@@REIFY_CLOCK_HEARTBEAT@@' "$_V_A_STDERR"
assert "V-a: stderr contains @@REIFY_CLOCK_START@@" \
    grep -q '@@REIFY_CLOCK_START@@' "$_V_A_STDERR"

# V-b: unbound-variable regression guard — MAX_WAIT=unlimited + immediately-passing fixture.
# Absent dispatch → stat returns mtime=0 → age=now-0 >> window=20 (default) → passes on
# first check without entering the wait; confirms the deadline arithmetic is never a crash.
DISPATCH_V_B="$(mktemp -u -p "$WORKDIR" dispatch-vb.XXXXXX)"  # name only, file absent → age >> window
PSI_V_B="$(make_psi_fixture 40)"   # avg10=40 < threshold=50 → PSI passes immediately
_V_B_STDERR="$(mktemp -p "$WORKDIR" vb-stderr.XXXXXX)"
_V_B_RC=0
timeout 15 \
    env REIFY_PSI_GATE_DISPATCH_FILE="$DISPATCH_V_B" \
        REIFY_PSI_GATE_PROC_PATH="$PSI_V_B" \
        REIFY_PSI_GATE_MAX_WAIT=unlimited \
        REIFY_PSI_GATE_POLL=1 \
    bash "$VERIFY" psi-gate \
    2>"$_V_B_STDERR" || _V_B_RC=$?

assert "V-b: unlimited + immediate-pass → exit 0 (unbound-variable regression; got $_V_B_RC)" \
    test "$_V_B_RC" -eq 0
assert "V-b: immediate-pass emits NO @@REIFY_CLOCK_STOP@@ (uncontended balance)" \
    bash -c '! grep -q "@@REIFY_CLOCK_STOP@@" "$1"' _ "$_V_B_STDERR"

# V-c: compile_gate (verify.sh compile-gate) under sustained pressure → admits, no CLOCK_STOP.
# Confirms compile_gate() intentionally leaves _ca_clock_reason unset (PRD D2: bounded
# admit-on-timeout is not a starvation source; clock-stop is out of scope for compile_gate).
PSI_V_C="$(make_psi_fixture 99)"
_V_C_STDERR="$(mktemp -p "$WORKDIR" vc-stderr.XXXXXX)"
_V_C_RC=0
timeout 15 \
    env REIFY_COMPILE_GATE_PROC_PATH="$PSI_V_C" \
        REIFY_COMPILE_GATE_MAX_WAIT=2 \
        REIFY_COMPILE_GATE_POLL=1 \
    bash "$VERIFY" compile-gate \
    2>"$_V_C_STDERR" || _V_C_RC=$?

assert "V-c: compile-gate under sustained pressure admits (exit 0; got $_V_C_RC)" \
    test "$_V_C_RC" -eq 0
assert "V-c: compile-gate emits NO @@REIFY_CLOCK_STOP@@ (PRD D2; intentionally unset)" \
    bash -c '! grep -q "@@REIFY_CLOCK_STOP@@" "$1"' _ "$_V_C_STDERR"

test_summary
