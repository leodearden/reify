#!/usr/bin/env bash
# Integration gate (PRD task ε): e2e test of the composed semaphore through verify.sh.
# Proves α+β+γ+δ compose correctly end-to-end by driving the REAL scripts/verify.sh
# in execute mode and asserting:
#   A — held-slot serialization (two concurrent task runs hold-serialize at N=1)
#   B — merge exemption (DF_VERIFY_ROLE=merge bypasses the held slot)
#   C — exit-75 propagation (acquisition deadline propagates out of verify.sh)
#   D — print-plan occt-cap=24 override + compile/check/clippy outside gated region

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

C_HOLD_S=10    # fixed: > REIFY_TEST_SEMAPHORE_WAIT=1 so the deadline fires while the slot is held
C_TIMEOUT=120  # generous anti-hang guard; exit 75 fires ~1s after WAIT=1, never the discriminator

echo "=== verify.sh semaphore e2e tests (task 4505, PRD task ε) ==="

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

# ===========================================================================
# Hermetic harness fixtures
# ===========================================================================

# make_stub_bin <dir>
# Write three executable stubs into <dir>:
#   cargo       — sleeps $REIFY_E2E_CARGO_SLEEP seconds (default 0), exits 0.
#                 The stub HOLDS the outer semaphore slot while it sleeps:
#                 verify.sh acquires the slot (@@SEMAPHORE_ACQUIRE@@), runs
#                 `timeout … cargo nextest run … 9<&-` (= stub cargo), then
#                 releases (@@SEMAPHORE_RELEASE@@) — so the slot is held for
#                 the stub sleep duration.  This is the serialization signal.
#   npm         — instant exit 0: neutralizes the GUI node lane
#                 (`npm ci && npm run typecheck && npm test`) without any
#                 network/install/build activity.
#   tree-sitter — instant exit 0: satisfies tree-sitter-generate.sh's
#                 `command -v tree-sitter` guard; parser is already up-to-date
#                 so the generate path is never reached anyway.
# This neutralizes ONLY the heavy external build tools; the REAL semaphore
# acquire/hold/release wiring in lib_test_semaphore.sh / verify.sh is left
# completely intact.
make_stub_bin() {
    local dir="$1"
    # stub cargo: --no-run-aware (task 4839).
    # When args contain --no-run (compile pass, outside the slot): exit 0 instantly.
    # Otherwise (execution pass, inside the slot): sleep $REIFY_E2E_CARGO_SLEEP.
    # This models reality precisely: the compile is moved outside the slot so it
    # runs without holding it; only the execution pass holds the slot and sleeps.
    # Keeps Section A's >=3000ms hold-serialization discriminator valid under
    # execution-only gating: serialized ≈ preamble + 2×2s, non-held ≈ preamble + 2s.
    cat > "$dir/cargo" <<'STUB_CARGO'
#!/usr/bin/env bash
for _arg in "$@"; do
    if [ "$_arg" = "--no-run" ]; then
        exit 0
    fi
done
sleep "${REIFY_E2E_CARGO_SLEEP:-0}"
exit 0
STUB_CARGO
    chmod +x "$dir/cargo"

    # stub npm: instant exit 0 — neutralizes gui node lane.
    cat > "$dir/npm" <<'STUB_NPM'
#!/usr/bin/env bash
exit 0
STUB_NPM
    chmod +x "$dir/npm"

    # stub tree-sitter: satisfies `command -v` guard; when called with `generate`
    # creates the expected output files so tree-sitter-generate.sh's post-run
    # check passes even in worktrees where parser.c has not been generated yet.
    cat > "$dir/tree-sitter" <<'STUB_TREESITTER'
#!/usr/bin/env bash
if [ "${1:-}" = "generate" ]; then
    mkdir -p src
    touch src/parser.c src/grammar.json src/node-types.json
fi
exit 0
STUB_TREESITTER
    chmod +x "$dir/tree-sitter"
}

