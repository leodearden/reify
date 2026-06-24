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
_LOAD_FACTOR="$(load_tolerance_factor)"
A_UPPER=$(( 20000 * _LOAD_FACTOR ))  # load-tolerant sanity ceiling; equals 20000 at idle factor=1
C_UPPER=$(( 8 * _LOAD_FACTOR ))      # Section C exit-75 budget; equals 8 at idle factor=1
C_HOLD_S=$(( 10 * _LOAD_FACTOR ))    # Section C holder sleep; equals 10 at idle; > C_UPPER
C_TIMEOUT=$(( 15 * _LOAD_FACTOR ))   # Section C outer guard; equals 15 at idle; > C_HOLD_S

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
    # stub cargo: sleep then succeed.
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

    # stub tree-sitter: instant exit 0 — satisfies `command -v` guard.
    cat > "$dir/tree-sitter" <<'STUB_TREESITTER'
#!/usr/bin/env bash
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
# is held for the run's whole duration, NOT merely PSI-admission-spaced.
# With stub cargo sleeping 2s and a single debug nextest pass:
#   serialized  ≈ preamble + 2×2s ≈ 4.2–4.8s  (the second run waits behind the first)
#   non-held    ≈ preamble + 2s   ≈ 2.2–2.8s  (both overlapping)
# The 3000ms lower bound sits clearly in the gap between the two regimes with
# load-tolerant margin, mirroring test_occt_flock_gate.sh Test 8 / esc-3939-94.
echo ""
echo "--- Section A: held-slot serialization (execute mode) ---"

RC1=0
RC2=0
MS=0
drive_two_concurrent_task_runs
# Both runs must have exited 0: a run that errors mid-slot-hold could still
# consume ~2s and satisfy the timing lower bound, producing a false green.
assert "both concurrent task runs exited 0 (rc1=${RC1}, rc2=${RC2})" \
    test "$RC1" -eq 0 -a "$RC2" -eq 0
assert "two concurrent task verify.sh test runs hold-serialize (elapsed >= 3000ms, got ${MS}ms)" \
    test "$MS" -ge 3000
# Loose upper-bound sanity: scales with load factor (equals 20000ms at idle factor=1).
assert "serialization elapsed within sanity bound (elapsed <= ${A_UPPER}ms, got ${MS}ms)" \
    test "$MS" -le "$A_UPPER"

# run_merge_while_task_slot_held
# Pins the single slot via an external flock holder for HOLD_S=6s, then times a
# DF_VERIFY_ROLE=merge verify.sh run (REIFY_TEST_SEMAPHORE_WAIT=30 so a non-exempt
# run would block, not exit-75 quickly).  Sets MERGE_RC and MERGE_S (seconds).
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

    # Time the merge-role run.  REIFY_TEST_SEMAPHORE_WAIT=$MERGE_WAIT ensures a
    # non-exempt run would block (not exit-75 quickly), so fast+exit0 proves real
    # bypass.  MERGE_WAIT = HOLD_S+24, always > HOLD_S at every factor.
    MERGE_RC=0
    (
        apply_hermetic_env "$_stubdir" "$_lock" "$MERGE_WAIT"
        DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) || MERGE_RC=$?

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
# With a background holder pinning the SINGLE slot for HOLD_S=6s and
# REIFY_TEST_SEMAPHORE_WAIT=30 (so a non-exempt run would block for up to 30s),
# "completes fast AND exits 0" is an unambiguous discriminator for the bypass:
#   exempt  → MERGE_S ≈ preamble_time (<4s), MERGE_RC=0
#   blocked → MERGE_S ≈ HOLD_S (~6s), then MERGE_RC=0
#   exit-75 → MERGE_S << HOLD_S, MERGE_RC=75 (wrong — that's WAIT<HOLD_S case)
echo ""
echo "--- Section B: merge exemption (execute mode) ---"

MERGE_RC=0
MERGE_S=0
EXEMPT_BOUND=$(( 4 * _LOAD_FACTOR ))  # scales with load; equals 4 at idle factor=1
HOLD_S=$(( 6 * _LOAD_FACTOR ))        # must stay > EXEMPT_BOUND at every factor
MERGE_WAIT=$(( HOLD_S + 24 ))         # merge-run wait; equals 30 at idle, always > HOLD_S
run_merge_while_task_slot_held
assert "merge-role verify.sh test proceeds while task slot is held (exit 0, got ${MERGE_RC})" \
    test "$MERGE_RC" -eq 0
assert "merge-role run did NOT wait for held task slot (elapsed ${MERGE_S}s < ${EXEMPT_BOUND}s, holder holds ${HOLD_S}s)" \
    test "$MERGE_S" -lt "$EXEMPT_BOUND"

