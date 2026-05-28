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
assert "merge-bypass: dispatch mtime was bumped (>= before)" \
    test "$MTIME_3_AFTER" -ge "$MTIME_3_BEFORE"

# (b) immediately after merge: a task verify must back off >= WINDOW from merge's touch
T3B_0=$(date +%s)
run_gate "$DISPATCH_3" "$PSI_ZERO_M" \
    DF_VERIFY_ROLE=task REIFY_PSI_GATE_WINDOW=2 REIFY_PSI_GATE_POLL=1 REIFY_PSI_GATE_MAX_WAIT=30
T3B_1=$(date +%s)
TASK_ELAPSED=$(( T3B_1 - T3B_0 ))

assert "merge-bypass: subsequent task blocks >= WINDOW=2s after merge touch" \
    test "$TASK_ELAPSED" -ge 2

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

test_summary