# apply_hermetic_env <stubdir> <lock_base> [wait_secs]
# Export the hermetic verify.sh env into the calling (sub)shell.
# MUST be called inside a subshell ( ... ) so exports do not leak to the outer
# shell and affect subsequent test sections.
#
# PATH ordering: stub dir FIRST, then ~/.cargo/bin.  verify.sh apply_env
# sources ~/.cargo/env, whose guard prepends ~/.cargo/bin ONLY when not already
# present.  By placing ~/.cargo/bin in PATH here, the guard is a no-op and
# the stub cargo (in $stubdir) stays first on PATH.  (PATH ORDERING GOTCHA
# documented in task 4505 analysis.)
#
# REIFY_PSI_GATE_DISABLE=1: skip the ./scripts/verify.sh psi-gate subprocess
# (CPU-pressure wait) — safe and correct in a hermetic test harness with no
# real compute load.
apply_hermetic_env() {
    local stubdir="$1"
    local lock_base="$2"
    local wait="${3:-1800}"
    export PATH="$stubdir:$HOME/.cargo/bin:$PATH"
    export REIFY_PSI_GATE_DISABLE=1
    export REIFY_TEST_SEMAPHORE_CONCURRENCY=1
    export REIFY_TEST_SEMAPHORE_LOCK="$lock_base"
    export REIFY_TEST_SEMAPHORE_WAIT="$wait"
}

# drive_two_concurrent_task_runs
# Spawn two concurrent DF_VERIFY_ROLE=task verify.sh runs with REIFY_E2E_CARGO_SLEEP=2
# and N=1 (shared slot), wait for both, and set MS to the total elapsed milliseconds.
# The slot is held for the full stub-cargo sleep duration on each run, so the second
# run's slot-acquire blocks until the first releases — i.e. true HOLD-serialization.
drive_two_concurrent_task_runs() {
    local _tmpdir _stubdir _lock
    _tmpdir="$(mktemp -d)"
    _TMPDIRS+=("$_tmpdir")
    _stubdir="$_tmpdir/stubs"
    _lock="$_tmpdir/sem.lock"
    mkdir -p "$_stubdir"
    make_stub_bin "$_stubdir"

    # Create shared event log for R-technique causal proof (Section A).
    # Both concurrent subshells append ACQUIRE/RELEASE lines to the same file.
    # REIFY_SLOT_EVENT_LOG is exported so it is inherited by the subshells;
    # unset after wait so it does not leak into Section B/C.
    local _eventlog
    _eventlog="$_tmpdir/events.log"
    : > "$_eventlog"
    A_EVENTLOG="$_eventlog"  # global: Section A assertions parse this after function returns
    export REIFY_SLOT_EVENT_LOG="$_eventlog"

    local _start_ns _end_ns
    _start_ns="$(date +%s%N)"

    local _pid1 _pid2
    # First concurrent task run.
    (
        apply_hermetic_env "$_stubdir" "$_lock"
        export REIFY_E2E_CARGO_SLEEP=2
        DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) &
    _pid1=$!

    # Second concurrent task run — same lock base so both compete for the single slot.
    (
        apply_hermetic_env "$_stubdir" "$_lock"
        export REIFY_E2E_CARGO_SLEEP=2
        DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) &
    _pid2=$!

    # Capture exit codes without letting set -e abort on non-zero child.
    local _rc1=0 _rc2=0
    wait "$_pid1" || _rc1=$?
    wait "$_pid2" || _rc2=$?
    unset REIFY_SLOT_EVENT_LOG  # don't leak event log path into Section B/C
    # Export globals so Section A can assert both runs completed successfully.
    # A failed run could still consume ~2s (if it errors mid-slot-hold) and
    # satisfy the timing lower bound, giving a false green for serialization.
    RC1=$_rc1
    RC2=$_rc2

    _end_ns="$(date +%s%N)"
    MS=$(( (_end_ns - _start_ns) / 1000000 ))
    echo "  [A] two concurrent task runs elapsed: ${MS}ms (rc1=$_rc1 rc2=$_rc2)" >&2

    # Clean up slot file left by the semaphore.
    rm -f "${_lock}.slot-1"
}

