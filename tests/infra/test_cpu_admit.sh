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
# Hermeticity: neutralize default-ON memory gating for the verify.sh wrapper paths
# AND the direct cpu-admit CLI path.
# psi_gate()/compile_gate() default REIFY_{PSI_GATE,COMPILE_GATE}_MEM_FULL_THRESHOLD
# to 10 (memory dimension default-ON).  The clock-stop wrapper cycles inherited from
# task 4837 (Cycle V) drive `bash "$VERIFY" psi-gate`/`compile-gate` WITHOUT a memory
# fixture, so without an override they read the live /proc/pressure/memory value and
# would block/flake on a memory-loaded host (esc-4861-101: pre-land merge of 4837 +
# this task surfaced V-a/V-b/V-c hangs).  Export a quiet memory fixture (memfull=0) so
# all wrapper subprocesses inherit a deterministic memory-ok state regardless of host
# load.  Per-case memory tests (Cycles H/I/K/L/M) override REIFY_*_MEM_PROC_PATH via
# their own env and are unaffected.  Mirrors the neutralization in
# scripts/test_psi_gate.sh (task 4861 step-9).
# As of task 4911 the direct cpu-admit CLI ALSO defaults memfull threshold to 10
# (memory dimension default-ON on the CLI/agent axis, matching the wrappers above),
# so cycles that invoke cpu-admit.sh directly without an explicit
# REIFY_CPU_ADMIT_MEM_PROC_PATH override (e.g. Cycle A, Cycle CS) would otherwise
# read the live /proc/pressure/memory value too — export the same quiet fixture for
# that knob so those cycles stay deterministic.
_MEM_PSI_QUIET="$(mktemp -p "$WORKDIR" mem-psi-quiet.XXXXXX)"
printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
    > "$_MEM_PSI_QUIET"
export REIFY_PSI_GATE_MEM_PROC_PATH="$_MEM_PSI_QUIET"
export REIFY_COMPILE_GATE_MEM_PROC_PATH="$_MEM_PSI_QUIET"
export REIFY_CPU_ADMIT_MEM_PROC_PATH="$_MEM_PSI_QUIET"
# ---------------------------------------------------------------------------

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

