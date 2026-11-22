#!/usr/bin/env bash
# tests/infra/test_cpu_load_governance_deflake.sh — meta-test for PRD T7 (task 4846).
#
# Proves that every live cgroup-governance cycle of test_cpu_load_governance.sh
# SKIPs under the REIFY_CPU_GOVERN_DISABLE=1 break-glass, in two halves: each
# registered cycle's gate SKIPs (A4, driven), and each cycle's if-head consults
# that gate, with no other path to host_supports_governance (A7, source lint).
#
# Assertions:
#   A2  the armed gates-only drive exits 0 under the no-progress stall guard.
#   A3  NON-VACUOUS: share_ge_proportional discriminator — broken->False, healthy->True.
#   A4  the drive emits the host-governance SKIP marker of all six live cycles.
#   A5  the drive short-circuited at the seam: the SUT printed no `Results:` line.
#   A6  GATES_ONLY without the REIFY_GOVTEST_TEST_MODE arming key is refused
#       loudly, and the suite runs on to Cycle SELF.
#   A7  each live cycle is headed `if [!] live_cycle_gate NAME; then`, and only
#       live_cycle_gate calls host_supports_governance; fixtures prove it flags.
#   A8  NON-VACUOUS: the stall guard TERMs a command gone quiet together with
#       its descendants (rc 124, naming its last output line), spares one still
#       emitting past its window, and passes a command's own exit status through,
#       124 included, without calling that a stall.
#
# Technique: the armed REIFY_CPU_GOV_TEST_GATES_ONLY=1 seam consults each live
# cycle's host gate and exits before any cycle runs, so the drive is sub-second
# and its guard never decides the verdict. The drive runs under
# _run_with_stall_guard: it and its descendants are TERMed only after more than
# _SUT_STALL_S s without output (technique T, docs/prds/infra-test-wallclock-
# deflake.md §2), while run_all.sh's member budget bounds its total runtime.
# The drive never reaches a cycle's own if-head, which is why A7 exists. The
# SUT's always-on body is its own run_all member's job, not this one's.
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUT="$SCRIPT_DIR/test_cpu_load_governance.sh"
INSTRUMENT="$SCRIPT_DIR/cpu_gov_instrument.py"
LIVE_CYCLES=(FIXTURE-6 ROW1-2 ROW1-1 ROW2_3 CONFINE-APPLIED ROW4)

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

# _term_descendants <pid>
#   TERMs every descendant of <pid>, deepest first so each parent is still there
#   to be asked for its own children. TERMing <pid> first would put them out of
#   reach, and its direct children alone would miss whatever runs under a command
#   substitution, such as the `systemctl --user` calls of the SUT's startup sweep.
_term_descendants() {
    local child
    for child in $(pgrep -P "$1" 2>/dev/null || true); do
        _term_descendants "$child"
        kill -TERM "$child" 2>/dev/null || true
    done
}

# _run_with_stall_guard <stall_s> <out_file> <cmd> [arg...]
#   Runs <cmd> with stdout+stderr in <out_file>. Returns <cmd>'s own exit status,
#   or 124 once <out_file> has not grown for more than <stall_s> seconds. `$SECONDS`
#   ticks in whole seconds, so `>` is what keeps that silence at least <stall_s>
#   real seconds. On a stall the guard prints a one-line notice naming <out_file>'s
#   last non-blank line, then TERMs <cmd>'s descendants and <cmd> itself: <cmd>'s
#   own EXIT trap ends it at once on TERM, which would leave a hung child running.
#   Technique T (docs/prds/infra-test-wallclock-deflake.md §2): the window bounds
#   SILENCE, so a slow <cmd> that keeps emitting is never killed. Its total
#   runtime stays bounded by run_all.sh's member budget, whose process-group
#   kill reaches <cmd> because it stays in this shell's group.
_run_with_stall_guard() {
    local stall_s="$1" out="$2"
    shift 2
    "$@" >"$out" 2>&1 </dev/null &
    local pid=$!
    local size=0 now=0 last_growth=$SECONDS rc=0 last_line
    while kill -0 "$pid" 2>/dev/null; do
        now=$(stat -c %s "$out" 2>/dev/null || echo 0)
        if [ "$now" != "$size" ]; then
            size=$now
            last_growth=$SECONDS
        elif (( SECONDS - last_growth > stall_s )); then
            last_line="$(awk 'NF { last = $0 } END { print last }' "$out" 2>/dev/null || true)"
            echo "  STALL GUARD: no output for ${stall_s}s; TERMing the command and its descendants; last output line: ${last_line:-<none>}"
            _term_descendants "$pid"
            kill -TERM "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
            return 124
        fi
        sleep 1
    done
    wait "$pid" || rc=$?
    return "$rc"
}