# ===========================================================================
# Section A: held-slot serialization (execute mode)
# ===========================================================================
# Two concurrent DF_VERIFY_ROLE=task runs must HOLD-serialize at N=1 — the slot
# is held for the EXECUTION pass only (task 4839: compile is outside the slot).
# The stub cargo is --no-run-aware: the --no-run compile pass exits 0 instantly
# (outside the slot), and only the execution pass sleeps REIFY_E2E_CARGO_SLEEP=2s
# (inside the slot).  So the timing remains:
#   serialized  ≈ preamble + 2×2s ≈ 4.2–4.8s  (the second run waits behind the first)
#   non-held    ≈ preamble + 2s   ≈ 2.2–2.8s  (both overlapping)
# The 3000ms lower bound sits clearly in the gap between the two regimes with
# load-tolerant margin.  Serialization is now ALSO proven by the causal event-log
# assertions below (R-technique, load-independent).
echo ""
echo "--- Section A: held-slot serialization (execute mode) ---"

RC1=0
RC2=0
MS=0
A_EVENTLOG=""
drive_two_concurrent_task_runs
# Both runs must have exited 0: a run that errors mid-slot-hold could still
# consume ~2s and satisfy the timing lower bound, producing a false green.
assert "both concurrent task runs exited 0 (rc1=${RC1}, rc2=${RC2})" \
    test "$RC1" -eq 0 -a "$RC2" -eq 0
assert "two concurrent task verify.sh test runs hold-serialize (elapsed >= 3000ms, got ${MS}ms)" \
    test "$MS" -ge 3000
# --- Section A causal assertions (R-technique): parse REIFY_SLOT_EVENT_LOG ---
# Assert (1): exactly 2 ACQUIRE + 2 RELEASE events — both runs traversed the
# gated region; guards against a vacuous empty-log green (e.g. DISABLE=1).
# Assert (2): max(ACQUIRE_ts) >= min(RELEASE_ts) — the second critical section
# began only after the first ended; proves true hold-serialization at N=1.
# RED with CONCURRENCY=2 (both acquire concurrently → max(ACQ) < min(REL)).
# RED with DISABLE=1 (no slot events → count 0 ≠ 2).
A_ACQ_COUNT=$(awk '$3 == "ACQUIRE"' "$A_EVENTLOG" | wc -l | tr -d ' ')
A_REL_COUNT=$(awk '$3 == "RELEASE"' "$A_EVENTLOG" | wc -l | tr -d ' ')
A_MAX_ACQ=$(awk '$3 == "ACQUIRE" { print $1 }' "$A_EVENTLOG" | sort -n | tail -1)
A_MIN_REL=$(awk '$3 == "RELEASE" { print $1 }' "$A_EVENTLOG" | sort -n | head -1)
echo "  [A-causal] acq=${A_ACQ_COUNT} rel=${A_REL_COUNT} max_acq=${A_MAX_ACQ} min_rel=${A_MIN_REL}" >&2
assert "Section A causal: exactly 2 ACQUIRE events in event log (got ${A_ACQ_COUNT})" \
    test "$A_ACQ_COUNT" -eq 2
assert "Section A causal: exactly 2 RELEASE events in event log (got ${A_REL_COUNT})" \
    test "$A_REL_COUNT" -eq 2
assert "Section A causal: max(ACQUIRE_ts) >= min(RELEASE_ts) — second CS began only after first CS ended" \
    test "$A_MAX_ACQ" -ge "$A_MIN_REL"

