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

[ -f "$SCRIPT_DIR/load_tolerance_lib.sh" ] || { echo "ERROR: load_tolerance_lib.sh not found at $SCRIPT_DIR/load_tolerance_lib.sh"; exit 1; }
source "$SCRIPT_DIR/load_tolerance_lib.sh"

C_HOLD_S=300   # hold-until-killed: > C_TIMEOUT=120 so the holder NEVER self-releases before verify.sh
               # returns.  The holder is explicitly killed after the verify.sh `wait`, so the WAIT=1
               # acquire ALWAYS times out → exit 75, independent of preamble duration (Fix 2, task 4864).
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
#   tree-sitter — satisfies tree-sitter-generate.sh's `command -v` guard.
#                 Pre-generation (below) ensures the staleness fast-path exits
#                 0 before calling tree-sitter; the 'generate' branch redirects
#                 output to a hermetic tmpdir (not tree-sitter-reify/src/) as a
#                 fail-fast fallback in case pre-generation does not succeed.
# This neutralizes ONLY the heavy external build tools; the REAL semaphore
# acquire/hold/release wiring in lib_test_semaphore.sh / verify.sh is left
# completely intact.
# _wait_for_holder_ready <marker> <deadline-seconds>
# Causal ordering (R-technique) for flock-holder readiness: polls for the READY
# marker file in 0.05s ticks, returning 0 as soon as it appears, or non-zero
# once the generous deadline elapses (T-technique anti-hang guard).
# The READY marker is touched by the holder subshell AFTER acquiring flock -x,
# so returning 0 causally guarantees the holder holds flock -x at the caller's
# next statement.  Replaces the load-fragile `sleep 0.2` assumption at all three
# holder sites (B, C/G, F1).  Mirrors _wait_for_reader_lock from task #4847.
_wait_for_holder_ready() {
    local marker="$1"
    local deadline_s="$2"
    local max_ticks=$(( deadline_s * 20 ))
    local tick=0
    while [ "$tick" -lt "$max_ticks" ]; do
        [ -f "$marker" ] && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
    return 1
}

# _wait_for_marker <file> <pattern> <deadline-seconds>
# Polls <file> for a line containing <pattern> (fixed-string grep) in 0.05s ticks.
# Returns 0 as soon as the marker appears, or non-zero once the generous deadline
# elapses.  Used for causal ordering on @@REIFY_CLOCK_*@@ markers in Section F
# (R-technique: proves verify.sh entered the contended wait while holder still holds).
_wait_for_marker() {
    local file="$1"
    local pattern="$2"
    local deadline_s="$3"
    local max_ticks=$(( deadline_s * 20 ))
    local tick=0
    while [ "$tick" -lt "$max_ticks" ]; do
        grep -qF "$pattern" "$file" 2>/dev/null && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
    return 1
}

# _load_scaled_deadline BASE [MAX]
# Echo a load-scaled deadline: BASE × load_tolerance_factor (from load_tolerance_lib.sh),
# clamped to MAX (if provided) so anti-hang guards never balloon to mask a genuine hang.
# On idle hosts (factor=1) the result equals BASE byte-for-byte (no regression).
# The MAX cap is the genuinely-testable behavior that anchors the Part B unit tests:
#   BASE=30  factor=4 → 120 (no cap)
#   BASE=180 factor=8 → 1440, cap 600 → 600
#   BASE=60  factor=1 → 60  (floor = BASE)
_load_scaled_deadline() {
    local _base="$1"
    local _max="${2:-}"
    local _scaled
    _scaled="$(load_tolerant_attempts "$_base")"
    if [ -n "$_max" ] && [ "$_scaled" -gt "$_max" ] 2>/dev/null; then
        _scaled="$_max"
    fi
    echo "$_scaled"
}

# _make_high_psi_fixture <dir>
# Writes a /proc/pressure/cpu-formatted fixture with avg10=99 into <dir> and
# echoes its path.  Mirrors test_cpu_admit.sh make_psi_fixture (avg10 fixed at
# 99, above the compile-gate threshold of 85) — use REIFY_COMPILE_GATE_PROC_PATH
# to point the compile-gate at this file and force a deterministic MAX_WAIT-second
# wait (admit-on-timeout), with no dependence on real host CPU pressure.
_make_high_psi_fixture() {
    local dir="$1"
    local fixture
    fixture="$(mktemp -p "$dir" psi-high.XXXXXX)"
    printf 'some avg10=99 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' > "$fixture"
    echo "$fixture"
}