# _print_sut_failure_evidence <rc> <out_file>
#   For a non-zero SUT status, prints the last 40 lines of its output, which this
#   test otherwise deletes. Each line gets a `  SUT| ` prefix so none can pass for
#   one of this test's own `  FAIL:` lines when run_all.sh collects FAIL detail.
#   Returns 0 always: evidence never changes a verdict.
_print_sut_failure_evidence() {
    local rc="$1" out="$2"
    [ "$rc" -ne 0 ] || return 0
    tail -n 40 "$out" 2>/dev/null | sed 's/^/  SUT| /' || true
}

echo "=== cpu-load-governance de-flake meta-test (task 4846) ==="

# ── A3 — NON-VACUOUS guard (share_ge_proportional) ────────────────────────────
# Re-confirms the ROW4-1 discriminator works in both directions co-located with
# the budget-gate change, discharging the NON-VACUOUS mandate.
# Broken governance (merge_share=0.5 < floor=0.65) → False → ROW4-1 would be RED.
# Healthy governance (merge_share=0.75 >= floor=0.65) → True → ROW4-1 is GREEN.
echo ""
echo "--- A3: NON-VACUOUS guard (share_ge_proportional) ---"

if ! command -v python3 >/dev/null 2>&1; then
    echo "  SKIP A3: python3 not on PATH"
elif ! [ -f "$INSTRUMENT" ]; then
    echo "  SKIP A3: cpu_gov_instrument.py not found at $INSTRUMENT"
else
    # A3a: broken governance (merge=100,task=100 → share=0.5; floor=0.75-0.10=0.65)
    #      share_ge_proportional must return False → ROW4-1 would be RED.
    #      We assert the Python call exits 1 (not ok), meaning it correctly detected
    #      broken governance.
    assert "A3a: share_ge_proportional(100,100,300,100,0.10) is False (broken→ROW4-1 RED)" \
        bash -c '
            python3 -c "
import sys
sys.path.insert(0, sys.argv[1])
from cpu_gov_instrument import share_ge_proportional
ok = share_ge_proportional(100.0, 100.0, 300.0, 100.0, 0.10)
# We want False; exit 0 if False (test passes), exit 1 if True (test fails).
sys.exit(0 if not ok else 1)
" "$1"
        ' _ "$SCRIPT_DIR"

    # A3b: healthy governance (merge=300,task=100 → share=0.75; floor=0.65)
    #      share_ge_proportional must return True → ROW4-1 is GREEN.
    assert "A3b: share_ge_proportional(300,100,300,100,0.10) is True (healthy→ROW4-1 GREEN)" \
        bash -c '
            python3 -c "
import sys
sys.path.insert(0, sys.argv[1])
from cpu_gov_instrument import share_ge_proportional
ok = share_ge_proportional(300.0, 100.0, 300.0, 100.0, 0.10)
# We want True; exit 0 if True (test passes), exit 1 if False (test fails).
sys.exit(0 if ok else 1)
" "$1"
        ' _ "$SCRIPT_DIR"
fi