# run_merge_while_task_slot_held
# Pins the single slot via an external flock holder for HOLD_S seconds, then runs a
# DF_VERIFY_ROLE=merge verify.sh run with REIFY_TEST_SEMAPHORE_WAIT=MERGE_WAIT (so a
# non-exempt run would block, not exit-75 quickly).  Sets MERGE_RC, MERGE_S, MERGE_ERR.
# Mirrors test_occt_flock_gate.sh Test 14: `( flock -x 9; sleep N ) 9>>"${LOCK}.slot-1" &`.
run_merge_while_task_slot_held() {
    local _tmpdir _stubdir _lock
    _tmpdir="$(mktemp -d)"
    _TMPDIRS+=("$_tmpdir")
    _stubdir="$_tmpdir/stubs"
    _lock="$_tmpdir/sem.lock"
    mkdir -p "$_stubdir"
    # Stub cargo with SLEEP=0: merge run should be fast (instant nextest pass).
    REIFY_E2E_CARGO_SLEEP=0 make_stub_bin "$_stubdir"

    # Spawn background external holder that pins slot-1 for HOLD_S seconds.
    # A non-exempt task run would block here for up to REIFY_TEST_SEMAPHORE_WAIT=30s.
    local _holder_pid
    ( flock -x 9; sleep "$HOLD_S" ) 9>>"${_lock}.slot-1" &
    _holder_pid=$!
    sleep 0.2  # give holder time to acquire the lock

    local _start_s _end_s
    _start_s="$(date +%s)"

    # Capture stderr for the bypass-marker assertion (Section B S-technique).
    MERGE_ERR="$_tmpdir/merge_err.txt"
    touch "$MERGE_ERR"

    # REIFY_TEST_SEMAPHORE_WAIT=$MERGE_WAIT (>HOLD_S): a non-exempt run would block
    # rather than exit-75, contrasting the merge bypass with the task-blocked path.
    MERGE_RC=0
    (
        apply_hermetic_env "$_stubdir" "$_lock" "$MERGE_WAIT"
        DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) 2>"$MERGE_ERR" || MERGE_RC=$?

    _end_s="$(date +%s)"
    MERGE_S=$(( _end_s - _start_s ))
    echo "  [B] merge-role run: rc=$MERGE_RC elapsed=${MERGE_S}s (holder holds ${HOLD_S}s)" >&2

    kill "$_holder_pid" 2>/dev/null || true
    wait "$_holder_pid" 2>/dev/null || true
    rm -f "${_lock}.slot-1"
}

# ===========================================================================
# Section B: merge exemption (execute mode)
# ===========================================================================
# DF_VERIFY_ROLE=merge bypasses test_semaphore_acquire entirely (lib lines 59-62).
# With a background holder pinning the SINGLE slot and MERGE_WAIT > HOLD_S
# (so a non-exempt task run would block, not exit-75 quickly), the bypass is
# proven by the structural marker in stderr — NOT by wall-clock timing.
echo ""
echo "--- Section B: merge exemption (execute mode) ---"

MERGE_RC=0
MERGE_S=0
MERGE_ERR=""
HOLD_S=6         # fixed: long enough that a non-exempt run would block
MERGE_WAIT=30    # fixed: WAIT > HOLD_S so a blocked run stays blocked, not exit-75
run_merge_while_task_slot_held
assert "merge-role verify.sh test proceeds while task slot is held (exit 0, got ${MERGE_RC})" \
    test "$MERGE_RC" -eq 0
# --- Section B structural assertion (S-technique): bypass marker in stderr ---
# Proves the merge-exemption CODE PATH executed specifically (not just exit 0).
# Fixed-string grep stops before the em-dash (U+2014) in the full message to avoid
# locale/encoding fragility; the substring is unique to the bypass path.
# RED when DF_VERIFY_ROLE=task (bypass marker absent → grep fails).
assert "Section B structural: stderr contains merge-bypass marker (lib_test_semaphore.sh: bypass (role=merge))" \
    grep -qF 'lib_test_semaphore.sh: bypass (role=merge)' "$MERGE_ERR"