# run_task_with_slot_held
# Pins the single slot via an external flock holder for C_HOLD_S seconds (scales
# with load; equals 10 at idle factor=1), then runs a DF_VERIFY_ROLE=task verify.sh
# run with REIFY_TEST_SEMAPHORE_WAIT=1 (times out after 1s → returns 75) and
# `timeout $C_TIMEOUT` outer guard (scales with load; equals 15 at idle).
# Sets C_RC, C_S, C_ERR. Cargo is never reached — the semaphore acquire fails first
# — confirming that C_RC=75 came from the acquire path, not a stubbed cargo step.
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

    # External holder pins slot-1 for C_HOLD_S seconds (scales with load; equals 10 at
    # idle factor=1) — longer than REIFY_TEST_SEMAPHORE_WAIT=1 at any factor, so the
    # acquire deadline fires while the slot is still held.
    local _holder_pid
    ( flock -x 9; sleep "$C_HOLD_S" ) 9>>"${_lock}.slot-1" &
    _holder_pid=$!
    sleep 0.2  # give holder time to acquire

    local _start_s _end_s
    _start_s="$(date +%s)"

    # REIFY_TEST_SEMAPHORE_WAIT=1: acquire deadline fires after 1s → returns 75.
    # verify.sh executor: `exit $_rc` propagates 75 out of the verify.sh process.
    # `timeout $C_TIMEOUT` outer guard scales with load (equals 15s at idle factor=1)
    # and prevents a hung test from blocking indefinitely.
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
assert "exit-75 fires within budget (elapsed ${C_S}s <= ${C_UPPER}; scales with load)" \
    test "$C_S" -le "$C_UPPER"
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
assert "all plan: every nextest run line BETWEEN acquire and release markers" \
    bash -c '
        ACQ=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        REL=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*RELEASE" | head -1 | cut -d: -f1)
        FIRST=$(printf "%s\n" "$1" | grep -n "cargo nextest run" | head -1 | cut -d: -f1)
        LAST=$(printf "%s\n" "$1" | grep -n "cargo nextest run" | tail -1 | cut -d: -f1)
        [ -n "$ACQ" ] && [ -n "$REL" ] && [ -n "$FIRST" ] && [ -n "$LAST" ]
        [ "$FIRST" -gt "$ACQ" ] && [ "$LAST" -lt "$REL" ]
    ' _ "$PLAN_ALL_FULL"

# ===========================================================================
# Section E: load-tolerant upper bounds oracle (task 4799)
# ===========================================================================
# Asserts the base-constant math (via a forced-factor lib subshell) and the
# two discriminator ordering-invariants that must hold at any load factor.
# Run with REIFY_LOAD_TOLERANCE_FACTOR=1 for a deterministic full-script
# demo independent of the box's live load (Sections A-D use the scaled variables,
# which equal the original literals only when REIFY_LOAD_TOLERANCE_FACTOR=1;
# Section E's own forced-factor subshell proves the scaling).
echo ""
echo "--- Section E: load-tolerant upper bounds oracle (task 4799) ---"

# Combined forced-factor proof: LA=128/NP=32 → factor=4.
# Verifies: A_UPPER base constant (20000), Section B discriminator (sample=5
# rejected by idle bound=4, accepted by scaled=16), Section C ordering (C_UPPER
# base=8 < C_HOLD_S base=10 < C_TIMEOUT base=15 holds at any common factor).
assert "E: forced-factor=4 (LA=128/NP=32): base constants and Section B/C invariants" \
    env -u REIFY_LOAD_TOLERANCE_FACTOR \
        REIFY_LOAD_TOLERANCE_LOADAVG=128 REIFY_LOAD_TOLERANCE_NPROC=32 SCRIPT_DIR="$SCRIPT_DIR" \
    bash -c '
        source "$SCRIPT_DIR/load_tolerance_lib.sh"
        f=$(load_tolerance_factor)
        [ "$f" -eq 4 ] || exit 1
        [ "$(( 20000 * f ))" -eq 80000 ] || exit 1
        base=4; scaled=$(( base * f ))
        ! [ 5 -lt "$base" ] && [ 5 -lt "$scaled" ] || exit 1
        [ "$(( 8 * f ))" -lt "$(( 10 * f ))" ] && [ "$(( 8 * f ))" -lt "$(( 15 * f ))" ]
    '

# Section B ordering invariant: exempt-vs-blocked discriminator survives scaling.
_e_exempt="${EXEMPT_BOUND:-}"
_e_hold="${HOLD_S:-}"
_e_mwait="${MERGE_WAIT:-}"
assert "E: ordering EXEMPT_BOUND < HOLD_S < MERGE_WAIT at live factor (Section B discriminator)" \
    bash -c '[ -n "$1" ] && [ -n "$2" ] && [ -n "$3" ] && \
             [ "$1" -lt "$2" ] && [ "$2" -lt "$3" ]' \
    _ "$_e_exempt" "$_e_hold" "$_e_mwait"

# Section C ordering invariant: exit-75 reachable before holder-release and outer-timeout.
_e_cupper="${C_UPPER:-}"
_e_chold="${C_HOLD_S:-}"
_e_ctimeout="${C_TIMEOUT:-}"
assert "E: ordering C_UPPER < C_HOLD_S AND C_UPPER < C_TIMEOUT (Section C exit-75 reachable)" \
    bash -c '[ -n "$1" ] && [ -n "$2" ] && [ -n "$3" ] && \
             [ "$1" -lt "$2" ] && [ "$1" -lt "$3" ]' \
    _ "$_e_cupper" "$_e_chold" "$_e_ctimeout"

test_summary