# ── A8 — NON-VACUOUS guard (_run_with_stall_guard) ─────────────────────────────
# The gates-only drive below runs under _run_with_stall_guard, which TERMs it only once its
# output has been silent for the stall window, never on cumulative runtime. Each
# case drives the guard with a hermetic stub that writes to its own file. A8b is
# the NON-VACUITY CRUX: its command outlives the stall window yet keeps emitting,
# so a guard that is really a cumulative `timeout <stall_s>` returns 124 there.
#   - rc capture is `_rc=0; helper ... || _rc=$?`, never a bare call: `set -e`
#     must not abort on the helper's meaningful rc, and an undefined helper reads
#     as 127, which the cases assert as EXACT rcs (never `-ne 0`) so 127 FAILs.
#   - no assertion bounds an elapsed time (technique T, docs/prds/infra-test-
#     wallclock-deflake.md).
#   - the guard's stdout is its stall notice. Each case sends it to its own file,
#     so an expected stall stays out of this transcript and the notice can be
#     asserted on.
echo ""
echo "--- A8: NON-VACUOUS stall guard (_run_with_stall_guard) ---"

_a8_dir="$(mktemp -d)"

# A8a (STALL CRUX): a command that goes quiet is TERMed, rc exactly 124, what it
# printed before going quiet is captured, and the guard's stall notice names that
# last line. `exec` makes the TERM reach sleep itself, so no orphan outlives the case.
_rc=0
_run_with_stall_guard 2 "$_a8_dir/stall.out" \
    bash -c 'echo started; exec sleep 30' >"$_a8_dir/stall.notice" || _rc=$?
assert "A8a: a command gone quiet is TERMed after the stall window (want rc=124, got ${_rc})" \
    test "$_rc" -eq 124
assert "A8a: the quiet command's output is captured (rc=${_rc}, 'started' in out file)" \
    grep -q 'started' "$_a8_dir/stall.out"
assert "A8a: the stall notice names the command's last output line (rc=${_rc}, 'started' in notice)" \
    grep -q 'started' "$_a8_dir/stall.notice"

# A8b (PROGRESS CRUX): a command that keeps emitting past the stall window is NOT
# killed. 70 ticks 0.1 s apart run for at least 7 s against a 5 s window.
_rc=0
_run_with_stall_guard 5 "$_a8_dir/progress.out" \
    bash -c 'for i in $(seq 70); do echo "tick $i"; sleep 0.1; done' || _rc=$?
assert "A8b: a command still emitting past the stall window is NOT killed (want rc=0, got ${_rc})" \
    test "$_rc" -eq 0
assert "A8b: the emitting command ran to completion (rc=${_rc}, 'tick 70' in out file)" \
    grep -q 'tick 70' "$_a8_dir/progress.out"

# A8c (STATUS PASSTHROUGH): the command's own exit status is returned unchanged.
_rc=0
_run_with_stall_guard 5 "$_a8_dir/status.out" \
    bash -c 'echo x; exit 3' || _rc=$?
assert "A8c: the command's own exit status passes through (want rc=3, got ${_rc})" \
    test "$_rc" -eq 3

# A8d (OWN 124 IS NOT A STALL): 124 is also what a `timeout` nested inside the
# command exits with. Passed through like any status, with no stall notice, so the
# notice marks only the guard's own TERM and never infers a stall from the number.
_rc=0
_run_with_stall_guard 5 "$_a8_dir/own124.out" \
    bash -c 'echo x; exit 124' >"$_a8_dir/own124.notice" || _rc=$?
assert "A8d: a command's own exit 124 passes through (want rc=124, got ${_rc})" \
    test "$_rc" -eq 124
assert "A8d: a command's own exit 124 draws no stall notice (rc=${_rc}, notice empty)" \
    test ! -s "$_a8_dir/own124.notice"