# run_cpu_admit_bounded <timeout_secs> <mode> <proc_path> [VAR=val ...]
# Like run_cpu_admit but wraps the invocation in `timeout <timeout_secs>` so a
# genuine continuous HOLD (task 4920: admit mode no longer admits-on-timeout)
# terminates the test deterministically instead of hanging the suite. When the
# hold outlives the bound, `timeout` SIGTERM-kills the process and ADMIT_RC is
# 124; ADMIT_STDERR still captures everything flushed before the kill
# (including any @@REIFY_CLOCK_STOP@@/HEARTBEAT markers already emitted).
run_cpu_admit_bounded() {
    local bound="$1" mode="$2" proc="$3"
    shift 3
    local _stderr_file
    _stderr_file="$(mktemp -p "$WORKDIR" admit-bounded-stderr.XXXXXX)"
    ADMIT_RC=0
    ADMIT_STDERR=""
    timeout "$bound" \
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
# Cycle B: admit-mode CONTINUOUS HOLD (task 4920 — admit-on-timeout removed).
# avg10=99 (stuck fixture), REIFY_CPU_ADMIT_MAX_WAIT=2 (now inoperative for
# admit — the hold ignores MAX_WAIT once a clock reason is set).
# B: proves the HOLD — fixture never clears, wrapped in `timeout 8` so the
#    test terminates deterministically → rc==124 (killed while still holding,
#    well past the old MAX_WAIT=2), stderr shows STOP + HEARTBEAT, but NEVER
#    an admit/fairness-floor message nor START (it was never admitted-on-timeout).
# B-drop: proves ADMIT-ON-PSI-DROP — same stuck start, but a background
#    updater clears the fixture to low avg10 after ~2s (the CS-a/V-a
#    R-technique) → exit 0, STOP+HEARTBEAT+START all present (balanced), no
#    fairness message (admits ONLY on PSI-drop, never on a timer).
# RED today: current cpu-admit.sh admits-on-timeout at MAX_WAIT=2s and the CLI
# sets no admit _ca_clock_reason, so `timeout 8` never fires (rc==0 at ~2s, no
# STOP marker at all) — these assertions fail fast against current code.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle B: admit-mode continuous hold (no admit-on-timeout) ---"

# B: the hold — fixture stays stuck at avg10=99 for the whole 8s window.
PSI_B="$(make_psi_fixture 99)"
run_cpu_admit_bounded 8 admit "$PSI_B" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1 REIFY_CLOCK_HEARTBEAT_SECS=1

assert "B: avg10=99 sustained → timeout 8 kills it (rc==124; HELD past old MAX_WAIT=2s)" \
    test "$ADMIT_RC" -eq 124
assert "B: stderr contains @@REIFY_CLOCK_STOP@@ reason=psi_pressure (hold entered)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_STOP@@ reason=psi_pressure"' _ "$ADMIT_STDERR"
assert "B: stderr contains @@REIFY_CLOCK_HEARTBEAT@@ (still holding, liveness)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_HEARTBEAT@@"' _ "$ADMIT_STDERR"
assert "B: stderr does NOT match admit/fairness/sustained-pressure (never admits-on-timeout)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "sustained pressure|fairness floor"' _ "$ADMIT_STDERR"
assert "B: stderr does NOT contain @@REIFY_CLOCK_START@@ (never admitted; STOP has no matching START)" \
    bash -c '! printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_START@@"' _ "$ADMIT_STDERR"

# B-drop: same stuck-fixture start, but a background updater clears it to
# avg10=10 after ~2s → the hold admits on the PSI drop, not on a timeout.
PSI_B_DROP="$(make_psi_fixture 99)"
(
    sleep 2
    printf 'some avg10=10.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        > "$PSI_B_DROP"
) &
_B_DROP_UPDATER=$!

run_cpu_admit_bounded 30 admit "$PSI_B_DROP" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1 REIFY_CLOCK_HEARTBEAT_SECS=1

kill "$_B_DROP_UPDATER" 2>/dev/null || true
wait "$_B_DROP_UPDATER" 2>/dev/null || true

assert "B-drop: admits once PSI drops (exit 0; got $ADMIT_RC)" \
    test "$ADMIT_RC" -eq 0
assert "B-drop: stderr contains @@REIFY_CLOCK_STOP@@ reason=psi_pressure" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_STOP@@ reason=psi_pressure"' _ "$ADMIT_STDERR"
assert "B-drop: stderr contains @@REIFY_CLOCK_HEARTBEAT@@" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_HEARTBEAT@@"' _ "$ADMIT_STDERR"
assert "B-drop: stderr contains @@REIFY_CLOCK_START@@ (STOP/START balanced)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_START@@"' _ "$ADMIT_STDERR"
assert "B-drop: stderr does NOT match fairness-floor admit-on-timeout message" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "sustained pressure|fairness floor"' _ "$ADMIT_STDERR"

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
# H1 (task 4920): now a HOLD proof — the memory dimension (task 4911) drives
# the same continuous-hold machinery as CPU pressure, not just admit-on-timeout.
# H2 (requeue exit 75) is unaffected by this task (admit-mode-only change) and
# stays as-is.  H3/H4 guard correct non-blocking cases (unaffected).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle H: memfull backoff via CLI core ---"

PSI_H_CPU="$(make_psi_fixture 0)"          # quiet CPU: avg10=0
PSI_H_MEM50="$(make_mem_psi_fixture 50)"   # memfull=50, memsome=0

# H1 (task 4920): admit-mode HOLD proof driven by the MEMORY dimension — quiet
# CPU(0) + stuck memfull=50 >= threshold=10, wrapped in `timeout 8` → rc==124
# (held well past the old MAX_WAIT=2s), stderr shows STOP+HEARTBEAT but never
# an admit/fairness marker nor START.  Proves the continuous hold reacts to
# memory PSI as well as CPU PSI (per 4911).
# RED today: current cpu-admit.sh admits-on-timeout at MAX_WAIT=2s and the CLI
# sets no admit _ca_clock_reason, so `timeout 8` never fires — fails fast.
run_cpu_admit_bounded 8 admit "$PSI_H_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_H_MEM50" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1 \
    REIFY_CLOCK_HEARTBEAT_SECS=1

assert "H1: quiet CPU + memfull=50 >= threshold=10 sustained → timeout 8 kills it (rc==124; held past old MAX_WAIT=2s)" \
    test "$ADMIT_RC" -eq 124
assert "H1: stderr contains @@REIFY_CLOCK_STOP@@ reason=psi_pressure (memory-driven hold entered)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_STOP@@ reason=psi_pressure"' _ "$ADMIT_STDERR"
assert "H1: stderr contains @@REIFY_CLOCK_HEARTBEAT@@ (still holding, liveness)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_HEARTBEAT@@"' _ "$ADMIT_STDERR"
assert "H1: stderr does NOT match admit/fairness/sustained-pressure (never admits-on-timeout)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "sustained pressure|fairness floor"' _ "$ADMIT_STDERR"
assert "H1: stderr does NOT contain @@REIFY_CLOCK_START@@ (never admitted; STOP has no matching START)" \
    bash -c '! printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_START@@"' _ "$ADMIT_STDERR"

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
run_cpu_admit admit "$PSI_H_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_H_MEM5" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1

assert "H3: memfull=5 < threshold=10 → fast admit exit 0" \
    test "$ADMIT_RC" -eq 0
assert "H3: memfull=5 < threshold=10 → no sustained-pressure marker" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "sustained pressure|fairness floor"' _ "$ADMIT_STDERR"

# H4: merge bypass — DF_VERIFY_ROLE=merge + memfull=50 → exit 0 fast
run_cpu_admit admit "$PSI_H_CPU" \
    DF_VERIFY_ROLE=merge \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_H_MEM50" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1

assert "H4: merge bypass + memfull=50 → exit 0" \
    test "$ADMIT_RC" -eq 0
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
run_cpu_admit admit "$PSI_I_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$NONEXISTENT_MEM" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1

assert "I2: nonexistent memory PROC_PATH + threshold=10 → exit 0 (fail-open)" \
    test "$ADMIT_RC" -eq 0

# I3 guard: memsome=5 < some_threshold=10 + memfull=0 → fast admit
PSI_I_MEM_SOME5="$(make_mem_psi_fixture 0 5)"   # memfull=0, memsome=5
run_cpu_admit requeue "$PSI_I_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_I_MEM_SOME5" \
    REIFY_CPU_ADMIT_MEM_SOME_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1

assert "I3: memsome=5 < some_threshold=10 + memfull=0 → fast admit exit 0" \
    test "$ADMIT_RC" -eq 0

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
run_psi_gate_mem "$PSI_K_CPU" "$PSI_K_MEM50" \
    DF_VERIFY_ROLE=merge \
    REIFY_PSI_GATE_MEM_FULL_THRESHOLD=10 \
    REIFY_PSI_GATE_WINDOW=0 \
    REIFY_PSI_GATE_MAX_WAIT=2 \
    REIFY_PSI_GATE_POLL=1

assert "K2: merge bypass + memfull=50 → exit 0" \
    test "$ADMIT_RC" -eq 0

# K3: CPU-only unchanged regression — quiet CPU + quiet memory → exit 0 fast
run_psi_gate_mem "$PSI_K_CPU" "$PSI_K_MEM0" \
    REIFY_PSI_GATE_MEM_FULL_THRESHOLD=10 \
    REIFY_PSI_GATE_WINDOW=0 \
    REIFY_PSI_GATE_MAX_WAIT=2 \
    REIFY_PSI_GATE_POLL=1

assert "K3: quiet CPU + quiet memory → exit 0 (CPU-only unchanged)" \
    test "$ADMIT_RC" -eq 0

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
# Cycle L: compile_gate wrapper memory wiring
# L1/L4 (task 4920): now HOLD proofs — compile_gate's memory-dimension backoff
# (task 4911) now drives the SAME continuous-hold machinery as CPU pressure
# (no more admit-on-timeout).  L2/L3 (merge bypass / quiet-memory fast-admit)
# are unaffected by this task and stay as-is.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle L: compile_gate wrapper memory wiring ---"

PSI_L_CPU="$(make_psi_fixture 0)"          # quiet CPU: avg10=0
PSI_L_MEM50="$(make_mem_psi_fixture 50)"   # memfull=50
PSI_L_MEM0="$(make_mem_psi_fixture 0)"     # quiet memory: memfull=0

run_compile_gate_mem() {
    # Invoke `bash verify.sh compile-gate` with CPU and memory fixtures.
    # Remaining args are passed as env overrides.
    local cpu_path="$1" mem_path="$2"
    shift 2
    local _stderr_file
    _stderr_file="$(mktemp -p "$WORKDIR" compile-gate-stderr.XXXXXX)"
    ADMIT_RC=0
    ADMIT_STDERR=""
    env "$@" \
        REIFY_COMPILE_GATE_PROC_PATH="$cpu_path" \
        REIFY_COMPILE_GATE_MEM_PROC_PATH="$mem_path" \
        bash "$VERIFY" compile-gate \
        2>"$_stderr_file" \
        || ADMIT_RC=$?
    ADMIT_STDERR="$(cat "$_stderr_file")"
    rm -f "$_stderr_file"
}

# run_compile_gate_mem_bounded <timeout_secs> <cpu_path> <mem_path> [VAR=val ...]
# Like run_compile_gate_mem but wraps the invocation in `timeout <timeout_secs>`
# so a genuine continuous HOLD (task 4920: compile_gate's admit-mode gate no
# longer admits-on-timeout) terminates the test deterministically instead of
# hanging the suite.  ADMIT_RC is 124 when the bound is hit mid-hold.
run_compile_gate_mem_bounded() {
    local bound="$1" cpu_path="$2" mem_path="$3"
    shift 3
    local _stderr_file
    _stderr_file="$(mktemp -p "$WORKDIR" compile-gate-bounded-stderr.XXXXXX)"
    ADMIT_RC=0
    ADMIT_STDERR=""
    timeout "$bound" \
        env "$@" \
            REIFY_COMPILE_GATE_PROC_PATH="$cpu_path" \
            REIFY_COMPILE_GATE_MEM_PROC_PATH="$mem_path" \
            bash "$VERIFY" compile-gate \
            2>"$_stderr_file" \
        || ADMIT_RC=$?
    ADMIT_STDERR="$(cat "$_stderr_file")"
    rm -f "$_stderr_file"
}

# L1 (task 4920): compile-gate HOLD proof — quiet CPU + stuck memfull=50 +
# explicit MEM_FULL_THRESHOLD=10, wrapped in `timeout 8` → rc==124 (held well
# past the old MAX_WAIT=2s), stderr shows STOP+HEARTBEAT on the verify.sh
# compile-gate path, never a fairness marker nor START.
# RED today: compile_gate still admits-on-timeout at MAX_WAIT=2s with no clock
# reason set → `timeout 8` never fires — fails fast against current code.
run_compile_gate_mem_bounded 8 "$PSI_L_CPU" "$PSI_L_MEM50" \
    REIFY_COMPILE_GATE_MEM_FULL_THRESHOLD=10 \
    REIFY_COMPILE_GATE_MAX_WAIT=2 \
    REIFY_COMPILE_GATE_POLL=1 \
    REIFY_CLOCK_HEARTBEAT_SECS=1

assert "L1: quiet CPU + memfull=50 >= threshold=10, compile-gate sustained → timeout 8 kills it (rc==124; held)" \
    test "$ADMIT_RC" -eq 124
assert "L1: stderr contains @@REIFY_CLOCK_STOP@@ reason=psi_pressure (compile-gate hold entered)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_STOP@@ reason=psi_pressure"' _ "$ADMIT_STDERR"
assert "L1: stderr contains @@REIFY_CLOCK_HEARTBEAT@@ (still holding, liveness)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_HEARTBEAT@@"' _ "$ADMIT_STDERR"
assert "L1: stderr does NOT match admit/fairness/sustained-pressure (never admits-on-timeout)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "sustained pressure|fairness floor"' _ "$ADMIT_STDERR"
assert "L1: stderr does NOT contain @@REIFY_CLOCK_START@@ (never admitted)" \
    bash -c '! printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_START@@"' _ "$ADMIT_STDERR"

# L2: merge bypass — same + DF_VERIFY_ROLE=merge → exit 0 fast (no wait)
run_compile_gate_mem "$PSI_L_CPU" "$PSI_L_MEM50" \
    DF_VERIFY_ROLE=merge \
    REIFY_COMPILE_GATE_MEM_FULL_THRESHOLD=10 \
    REIFY_COMPILE_GATE_MAX_WAIT=2 \
    REIFY_COMPILE_GATE_POLL=1

assert "L2: merge bypass + memfull=50 → exit 0" \
    test "$ADMIT_RC" -eq 0

# L3: CPU-only unchanged regression — quiet CPU + quiet memory → exit 0 fast
run_compile_gate_mem "$PSI_L_CPU" "$PSI_L_MEM0" \
    REIFY_COMPILE_GATE_MEM_FULL_THRESHOLD=10 \
    REIFY_COMPILE_GATE_MAX_WAIT=2 \
    REIFY_COMPILE_GATE_POLL=1

assert "L3: quiet CPU + quiet memory → exit 0 fast (CPU-only unchanged)" \
    test "$ADMIT_RC" -eq 0

# L4 (task 4920): default-ON HOLD proof — memfull=50 + NO explicit threshold
# (rely on default=10) + quiet CPU, wrapped in `timeout 8` → rc==124 (held),
# stderr shows STOP+HEARTBEAT on the verify.sh compile-gate path.
# RED today: default-ON threshold engages the OLD admit-on-timeout at
# MAX_WAIT=2s with no clock reason → `timeout 8` never fires — fails fast.
run_compile_gate_mem_bounded 8 "$PSI_L_CPU" "$PSI_L_MEM50" \
    REIFY_COMPILE_GATE_MAX_WAIT=2 \
    REIFY_COMPILE_GATE_POLL=1 \
    REIFY_CLOCK_HEARTBEAT_SECS=1

assert "L4: default-ON threshold: memfull=50 + no explicit threshold, sustained → timeout 8 kills it (rc==124; held)" \
    test "$ADMIT_RC" -eq 124
assert "L4: stderr contains @@REIFY_CLOCK_STOP@@ reason=psi_pressure (default-threshold hold entered)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_STOP@@ reason=psi_pressure"' _ "$ADMIT_STDERR"
assert "L4: stderr contains @@REIFY_CLOCK_HEARTBEAT@@ (still holding, liveness)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_HEARTBEAT@@"' _ "$ADMIT_STDERR"
assert "L4: stderr does NOT contain @@REIFY_CLOCK_START@@ (never admitted)" \
    bash -c '! printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_START@@"' _ "$ADMIT_STDERR"

# ---------------------------------------------------------------------------
# Cycle M: default-ON memfull dimension (CLI core, NO explicit threshold env)
# Exercises the cpu-admit.sh direct-exec/CLI-agent-axis DEFAULT (memory
# dimension default-ON since task 4911) rather than an explicit
# REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD override (contrast Cycle H, which always
# sets the threshold explicitly).
# M1 (task 4920): now a HOLD proof — default-ON memfull backoff drives the
# same continuous-hold machinery as an explicit threshold (no more
# admit-on-timeout).  M2 (requeue) is unaffected by this admit-mode-only
# change and stays as-is.  M3/M4/M5 guards stay green (unaffected).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle M: default-ON memfull dimension (CLI core, no explicit threshold env) ---"

PSI_M_CPU="$(make_psi_fixture 0)"                 # quiet CPU: avg10=0
PSI_M_MEM50="$(make_mem_psi_fixture 50)"          # memfull=50, memsome=0
PSI_M_MEM5="$(make_mem_psi_fixture 5)"            # memfull=5, memsome=0
PSI_M_MEM0_SOME50="$(make_mem_psi_fixture 0 50)"  # memfull=0, memsome=50

# M1 (task 4920): default-ON HOLD proof — admit mode, quiet CPU + stuck
# memfull=50, NO explicit REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD (rely on the
# default), wrapped in `timeout 8` → rc==124 (held well past the old
# MAX_WAIT=2s), stderr shows STOP+HEARTBEAT but never a fairness marker nor
# START.
# RED today: current cpu-admit.sh admits-on-timeout at MAX_WAIT=2s and the CLI
# sets no admit _ca_clock_reason, so `timeout 8` never fires — fails fast.
run_cpu_admit_bounded 8 admit "$PSI_M_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_M_MEM50" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1 \
    REIFY_CLOCK_HEARTBEAT_SECS=1

assert "M1: default-ON memfull=50, admit, NO explicit threshold, sustained → timeout 8 kills it (rc==124; held)" \
    test "$ADMIT_RC" -eq 124
assert "M1: stderr contains @@REIFY_CLOCK_STOP@@ reason=psi_pressure (default-threshold hold entered)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_STOP@@ reason=psi_pressure"' _ "$ADMIT_STDERR"
assert "M1: stderr contains @@REIFY_CLOCK_HEARTBEAT@@ (still holding, liveness)" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_HEARTBEAT@@"' _ "$ADMIT_STDERR"
assert "M1: stderr does NOT match admit/fairness/sustained-pressure (never admits-on-timeout)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "sustained pressure|fairness floor"' _ "$ADMIT_STDERR"
assert "M1: stderr does NOT contain @@REIFY_CLOCK_START@@ (never admitted)" \
    bash -c '! printf "%s\n" "$1" | grep -q "@@REIFY_CLOCK_START@@"' _ "$ADMIT_STDERR"

# M2 (RED driver): requeue mode, same fixtures, NO explicit threshold →
# exit 75 AND elapsed >= 2.  RED today (OFF → instant admit exit 0).
TM2_0=$(date +%s)
run_cpu_admit requeue "$PSI_M_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_M_MEM50" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1
TM2_1=$(date +%s)
ELAPSED_M2=$(( TM2_1 - TM2_0 ))

assert "M2: default-ON memfull=50, requeue, NO explicit threshold → exit 75" \
    test "$ADMIT_RC" -eq 75
assert "M2: elapsed >= MAX_WAIT=2s" \
    test "$ELAPSED_M2" -ge 2

# M3 (guard): admit mode, quiet CPU + memfull=5 (< default threshold=10), NO
# explicit threshold → fast admit exit 0, no fairness/sustained-pressure
# marker.  Guards against over-aggressive backoff; passes before & after.
run_cpu_admit admit "$PSI_M_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_M_MEM5" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1

assert "M3: memfull=5 < default threshold, no explicit env → fast admit exit 0" \
    test "$ADMIT_RC" -eq 0
assert "M3: memfull=5 < default threshold → no sustained-pressure marker" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "sustained pressure|fairness floor"' _ "$ADMIT_STDERR"

# M4 (guard): memsome stays OFF by default — requeue mode, quiet CPU +
# fixture memfull=0/memsome=50, NO REIFY_CPU_ADMIT_MEM_SOME_THRESHOLD →
# fast admit exit 0 (line 394 unchanged by this task).
run_cpu_admit requeue "$PSI_M_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_M_MEM0_SOME50" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1

assert "M4: memsome=50 with NO SOME_THRESHOLD env → fast admit exit 0 (memsome stays OFF by default)" \
    test "$ADMIT_RC" -eq 0

# M5 (guard): merge bypass intact — admit mode, DF_VERIFY_ROLE=merge +
# memfull=50, NO explicit threshold → exit 0 fast AND stderr marks
# 'bypass (role=merge)'.
run_cpu_admit admit "$PSI_M_CPU" \
    DF_VERIFY_ROLE=merge \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_M_MEM50" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1

assert "M5: merge bypass + memfull=50, no explicit threshold → exit 0" \
    test "$ADMIT_RC" -eq 0
assert "M5: merge bypass → stderr marks 'bypass (role=merge)'" \
    bash -c 'printf "%s\n" "$1" | grep -qF "bypass (role=merge)"' _ "$ADMIT_STDERR"

# ---------------------------------------------------------------------------
# Cycle N: explicit-empty escape hatch disables memfull on the CLI/agent axis
# Regression for review finding robustness_contract_violation @ cpu-admit.sh:399.
# N1/N2 are RED drivers: line-399's `${REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD:-10}`
# (colon-minus) coerces an explicit-empty value back to 10, so the documented
# "empty = OFF" escape hatch (header knob doc, inline comment, CLAUDE.md) does
# NOT work today — an operator setting REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD=""
# to disable memory gating during an incident would instead still get memfull
# backoff at threshold 10.  GREEN after step-7 flips the operator to
# unset-only `${REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD-10}`, which preserves an
# explicit-empty value instead of coercing it.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle N: explicit-empty escape hatch disables memfull ---"

PSI_N_CPU="$(make_psi_fixture 0)"          # quiet CPU: avg10=0
PSI_N_MEM50="$(make_mem_psi_fixture 50)"   # memfull=50, memsome=0

# N1 (RED driver): admit mode, quiet CPU + memfull=50 + explicit-empty
# REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD= (run_cpu_admit forwards it via
# `env "$@"`, so cpu-admit sees the var SET-BUT-EMPTY, i.e. defined-not-unset)
# → assert exit 0 AND elapsed < 2 (instant admit) AND no sustained-pressure/
# fairness-floor marker.  RED today: colon-minus coerces the explicit-empty
# value to 10 → memfull=50 >= 10 → backs off → elapsed >= 2 → fails.
run_cpu_admit admit "$PSI_N_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_N_MEM50" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD= \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1

assert "N1: explicit-empty MEM_FULL_THRESHOLD + memfull=50, admit → exit 0" \
    test "$ADMIT_RC" -eq 0
# NOTE: "instant admit" is verified load-independently by the marker assertion
# below (an on-by-mistake memory dimension would back off → admit-on-timeout →
# emit a sustained-pressure/fairness-floor marker).  An absolute wall-clock
# `elapsed < 2s` upper bound was deliberately NOT used — it is the flaky class
# de-flaked by tasks 4841-4847 and guarded by
# tests/infra/test_no_new_wallclock_upper_bounds.sh.
assert "N1: no sustained-pressure/fairness-floor marker (memory dimension OFF)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "sustained pressure|fairness floor"' _ "$ADMIT_STDERR"

# N2 (RED driver): requeue mode, same fixtures + explicit-empty threshold →
# assert exit 0 (admits; does NOT exit 75) AND elapsed < 2.  This is the
# operator break-glass scenario in requeue mode: an explicit-empty threshold
# must NOT requeue-on-memory.  RED today (empty coerced to 10 → backs off →
# exit 75); GREEN after the fix.
run_cpu_admit requeue "$PSI_N_CPU" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_N_MEM50" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD= \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1

# NOTE: "no memory-triggered requeue" is verified load-independently by the
# exit-code assertion below (an on-by-mistake memory dimension would back off
# in requeue mode → exit 75).  No absolute wall-clock `elapsed < 2s` bound is
# used — see the N1 note above and test_no_new_wallclock_upper_bounds.sh.
assert "N2: explicit-empty MEM_FULL_THRESHOLD + memfull=50, requeue → exit 0 (NOT 75)" \
    test "$ADMIT_RC" -eq 0

# ---------------------------------------------------------------------------
# Cycle O: reason-less admit defensive fail-open (cpu-admit.sh:236-250, task 4920)
# Every production admit caller (compile_gate, the CLI/agent-shim main-guard,
# cpu-governed-exec's degrade path) sets _ca_clock_reason="psi_pressure" — the
# CLI main-guard does so unconditionally for mode admit|requeue (cpu-admit.sh
# ~L453), so this branch is UNREACHABLE via run_cpu_admit/the CLI. Exercise it
# directly by sourcing cpu-admit.sh as a library (bypassing the main-guard,
# which never runs when BASH_SOURCE[0] != $0) and calling cpu_admit with
# _ca_clock_reason left empty, mirroring a hypothetical caller that forgot to
# set the reason. Uses a sustained high-PSI fixture so a regression that
# removes the guard is caught by a `timeout 8` kill (rc==124) rather than
# silently passing. Proves the only safeguard against a blind/markerless hold
# (which would otherwise risk exit-124/BLOCKED, per the inline comment at
# cpu-admit.sh:236-246) actually fires: immediate exit 0, a stderr warning
# naming the missing clock-stop reason, and NO @@REIFY_CLOCK_STOP@@ (no
# marker coverage for a reason-less call — never exits 75 either, since admit
# mode never requeues).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle O: reason-less admit defensive fail-open ---"

PSI_O="$(make_psi_fixture 99)"   # sustained high PSI — would otherwise HOLD forever
_O_STDERR="$(mktemp -p "$WORKDIR" cycle-o-stderr.XXXXXX)"
_O_RC=0
timeout 8 bash -c '
    source "$1"
    _ca_threshold=50
    _ca_max_wait=2
    _ca_poll=1
    _ca_proc_path="$2"
    _ca_disable=""
    _ca_window=""
    _ca_dispatch=""
    _ca_log_prefix="cpu-admit"
    _ca_gate_name=""
    _ca_failopen_txt="fail-open"
    _ca_mem_proc_path="$3"
    _ca_mem_full_threshold=""
    _ca_mem_some_threshold=""
    _ca_clock_reason=""
    cpu_admit admit
' _ "$CPU_ADMIT" "$PSI_O" "$_MEM_PSI_QUIET" \
    2>"$_O_STDERR" || _O_RC=$?

assert "O: reason-less admit under sustained PSI=99 → exit 0 (defensive fail-open, not a blind hold; got $_O_RC)" \
    test "$_O_RC" -eq 0
assert "O: stderr warns about the missing clock-stop reason (admitting immediately)" \
    grep -qi "no clock-stop reason" "$_O_STDERR"
assert "O: stderr does NOT contain @@REIFY_CLOCK_STOP@@ (no marker coverage for a reason-less call)" \
    bash -c '! grep -q "@@REIFY_CLOCK_STOP@@" "$1"' _ "$_O_STDERR"

# ---------------------------------------------------------------------------
# Cycle CS: cpu_admit clock-stop marker cycle (step-5/task 4837; CS-b reversed
# by task 4920 now that admit-mode is IN clock-stop scope).
# Tests the @@REIFY_CLOCK_*@@ marker emission on both the requeue (CS-a/CS-c)
# and admit (CS-b) paths.
#
# (CS-a) requeue MAX_WAIT=unlimited: high-PSI fixture cleared after ~2s by a
#         backgrounded updater → exit 0 (never 75), elapsed >= 1500ms, stderr has
#         @@REIFY_CLOCK_STOP@@ reason=psi_pressure + @@REIFY_CLOCK_START@@, and
#         with REIFY_CLOCK_HEARTBEAT_SECS=1 also @@REIFY_CLOCK_HEARTBEAT@@.
# (CS-b) admit mode under sustained pressure (task 4920): admit no longer
#         admits-on-timeout — it HOLDS, so `timeout 8` kills the still-holding
#         process (rc==124) and stderr DOES contain @@REIFY_CLOCK_STOP@@
#         reason=psi_pressure + @@REIFY_CLOCK_HEARTBEAT@@ (admit is now IN
#         clock-stop scope; PRD D2 reversed once 4838 deployed).
# (CS-c) requeue immediate-pass (low avg10) → exit 0, no STOP marker (balance).
# RED today (CS-b): current cpu-admit.sh still admits-on-timeout at MAX_WAIT=2s
# with no clock reason set on the admit CLI path, so `timeout 8` never fires
# and no STOP marker is emitted — fails fast against current code.
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

# CS-b (task 4920, reversed): admit mode (compile_gate/CLI path) under
# sustained pressure + short MAX_WAIT now HOLDS instead of admitting-on-
# timeout, so `timeout 8` kills the still-holding process and stderr DOES
# contain @@REIFY_CLOCK_STOP@@ + @@REIFY_CLOCK_HEARTBEAT@@.
PSI_CS_B="$(make_psi_fixture 99)"
_CS_B_STDERR="$(mktemp -p "$WORKDIR" cs-b-stderr.XXXXXX)"
_CS_B_RC=0
timeout 8 \
    env REIFY_CPU_ADMIT_PROC_PATH="$PSI_CS_B" \
        REIFY_CPU_ADMIT_MAX_WAIT=2 \
        REIFY_CPU_ADMIT_POLL=1 \
        REIFY_CLOCK_HEARTBEAT_SECS=1 \
        bash "$CPU_ADMIT" admit \
    2>"$_CS_B_STDERR" || _CS_B_RC=$?

assert "CS-b: admit under sustained pressure HOLDS (timeout 8 kills it; rc==124; got $_CS_B_RC)" \
    test "$_CS_B_RC" -eq 124
assert "CS-b: admit mode DOES emit @@REIFY_CLOCK_STOP@@ reason=psi_pressure (task 4920: admit now IN clock-stop scope)" \
    grep -q '@@REIFY_CLOCK_STOP@@ reason=psi_pressure' "$_CS_B_STDERR"
assert "CS-b: admit mode emits @@REIFY_CLOCK_HEARTBEAT@@ while holding" \
    grep -q '@@REIFY_CLOCK_HEARTBEAT@@' "$_CS_B_STDERR"

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
# Cycle FC: fork-count hardening — requeue poll loop forks ZERO `date` calls
# (task 4930 — clock_now_epoch no-fork epoch read, esc-3002-142 swap-thrash
# hardening). Behavioral (not source-grep) proof, mirroring test_test_run_
# semaphore.sh's Test CE-2: PATH-shadow `date` with a counting stub that
# appends to a counter file then execs the real binary, drive a contended
# bounded-MAX_WAIT=2 requeue into its poll loop, and assert the counter is 0.
# Only `date` is shadowed — cpu_admit_read_avg10's awk /proc read is the
# actual PSI signal and is deliberately left untouched (do not shadow awk).
# RED today: cpu_admit forks date at loop-top, re-sample, elapsed-on-admit,
# and via clock_maybe_heartbeat (shared helper) every 1s iteration — counter > 0.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle FC: requeue poll loop forks ZERO date calls (no-fork hardening) ---"

# make_counting_date_stub <dir> <counter_file>
# Writes an executable `date` stub into <dir> that appends one line to
# <counter_file> then execs the REAL date binary (resolved via `command -v`
# BEFORE <dir> is prepended to PATH, so the stub never recurses into itself).
make_counting_date_stub() {
    local dir="$1" counter="$2"
    local real
    real="$(command -v date)"
    cat > "$dir/date" <<STUB
#!/usr/bin/env bash
echo 1 >> "$counter"
exec "$real" "\$@"
STUB
    chmod +x "$dir/date"
}

PSI_FC="$(make_psi_fixture 99)"
_FC_STUBDIR="$(mktemp -d -p "$WORKDIR")"
_FC_DATE_COUNT="$(mktemp -p "$WORKDIR")"
make_counting_date_stub "$_FC_STUBDIR" "$_FC_DATE_COUNT"

run_cpu_admit requeue "$PSI_FC" \
    "PATH=$_FC_STUBDIR:$PATH" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 \
    REIFY_CPU_ADMIT_POLL=1

_FC_DATE_CALLS=$(wc -l < "$_FC_DATE_COUNT" | tr -d ' ')

assert "FC-a: requeue MAX_WAIT=2 under sustained PSI=99 exits 75 (got $ADMIT_RC)" \
    test "$ADMIT_RC" -eq 75
assert "FC-b: requeue poll loop forks ZERO date calls (got $_FC_DATE_CALLS)" \
    test "$_FC_DATE_CALLS" -eq 0

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
# (V-c) compile_gate confirmation (task 4920, reversed) — verify.sh
#         compile-gate under avg10=99, MAX_WAIT=2, HEARTBEAT_SECS=1, wrapped in
#         `timeout 8`: → HOLDS past the old MAX_WAIT (rc==124), stderr DOES
#         contain @@REIFY_CLOCK_STOP@@ reason=psi_pressure + @@REIFY_CLOCK_HEARTBEAT@@.
#         Confirms compile_gate() now sets _ca_clock_reason=psi_pressure (PRD D2
#         reversed: compile_gate moved INTO clock-stop scope by task 4920).
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

# V-c (task 4920, reversed): compile_gate (verify.sh compile-gate) under
# sustained pressure now HOLDS instead of admitting — `timeout 8` kills the
# still-holding process and stderr DOES contain @@REIFY_CLOCK_STOP@@ +
# @@REIFY_CLOCK_HEARTBEAT@@ (compile_gate now sets _ca_clock_reason=psi_pressure).
PSI_V_C="$(make_psi_fixture 99)"
_V_C_STDERR="$(mktemp -p "$WORKDIR" vc-stderr.XXXXXX)"
_V_C_RC=0
timeout 8 \
    env REIFY_COMPILE_GATE_PROC_PATH="$PSI_V_C" \
        REIFY_COMPILE_GATE_MAX_WAIT=2 \
        REIFY_COMPILE_GATE_POLL=1 \
        REIFY_CLOCK_HEARTBEAT_SECS=1 \
    bash "$VERIFY" compile-gate \
    2>"$_V_C_STDERR" || _V_C_RC=$?

assert "V-c: compile-gate under sustained pressure HOLDS (timeout 8 kills it; rc==124; got $_V_C_RC)" \
    test "$_V_C_RC" -eq 124
assert "V-c: compile-gate DOES emit @@REIFY_CLOCK_STOP@@ reason=psi_pressure (task 4920: compile_gate now IN clock-stop scope)" \
    grep -q '@@REIFY_CLOCK_STOP@@ reason=psi_pressure' "$_V_C_STDERR"
assert "V-c: compile-gate emits @@REIFY_CLOCK_HEARTBEAT@@ while holding" \
    grep -q '@@REIFY_CLOCK_HEARTBEAT@@' "$_V_C_STDERR"

test_summary
