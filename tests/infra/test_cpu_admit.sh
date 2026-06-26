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
# make_mem_psi_fixture <memfull> [memsome]
# Writes a /proc/pressure/memory-formatted fixture (some + full lines) and
# echoes its path.  memsome defaults to 0 if not specified.
# ---------------------------------------------------------------------------
make_mem_psi_fixture() {
    local memfull="$1"
    local memsome="${2:-0}"
    local fixture
    fixture="$(mktemp -p "$WORKDIR" mem-psi-fixture.XXXXXX)"
    printf 'some avg10=%s avg60=0.00 avg300=0.00 total=0\nfull avg10=%s avg60=0.00 avg300=0.00 total=0\n' \
        "$memsome" "$memfull" > "$fixture"
    echo "$fixture"
}

# ---------------------------------------------------------------------------
# Cycle H: memfull backoff via CLI core
# Inject quiet CPU(0) + memfull=50 >= threshold=10 → gate backs off on memory.
# H1/H2 are RED drivers (no memory dimension yet → instant admit → fail).
# H3/H4 guard correct non-blocking cases.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle H: memfull backoff via CLI core ---"

PSI_H_CPU="$(make_psi_fixture 0)"          # quiet CPU: avg10=0
PSI_H_MEM50="$(make_mem_psi_fixture 50)"   # memfull=50, memsome=0

# H1: admit mode, quiet CPU + memfull=50 >= threshold=10, MAX_WAIT=2/POLL=1
# → exit 0 (admit-on-timeout) AND elapsed >= 2 AND stderr matches admit/fairness
TH1_0=$(date +%s)
run_cpu_admit admit "$PSI_H_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_H_MEM50" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1
TH1_1=$(date +%s)
ELAPSED_H1=$(( TH1_1 - TH1_0 ))

assert "H1: quiet CPU + memfull=50 >= threshold=10, admit → exit 0 (admit-on-timeout)" \
    test "$ADMIT_RC" -eq 0
assert "H1: elapsed >= MAX_WAIT=2s (backed off on memory before admitting)" \
    test "$ELAPSED_H1" -ge 2
assert "H1: stderr matches admit/fairness/sustained-pressure (memory backoff confirmed)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "admit|fairness|sustained pressure"' _ "$ADMIT_STDERR"

# H2: requeue mode, quiet CPU + memfull=50, MAX_WAIT=2/POLL=1 → exit 75
TH2_0=$(date +%s)
run_cpu_admit requeue "$PSI_H_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_H_MEM50" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1
TH2_1=$(date +%s)
ELAPSED_H2=$(( TH2_1 - TH2_0 ))

assert "H2: quiet CPU + memfull=50 >= threshold=10, requeue → exit 75" \
    test "$ADMIT_RC" -eq 75
assert "H2: elapsed >= MAX_WAIT=2s" \
    test "$ELAPSED_H2" -ge 2

# H3 guard: memfull=5 < threshold=10 → fast admit (no backoff)
PSI_H_MEM5="$(make_mem_psi_fixture 5)"
TH3_0=$(date +%s)
run_cpu_admit admit "$PSI_H_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_H_MEM5" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1
TH3_1=$(date +%s)
ELAPSED_H3=$(( TH3_1 - TH3_0 ))

assert "H3: memfull=5 < threshold=10 → fast admit exit 0" \
    test "$ADMIT_RC" -eq 0
assert "H3: memfull=5 < threshold=10 → fast (elapsed < 2)" \
    test "$ELAPSED_H3" -lt 2
assert "H3: memfull=5 < threshold=10 → no sustained-pressure marker" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "sustained pressure|fairness floor"' _ "$ADMIT_STDERR"

# H4: merge bypass — DF_VERIFY_ROLE=merge + memfull=50 → exit 0 fast
TH4_0=$(date +%s)
run_cpu_admit admit "$PSI_H_CPU" \
    DF_VERIFY_ROLE=merge \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_H_MEM50" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1
TH4_1=$(date +%s)
ELAPSED_H4=$(( TH4_1 - TH4_0 ))

assert "H4: merge bypass + memfull=50 → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "H4: merge bypass → fast (elapsed < 2)" \
    test "$ELAPSED_H4" -lt 2