# assert_marker <label> <file> <token>
# Checks that <file> contains the literal <token> string (fixed-string grep).
# The token argument rides ONLY in the suppressed grep-command argument — it
# never appears in the echoed PASS/FAIL description — so @@REIFY_CLOCK_*@@
# tokens cannot leak into the parent verify stream via assert()'s stdout echo
# (esc-4789-63 / feedback_heartbeat_idle_backstop_false_kill_leaked_markers).
# Use this helper for ANY assertion whose pattern is an orchestrator-parsed
# marker (@@REIFY_CLOCK_*@@, @@SEMAPHORE_ACQUIRE@@, @@SEMAPHORE_RELEASE@@, …).
assert_marker() {
    local label="$1"
    local file="$2"
    local token="$3"
    assert "$label" grep -qF "$token" "$file"
}

make_stub_bin() {
    local dir="$1"
    # stub cargo: sleeps $REIFY_E2E_CARGO_SLEEP seconds, exits 0.
    # Task 4862 revert: build+test are one unbroken slot-held block; there is no
    # --no-run compile pass outside the slot. The stub holds the slot for the full
    # sleep duration, modeling the unified build+exec pass.
    # Keeps Section A's >=3000ms hold-serialization discriminator valid:
    #   serialized  ≈ preamble + 2×2s (second run waits behind first's slot-hold)
    #   non-held    ≈ preamble + 2s   (both overlapping)
    cat > "$dir/cargo" <<'STUB_CARGO'
#!/usr/bin/env bash
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

    # Pre-seed tree-sitter generated files using the REAL tree-sitter so that
    # tree-sitter-generate.sh's staleness fast-path exits 0 in every hermetic
    # subshell without reaching the stub's 'generate' branch.  This prevents
    # the stub from writing 0-byte output stubs into the real tree-sitter-reify/src/.
    # PATH is prepended with ~/.cargo/bin so tree-sitter is findable before
    # apply_hermetic_env puts the stub binary first.
    # If parser.c is 0-byte (left by a prior test run's stub), force-regen to
    # restore real content; otherwise the normal staleness check suffices.
    local _ts_dir="$REPO_ROOT/tree-sitter-reify"
    if [ -f "$_ts_dir/src/parser.c" ] && [ ! -s "$_ts_dir/src/parser.c" ]; then
        if ! PATH="$HOME/.cargo/bin:$PATH" bash "$REPO_ROOT/scripts/tree-sitter-generate.sh" \
                --force >/dev/null 2>&1; then
            echo "  [make_stub_bin] WARNING: tree-sitter pre-generation (--force) failed — stub may write to tree-sitter-reify/src/" >&2
        fi
    else
        if ! PATH="$HOME/.cargo/bin:$PATH" bash "$REPO_ROOT/scripts/tree-sitter-generate.sh" \
                >/dev/null 2>&1; then
            echo "  [make_stub_bin] WARNING: tree-sitter pre-generation failed — stub may write to tree-sitter-reify/src/" >&2
        fi
    fi

    # stub tree-sitter: satisfies `command -v` guard.
    # Pre-generation above ensures the staleness fast-path exits before this stub's
    # 'generate' branch is reached.  If it IS reached (pre-gen failed), write to a
    # hermetic tmpdir rather than $PWD/src/ (= tree-sitter-reify/src/) so we never
    # contaminate the real source tree with 0-byte stubs.  tree-sitter-generate.sh's
    # post-check then fails (files not in expected src/), propagating as verify.sh
    # non-zero → caught by the relevant section's exit-code assertion (fail-fast).
    local _ts_hermetic_out="$dir/ts-output"
    cat > "$dir/tree-sitter" <<STUB_TREESITTER
#!/usr/bin/env bash
if [ "\${1:-}" = "generate" ]; then
    mkdir -p "${_ts_hermetic_out}"
    touch "${_ts_hermetic_out}/parser.c" "${_ts_hermetic_out}/grammar.json" "${_ts_hermetic_out}/node-types.json"
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
    # Skip the PSI gate subprocess (CPU-pressure wait) — safe and correct in a
    # hermetic test harness with no real compute load.
    export REIFY_PSI_GATE_DISABLE=1
    # Skip the compile-gate subprocess (CPU-pressure admission, task 4853).
    # Rationale: the compile-gate runs on the test path (verify.sh add_test_passes)
    # as role=task under run_all.sh, and under load (avg10>=85) waits up to 300s
    # (admit-on-timeout) in the execute-mode preamble.  That wait races the
    # fixed-duration slot holders — flipping Section C exit-75→0, dropping
    # Section F1 clock markers, and ballooning Section A toward the suite timeout
    # (esc-4288-206 recurrence class).  Like the PSI gate, the compile-gate is
    # CPU-pressure admission noise with no real compute load in a stubbed hermetic
    # harness; disabling it is safe and correct here.
    export REIFY_COMPILE_GATE_DISABLE=1
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

    # Capture each concurrent run's stderr to a file (NOT the test's stderr).
    # These task-role runs HOLD-serialize on the slot and therefore emit
    # @@REIFY_CLOCK_{STOP,HEARTBEAT,START}@@ markers. If those leaked to the
    # outer verify's stderr, the orchestrator's clock-stop heartbeat-idle
    # backstop (dark_factory:1916) would mistake a TEST subprocess's wait for
    # the real verify's wait and kill the run mid-nextest (esc-4802-228).
    # Section A asserts only on the event log + timing, never on stderr, so
    # capturing here is loss-free (matches Sections B/C/E/F's 2>"$*_ERR").
    local _pid1 _pid2
    # First concurrent task run.
    (
        apply_hermetic_env "$_stubdir" "$_lock"
        export REIFY_E2E_CARGO_SLEEP=2
        DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) 2>"$_tmpdir/runA1.err" &
    _pid1=$!

    # Second concurrent task run — same lock base so both compete for the single slot.
    (
        apply_hermetic_env "$_stubdir" "$_lock"
        export REIFY_E2E_CARGO_SLEEP=2
        DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) 2>"$_tmpdir/runA2.err" &
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
# _load_scaled_deadline unit tests (Part B helper, task #4895 S3)
# ===========================================================================
# Deterministic: REIFY_LOAD_TOLERANCE_FACTOR env-injection overrides host load,
# exactly like test_load_tolerance_lib.sh Test 6.  Assertions are exact integer
# identities of load_tolerant_attempts (BASE x factor) plus the MAX-cap logic.
# '|| echo UNDEFINED' prevents set -e from killing the script when
# _load_scaled_deadline is not yet defined (RED state: command-not-found -> FAIL).
echo ""
echo "--- _load_scaled_deadline unit tests (Part B helper, task 4895) ---"

_LSD_T1="$(REIFY_LOAD_TOLERANCE_FACTOR=4 _load_scaled_deadline 30 2>/dev/null || echo UNDEFINED)"
_LSD_T2="$(REIFY_LOAD_TOLERANCE_FACTOR=8 _load_scaled_deadline 180 600 2>/dev/null || echo UNDEFINED)"
_LSD_T3="$(REIFY_LOAD_TOLERANCE_FACTOR=1 _load_scaled_deadline 60 2>/dev/null || echo UNDEFINED)"

assert "_load_scaled_deadline factor=4 base=30 == 120 (scales: 30x4)" \
    test "$_LSD_T1" = "120"
assert "_load_scaled_deadline factor=8 base=180 max=600 == 600 (MAX cap: 180x8=1440 clamped to 600)" \
    test "$_LSD_T2" = "600"
assert "_load_scaled_deadline factor=1 base=60 == 60 (idle floor: factor=1 preserves BASE)" \
    test "$_LSD_T3" = "60"

# ===========================================================================
# Section A: held-slot serialization (execute mode)
# ===========================================================================
# Two concurrent DF_VERIFY_ROLE=task runs must HOLD-serialize at N=1 — the slot
# wraps the entire build+exec block (task 4862 revert: no separate compile pass
# outside the slot). The stub cargo sleeps REIFY_E2E_CARGO_SLEEP=2s inside the
# held slot, so the timing is:
#   serialized  ≈ preamble + 2×2s ≈ 4.2–4.8s  (the second run waits behind the first)
#   non-held    ≈ preamble + 2s   ≈ 2.2–2.8s  (both overlapping)
# The 3000ms lower bound sits clearly in the gap between the two regimes with
# load-tolerant margin.  Serialization is ALSO proven by the causal event-log
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
    local _holder_pid _ready
    _ready="$_tmpdir/holder-ready"
    ( flock -x 9; touch "$_ready"; sleep "$HOLD_S" ) 9>>"${_lock}.slot-1" &
    _holder_pid=$!
    _wait_for_holder_ready "$_ready" 30  # R-technique: causally guarantees holder holds flock -x

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
# Task 4862 revert: build+exec are one slot-held block. The stub cargo is never
# reached because the semaphore acquire fails first — confirming C_RC=75 came from
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

    # External holder pins slot-1 for C_HOLD_S seconds (hold-until-killed: > C_TIMEOUT=120)
    # so the acquire deadline ALWAYS fires before the holder self-releases.
    local _holder_pid _ready
    _ready="$_tmpdir/holder-ready"
    ( flock -x 9; touch "$_ready"; sleep "$C_HOLD_S" ) 9>>"${_lock}.slot-1" &
    _holder_pid=$!
    _wait_for_holder_ready "$_ready" 30  # R-technique: causally guarantees holder holds flock -x

    local _start_s _end_s
    _start_s="$(date +%s)"

    # REIFY_TEST_SEMAPHORE_WAIT=1: acquire deadline fires after 1s → returns 75.
    # verify.sh executor: `exit $_rc` propagates 75 out of the verify.sh process.
    # `timeout $C_TIMEOUT` outer guard (generous anti-hang; never the discriminator).
    C_RC=0
    (
        apply_hermetic_env "$_stubdir" "$_lock" 1
        # Section G opt-in: re-enable the compile-gate with a fake avg10=99 PSI
        # fixture to force a deterministic MAX_WAIT-second admit-on-timeout wait
        # (overrides Fix 1's REIFY_COMPILE_GATE_DISABLE=1).  Normal Section C
        # runs have C_INJECT_COMPILE_WAIT empty → this block is skipped entirely.
        if [ -n "${C_INJECT_COMPILE_WAIT:-}" ]; then
            export REIFY_COMPILE_GATE_DISABLE=
            export REIFY_COMPILE_GATE_PROC_PATH="${C_INJECT_PSI:-}"
            export REIFY_COMPILE_GATE_MAX_WAIT="$C_INJECT_COMPILE_WAIT"
            export REIFY_COMPILE_GATE_POLL=1
            export REIFY_COMPILE_GATE_THRESHOLD=85
        fi
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
assert "all plan: every nextest run line BETWEEN acquire and release markers (task 4862 revert: build inside slot)" \
    bash -c '
        ACQ=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        REL=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*RELEASE" | head -1 | cut -d: -f1)
        # All nextest passes are inside the slot; no --no-run filter needed (post-4862 revert).
        FIRST=$(printf "%s\n" "$1" | grep -n "cargo nextest run" | head -1 | cut -d: -f1)
        LAST=$(printf "%s\n" "$1" | grep -n "cargo nextest run" | tail -1 | cut -d: -f1)
        [ -n "$ACQ" ] && [ -n "$REL" ] && [ -n "$FIRST" ] && [ -n "$LAST" ]
        [ "$FIRST" -gt "$ACQ" ] && [ "$LAST" -lt "$REL" ]
    ' _ "$PLAN_ALL_FULL"
assert "all plan: NO 'cargo nextest run ... --no-run' line before acquire marker (task 4862 revert: build inside slot)" \
    bash -c '! printf "%s\n" "$1" | grep -q "cargo nextest run.*--no-run"' _ "$PLAN_ALL_FULL"

# task 4853: compile-gate ordering on the test path — compile-gate now sits
# BEFORE @@SEMAPHORE_ACQUIRE@@ as a block-entry load gate for the unified build+test block.
# Uses PLAN_TEST_FULL (includes # comment lines) so the ACQUIRE marker is visible.
assert "test plan: compile-gate ordered BEFORE ACQUIRE marker (block-entry load gate, tasks 4853/4862)" \
    bash -c '
        CG=$(printf "%s\n" "$1" | grep -n "verify\.sh compile-gate" | head -1 | cut -d: -f1)
        ACQ=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        [ -n "$CG" ] && [ -n "$ACQ" ] && [ "$CG" -lt "$ACQ" ]
    ' _ "$PLAN_TEST_FULL"

# ===========================================================================
# Section F: clock-stop marker emit + print-plan clock-stop annotation
# ===========================================================================
# F1: With REIFY_TEST_SEMAPHORE_WAIT=unlimited and a hold-until-killed flock
#     holder pinning the single slot, verify.sh test --scope all exits 0
#     (continuous wait, never exit-75), and stderr contains all three
#     @@REIFY_CLOCK_{STOP,HEARTBEAT,START}@@ markers with reason=test_slot_starvation.
#     The holder is killed after STOP+HEARTBEAT are observed (causal R-technique
#     handshake — load-independent proof of a real wait, task 4881).
#     (Proves reify-side emit + block-then-run; DF clock-exclusion is
#      dark_factory:1916's scope, tested separately.)
# F2: verify.sh test --scope all --print-plan: the @@SEMAPHORE_ACQUIRE@@ # comment
#     annotation references the clock-stop region (contains "REIFY_CLOCK",
#     "clock-stop", or "dark_factory:1916"), and the ACQUIRE line is a # comment
#     NOT a bare command.  RED today — annotation does not yet mention clock-stop.
echo ""
echo "--- Section F: clock-stop markers + print-plan clock-stop annotation ---"

F_HOLD_S=20      # fragile-window reference (old holder threshold); used by Section H
                 # inject calculation (F_INJECT_COMPILE_WAIT = F_HOLD_S + 10 = 30s).
                 # The holder inside run_unlimited_wait_with_slot_held is now
                 # hold-until-killed (sleep 300), independent of this value.

run_unlimited_wait_with_slot_held() {
    local _tmpdir _stubdir _lock
    _tmpdir="$(mktemp -d)"
    _TMPDIRS+=("$_tmpdir")
    _stubdir="$_tmpdir/stubs"
    _lock="$_tmpdir/sem.lock"
    mkdir -p "$_stubdir"
    make_stub_bin "$_stubdir"

    F_ERR="$_tmpdir/f_err.txt"
    touch "$F_ERR"

    # Hold-until-killed holder: sleeps 300s so it NEVER self-releases before
    # the REIFY_CLOCK_STOP marker is observed.  Explicitly killed after both
    # STOP and HEARTBEAT markers are seen in F_ERR, decoupling correctness from
    # preamble/wall-clock duration (causal R-technique, task 4881; mirrors
    # run_task_with_slot_held's C_HOLD_S=300 pattern from task 4864).
    local _holder_pid _ready
    _ready="$_tmpdir/holder-ready"
    ( flock -x 9; touch "$_ready"; sleep 300 ) 9>>"${_lock}.slot-1" &
    _holder_pid=$!
    _wait_for_holder_ready "$_ready" 30  # R-technique: causally guarantees holder holds flock -x

    # Launch verify.sh in the BACKGROUND so we can poll its stderr for clock
    # markers while the holder still holds the slot.  Anti-hang guard: 180s
    # (generous; never the discriminator — holder is killed on marker arrival).
    F_RC=0
    local _run_pid
    # set -m enables job control so the subshell below gets its own process group
    # (PGID == _run_pid).  This allows the abort paths to send SIGTERM to the
    # entire group (including timeout + verify.sh + any nextest children) via
    # `kill -- -$_run_pid`, preventing orphaned slot-holders from cascading into
    # later test sections.  set +m restores the default state immediately after.
    set -m
    (
        apply_hermetic_env "$_stubdir" "$_lock" unlimited
        export REIFY_CLOCK_HEARTBEAT_SECS=1
        # Section H opt-in: re-enable the compile-gate with a fake avg10=99 PSI
        # fixture to force a deterministic MAX_WAIT-second admit-on-timeout wait
        # (overrides apply_hermetic_env's REIFY_COMPILE_GATE_DISABLE=1).  Normal
        # Section F1 runs have F_INJECT_COMPILE_WAIT empty → this block is skipped.
        if [ -n "${F_INJECT_COMPILE_WAIT:-}" ]; then
            export REIFY_COMPILE_GATE_DISABLE=
            export REIFY_COMPILE_GATE_PROC_PATH="${F_INJECT_PSI:-}"
            export REIFY_COMPILE_GATE_MAX_WAIT="$F_INJECT_COMPILE_WAIT"
            export REIFY_COMPILE_GATE_POLL=1
            export REIFY_COMPILE_GATE_THRESHOLD=85
        fi
        DF_VERIFY_ROLE=task timeout 180 bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) 2>"$F_ERR" & _run_pid=$!
    set +m  # restore default job control state

    # Causal handshake (R-technique): poll F_ERR for CLOCK_STOP then
    # CLOCK_HEARTBEAT.  CLOCK_STOP is emitted when the acquire first blocks
    # (proves verify.sh entered the contended wait while the holder holds);
    # CLOCK_HEARTBEAT proves >=1 heartbeat interval elapsed inside the wait.
    # Both must appear while the holder still holds the slot.  These marker
    # assertions are the strictly stronger, load-independent proof of a real wait
    # (no wall-clock discriminator needed).
    _wait_for_marker "$F_ERR" '@@REIFY_CLOCK_STOP@@' 120 \
        || { echo "  [F] ERROR: CLOCK_STOP not observed within 120s; aborting" >&2
             kill -- -"$_run_pid" 2>/dev/null || kill "$_run_pid" 2>/dev/null || true
             kill "$_holder_pid" 2>/dev/null || true
             wait "$_run_pid" "$_holder_pid" 2>/dev/null || true
             rm -f "${_lock}.slot-1"; F_RC=99; return 1; }
    _wait_for_marker "$F_ERR" '@@REIFY_CLOCK_HEARTBEAT@@' 120 \
        || { echo "  [F] ERROR: CLOCK_HEARTBEAT not observed within 120s; aborting" >&2
             kill -- -"$_run_pid" 2>/dev/null || kill "$_run_pid" 2>/dev/null || true
             kill "$_holder_pid" 2>/dev/null || true
             wait "$_run_pid" "$_holder_pid" 2>/dev/null || true
             rm -f "${_lock}.slot-1"; F_RC=99; return 1; }

    # Markers observed: kill the holder so the slot frees.  verify.sh will then
    # acquire, run the stub nextest pass, emit CLOCK_START, and exit 0.
    kill "$_holder_pid" 2>/dev/null || true
    wait "$_holder_pid" 2>/dev/null || true

    # Wait for verify.sh to complete and capture its exit code.
    wait "$_run_pid" || F_RC=$?
    echo "  [F] unlimited-wait queued-then-ran: rc=$F_RC (holder killed after STOP+HEARTBEAT observed)" >&2

    rm -f "${_lock}.slot-1"
}

F_RC=0
F_ERR=""
run_unlimited_wait_with_slot_held
assert "F1: unlimited-wait verify.sh exits 0 when slot eventually freed (got ${F_RC})" \
    test "$F_RC" -eq 0
assert_marker "F1: F_ERR captured the CLOCK_STOP marker (reason=test_slot_starvation)" \
    "$F_ERR" '@@REIFY_CLOCK_STOP@@ reason=test_slot_starvation'
assert_marker "F1: F_ERR captured the CLOCK_HEARTBEAT marker" \
    "$F_ERR" '@@REIFY_CLOCK_HEARTBEAT@@'
assert_marker "F1: F_ERR captured the CLOCK_START marker (reason=test_slot_starvation)" \
    "$F_ERR" '@@REIFY_CLOCK_START@@ reason=test_slot_starvation'

# F2: print-plan ACQUIRE annotation must reference the clock-stop region.
# Captures the ACQUIRE # comment line from --print-plan and asserts it contains
# "REIFY_CLOCK", "clock-stop", or "dark_factory:1916".
# RED today: the current annotation does not mention the clock-stop seam.
F2_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan 2>/dev/null)"
F2_ACQ_LINE="$(printf '%s\n' "$F2_PLAN" | grep 'test-run semaphore.*ACQUIRE' | head -1)"
echo "  [F2] ACQUIRE annotation: ${F2_ACQ_LINE}" >&2
assert "F2: ACQUIRE annotation is a # comment (not a bare timeout/exec command)" \
    bash -c 'printf "%s\n" "$1" | grep -q "^#"' _ "$F2_ACQ_LINE"
assert "F2: ACQUIRE annotation references clock-stop region (REIFY_CLOCK / clock-stop / dark_factory:1916)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "REIFY_CLOCK|clock-stop|dark_factory:1916"' _ "$F2_ACQ_LINE"

# run_hermetic_execute_capture
# Drives ONE hermetic execute-mode run (DF_VERIFY_ROLE=task, SLEEP=0, no external
# holder) and captures stderr to E_ERR.  Sets E_RC (expected 0 at idle).
# Used by Section E to prove apply_hermetic_env neutralizes the compile-gate.
run_hermetic_execute_capture() {
    local _tmpdir _stubdir _lock
    _tmpdir="$(mktemp -d)"
    _TMPDIRS+=("$_tmpdir")
    _stubdir="$_tmpdir/stubs"
    _lock="$_tmpdir/sem.lock"
    mkdir -p "$_stubdir"
    make_stub_bin "$_stubdir"

    E_ERR="$_tmpdir/e_err.txt"
    touch "$E_ERR"

    E_RC=0
    (
        apply_hermetic_env "$_stubdir" "$_lock"
        DF_VERIFY_ROLE=task timeout 60 bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) 2>"$E_ERR" || E_RC=$?
}

# ===========================================================================
# Section E: compile-gate neutralized in hermetic env (load-robustness root-cause guard)
# ===========================================================================
# S-technique structural proof: apply_hermetic_env must export
# REIFY_COMPILE_GATE_DISABLE=1, causing cpu-admit.sh to emit the fixed marker
# "verify.sh: compile-gate disabled" to stderr.  This is the load-independent
# proof that the task-4853 compile-gate (verify.sh add_test_passes, test path,
# role=task, up to 300s admit-on-timeout wait) is neutralized in every hermetic
# execute section — the root-cause guard against the esc-4288-206 recurrence.
# RED today: apply_hermetic_env does not export REIFY_COMPILE_GATE_DISABLE, so
# the disable marker is absent (gate admits/fail-opens silently) → grep fails.
echo ""
echo "--- Section E: compile-gate neutralized in hermetic env (load-robustness root-cause guard) ---"

E_RC=0
E_ERR=""
run_hermetic_execute_capture
assert "Section E structural: stderr contains compile-gate disabled marker (verify.sh: compile-gate disabled)" \
    grep -qF 'verify.sh: compile-gate disabled' "$E_ERR"

# ===========================================================================
# Section G: exit-75 survives an inflated preamble (deterministic fake-PSI,
#            non-vacuous robustness proof)
# ===========================================================================
# Re-enables the compile-gate inside run_task_with_slot_held via the opt-in
# C_INJECT_COMPILE_WAIT/C_INJECT_PSI injection: fake avg10=99 PSI fixture,
# MAX_WAIT=12s, POLL=1.  The compile-gate waits ~12s (admit-on-timeout),
# outlasting the current fixed-duration C_HOLD_S=10 holder → slot is FREE when
# verify.sh tries to acquire → exit 0 (not 75) → RED today.
# After Fix 2 (step-4: C_HOLD_S→300 + READY handshake), the holder is still
# active after the ~12s preamble → acquire fails (WAIT=1) → exit 75 → GREEN.
# This is the non-vacuous robustness proof: reverts to RED if Fix 2 is reverted.
echo ""
echo "--- Section G: exit-75 survives inflated preamble (fake-PSI compile-gate, non-vacuous proof) ---"

_G_TMPDIR="$(mktemp -d)"
_TMPDIRS+=("$_G_TMPDIR")
C_RC=0
C_ERR=""
C_INJECT_PSI="$(_make_high_psi_fixture "$_G_TMPDIR")"
C_INJECT_COMPILE_WAIT=12    # compile-gate waits ~12s; holder (C_HOLD_S=300) still holds → acquire WAIT=1 → exit 75 (GREEN; RED if C_HOLD_S reverted to <12)
run_task_with_slot_held
C_INJECT_COMPILE_WAIT=      # reset so any future section uses the normal compile-gate-disabled path
C_INJECT_PSI=
assert "Section G: exit-75 survives inflated preamble (compile-gate wait > holder, got ${C_RC})" \
    test "$C_RC" -eq 75

# ===========================================================================
# Section H: clock-marker proof survives an inflated preamble
#            (fake-PSI compile-gate, non-vacuous robustness proof)
# ===========================================================================
# Re-enables the compile-gate inside run_unlimited_wait_with_slot_held via the
# opt-in F_INJECT_COMPILE_WAIT/F_INJECT_PSI injection: fake avg10=99 PSI fixture,
# MAX_WAIT=$(( F_HOLD_S + 10 ))=30s, POLL=1.  The compile-gate waits ~30s
# (admit-on-timeout), outlasting the current fixed-duration F_HOLD_S=20s holder
# → slot is FREE when verify.sh reaches @@SEMAPHORE_ACQUIRE@@ → no CLOCK_STOP
# emitted → RED today.
# After Fix (step-2: hold-until-killed + causal handshake), the holder is still
# active after the ~30s preamble → @@REIFY_CLOCK_STOP@@ is observed while holder
# still holds → GREEN.
# This is the non-vacuous robustness proof: reverts to RED if the hold-until-killed
# change is reverted.
echo ""
echo "--- Section H: clock-marker proof survives inflated preamble (fake-PSI, non-vacuous proof) ---"

_H_TMPDIR="$(mktemp -d)"
_TMPDIRS+=("$_H_TMPDIR")
F_RC=0
F_ERR=""
F_INJECT_PSI="$(_make_high_psi_fixture "$_H_TMPDIR")"
F_INJECT_COMPILE_WAIT=$(( F_HOLD_S + 10 ))  # 30s > old 20s holder → defeats fixed-holder harness
run_unlimited_wait_with_slot_held
F_INJECT_COMPILE_WAIT=      # reset so normal runs use compile-gate-disabled path
F_INJECT_PSI=
assert "Section H: unlimited-wait exits 0 despite inflated preamble (got ${F_RC})" \
    test "$F_RC" -eq 0
assert_marker "Section H: F_ERR captured CLOCK_STOP despite inflated preamble (holder outlasts preamble)" \
    "$F_ERR" '@@REIFY_CLOCK_STOP@@ reason=test_slot_starvation'

# ===========================================================================
# Section I: clock-marker isolation regression guard (static source scan)
# ===========================================================================
# Statically scans this file's own source to ensure no raw `assert` description
# (echoed to stdout by test_helpers.sh assert()) embeds an orchestrator-consumed
# @@…@@ marker token.  A leaked token on the parent verify stream triggers
# dark_factory:1916's heartbeat-idle backstop (esc-4789-63 / feedback pattern).
# RED on unpatched code: matches the 4 leaky descriptions at F1/H before Step 2.
# GREEN after Step 2: assert_marker() lines start with `assert_` (not `assert `
# with space), grep-pattern args start with `grep`, so no assert description
# contains the @@-delimited token.
#
# The regex targets the full @@[A-Z_]+@@ family (not just @@REIFY_CLOCK) so the
# guard matches its own stated invariant: "no orchestrator-consumed marker in any
# assert description" — covering @@SEMAPHORE_ACQUIRE@@, @@SEMAPHORE_RELEASE@@,
# and any future marker families, not only the one that regressed.
echo ""
echo "--- Section I: clock-marker isolation regression guard (static source scan) ---"

SELF="${BASH_SOURCE[0]}"
assert "Section I: no assert description embeds an orchestrator-consumed marker token (@@...@@ family; parent-stream isolation, Sections A/F/H)" \
    bash -c '! grep -nE "^[[:space:]]*assert[[:space:]].*@@[A-Z_]+" "$1"' _ "$SELF"

test_summary