# A8e (SUBTREE REAPING): on a stall the guard TERMs the command's whole subtree, not
# just the command: the command's own EXIT trap can end it at once and orphan a hung
# child. The stub nests a command substitution, as the SUT's startup sweep does
# around `systemctl`, so the leaf `sleep` is a grandchild of the command. The
# command and the substitution each handle TERM, which bash defers until its
# foreground child ends: both outlive the guard's signals, and the substitution
# records how its leaf ended, 143 if the guard TERMed it and 0 after its full 30 s.
_rc=0
_run_with_stall_guard 2 "$_a8_dir/subtree.out" \
    bash -c 'echo started; trap : TERM; : "$(trap : TERM; sleep 30; echo "leaf-rc=$?" >"$1")"' \
    _ "$_a8_dir/subtree.leaf-rc" >"$_a8_dir/subtree.notice" || _rc=$?
assert "A8e: a stalled command is TERMed (want rc=124, got ${_rc})" \
    test "$_rc" -eq 124
assert "A8e: its grandchild was TERMed too, not left running (want leaf-rc=143, got $(cat "$_a8_dir/subtree.leaf-rc" 2>/dev/null || echo none))" \
    grep -qx 'leaf-rc=143' "$_a8_dir/subtree.leaf-rc"

rm -rf "$_a8_dir"

# ── A2 + A4 + A5 — the armed gates-only drive ─────────────────────────────────
echo ""
echo "--- A2+A4+A5: armed REIFY_CPU_GOV_TEST_GATES_ONLY drive ---"

assert "SUT exists at $SUT" \
    test -f "$SUT"

# The stall guard, not a total-time cap: _SUT_STALL_S seconds without output
# means the SUT is hung, however loaded the host is.
_SUT_STALL_S=120
_SUT_OUT="$(mktemp)"
_SUT_RC=0
_run_with_stall_guard "$_SUT_STALL_S" "$_SUT_OUT" \
    env -u REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS REIFY_CPU_GOVERN_DISABLE=1 REIFY_GOVTEST_TEST_MODE=1 REIFY_CPU_GOV_TEST_GATES_ONLY=1 bash "$SUT" \
    || _SUT_RC=$?

_sut_exit0() {
    if [ "$_SUT_RC" -ne 0 ]; then
        echo "SUT rc=$_SUT_RC, want 0"
        _print_sut_failure_evidence "$_SUT_RC" "$_SUT_OUT"
        return 1
    fi
    return 0
}
assert "A2: SUT exits 0 under the armed gates-only drive" \
    _sut_exit0