assert "H4: merge bypass → stderr marks 'bypass (role=merge)'" \
    bash -c 'printf "%s\n" "$1" | grep -qF "bypass (role=merge)"' _ "$ADMIT_STDERR"

# ---------------------------------------------------------------------------
# Cycle I: memsome early-warning + memory fail-open via CLI
# I1 is the RED driver: memsome backoff not yet implemented (only memfull exists).
# I2 guards fail-open on unreadable memory source.
# I3 guards memsome below threshold → fast admit.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle I: memsome early-warning + memory fail-open via CLI ---"

PSI_I_CPU="$(make_psi_fixture 0)"          # quiet CPU: avg10=0

# I1 (RED driver): quiet CPU + memfull=0 + memsome=50 + SOME_THRESHOLD=10,
# requeue mode, MAX_WAIT=2/POLL=1 → exit 75 (memsome backs off)
# After step-2 only memfull exists; memsome ignored → instant admit exit 0 → I1 fails.
PSI_I_MEM_SOME50="$(make_mem_psi_fixture 0 50)"   # memfull=0, memsome=50
TI1_0=$(date +%s)
run_cpu_admit requeue "$PSI_I_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_I_MEM_SOME50" \
    REIFY_CPU_ADMIT_MEM_SOME_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1
TI1_1=$(date +%s)
ELAPSED_I1=$(( TI1_1 - TI1_0 ))

assert "I1: quiet CPU + memfull=0 + memsome=50 >= some_threshold=10, requeue → exit 75" \
    test "$ADMIT_RC" -eq 75
assert "I1: elapsed >= MAX_WAIT=2s (backed off on memsome)" \
    test "$ELAPSED_I1" -ge 2

# I2: fail-open — nonexistent memory PROC_PATH + FULL_THRESHOLD=10 + quiet CPU
# → fast admit exit 0 (memory source unreadable → fail-open, never blocks)
NONEXISTENT_MEM="$WORKDIR/nope/pressure-memory"
TI2_0=$(date +%s)
run_cpu_admit admit "$PSI_I_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$NONEXISTENT_MEM" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1
TI2_1=$(date +%s)
ELAPSED_I2=$(( TI2_1 - TI2_0 ))

assert "I2: nonexistent memory PROC_PATH + threshold=10 → exit 0 (fail-open)" \
    test "$ADMIT_RC" -eq 0
assert "I2: fail-open → fast (elapsed < 2)" \
    test "$ELAPSED_I2" -lt 2

# I3 guard: memsome=5 < some_threshold=10 + memfull=0 → fast admit
PSI_I_MEM_SOME5="$(make_mem_psi_fixture 0 5)"   # memfull=0, memsome=5
TI3_0=$(date +%s)
run_cpu_admit requeue "$PSI_I_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_I_MEM_SOME5" \
    REIFY_CPU_ADMIT_MEM_SOME_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1
TI3_1=$(date +%s)
ELAPSED_I3=$(( TI3_1 - TI3_0 ))

assert "I3: memsome=5 < some_threshold=10 + memfull=0 → fast admit exit 0" \
    test "$ADMIT_RC" -eq 0
assert "I3: fast (elapsed < 2)" \
    test "$ELAPSED_I3" -lt 2

# ---------------------------------------------------------------------------
# Cycle K: psi_gate wrapper memory wiring (default-ON, flock path)
# K1/K4 are RED drivers: psi_gate does not yet set _ca_mem_* so memory gating
# is disabled → no backoff on memfull=50 → instant exit 0 → fail.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle K: psi_gate wrapper memory wiring ---"

PSI_K_CPU="$(make_psi_fixture 0)"          # quiet CPU: avg10=0
PSI_K_MEM50="$(make_mem_psi_fixture 50)"   # memfull=50
PSI_K_MEM0="$(make_mem_psi_fixture 0)"     # quiet memory: memfull=0

