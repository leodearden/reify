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
# Cycle A: low PSI admits instantly — both modes exit 0 fast
# (G6-safe: PSI % comparisons mirroring the landed gates, no guessed thresholds)
# avg10=40 < default THRESHOLD=50 → BOTH admit and requeue exit 0, elapsed < 2s
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle A: low PSI admits instantly ---"

PSI_A="$(make_psi_fixture 40)"

TA_0=$(date +%s)
run_cpu_admit admit "$PSI_A"
TA_1=$(date +%s)
ELAPSED_A=$(( TA_1 - TA_0 ))
assert "A-admit: avg10=40 < THRESHOLD=50 → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "A-admit: returned fast (< 2s)" \
    test "$ELAPSED_A" -lt 2

TA2_0=$(date +%s)
run_cpu_admit requeue "$PSI_A"
TA2_1=$(date +%s)
ELAPSED_A2=$(( TA2_1 - TA2_0 ))
assert "A-requeue: avg10=40 < THRESHOLD=50 → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "A-requeue: returned fast (< 2s)" \
    test "$ELAPSED_A2" -lt 2

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
# Cycle D: merge bypass — DF_VERIFY_ROLE=merge + avg10=99 → exit 0 fast (< 2s)
# both modes; MAX_WAIT=5/POLL=1 safety cap: without bypass would block on 99
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle D: merge bypass ---"

PSI_D="$(make_psi_fixture 99)"

TD_0=$(date +%s)
run_cpu_admit admit "$PSI_D" \
    DF_VERIFY_ROLE=merge REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1
TD_1=$(date +%s)
ELAPSED_D=$(( TD_1 - TD_0 ))
assert "D-admit: merge bypass → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "D-admit: merge bypass → returned fast (< 2s)" \
    test "$ELAPSED_D" -lt 2

TD2_0=$(date +%s)
run_cpu_admit requeue "$PSI_D" \
    DF_VERIFY_ROLE=merge REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1
TD2_1=$(date +%s)
ELAPSED_D2=$(( TD2_1 - TD2_0 ))
assert "D-requeue: merge bypass → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "D-requeue: merge bypass → returned fast (< 2s)" \
    test "$ELAPSED_D2" -lt 2

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
# Cycle F: DISABLE break-glass — REIFY_CPU_ADMIT_DISABLE=1 + avg10=99 → exit 0 fast
# MAX_WAIT=5/POLL=1 safety: without DISABLE would block on avg10=99
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle F: DISABLE break-glass ---"

PSI_F="$(make_psi_fixture 99)"

TF_0=$(date +%s)
run_cpu_admit admit "$PSI_F" \
    REIFY_CPU_ADMIT_DISABLE=1 REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1
TF_1=$(date +%s)
ELAPSED_F=$(( TF_1 - TF_0 ))
assert "F-admit: DISABLE=1 + avg10=99 → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "F-admit: returned fast (< 2s)" \
    test "$ELAPSED_F" -lt 2

TF2_0=$(date +%s)
run_cpu_admit requeue "$PSI_F" \
    REIFY_CPU_ADMIT_DISABLE=1 REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1
TF2_1=$(date +%s)
ELAPSED_F2=$(( TF2_1 - TF2_0 ))
assert "F-requeue: DISABLE=1 + avg10=99 → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "F-requeue: returned fast (< 2s)" \
    test "$ELAPSED_F2" -lt 2

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

test_summary