_sut_skipped_every_live_cycle() {
    local name missing=()
    for name in "${LIVE_CYCLES[@]}"; do
        grep -qF -- "SKIP $name: host does not support cgroup governance" "$_SUT_OUT" \
            || missing+=("$name")
    done
    if [ "${#missing[@]}" -ne 0 ]; then
        echo "missing host-governance SKIP markers: ${missing[*]}"
        tail -n 40 "$_SUT_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "A4: SUT output contains the host-governance SKIP marker of every live cycle" \
    _sut_skipped_every_live_cycle

_sut_exited_at_seam() {
    if grep -q '^Results:' "$_SUT_OUT"; then
        echo "SUT ran the FULL suite — the gates-only seam did not take effect:"
        tail -n 15 "$_SUT_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "A5: the gates-only seam short-circuits before any cycle (no Results: line)" \
    _sut_exited_at_seam

# ── A6 — unarmed GATES_ONLY is refused loudly ─────────────────────────────────
echo ""
echo "--- A6: GATES_ONLY without REIFY_GOVTEST_TEST_MODE ---"

_DISARM_OUT="$(mktemp)"

_gates_only_refused_without_arming() {
    local pid
    : > "$_DISARM_OUT"

    timeout 120 env -u REIFY_GOVTEST_TEST_MODE -u REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS REIFY_CPU_GOVERN_DISABLE=1 REIFY_CPU_GOV_TEST_GATES_ONLY=1 bash "$SUT" > "$_DISARM_OUT" 2>&1 &
    pid=$!

    until grep -q 'GATES_ONLY=1 IGNORED' "$_DISARM_OUT" 2>/dev/null \
        && grep -q 'Cycle SELF' "$_DISARM_OUT" 2>/dev/null; do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
    done

    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true

    if ! grep -q 'GATES_ONLY=1 IGNORED' "$_DISARM_OUT" 2>/dev/null; then
        echo "unarmed GATES_ONLY was not refused loudly:"
        head -n 20 "$_DISARM_OUT" 2>/dev/null || true
        return 1
    fi
    if ! grep -q 'Cycle SELF' "$_DISARM_OUT" 2>/dev/null; then
        echo "unarmed GATES_ONLY short-circuited anyway — the suite never reached Cycle SELF:"
        head -n 20 "$_DISARM_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "A6: GATES_ONLY without REIFY_GOVTEST_TEST_MODE is refused loudly and the suite runs on" \
    _gates_only_refused_without_arming

# ── A7 — every live cycle's if-head consults its gate ─────────────────────────
echo ""
echo "--- A7: live-cycle -> live_cycle_gate wiring (source lint + fixtures) ---"

# _gate_wiring_violations FILE — prints one line per violation; returns 1 if any.
_gate_wiring_violations() {
    local file="$1" name bypasses rc=0
    for name in "${LIVE_CYCLES[@]}"; do
        grep -Eq "^[[:space:]]*(if|elif)[[:space:]]+(![[:space:]]+)?live_cycle_gate[[:space:]]+${name}[[:space:]]*;[[:space:]]*then([[:space:]]|\$)" "$file" && continue
        echo "$name: no cycle headed 'if [!] live_cycle_gate $name; then'"
        rc=1
    done
    bypasses="$(awk '
        /^[A-Za-z_][A-Za-z0-9_]*\(\)[[:space:]]*\{/ { fn = $1; sub(/\(\).*/, "", fn); next }
        /^\}/ { fn = "" }
        /^[[:space:]]*#/ { next }
        /(^|[^A-Za-z0-9_])host_supports_governance([^A-Za-z0-9_]|$)/ && fn != "live_cycle_gate" {
            print "line " NR " (" (fn == "" ? "top level" : fn) "): calls host_supports_governance outside live_cycle_gate"
        }
    ' "$file")"
    if [ -n "$bypasses" ]; then
        echo "$bypasses"
        rc=1
    fi
    return "$rc"
}

_FIXTURE_DIR="$(mktemp -d)"

# _wiring_lint_rejects FIXTURE NEEDLE — 0 when the lint rejects FIXTURE naming NEEDLE.
_wiring_lint_rejects() {
    local out rc=0
    if cmp -s "$1" "$SUT"; then
        echo "fixture $1 is identical to the SUT — its mutation no longer applies"
        return 1
    fi
    out="$(_gate_wiring_violations "$1")" || rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "lint accepted a fixture it must reject (want a violation naming '$2')"
        return 1
    fi
    if ! grep -qF -- "$2" <<<"$out"; then
        echo "lint rejected the fixture without naming '$2':"
        echo "$out"
        return 1
    fi
    return 0
}

assert "A7a: every live cycle of the SUT is headed by its live_cycle_gate, and nothing else calls host_supports_governance" \
    _gate_wiring_violations "$SUT"

sed -E 's/live_cycle_gate[[:space:]]+ROW4([[:space:]]*;)/false\1/' "$SUT" > "$_FIXTURE_DIR/dropped-gate.sh"
assert "A7b: the lint rejects a SUT whose ROW4 if-head no longer consults live_cycle_gate" \
    _wiring_lint_rejects "$_FIXTURE_DIR/dropped-gate.sh" "ROW4:"

{ cat "$SUT"; echo 'if host_supports_governance; then :; fi'; } > "$_FIXTURE_DIR/bypass-gate.sh"
assert "A7c: the lint rejects a SUT with a direct host_supports_governance caller" \
    _wiring_lint_rejects "$_FIXTURE_DIR/bypass-gate.sh" "calls host_supports_governance outside live_cycle_gate"

rm -rf "$_FIXTURE_DIR"
rm -f "$_SUT_OUT" "$_DISARM_OUT"

# ── Final summary ──────────────────────────────────────────────────────────────
test_summary