run_psi_gate_mem() {
    # Invoke `bash verify.sh psi-gate` with isolated dispatch file, CPU fixture,
    # and memory fixture.  Remaining args are passed as env overrides.
    local cpu_path="$1" mem_path="$2"
    shift 2
    local dispatch_file
    dispatch_file="$(mktemp -p "$WORKDIR" psi-dispatch.XXXXXX)"
    local _stderr_file
    _stderr_file="$(mktemp -p "$WORKDIR" psi-gate-stderr.XXXXXX)"
    ADMIT_RC=0
    ADMIT_STDERR=""
    env "$@" \
        REIFY_PSI_GATE_PROC_PATH="$cpu_path" \
        REIFY_PSI_GATE_MEM_PROC_PATH="$mem_path" \
        REIFY_PSI_GATE_DISPATCH_FILE="$dispatch_file" \
        bash "$VERIFY" psi-gate \
        2>"$_stderr_file" \
        || ADMIT_RC=$?
    ADMIT_STDERR="$(cat "$_stderr_file")"
    rm -f "$_stderr_file" "$dispatch_file" "${dispatch_file}.lock" 2>/dev/null || true
}

# K1 (RED driver): quiet CPU + memfull=50 + explicit MEM_FULL_THRESHOLD=10,
# WINDOW=0, MAX_WAIT=2/POLL=1 → exit 75 (psi_gate backs off on memory, requeues)
TK1_0=$(date +%s)
run_psi_gate_mem "$PSI_K_CPU" "$PSI_K_MEM50" \
    REIFY_PSI_GATE_MEM_FULL_THRESHOLD=10 \
    REIFY_PSI_GATE_WINDOW=0 \
    REIFY_PSI_GATE_MAX_WAIT=2 \
    REIFY_PSI_GATE_POLL=1
TK1_1=$(date +%s)
ELAPSED_K1=$(( TK1_1 - TK1_0 ))

assert "K1: quiet CPU + memfull=50 >= threshold=10, psi_gate → exit 75" \
    test "$ADMIT_RC" -eq 75
assert "K1: elapsed >= MAX_WAIT=2s (psi_gate backed off on memory)" \
    test "$ELAPSED_K1" -ge 2

# K2: merge bypass — same + DF_VERIFY_ROLE=merge → exit 0 fast
TK2_0=$(date +%s)
run_psi_gate_mem "$PSI_K_CPU" "$PSI_K_MEM50" \
    DF_VERIFY_ROLE=merge \
    REIFY_PSI_GATE_MEM_FULL_THRESHOLD=10 \
    REIFY_PSI_GATE_WINDOW=0 \
    REIFY_PSI_GATE_MAX_WAIT=2 \
    REIFY_PSI_GATE_POLL=1
TK2_1=$(date +%s)
ELAPSED_K2=$(( TK2_1 - TK2_0 ))

assert "K2: merge bypass + memfull=50 → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "K2: merge bypass → fast (elapsed < 2)" \
    test "$ELAPSED_K2" -lt 2

# K3: CPU-only unchanged regression — quiet CPU + quiet memory → exit 0 fast
TK3_0=$(date +%s)
run_psi_gate_mem "$PSI_K_CPU" "$PSI_K_MEM0" \
    REIFY_PSI_GATE_MEM_FULL_THRESHOLD=10 \
    REIFY_PSI_GATE_WINDOW=0 \
    REIFY_PSI_GATE_MAX_WAIT=2 \
    REIFY_PSI_GATE_POLL=1
TK3_1=$(date +%s)
ELAPSED_K3=$(( TK3_1 - TK3_0 ))

assert "K3: quiet CPU + quiet memory → exit 0 (CPU-only unchanged)" \
    test "$ADMIT_RC" -eq 0
assert "K3: quiet CPU + quiet memory → fast (elapsed < 2)" \
    test "$ELAPSED_K3" -lt 2

# K4 (RED driver): default-ON — memfull=50 + NO explicit REIFY_PSI_GATE_MEM_FULL_THRESHOLD
# (rely on wrapper default=10) + quiet CPU + MAX_WAIT=2 → exit 75 (default threshold engages)
TK4_0=$(date +%s)
run_psi_gate_mem "$PSI_K_CPU" "$PSI_K_MEM50" \
    REIFY_PSI_GATE_WINDOW=0 \
    REIFY_PSI_GATE_MAX_WAIT=2 \
    REIFY_PSI_GATE_POLL=1
TK4_1=$(date +%s)
ELAPSED_K4=$(( TK4_1 - TK4_0 ))

assert "K4: default-ON threshold: memfull=50 + no explicit threshold → exit 75" \
    test "$ADMIT_RC" -eq 75
assert "K4: elapsed >= MAX_WAIT=2s (default threshold engaged)" \
    test "$ELAPSED_K4" -ge 2

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