# run_task_with_slot_held
# Pins the single slot via an external flock holder for C_HOLD_S seconds (fixed,
# > REIFY_TEST_SEMAPHORE_WAIT=1 so the deadline fires while the slot is held), then
# runs a DF_VERIFY_ROLE=task verify.sh with `timeout C_TIMEOUT` as a generous
# anti-hang guard (never the discriminator — exit 75 fires ~1s after WAIT=1).
# Sets C_RC, C_S, C_ERR.
# Note (task 4839): the --no-run compile pass runs BEFORE the slot acquire and
# exits 0 instantly (stub is --no-run-aware). The EXECUTION cargo is never reached
# because the semaphore acquire fails first — confirming that C_RC=75 came from
# the acquire path, not a stubbed cargo step.
run_task_with_slot_held() {
    local _tmpdir _stubdir _lock
    _tmpdir="$(mktemp -d)"
    _TMPDIRS+=("$_tmpdir")
    _stubdir="$_tmpdir/stubs"
    _lock="$_tmpdir/sem.lock"
    mkdir -p "$_stubdir"
    make_stub_bin "$_stubdir"

    C_ERR="$_tmpdir/c_err.txt"
    touch "$C_ERR"

    # External holder pins slot-1 for C_HOLD_S seconds (fixed, > WAIT=1) so the
    # acquire deadline fires while the slot is still held.
    local _holder_pid
    ( flock -x 9; sleep "$C_HOLD_S" ) 9>>"${_lock}.slot-1" &
    _holder_pid=$!
    sleep 0.2  # give holder time to acquire

    local _start_s _end_s
    _start_s="$(date +%s)"

    # REIFY_TEST_SEMAPHORE_WAIT=1: acquire deadline fires after 1s → returns 75.
    # verify.sh executor: `exit $_rc` propagates 75 out of the verify.sh process.
    # `timeout $C_TIMEOUT` outer guard (generous anti-hang; never the discriminator).
    C_RC=0
    (
        apply_hermetic_env "$_stubdir" "$_lock" 1
        DF_VERIFY_ROLE=task timeout "$C_TIMEOUT" bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) 2>"$C_ERR" || C_RC=$?

    _end_s="$(date +%s)"
    C_S=$(( _end_s - _start_s ))
    echo "  [C] task run with held slot: rc=$C_RC elapsed=${C_S}s (WAIT=1s)" >&2

    kill "$_holder_pid" 2>/dev/null || true
    wait "$_holder_pid" 2>/dev/null || true
    rm -f "${_lock}.slot-1"
}

# ===========================================================================
# Section C: exit-75 propagation (execute mode)
# ===========================================================================
# With the single slot pinned by an external holder and REIFY_TEST_SEMAPHORE_WAIT=1,
# test_semaphore_acquire times out after 1s and returns 75.  verify.sh's executor
# catches this and runs `exit $_rc` — propagating 75 OUT of the verify.sh process.
# Asserting the verify.sh-level stderr message proves the 75 came from verify.sh's
# own acquire branch, not from a stubbed sub-step — the exact contract the
# orchestrator's exit-75→requeue path depends on.
echo ""
echo "--- Section C: exit-75 propagation (execute mode) ---"

C_RC=0
C_S=0
C_ERR=""
run_task_with_slot_held
assert "verify.sh exits 75 (EX_TEMPFAIL) on acquisition deadline (got ${C_RC})" \
    test "$C_RC" -eq 75
assert "stderr shows exit-75 propagated THROUGH verify.sh (verify.sh: FAILED (exit 75): ...)" \
    grep -qE 'verify\.sh: FAILED \(exit 75\): test-run semaphore acquire' "$C_ERR"

# capture_plans
# Capture print-plan output for Section D assertions (once each, no stubs needed).
# Sets:
#   PLAN_TEST_FULL — full output of `verify.sh test --scope all --print-plan`
#                    (includes # comment lines for ACQUIRE/RELEASE markers)
#   PLAN_TEST_CMDS — commands-only view (grep -v '^#' of PLAN_TEST_FULL)
#   PLAN_ALL_FULL  — full output of `verify.sh all --scope all --print-plan`
#                    (used for index-based ordering assertions)
# Uses the REAL verify.sh scripts, no env manipulation — print-plan is a pure
# static plan builder, never executes cargo/npm/tree-sitter.
capture_plans() {
    PLAN_TEST_FULL="$(bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan 2>/dev/null)"
    PLAN_TEST_CMDS="$(printf '%s\n' "$PLAN_TEST_FULL" | grep -v '^#')"
    PLAN_ALL_FULL="$(bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan 2>/dev/null)"
}

# ===========================================================================
# Section D: print-plan oracle — occt cap=24 + gated-region ordering
# ===========================================================================
# Hermetic (no stubs / no timing): asserts the plan STRUCTURE through --print-plan.
#   (a) All `cargo nextest run` lines in test plan carry --config-file with a
#       reify-nextest-occt path (the γ/4503 cap mechanism, NOT inline --config).
#   (b) .config/nextest.toml pins occt max-threads=24; gen-nextest-config.sh
#       resolves it to 24 (integration-faithful cap assertion).
#   (c) Using `all --scope all --print-plan` (full plan), cargo clippy and
#       cargo check -p reify-gui appear BEFORE the ACQUIRE marker, and every
#       cargo nextest run line appears strictly BETWEEN the ACQUIRE and RELEASE
#       markers (re-verifies β gated-region oracle + γ cap as single gate).
# Intentionally re-consolidates β's gated-region oracle and γ's cap=24 here.
echo ""
echo "--- Section D: print-plan oracle (occt cap=24 + gated-region ordering) ---"

capture_plans
assert "test plan: all nextest run lines carry --config-file with reify-nextest-occt path" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo nextest run" && ! printf "%s\n" "$1" | grep "cargo nextest run" | grep -v -- "--config-file.*reify-nextest-occt"' \
    _ "$PLAN_TEST_CMDS"
assert ".config/nextest.toml pins occt max-threads=24" \
    grep -qE 'occt = \{ max-threads = 24 \}' "$REPO_ROOT/.config/nextest.toml"
assert "gen-nextest-config.sh resolves occt cap to 24" \
    bash -c '_p=$(bash "$1/scripts/gen-nextest-config.sh"); rc=0; grep -qE "^occt = \{ max-threads = 24 \}" "$_p" || rc=1; rm -f "$_p"; exit $rc' \
    _ "$REPO_ROOT"
assert "all plan: cargo clippy ordered BEFORE acquire marker (outside gated region)" \
    bash -c '
        ACQ=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        CLIP=$(printf "%s\n" "$1" | grep -n "cargo clippy" | head -1 | cut -d: -f1)
        [ -n "$ACQ" ] && [ -n "$CLIP" ] && [ "$CLIP" -lt "$ACQ" ]
    ' _ "$PLAN_ALL_FULL"
assert "all plan: cargo check -p reify-gui ordered BEFORE acquire marker (outside gated region)" \
    bash -c '
        ACQ=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        CHK=$(printf "%s\n" "$1" | grep -n "cargo check -p reify-gui" | head -1 | cut -d: -f1)
        [ -n "$ACQ" ] && [ -n "$CHK" ] && [ "$CHK" -lt "$ACQ" ]
    ' _ "$PLAN_ALL_FULL"
assert "all plan: every nextest EXECUTION run line BETWEEN acquire and release markers (task 4839: exclude --no-run compile lines)" \
    bash -c '
        ACQ=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        REL=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*RELEASE" | head -1 | cut -d: -f1)
        # EXECUTION passes only: exclude --no-run compile lines (those are OUTSIDE the slot)
        FIRST=$(printf "%s\n" "$1" | grep -n "cargo nextest run" | grep -v -- "--no-run" | head -1 | cut -d: -f1)
        LAST=$(printf "%s\n" "$1" | grep -n "cargo nextest run" | grep -v -- "--no-run" | tail -1 | cut -d: -f1)
        [ -n "$ACQ" ] && [ -n "$REL" ] && [ -n "$FIRST" ] && [ -n "$LAST" ]
        [ "$FIRST" -gt "$ACQ" ] && [ "$LAST" -lt "$REL" ]
    ' _ "$PLAN_ALL_FULL"
assert "all plan: every --no-run compile line ordered BEFORE acquire marker (outside slot, task 4839)" \
    bash -c '
        ACQ=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        # All --no-run lines must be strictly before the ACQUIRE marker.
        # (No --no-run line should exist yet pre-impl, so this is RED today.)
        NORUN_COUNT=$(printf "%s\n" "$1" | grep -n "cargo nextest run.*--no-run" | wc -l | tr -d " ")
        [ "$NORUN_COUNT" -gt 0 ]
        LAST_NORUN=$(printf "%s\n" "$1" | grep -n "cargo nextest run.*--no-run" | tail -1 | cut -d: -f1)
        [ -n "$ACQ" ] && [ -n "$LAST_NORUN" ] && [ "$LAST_NORUN" -lt "$ACQ" ]
    ' _ "$PLAN_ALL_FULL"

test_summary
