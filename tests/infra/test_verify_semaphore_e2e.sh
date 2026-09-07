#!/usr/bin/env bash
# Integration gate (PRD task ε): e2e test of the composed semaphore through verify.sh.
# Proves α+β+γ+δ compose correctly end-to-end by driving the REAL scripts/verify.sh
# in execute mode and asserting:
#   A — held-slot serialization (two concurrent task runs hold-serialize at N=1)
#   B — merge exemption (DF_VERIFY_ROLE=merge bypasses the held slot)
#   C — exit-75 propagation (acquisition deadline propagates out of verify.sh)
#   D — print-plan occt-cap=24 override + compile/check/clippy outside gated region
#
# Task 5144 residual failure-mode stabilization (07-07/08 survey; this suite was
# the top infra flakiness offender). Four residual live modes, each root-caused
# and resolved below; all four are now covered by this file's own assertions
# (unit-test blocks Part C/D/E/F for the mechanism, Sections A/B/C/F1/F2 for the
# execute-mode acceptance signal, load-accepted per step-9 — see below):
#
#   mode-1 (5077) — nested verify.sh exit 127 + "test_helpers.sh:42 integer
#     expression expected" + "tree-sitter pre-generation failed". Two causes:
#     (a) the OLD per-section REAL tree-sitter pre-gen inside make_stub_bin was
#         load-fragile — an interrupted regen left partial/0-byte outputs that
#         cascaded into the next section's nested-verify post-check. Fixed by a
#         ONE-TIME suite-start readiness gate (ensure_tree_sitter_ready /
#         _ts_outputs_healthy / _ts_ready_check, below) so every nested verify
#         deterministically hits the up-to-date fast-path (no regen -> no
#         corruption -> no cascade).
#     (b) an empty REIFY_SLOT_EVENT_LOG (both nested runs exiting before ever
#         acquiring the slot) fed an empty operand into Section A's raw
#         `test -ge` causal check, surfacing as "integer expression expected"
#         via test_helpers.sh assert()'s `if "$@"` line. Current main
#         test_helpers.sh:42 (task 4968) is already guarded — the failing
#         "line 42" copy in the 5077 log was a STALE branch's nested
#         test_helpers.sh from an earlier era. Fixed at THIS test's level (not
#         by editing test_helpers.sh) via _assert_serialized_events, which
#         validates all four causal operands as non-empty integers before any
#         -eq/-ge comparison touches them.
#   mode-2 (5125) — Section B rc=2: a merge-role nested verify re-emitted
#     run_all.sh's plan and recursed (run_all -> this test -> merge verify ->
#     run_all -> ...), SIGKILLed at the 30m wall. Already fixed ON MAIN by the
#     REIFY_INFRA_SUITE_ACTIVE re-entrancy sentinel (commit cd12443277, task
#     5125) and inherited by this branch — no 5144 change was needed here; see
#     run_merge_while_task_slot_held's REIFY_INFRA_SUITE_ACTIVE=1 spawn below.
#   mode-3 (5127) — rc=127. The task hypothesized a hardcoded sandbox
#     copy-list; verified FALSE for this test — it copies no scripts into any
#     sandbox, every section runs $REPO_ROOT/scripts/verify.sh directly
#     (make_stub_bin only stubs cargo/npm/tree-sitter onto PATH). rc=127 is
#     therefore the same recursion class as mode-2 (a nested run_all hitting a
#     not-yet-present command on a stale branch): eliminated by the inherited
#     mode-2 guard and now diagnosable via _dump_captured_stderr if it recurs.
#     No copy-list-derivation code was added — it would be dead code against a
#     non-existent list.
#   mode-4 (5111) — silent FAIL with no captured reason: the exit-code
#     assertions (Sections A/B/C/F1/F2) dumped only the numeric checker's
#     (empty) output, never the nested run's own stderr. Fixed by
#     _dump_captured_stderr, wired into every execute-mode section on failure.
#
# Acceptance (task 5144 step-9): the full suite (unit-test blocks + Sections
# A-I) is green both at idle and under tests/infra/cpu_load_fixture.sh-induced
# load. Every wall-clock DEADLINE in the acceptance path is already
# _load_scaled_deadline-scaled (REIFY_LOAD_TOLERANCE_FACTOR-scaled); HOLD_S /
# MERGE_WAIT / C_HOLD_S stay intentionally FIXED — they are external-holder
# hold durations, not anti-hang deadlines, and scaling them would only
# lengthen the acceptance run without changing what is proven. No assertion
# went flaky under load during step-9's acceptance pass, so no deadline
# required rescaling.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/load_tolerance_lib.sh" ] || { echo "ERROR: load_tolerance_lib.sh not found at $SCRIPT_DIR/load_tolerance_lib.sh"; exit 1; }
source "$SCRIPT_DIR/load_tolerance_lib.sh"

# For nextest_available_in_plan (the plan-header availability probe below).
# Sourcing the lib installs no trap and builds no environment — only
# nextest_absent_init does that, and this suite deliberately never calls it.
[ -f "$SCRIPT_DIR/nextest_absent_lib.sh" ] || { echo "ERROR: nextest_absent_lib.sh not found at $SCRIPT_DIR/nextest_absent_lib.sh"; exit 1; }
source "$SCRIPT_DIR/nextest_absent_lib.sh"

# compute_sha256 (task 5144): needed by the tree-sitter readiness helpers
# below (ensure_tree_sitter_ready's stamp-match check) and by the
# _make_fake_ts_dir unit-test fixture, so both sides of the contract use the
# EXACT same hash mechanism as scripts/tree-sitter-generate.sh itself.
[ -f "$REPO_ROOT/scripts/lib.sh" ] || { echo "ERROR: lib.sh not found at $REPO_ROOT/scripts/lib.sh"; exit 1; }
source "$REPO_ROOT/scripts/lib.sh"

# Preserve a keepalive fd (task 4931 / esc-3002-145 FIX #2): _wait_for_marker's
# up-to-600s poll (Section F) and _wait_for_holder_ready's poll both run with
# zero interim output, which could otherwise trip dark-factory's clock-stop
# heartbeat-idle backstop (180s) into a false 'wedged' kill of a genuinely-alive
# poll. fd 3 is duplicated from the ORIGINAL stderr here, before any assertion
# runs, so a keepalive emitted from inside either poll loop reaches the real
# stream even if some future caller wraps them in output suppression. Kept
# distinct from $F_ERR/$C_ERR/$MERGE_ERR (the files this suite greps for
# child-verify markers, esc-4789-63) so the keepalive can never pollute those
# marker assertions.
exec 3>&2
export REIFY_KEEPALIVE_FD=3

# _load_scaled_deadline BASE [MAX]
# Echo a load-scaled deadline: BASE × load_tolerance_factor (from load_tolerance_lib.sh),
# clamped to MAX (if provided) so anti-hang guards never balloon to mask a genuine hang.
# On idle hosts (factor=1) the result equals BASE byte-for-byte (no regression).
# The MAX cap is the genuinely-testable behavior that anchors the Part B unit tests:
#   BASE=30  factor=4 → 120 (no cap)
#   BASE=180 factor=8 → 1440, cap 600 → 600
#   BASE=60  factor=1 → 60  (floor = BASE)
# Defined early (before C_HOLD_S/C_TIMEOUT) so C_TIMEOUT can be computed at startup.
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

C_HOLD_S=300   # hold-until-killed: holder never self-releases before verify.sh returns (> max scaled
               # C_TIMEOUT of 200).  Explicitly killed after the verify.sh `wait`, so the WAIT=1
               # acquire ALWAYS times out → exit 75, independent of preamble duration (Fix 2, task 4864).
C_TIMEOUT="$(_load_scaled_deadline 120 200)"  # generous anti-hang guard; exit 75 fires ~1s after WAIT=1, never the discriminator

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
#                 The suite-start ensure_tree_sitter_ready call (task 5144 F1,
#                 below Part D / before Section A) readies the REAL
#                 tree-sitter-reify/src/ ONCE so the staleness fast-path exits
#                 0 before calling tree-sitter in every hermetic subshell; the
#                 'generate' branch here redirects output to a hermetic tmpdir
#                 (not tree-sitter-reify/src/) as a fail-fast fallback in case
#                 readiness did not hold (execute-mode sections gate on
#                 _TS_READY and skip driving entirely in that case).
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
# holder sites (B, C, F1).  Mirrors _wait_for_reader_lock from task #4847.
_wait_for_holder_ready() {
    local marker="$1"
    local deadline_s="$2"
    local max_ticks=$(( deadline_s * 20 ))
    local tick=0
    while [ "$tick" -lt "$max_ticks" ]; do
        [ -f "$marker" ] && return 0
        sleep 0.05
        # Keepalive tick (task 4931 / esc-3002-145 FIX #2): keeps the 0.05s
        # cadence intact; load_tolerant_keepalive itself throttles to at most
        # one @@REIFY_CLOCK_HEARTBEAT@@ per REIFY_CLOCK_HEARTBEAT_SECS, so this
        # adds negligible overhead per tick.
        load_tolerant_keepalive
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
        # Keepalive tick (task 4931 / esc-3002-145 FIX #2): this poll can run
        # up to ~600s under load (_load_scaled_deadline 120 600 in Section F),
        # entirely on the caller's own stderr with zero interim output.
        # load_tolerant_keepalive throttles to REIFY_CLOCK_HEARTBEAT_SECS, so
        # the 0.05s cadence is unaffected.
        load_tolerant_keepalive
        tick=$(( tick + 1 ))
    done
    return 1
}

# _clear_psi_fixture_on_marker <watch_file> <marker> <deadline_s> <fixture_path> <quiet_avg10>
# Causal replacement for a fixed `sleep N` PSI-fixture updater (task 5839).
# Blocks until the gate under test PROVES it observed pressure — i.e. until
# <marker> lands in <watch_file> — and only then clears <fixture_path> to
# <quiet_avg10>.  A wall-clock delay cannot express this: the gate's startup
# latency (bash + verify.sh's preamble, which sources lib.sh, cpu-admit.sh and
# lib_clock_stop.sh) is unbounded under load, and cpu-admit.sh admits on its
# FIRST poll when avg10 is already below threshold — taking the uncontended
# fast path with _ca_waited=0, so clock_enter_wait/clock_maybe_heartbeat are
# never reached and clock_exit_wait emits no START.  Measured at base
# HEAD=c89bac9267: rc=0 with ZERO clock markers and empty stderr.
# Returns 0 once rewritten; 1 with the fixture UNCHANGED if the deadline
# elapses, so a gate that never entered the wait fails LOUDLY downstream
# (its own `timeout` bound fires → rc 124) instead of being handed a spurious
# admit that would leave only the marker asserts red — the very signature 5839
# exists to remove.
#
# MARKER-STREAM NOTE: this helper is not marker-silent, and is not meant to be.
# _wait_for_marker calls load_tolerant_keepalive on every 0.05s tick, and that
# emits a real @@REIFY_CLOCK_HEARTBEAT@@ line to ${REIFY_KEEPALIVE_FD:-2} — fd 3
# in this suite (line 99), i.e. the parent stream — on the FIRST tick, because
# the throttle's _REIFY_KEEPALIVE_LAST_EMIT_EPOCH starts empty and defaults to 0.
# That emission is deliberate (it is what keeps a long causal wait from tripping
# dark-factory's 180s heartbeat-idle backstop) and it can never reach the file
# being polled, since fd 3 is dup'd from the ORIGINAL stderr, kept distinct from
# every $*_ERR capture this suite greps.  What must stay marker-free is the
# helper's own DIAGNOSTIC LINE below (T1f pins exactly that, and no more).
_clear_psi_fixture_on_marker() {
    local _watch="$1" _marker="$2" _deadline_s="$3" _fixture="$4" _avg10="$5"
    if ! _wait_for_marker "$_watch" "$_marker" "$_deadline_s"; then
        # Deliberately does NOT echo "$_marker": the token would leak into the
        # parent verify stream and be parsed as a real clock-stop event by
        # dark_factory:1916 (esc-4789-63, the same hazard assert_marker
        # documents).  Describe the marker, never print it.
        echo "  [5839] _clear_psi_fixture_on_marker: gate never emitted the awaited clock marker within ${_deadline_s}s; fixture left stuck (downstream asserts will fail loudly)" >&2
        return 1
    fi
    # Atomic swap: the gate re-opens <fixture_path> on every poll, so a
    # truncate-then-write could be read mid-write.  An empty read would make
    # cpu_admit's `[ -z "$_avg10" ]` branch admit, silently converting a
    # partial write into a passing test.  Same-directory temp + mv is a
    # rename, so every poll sees either the old or the new content.
    local _tmp="${_fixture}.tmp.$$"
    printf 'some avg10=%s avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        "$_avg10" > "$_tmp"
    mv -f "$_tmp" "$_fixture"
    return 0
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

# ===========================================================================
# Tree-sitter readiness helpers (task 5144 F1)
# ===========================================================================
# _ts_outputs_healthy <ts_src_dir>
# Returns 0 iff <ts_src_dir>/src/{parser.c,grammar.json,node-types.json} all
# exist AND are non-empty ([ -s ]); else 1.  Pure — reads only under the
# given dir, never mutates anything.  This is the same output signature
# tree-sitter-generate.sh's own post-generation existence loop checks
# (tree-sitter-generate.sh:167-172), plus the non-empty guard that catches
# the 0-byte-parser.c corruption class left by a killed/interrupted real
# generation under load (task 5144 cause 1).
_ts_outputs_healthy() {
    local _ts_src_dir="$1"
    local _f
    for _f in parser.c grammar.json node-types.json; do
        [ -s "$_ts_src_dir/src/$_f" ] || return 1
    done
    return 0
}

# _ts_ready_check <ts_src_dir>
# True iff <ts_src_dir>'s outputs are healthy (_ts_outputs_healthy) AND
# <ts_src_dir>/src/.grammar_hash.stamp matches the current sha256 of
# <ts_src_dir>/grammar.js — the same up-to-date contract
# tree-sitter-generate.sh's own non-force staleness check uses
# (tree-sitter-generate.sh:50-63). Pure — reads only, never mutates.
_ts_ready_check() {
    local _ts_src_dir="$1"
    _ts_outputs_healthy "$_ts_src_dir" || return 1
    [ -f "$_ts_src_dir/src/.grammar_hash.stamp" ] || return 1
    local _hash
    _hash="$(compute_sha256 "$_ts_src_dir/grammar.js" | awk '{print $1}')"
    [ "$(cat "$_ts_src_dir/src/.grammar_hash.stamp" 2>/dev/null)" = "$_hash" ]
}

# _ts_real_regenerate
# Default regenerator for ensure_tree_sitter_ready's suite-start (no-args)
# call: force-regenerates the REAL (gitignored) tree-sitter-reify/src/ via
# the real scripts/tree-sitter-generate.sh --force. ~/.cargo/bin is prepended
# to PATH so the real tree-sitter CLI resolves ahead of any stub later placed
# on PATH by make_stub_bin/apply_hermetic_env. Wrapped in a
# _load_scaled_deadline-scaled timeout so a hung/wedged generation cannot
# wedge the whole suite under cpu_load_fixture load (BASE=180 covers
# tree-sitter-generate.sh's own worst case: 120s lock wait + 60s generate).
_ts_real_regenerate() {
    PATH="$HOME/.cargo/bin:$PATH" timeout "$(_load_scaled_deadline 180 600)" \
        bash "$REPO_ROOT/scripts/tree-sitter-generate.sh" --force
}

# ensure_tree_sitter_ready [<ts_src_dir> <regenerator...>]
# Idempotently guarantees <ts_src_dir>'s generated tree-sitter outputs are
# ready (_ts_ready_check). If already ready, returns immediately WITHOUT
# invoking the regenerator. Otherwise invokes <regenerator...> once and
# re-checks. Sets the GLOBAL _TS_READY to "1" on success or "0" on failure
# (with a diagnostic naming tree-sitter to stderr), returning 0/1 to match.
# The regenerator's own exit code is never allowed to escape this function
# (guarded with `|| true`) — its effect is observed only via the re-check, so
# a failing/timed-out regenerator FAILS this function cleanly instead of
# aborting the whole suite under set -euo pipefail.
#
# Test seam: called with explicit <ts_src_dir>/<regenerator...> args (see the
# unit tests below), tests exercise every branch against _make_fake_ts_dir
# fixtures + injected fake regenerators, never touching the real (gitignored)
# tree-sitter-reify/src/ or invoking the real tree-sitter CLI. Called with NO
# args (the suite-start call, below Part D and before Section A) it defaults
# to the real tree-sitter-reify dir and _ts_real_regenerate.
ensure_tree_sitter_ready() {
    local _ts_dir
    if [ "$#" -eq 0 ]; then
        _ts_dir="$REPO_ROOT/tree-sitter-reify"
        set -- _ts_real_regenerate
    else
        _ts_dir="$1"
        shift
    fi

    if _ts_ready_check "$_ts_dir"; then
        _TS_READY=1
        return 0
    fi

    "$@" || true

    if _ts_ready_check "$_ts_dir"; then
        _TS_READY=1
        return 0
    fi

    _TS_READY=0
    echo "ERROR: ensure_tree_sitter_ready: tree-sitter artifacts under '$_ts_dir' are not ready after a regeneration attempt (tree-sitter CLI/grammar generation failed, or still unhealthy/stale) — cannot run execute-mode e2e sections" >&2
    return 1
}

# ===========================================================================
# _assert_serialized_events helper (task 5144 F2)
# ===========================================================================
# _assert_serialized_events <acq> <rel> <max_acq> <min_rel>
# Guarded replacement for a raw `test "$max_acq" -ge "$min_rel"` causal check
# (Section A below). Validates all four operands are non-empty integers
# (the same ''|*[!0-9]*-case idiom load_tolerance_lib.sh uses) BEFORE any
# -eq/-ge comparison touches them, so an empty/non-numeric operand — the
# exact symptom left by an empty REIFY_SLOT_EVENT_LOG (task 5144 cause 1:
# both nested verify runs exiting before ever acquiring the slot) — never
# reaches a bare `test` and produces "test: : integer expression expected".
# That message was a SYMPTOM of the empty log surfacing through
# test_helpers.sh's assert() `if "$@"` line, not a test_helpers.sh bug.
#
# Returns 0 iff acq==2, rel==2, AND max(ACQUIRE_ts) >= min(RELEASE_ts) — i.e.
# both runs traversed the gated region and the second critical section began
# only after the first ended (true hold-serialization at N=1). Returns 1
# with a diagnostic on stderr otherwise: naming the empty/incomplete event
# log when any operand is non-numeric or when acq/rel==0 (no ACQUIRE/RELEASE
# events recorded at all), or a descriptive not-serialized message when the
# counts/ordering simply don't satisfy serialization. Pure — never mutates
# anything, safe to call directly under set -euo pipefail via `assert`.
_assert_serialized_events() {
    local _acq="$1" _rel="$2" _max_acq="$3" _min_rel="$4"
    local _v

    for _v in "$_acq" "$_rel" "$_max_acq" "$_min_rel"; do
        case "$_v" in
            ''|*[!0-9]*)
                echo "ERROR: _assert_serialized_events: event log is empty or incomplete — no ACQUIRE/RELEASE events recorded (acq='${_acq}' rel='${_rel}' max_acq='${_max_acq}' min_rel='${_min_rel}'); cannot assert causal serialization" >&2
                return 1
                ;;
        esac
    done

    # All four operands are confirmed non-empty integers below this point —
    # bare -eq/-ge comparisons are now safe.
    if [ "$_acq" -eq 0 ] || [ "$_rel" -eq 0 ]; then
        echo "ERROR: _assert_serialized_events: event log is empty — no ACQUIRE/RELEASE events recorded (acq=${_acq} rel=${_rel})" >&2
        return 1
    fi

    if [ "$_acq" -eq 2 ] && [ "$_rel" -eq 2 ] && [ "$_max_acq" -ge "$_min_rel" ]; then
        return 0
    fi

    echo "ERROR: _assert_serialized_events: events NOT serialized (acq=${_acq} rel=${_rel} max_acq=${_max_acq} min_rel=${_min_rel}; expected exactly 2 ACQUIRE + 2 RELEASE events with max(ACQUIRE_ts) >= min(RELEASE_ts))" >&2
    return 1
}

# ===========================================================================
# _dump_captured_stderr helper (task 5144 F2 mode-4)
# ===========================================================================
# _dump_captured_stderr <label> <errfile>
# Observability helper for the 5111 "silent FAIL" mode: dumps a nested run's
# captured stderr (tail -50, matching assert()'s own captured-output dump in
# test_helpers.sh) under a clear bracketed header naming <label>, so a
# failing exit-code assertion (Sections A/B/C/F) or a failed Section F2
# print-plan capture has a diagnosable reason in the log instead of silence.
# <errfile> missing or empty (0 bytes) prints a graceful "(no stderr
# captured)" note rather than an empty/confusing header.
#
# MUST NEVER abort its caller under set -euo pipefail: always returns 0
# regardless of <errfile>'s state, because it is invoked as a bare
# diagnostic statement (not wrapped in a guarded assert). Complements, not
# replaces, assert()'s own dump-on-FAIL (which dumps the CHECKER's captured
# output — empty for a numeric `test $RC -eq 0` checker); this dumps the
# SEPARATE nested-run errfile that holds the actual reason.
_dump_captured_stderr() {
    local _label="$1"
    local _errfile="$2"
    echo "  ---- [${_label}] captured stderr ----"
    if [ -s "$_errfile" ]; then
        tail -n 50 "$_errfile" | sed 's/^/  | /'
    else
        echo "  (no stderr captured)"
    fi
    echo "  ---- [${_label}] end captured stderr ----"
    return 0
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

    # stub tree-sitter: satisfies `command -v` guard.
    # The suite-start ensure_tree_sitter_ready call (task 5144 F1) readies the
    # REAL tree-sitter-reify/src/ ONCE, before any execute-mode section calls
    # make_stub_bin, so the staleness fast-path exits before this stub's
    # 'generate' branch is reached. If it IS somehow reached, write to a
    # hermetic tmpdir rather than $PWD/src/ (= tree-sitter-reify/src/) so we
    # never contaminate the real source tree with 0-byte stubs.
    # tree-sitter-generate.sh's post-check then fails (files not in expected
    # src/), propagating as verify.sh non-zero → caught by the relevant
    # section's exit-code assertion (fail-fast).
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
#
# REIFY_TS_FRESHNESS_TARGET_DIR: a per-run throwaway target tree, so the
# tree-sitter freshness guard's ensure/check leaves are clean no-ops under a
# stubbed cargo and never read or mutate the lane's real ./target (see the
# rationale at the export below).
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
    # Redirect the tree-sitter freshness guard (task 5629) at a throwaway,
    # per-run target tree instead of this lane's real ./target.
    #
    # WHY: verify.sh runs ./scripts/tree-sitter-freshness.sh `ensure` before the
    # cargo wave and `check` after it, and `check`'s post-condition is "every
    # archive rebuilt inside this run's window matches the sources on disk".
    # Here cargo is STUBBED, so nothing is ever compiled and the real target/'s
    # archives keep whatever provenance the last genuine build left behind — a
    # post-condition asserted against archives this run never touched.  In a lane
    # whose archive is legitimately stale (or merely unattested) that is an
    # unconditional RED with no relation to the semaphore behaviour under test.
    #
    # Pointing the knob at an empty per-run dir makes BOTH leaves clean no-ops
    # ("no built archive under <dir>; nothing to force/check", exit 0).  It also
    # keeps `ensure`'s repair path from bumping mtimes on the worktree's tracked
    # tree-sitter sources — in a CoW-seeded warm lane those mtimes are exactly
    # the signal that mechanism reads.  Both halves are pinned by
    # test_freshness_target_dir_override_is_a_clean_noop and
    # test_semaphore_e2e_harness_isolates_the_freshness_guard in
    # tests/infra/test_tree_sitter_pipeline.sh.
    #
    # Harness-side, not gate-side: verify.sh cannot distinguish a stubbed cargo
    # from a real one, so any relaxation inside the guard would weaken the real
    # merge gate — precisely the post-condition that closes this defect class.
    # Mirrors how this helper already neutralizes the PSI and compile gates.
    #
    # Derived from $stubdir (which every call site places inside its own
    # `mktemp -d`), so the path is per-run and is reclaimed by _TMPDIRS cleanup.
    # The guard mkdir -p's it on demand; this helper stays exports-only.
    export REIFY_TS_FRESHNESS_TARGET_DIR="$stubdir.ts-freshness-target"
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
    # A_ERR1/A_ERR2 (task 5144 F2 mode-4): globals exposing each run's errfile
    # so a failing exit-code assertion can dump the reason via
    # _dump_captured_stderr, mirroring $MERGE_ERR/$C_ERR/$F_ERR.
    A_ERR1="$_tmpdir/runA1.err"
    A_ERR2="$_tmpdir/runA2.err"
    local _pid1 _pid2
    # First concurrent task run.
    (
        apply_hermetic_env "$_stubdir" "$_lock"
        export REIFY_E2E_CARGO_SLEEP=2
        DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) 2>"$A_ERR1" &
    _pid1=$!

    # Second concurrent task run — same lock base so both compete for the single slot.
    (
        apply_hermetic_env "$_stubdir" "$_lock"
        export REIFY_E2E_CARGO_SLEEP=2
        DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) 2>"$A_ERR2" &
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
# Unit-test-only fixtures (task 5144): synthetic tree-sitter output trees +
# a generic output-capture helper for the new helper unit-test blocks below.
# Hermetic — these NEVER touch the real (gitignored) tree-sitter-reify/src/.
# ===========================================================================

# _make_fake_ts_dir <dir> <mode>
# Populate <dir> as a synthetic tree-sitter-reify-shaped source tree (a
# grammar.js at <dir>/grammar.js plus the three generated outputs and the
# stamp file under <dir>/src/), with controllable health so
# _ts_outputs_healthy/ensure_tree_sitter_ready unit tests can exercise every
# branch without ever touching the real tree-sitter-reify/src/.
# <mode>:
#   healthy           — all three outputs present, non-empty, stamp matches
#                        the real sha256 of the fixture grammar.js (mirrors
#                        tree-sitter-generate.sh's own staleness contract).
#   zero_byte_parser  — src/parser.c exists but is 0 bytes (the exact
#                        corruption class left by a killed/interrupted
#                        real-tree-sitter run under load, task 5144 cause 1).
#   missing_output    — src/node-types.json is entirely absent.
#   stamp_mismatch    — all three outputs present + non-empty, but the stamp
#                        file holds a hash that does NOT match grammar.js.
_make_fake_ts_dir() {
    local _dir="$1"
    local _mode="$2"
    mkdir -p "$_dir/src"
    printf 'module.exports = grammar({ name: "fake_reify_fixture_5144" });\n' > "$_dir/grammar.js"
    local _hash
    _hash="$(compute_sha256 "$_dir/grammar.js" | awk '{print $1}')"

    case "$_mode" in
        healthy)
            printf 'FAKE_PARSER_C\n' > "$_dir/src/parser.c"
            printf '{"FAKE":"grammar"}\n' > "$_dir/src/grammar.json"
            printf '{"FAKE":"node-types"}\n' > "$_dir/src/node-types.json"
            printf '%s' "$_hash" > "$_dir/src/.grammar_hash.stamp"
            ;;
        zero_byte_parser)
            : > "$_dir/src/parser.c"
            printf '{"FAKE":"grammar"}\n' > "$_dir/src/grammar.json"
            printf '{"FAKE":"node-types"}\n' > "$_dir/src/node-types.json"
            printf '%s' "$_hash" > "$_dir/src/.grammar_hash.stamp"
            ;;
        missing_output)
            printf 'FAKE_PARSER_C\n' > "$_dir/src/parser.c"
            printf '{"FAKE":"grammar"}\n' > "$_dir/src/grammar.json"
            printf '%s' "$_hash" > "$_dir/src/.grammar_hash.stamp"
            ;;
        stamp_mismatch)
            printf 'FAKE_PARSER_C\n' > "$_dir/src/parser.c"
            printf '{"FAKE":"grammar"}\n' > "$_dir/src/grammar.json"
            printf '{"FAKE":"node-types"}\n' > "$_dir/src/node-types.json"
            printf 'deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef' > "$_dir/src/.grammar_hash.stamp"
            ;;
        *)
            echo "_make_fake_ts_dir: unknown mode '$_mode'" >&2
            return 1
            ;;
    esac
}

# _run_capturing <outfile> <cmd...>
# Run <cmd...> with combined stdout+stderr redirected to <outfile>; returns
# <cmd...>'s own exit code so callers can assert on BOTH the captured text
# (substring checks) and the numeric result, e.g.:
#   _rc=0; _run_capturing "$out" some_helper args || _rc=$?
# set -e-safe ONLY when callers use that `|| _rc=$?` idiom (matching this
# file's existing `wait "$_pid" || _rc=$?` pattern) — this helper's own
# non-zero return would otherwise abort the suite under set -euo pipefail.
_run_capturing() {
    local _outfile="$1"
    shift
    local _rc=0
    "$@" >"$_outfile" 2>&1 || _rc=$?
    return "$_rc"
}

# ===========================================================================
# _load_scaled_deadline unit tests (Part B helper, task #4895 S3)
# ===========================================================================
# Deterministic: REIFY_LOAD_TOLERANCE_FACTOR env-injection overrides host load,
# exactly like test_load_tolerance_lib.sh Test 6.  Assertions are exact integer
# identities of load_tolerant_attempts (BASE x factor) plus the MAX-cap logic.
# '|| echo UNDEFINED' provides a safe fallback for any unexpected definition failure;
# _load_scaled_deadline is defined early in the file (before C_HOLD_S/C_TIMEOUT).
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
# _clear_psi_fixture_on_marker unit tests (G2 causal updater, task 5839)
# ===========================================================================
# Pins the contract of the causal replacement for Section G2's old fixed
# `sleep 2` PSI-fixture updater: the fixture must be cleared when the gate has
# PROVEN it observed pressure (a clock marker landed in the watch file), and
# NEVER on wall-clock alone.  Every case here is hermetic — the watch file is a
# plain fixture written by this block, no verify.sh is spawned — so the whole
# block runs in ~3s and is load-independent apart from T1e's deliberate 2s
# writer delay.  Runs early (before the heavy execute sections) for fast
# feedback.  The awaited token rides only in printf/helper ARGUMENTS here,
# never in an echoed assert description (esc-4789-63; see assert_marker).
echo ""
echo "--- _clear_psi_fixture_on_marker unit tests (G2 causal updater, task 5839) ---"

_CPF_DIR="$(mktemp -d)"
_TMPDIRS+=("$_CPF_DIR")
_CPF_STUCK='some avg10=99.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n'

# --- T1a/T1b: marker ALREADY present → returns 0 and rewrites the fixture ---
_CPF_WATCH_A="$_CPF_DIR/watch_a.txt"
_CPF_PSI_A="$_CPF_DIR/psi_a"
# shellcheck disable=SC2059  # _CPF_STUCK is a literal format string, not user input
printf "$_CPF_STUCK" > "$_CPF_PSI_A"
printf '%s\n' '@@REIFY_CLOCK_HEARTBEAT@@ reason=psi_pressure waited=1' > "$_CPF_WATCH_A"

_CPF_RC_A=0
_clear_psi_fixture_on_marker "$_CPF_WATCH_A" '@@REIFY_CLOCK_HEARTBEAT@@' 5 "$_CPF_PSI_A" 10.00 \
    2>/dev/null || _CPF_RC_A=$?

assert "T1a: _clear_psi_fixture_on_marker returns 0 when the awaited marker is already present" \
    test "$_CPF_RC_A" -eq 0
assert "T1b: marker observed → fixture rewritten to the quiet avg10 (10.00 present, 99.00 gone)" \
    bash -c 'grep -qF "avg10=10.00" "$1" && ! grep -qF "avg10=99.00" "$1"' _ "$_CPF_PSI_A"

# --- T1c/T1d/T1f: deadline expiry → non-zero, fixture UNCHANGED, no token leak ---
# T1d is THE assert that encodes the 5839 fix: a wall-clock deadline must never
# be sufficient to clear the fixture.  Deadline 1 keeps this to ~1s
# (_wait_for_marker ticks at 0.05s × 20/s).
_CPF_WATCH_C="$_CPF_DIR/watch_c.txt"
_CPF_PSI_C="$_CPF_DIR/psi_c"
_CPF_DIAG_C="$_CPF_DIR/diag_c.txt"
: > "$_CPF_WATCH_C"
# shellcheck disable=SC2059
printf "$_CPF_STUCK" > "$_CPF_PSI_C"

_CPF_RC_C=0
_clear_psi_fixture_on_marker "$_CPF_WATCH_C" '@@REIFY_CLOCK_HEARTBEAT@@' 1 "$_CPF_PSI_C" 10.00 \
    2>"$_CPF_DIAG_C" || _CPF_RC_C=$?

assert "T1c: deadline elapses with no marker → returns non-zero (got ${_CPF_RC_C})" \
    test "$_CPF_RC_C" -ne 0
assert "T1d: deadline expiry leaves the fixture UNCHANGED (never clears on wall-clock alone)" \
    bash -c 'grep -qF "avg10=99.00" "$1" && ! grep -qF "avg10=10.00" "$1"' _ "$_CPF_PSI_C"
# T1f is scoped to the helper's OWN fd-2 diagnostic ($_CPF_DIAG_C) and claims
# nothing about the parent stream: _wait_for_marker's per-tick
# load_tolerant_keepalive legitimately writes a @@REIFY_CLOCK_HEARTBEAT@@ line to
# fd 3 (the suite's real stderr) on its first tick, so the run above DOES put a
# marker on the parent stream by design.  The regression this catches is a
# diagnostic that interpolates the awaited token (esc-4789-63) — see the helper's
# MARKER-STREAM NOTE for why the two streams are deliberately different.
assert "T1f: the timeout DIAGNOSTIC LINE carries no clock-marker token (the fd-3 keepalive heartbeat is separate and intentional)" \
    bash -c '! grep -qF "@@REIFY_CLOCK_" "$1"' _ "$_CPF_DIAG_C"

# --- T1e: late-arriving marker → the helper BLOCKS until it exists ---
# The elapsed bound is guaranteed by construction, not tuned: the writer sleeps
# 2s before the marker exists, so the helper cannot return before t0+2, and
# floor(t0+2) >= floor(t0)+1 makes the >= 1 integer-seconds check exact.
_CPF_WATCH_E="$_CPF_DIR/watch_e.txt"
_CPF_PSI_E="$_CPF_DIR/psi_e"
: > "$_CPF_WATCH_E"
# shellcheck disable=SC2059
printf "$_CPF_STUCK" > "$_CPF_PSI_E"
(
    sleep 2
    printf '%s\n' '@@REIFY_CLOCK_HEARTBEAT@@ reason=psi_pressure waited=1' >> "$_CPF_WATCH_E"
) &
_CPF_WRITER_E=$!

_CPF_T0="$(date +%s)"
_CPF_RC_E=0
_clear_psi_fixture_on_marker "$_CPF_WATCH_E" '@@REIFY_CLOCK_HEARTBEAT@@' \
    "$(_load_scaled_deadline 30 120)" "$_CPF_PSI_E" 10.00 2>/dev/null || _CPF_RC_E=$?
_CPF_T1="$(date +%s)"

kill "$_CPF_WRITER_E" 2>/dev/null || true
wait "$_CPF_WRITER_E" 2>/dev/null || true

_CPF_ELAPSED_E=$(( _CPF_T1 - _CPF_T0 ))
assert "T1e: late-arriving marker is observed → returns 0 (got ${_CPF_RC_E})" \
    test "$_CPF_RC_E" -eq 0
assert "T1e: late-arriving marker → fixture rewritten to the quiet avg10" \
    bash -c 'grep -qF "avg10=10.00" "$1" && ! grep -qF "avg10=99.00" "$1"' _ "$_CPF_PSI_E"
assert "T1e: helper BLOCKED until the marker existed (elapsed ${_CPF_ELAPSED_E}s >= 1; writer sleeps 2s)" \
    test "$_CPF_ELAPSED_E" -ge 1

# ===========================================================================
# _ts_outputs_healthy unit tests (Part C helper, task 5144 F1)
# ===========================================================================
# Pure existence+non-empty check over a <ts_src_dir> (TS_DIR-shaped: outputs
# live at <ts_src_dir>/src/{parser.c,grammar.json,node-types.json}) — exactly
# the corruption signature left by a killed/interrupted real-tree-sitter run
# under load (task 5144 cause 1). Exercised against _make_fake_ts_dir
# fixtures so the real (gitignored) tree-sitter-reify/src/ is never touched.
echo ""
echo "--- _ts_outputs_healthy unit tests (Part C helper, task 5144 F1) ---"

_TSH_DIR="$(mktemp -d)"
_TMPDIRS+=("$_TSH_DIR")
_make_fake_ts_dir "$_TSH_DIR/healthy" healthy
_make_fake_ts_dir "$_TSH_DIR/zero_byte" zero_byte_parser
_make_fake_ts_dir "$_TSH_DIR/missing" missing_output

_TSH_RC1=0
_ts_outputs_healthy "$_TSH_DIR/healthy" 2>/dev/null || _TSH_RC1=$?
_TSH_RC2=0
_ts_outputs_healthy "$_TSH_DIR/zero_byte" 2>/dev/null || _TSH_RC2=$?
_TSH_RC3=0
_ts_outputs_healthy "$_TSH_DIR/missing" 2>/dev/null || _TSH_RC3=$?

assert "_ts_outputs_healthy: healthy dir (all 3 outputs present + non-empty) -> rc 0 (got ${_TSH_RC1})" \
    test "$_TSH_RC1" -eq 0
assert "_ts_outputs_healthy: 0-byte parser.c -> rc 1 (got ${_TSH_RC2})" \
    test "$_TSH_RC2" -eq 1
assert "_ts_outputs_healthy: missing node-types.json -> rc 1 (got ${_TSH_RC3})" \
    test "$_TSH_RC3" -eq 1

# ===========================================================================
# ensure_tree_sitter_ready unit tests (Part D helper, task 5144 F1)
# ===========================================================================
# Exercises ensure_tree_sitter_ready's readiness/regenerate/re-check branches
# via _make_fake_ts_dir fixtures + an injected fake regenerator (test seam:
# `ensure_tree_sitter_ready <ts_dir> <regenerator...>`) so the unit tests
# NEVER touch the real (gitignored) tree-sitter-reify/src/ or invoke the
# real tree-sitter CLI. The suite-start call (below Part D, before Section A)
# uses the real defaults (no args).
echo ""
echo "--- ensure_tree_sitter_ready unit tests (Part D helper, task 5144 F1) ---"

_ETSR_DIR="$(mktemp -d)"
_TMPDIRS+=("$_ETSR_DIR")

# Case 1: already-healthy dir -> must NOT invoke the regenerator.
_ETSR_CALLED_1="$_ETSR_DIR/called-1"
_make_fake_ts_dir "$_ETSR_DIR/case1" healthy
_fake_regen_sentinel_1() { echo called >> "$_ETSR_CALLED_1"; return 0; }
_TS_READY=""
_ETSR_RC1=0
ensure_tree_sitter_ready "$_ETSR_DIR/case1" _fake_regen_sentinel_1 2>/dev/null || _ETSR_RC1=$?
_ETSR_READY1="$_TS_READY"

assert "ensure_tree_sitter_ready: already-healthy dir -> _TS_READY=1 (got '${_ETSR_READY1}')" \
    test "$_ETSR_READY1" = "1"
assert "ensure_tree_sitter_ready: already-healthy dir -> rc 0 (got ${_ETSR_RC1})" \
    test "$_ETSR_RC1" -eq 0
assert "ensure_tree_sitter_ready: already-healthy dir -> regenerator NOT invoked" \
    bash -c '[ ! -s "$1" ]' _ "$_ETSR_CALLED_1"

# Case 2: unhealthy dir + a fake regenerator that HEALS it.
_make_fake_ts_dir "$_ETSR_DIR/case2" zero_byte_parser
_fake_regen_heals() { _make_fake_ts_dir "$_ETSR_DIR/case2" healthy; }
_TS_READY=""
_ETSR_RC2=0
ensure_tree_sitter_ready "$_ETSR_DIR/case2" _fake_regen_heals 2>/dev/null || _ETSR_RC2=$?
_ETSR_READY2="$_TS_READY"

assert "ensure_tree_sitter_ready: unhealthy dir + healing regen -> _TS_READY=1 (got '${_ETSR_READY2}')" \
    test "$_ETSR_READY2" = "1"
assert "ensure_tree_sitter_ready: unhealthy dir + healing regen -> rc 0 (got ${_ETSR_RC2})" \
    test "$_ETSR_RC2" -eq 0

# Case 3: unhealthy dir + a fake regenerator that FAILS to heal.
_make_fake_ts_dir "$_ETSR_DIR/case3" missing_output
_fake_regen_noop() { return 0; }
_TS_READY=""
_ETSR_ERR3="$_ETSR_DIR/err3.txt"
_ETSR_RC3=0
_run_capturing "$_ETSR_ERR3" ensure_tree_sitter_ready "$_ETSR_DIR/case3" _fake_regen_noop || _ETSR_RC3=$?
_ETSR_READY3="$_TS_READY"

assert "ensure_tree_sitter_ready: unhealthy dir + non-healing regen -> _TS_READY=0 (got '${_ETSR_READY3}')" \
    test "$_ETSR_READY3" = "0"
assert "ensure_tree_sitter_ready: unhealthy dir + non-healing regen -> rc non-zero (got ${_ETSR_RC3})" \
    test "$_ETSR_RC3" -ne 0
assert "ensure_tree_sitter_ready: non-healing case emits a non-empty diagnostic naming tree-sitter" \
    bash -c '[ -s "$1" ] && grep -qi "tree-sitter" "$1"' _ "$_ETSR_ERR3"

# ===========================================================================
# _assert_serialized_events unit tests (Part E helper, task 5144 F2)
# ===========================================================================
# Guarded replacement for Section A's raw `test "$A_MAX_ACQ" -ge "$A_MIN_REL"`
# causal check: validates ACQUIRE/RELEASE counts and both timestamps are
# non-empty integers BEFORE any `-ge` comparison, so `test` never sees an
# empty/non-numeric operand. This is the fix for the 5077 log's failure mode:
# both nested verify runs exited 127, leaving an EMPTY event log, so
# A_MAX_ACQ/A_MIN_REL came back empty and the raw `test "" -ge ""` produced
# "test_helpers.sh: line 42: test: : integer expression expected" — a SYMPTOM
# of the empty event log (test_helpers.sh's line-42 `if "$@"` merely executed
# the caller's malformed comparison), not a test_helpers.sh bug.
echo ""
echo "--- _assert_serialized_events unit tests (Part E helper, task 5144 F2) ---"

_ASE_DIR="$(mktemp -d)"
_TMPDIRS+=("$_ASE_DIR")

# Case 1: acq=2 rel=2 max_acq=200 min_rel=100 -> serialized (rc 0).
_ASE_OUT1="$_ASE_DIR/case1.txt"
_ASE_RC1=0
_run_capturing "$_ASE_OUT1" _assert_serialized_events 2 2 200 100 || _ASE_RC1=$?

assert "_assert_serialized_events: acq=2 rel=2 max_acq=200 min_rel=100 -> rc 0 (serialized, got ${_ASE_RC1})" \
    test "$_ASE_RC1" -eq 0

# Case 2: acq=2 rel=2 max_acq=100 min_rel=200 -> NOT serialized (rc non-zero, clear message).
_ASE_OUT2="$_ASE_DIR/case2.txt"
_ASE_RC2=0
_run_capturing "$_ASE_OUT2" _assert_serialized_events 2 2 100 200 || _ASE_RC2=$?

assert "_assert_serialized_events: acq=2 rel=2 max_acq=100 min_rel=200 -> rc non-zero (NOT serialized, got ${_ASE_RC2})" \
    test "$_ASE_RC2" -ne 0
assert "_assert_serialized_events: NOT-serialized case emits a non-empty diagnostic message" \
    bash -c '[ -s "$1" ]' _ "$_ASE_OUT2"

# Case 3: acq=0 rel=0 max_acq="" min_rel="" (empty event log — the 5077 case)
# -> rc non-zero, output does NOT contain "integer expression expected", DOES
# contain a clear "event log empty"/"no ACQUIRE/RELEASE events" diagnostic.
_ASE_OUT3="$_ASE_DIR/case3.txt"
_ASE_RC3=0
_run_capturing "$_ASE_OUT3" _assert_serialized_events 0 0 "" "" || _ASE_RC3=$?

assert "_assert_serialized_events: empty event log (acq=0 rel=0) -> rc non-zero (got ${_ASE_RC3})" \
    test "$_ASE_RC3" -ne 0
assert "_assert_serialized_events: empty event log -> output does NOT contain 'integer expression expected'" \
    bash -c '! grep -qF "integer expression expected" "$1"' _ "$_ASE_OUT3"
assert "_assert_serialized_events: empty event log -> output names the empty event log (event log empty / no ACQUIRE.RELEASE events)" \
    bash -c 'grep -Eqi "event log (is )?empty|no ACQUIRE.*RELEASE events" "$1"' _ "$_ASE_OUT3"

# ===========================================================================
# _dump_captured_stderr unit tests (Part F helper, task 5144 F2 mode-4)
# ===========================================================================
# _dump_captured_stderr is defined above (line ~381), well ahead of these
# tests — no ordering/definition dependency here. Part F helper for mode-4
# observability (5111 "silent FAIL"): a nested run's captured stderr,
# dumped under a clear bracketed header, so a failing exit-code assertion
# (Sections A/B/C/F) or a set-e-safe capture failure (Section F2) leaves a
# diagnosable reason instead of silence.
# Contract: MUST NEVER abort its caller under set -euo pipefail — always
# returns rc 0 regardless of whether the errfile has content, is empty, or
# is missing — because it is invoked as a bare diagnostic statement, not
# wrapped in a guarded assert. Uses _run_capturing to capture the helper's
# OWN stdout+stderr so tests can assert on both content and rc.
echo ""
echo "--- _dump_captured_stderr unit tests (Part F helper, task 5144 F2 mode-4) ---"

_DCS_DIR="$(mktemp -d)"
_TMPDIRS+=("$_DCS_DIR")

# Case 1: non-empty errfile -> a bracketed header naming the label AND the
# errfile's own content (e.g. via tail) appear in the output.
_DCS_ERR1="$_DCS_DIR/err1.txt"
printf 'nested verify.sh: something exploded\nline 2 of stderr\n' > "$_DCS_ERR1"
_DCS_OUT1="$_DCS_DIR/out1.txt"
_DCS_RC1=0
_run_capturing "$_DCS_OUT1" _dump_captured_stderr "my-label-1" "$_DCS_ERR1" || _DCS_RC1=$?

assert "_dump_captured_stderr: non-empty errfile -> rc 0 (got ${_DCS_RC1})" \
    test "$_DCS_RC1" -eq 0
assert "_dump_captured_stderr: non-empty errfile -> output has a bracketed header naming the label" \
    bash -c 'grep -qF "[my-label-1]" "$1"' _ "$_DCS_OUT1"
assert "_dump_captured_stderr: non-empty errfile -> output contains the errfile's own content" \
    bash -c 'grep -qF "something exploded" "$1"' _ "$_DCS_OUT1"

# Case 2: empty (0-byte) errfile -> a graceful "(no stderr captured)" note, no crash.
_DCS_ERR2="$_DCS_DIR/err2.txt"
: > "$_DCS_ERR2"
_DCS_OUT2="$_DCS_DIR/out2.txt"
_DCS_RC2=0
_run_capturing "$_DCS_OUT2" _dump_captured_stderr "my-label-2" "$_DCS_ERR2" || _DCS_RC2=$?

assert "_dump_captured_stderr: empty errfile -> rc 0, does not crash (got ${_DCS_RC2})" \
    test "$_DCS_RC2" -eq 0
assert "_dump_captured_stderr: empty errfile -> output has a '(no stderr captured)' style note" \
    bash -c 'grep -qi "no stderr captured" "$1"' _ "$_DCS_OUT2"

# Case 3: missing/nonexistent errfile -> same graceful note, rc 0, caller
# survives (no set -e abort) — proven by requiring rc EXACTLY 0 (the
# strongest guarantee: a function that always returns 0 can never trip
# `set -e` regardless of whether its caller guards the call).
_DCS_ERR3="$_DCS_DIR/does-not-exist.txt"
_DCS_OUT3="$_DCS_DIR/out3.txt"
_DCS_RC3=0
_run_capturing "$_DCS_OUT3" _dump_captured_stderr "my-label-3" "$_DCS_ERR3" || _DCS_RC3=$?

assert "_dump_captured_stderr: missing errfile -> rc 0, caller survives (got ${_DCS_RC3})" \
    test "$_DCS_RC3" -eq 0
assert "_dump_captured_stderr: missing errfile -> output has a '(no stderr captured)' style note" \
    bash -c 'grep -qi "no stderr captured" "$1"' _ "$_DCS_OUT3"

# ===========================================================================
# Suite-start tree-sitter readiness gate (task 5144 F1)
# ===========================================================================
# ONE-TIME real regeneration/readiness check, before ANY execute-mode section
# runs a nested verify.sh. This is the structural fix for the tree-sitter
# prerequisite cascade (task 5144 cause 1): the OLD per-section real
# pre-generation inside make_stub_bin was the load-fragile step — an
# interrupted real-tree-sitter run under load left partial/corrupt outputs
# (via tree-sitter-generate.sh's own _cleanup_partial_outputs), and the NEXT
# section's nested verify then failed its own tree-sitter post-check,
# cascading through B/C/F. Doing readiness ONCE here makes every nested
# verify.sh deterministically hit the "up to date" fast-path (no regen -> no
# corruption -> no cascade). Execute-mode sections (A/B/C/F1) below gate on
# _TS_READY and fail loudly+cleanly instead of cascading cryptically when
# tree-sitter could not be readied.
#
# Hard prerequisite (reviewer follow-up, task 5144): _ts_real_regenerate can
# only ready tree-sitter-reify/src/ if the real tree-sitter CLI is on PATH
# (or under ~/.cargo/bin — see its PATH prepend above). That CLI is installed
# by scripts/setup-dev.sh ("---------- tree-sitter-cli ----------": `cargo
# install tree-sitter-cli --locked`), which every dev/CI environment running
# this suite is expected to have run — so its absence is an environment-
# provisioning gap, not something this suite can route around. test_helpers.sh
# (out of this task's scope) has only PASS/FAIL counters, no distinct SKIP
# bucket, so a CLI-absent run necessarily reports as a FAIL — the
# "Section X SKIPPED: tree-sitter artifacts not ready" wording in the asserts
# below (and the WARNING line right here) is what distinguishes that failure
# signature from a genuine semaphore regression: grep this file's own log
# output for "ts-ready" or "SKIPPED: tree-sitter" before suspecting the
# semaphore mechanism itself.
echo ""
echo "--- Tree-sitter readiness (suite start, task 5144 F1) ---"
_TS_READY=0
ensure_tree_sitter_ready || true
if [ "$_TS_READY" = "1" ]; then
    echo "  [ts-ready] tree-sitter artifacts ready — execute-mode sections will run" >&2
else
    echo "  [ts-ready] WARNING: tree-sitter artifacts NOT ready (environment-provisioning gap, e.g. missing tree-sitter CLI — see scripts/setup-dev.sh — not a semaphore regression) — execute-mode sections (A/B/C/F1) will FAIL loudly instead of cascading" >&2
fi

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
if [ "$_TS_READY" = "1" ]; then
    drive_two_concurrent_task_runs
    # Both runs must have exited 0: a run that errors mid-slot-hold could still
    # consume ~2s and satisfy the timing lower bound, producing a false green.
    assert "both concurrent task runs exited 0 (rc1=${RC1}, rc2=${RC2})" \
        test "$RC1" -eq 0 -a "$RC2" -eq 0
    # Mode-4 observability (task 5144): surface each nested run's captured
    # stderr when the exit-code assertion above fails, so a Section A failure
    # has a diagnosable reason instead of just the bare rc.
    if [ "$RC1" -ne 0 ] || [ "$RC2" -ne 0 ]; then
        _dump_captured_stderr "Section A run 1 (rc=${RC1})" "$A_ERR1"
        _dump_captured_stderr "Section A run 2 (rc=${RC2})" "$A_ERR2"
    fi
    assert "two concurrent task verify.sh test runs hold-serialize (elapsed >= 3000ms, got ${MS}ms)" \
        test "$MS" -ge 3000
    # --- Section A causal assertions (R-technique): parse REIFY_SLOT_EVENT_LOG ---
    # Single guarded assertion (task 5144 F2): exactly 2 ACQUIRE + 2 RELEASE
    # events — both runs traversed the gated region; guards against a vacuous
    # empty-log green (e.g. DISABLE=1) — AND max(ACQUIRE_ts) >= min(RELEASE_ts)
    # — the second critical section began only after the first ended; proves
    # true hold-serialization at N=1. _assert_serialized_events guards all
    # four operands as non-empty integers BEFORE any -eq/-ge comparison, so an
    # empty REIFY_SLOT_EVENT_LOG (task 5144 cause 1: both nested verify runs
    # exiting before ever acquiring the slot) FAILs with a clear diagnostic
    # instead of "test: : integer expression expected".
    # RED with CONCURRENCY=2 (both acquire concurrently → max(ACQ) < min(REL)).
    # RED with DISABLE=1 (no slot events → count 0 ≠ 2).
    A_ACQ_COUNT=$(awk '$3 == "ACQUIRE"' "$A_EVENTLOG" | wc -l | tr -d ' ')
    A_REL_COUNT=$(awk '$3 == "RELEASE"' "$A_EVENTLOG" | wc -l | tr -d ' ')
    A_MAX_ACQ=$(awk '$3 == "ACQUIRE" { print $1 }' "$A_EVENTLOG" | sort -n | tail -1)
    A_MIN_REL=$(awk '$3 == "RELEASE" { print $1 }' "$A_EVENTLOG" | sort -n | head -1)
    echo "  [A-causal] acq=${A_ACQ_COUNT} rel=${A_REL_COUNT} max_acq=${A_MAX_ACQ} min_rel=${A_MIN_REL}" >&2
    assert "Section A causal: events serialized (2 ACQUIRE + 2 RELEASE, max(ACQUIRE_ts) >= min(RELEASE_ts); acq=${A_ACQ_COUNT} rel=${A_REL_COUNT} max_acq=${A_MAX_ACQ} min_rel=${A_MIN_REL})" \
        _assert_serialized_events "$A_ACQ_COUNT" "$A_REL_COUNT" "$A_MAX_ACQ" "$A_MIN_REL"
else
    assert "Section A SKIPPED: tree-sitter artifacts not ready — cannot run execute-mode e2e sections (see readiness diagnostic above)" \
        false
fi

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
    _wait_for_holder_ready "$_ready" "$(_load_scaled_deadline 30 180)"  # R-technique: causally guarantees holder holds flock -x

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
        # REIFY_INFRA_SUITE_ACTIVE=1 (task 5125 re-entrancy guard): this Section-B
        # merge-role verify runs INSIDE the run_all.sh infra pool (run_all
        # executes this very test). A merge-role verify re-emits the wholesale
        # run_all.sh plan line, so without this sentinel it recurses:
        # run_all -> this test -> merge verify -> run_all -> ... (unbounded
        # fork-bomb, SIGKILLed at the 30m wall). The sentinel trips verify.sh's
        # re-entrancy guard so the nested verify emits no infra suite. Scoped to
        # THIS spawn (not broadcast on run_all's plan line) so it never leaks as
        # an ambient export into the other ~102 pool tests (cf.
        # test_run_all_ambient_isolation.sh). The bypass assertions below are
        # unaffected — the merge verify still bypasses the held slot and exits 0.
        REIFY_INFRA_SUITE_ACTIVE=1 DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" test --scope all
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
if [ "$_TS_READY" = "1" ]; then
    run_merge_while_task_slot_held
    assert "merge-role verify.sh test proceeds while task slot is held (exit 0, got ${MERGE_RC})" \
        test "$MERGE_RC" -eq 0
    # Mode-4 observability (task 5144): surface the nested run's captured
    # stderr when the exit-code assertion above fails.
    if [ "$MERGE_RC" -ne 0 ]; then
        _dump_captured_stderr "Section B merge-role run (rc=${MERGE_RC})" "$MERGE_ERR"
    fi
    # --- Section B structural assertion (S-technique): bypass marker in stderr ---
    # Proves the merge-exemption CODE PATH executed specifically (not just exit 0).
    # Fixed-string grep stops before the em-dash (U+2014) in the full message to avoid
    # locale/encoding fragility; the substring is unique to the bypass path.
    # RED when DF_VERIFY_ROLE=task (bypass marker absent → grep fails).
    assert "Section B structural: stderr contains merge-bypass marker (lib_test_semaphore.sh: bypass (role=merge))" \
        grep -qF 'lib_test_semaphore.sh: bypass (role=merge)' "$MERGE_ERR"
else
    assert "Section B SKIPPED: tree-sitter artifacts not ready — cannot run execute-mode e2e sections (see readiness diagnostic above)" \
        false
fi

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

    # External holder pins slot-1 for C_HOLD_S seconds (hold-until-killed: > load-scaled C_TIMEOUT)
    # so the acquire deadline ALWAYS fires before the holder self-releases.
    local _holder_pid _ready
    _ready="$_tmpdir/holder-ready"
    ( flock -x 9; touch "$_ready"; sleep "$C_HOLD_S" ) 9>>"${_lock}.slot-1" &
    _holder_pid=$!
    _wait_for_holder_ready "$_ready" "$(_load_scaled_deadline 30 180)"  # R-technique: causally guarantees holder holds flock -x

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
if [ "$_TS_READY" = "1" ]; then
    run_task_with_slot_held
    assert "verify.sh exits 75 (EX_TEMPFAIL) on acquisition deadline (got ${C_RC})" \
        test "$C_RC" -eq 75
    # Mode-4 observability (task 5144): surface the nested run's captured
    # stderr when the exit-code assertion above fails.
    if [ "$C_RC" -ne 75 ]; then
        _dump_captured_stderr "Section C task run with held slot (rc=${C_RC})" "$C_ERR"
    fi
    assert "stderr shows exit-75 propagated THROUGH verify.sh (verify.sh: FAILED (exit 75): ...)" \
        grep -qE 'verify\.sh: FAILED \(exit 75\): test-run semaphore acquire' "$C_ERR"
else
    assert "Section C SKIPPED: tree-sitter artifacts not ready — cannot run execute-mode e2e sections (see readiness diagnostic above)" \
        false
fi

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
#   (b) .config/nextest.toml pins occt max-threads=24; gen-nextest-config.sh is
#       exercised twice via HOST-INJECTED/DETERMINISTIC knobs (env -u
#       REIFY_OCCT_NEXTEST_MAX_THREADS -u REIFY_OCCT_NEXTEST_HARD_CAP -u
#       REIFY_OCCT_GIB_PER_THREAD so all derivation overrides are absent):
#       workstation-class NPROC=32/MEM=128 -> min(24,32,64)=24, and laptop-class
#       NPROC=16/MEM=32 -> min(24,16,16)=16. Does NOT read the real runner's nproc/RAM.
#   (c) Using `all --scope all --print-plan` (full plan), cargo clippy and
#       cargo check -p reify-gui appear BEFORE the ACQUIRE marker, and every
#       cargo nextest run line appears strictly BETWEEN the ACQUIRE and RELEASE
#       markers (re-verifies β gated-region oracle + γ cap as single gate).
# Intentionally re-consolidates β's gated-region oracle and γ's cap=24 here.
echo ""
echo "--- Section D: print-plan oracle (occt cap=24 + gated-region ordering) ---"

capture_plans

# HOST-INDEPENDENCE (task 5599). On a host WITHOUT cargo-nextest installed,
# verify.sh gracefully emits `cargo test` instead (plan header nextest=0), and
# the assertions below split into two kinds:
#   - --config-file presence is NEXTEST-ONLY (--config-file is a nextest flag;
#     the fallback line is `cargo test --workspace -- --test-threads=1 9<&-`
#     with no --config-file at all), so it is guarded by NEXTEST_AVAILABLE with
#     an else arm asserting the fallback shape;
#   - the gated-region ORDERING and the absence of --no-run are plan-SHAPE
#     facts that hold identically on the fallback (measured on the nx0
#     `all --scope all` plan: ACQUIRE 24 < cargo test 29 < RELEASE 30), so they
#     use the `cargo (test|nextest run)` alternation instead.
# The probe reads the header off the already-captured PLAN_TEST_FULL — no extra
# verify.sh invocation. That "no extra invocation" property is exactly why
# nextest_available_in_plan exists (tests/infra/nextest_absent_lib.sh, task
# 5644): verify.sh cannot emit the `nextest=` header without genuinely probing,
# and the worst case forks cargo up to 4x plus two sleeps. The idiom's home is
# now that lib rather than a sibling suite.
#
# The lib's extractor is `|| true`-guarded, which CONVERTS an aborting capture
# into a quiet "unavailable" rather than eliminating the failure. A wrong answer
# still fails loudly here, but for a reason that has to be preserved
# deliberately: the else arm below asserts the fallback shape POSITIVELY
# ("cargo-test fallback lines carry NO --config-file" requires `cargo test` to
# be present AND --config-file to be absent), so on a nextest-present host it
# fails on both counts. The ordering and no---no-run asserts are unguarded
# anyway — they use the `cargo (test|nextest run)` alternation and hold on both
# host shapes.
# Guarded by tests/infra/test_verify_nextest_absent_suites.sh.
NEXTEST_AVAILABLE=0
if nextest_available_in_plan "$PLAN_TEST_FULL"; then
    NEXTEST_AVAILABLE=1
fi

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "test plan: all nextest run lines carry --config-file with reify-nextest-occt path" \
        bash -c 'printf "%s\n" "$1" | grep -q "cargo nextest run" && ! printf "%s\n" "$1" | grep "cargo nextest run" | grep -v -- "--config-file.*reify-nextest-occt"' \
        _ "$PLAN_TEST_CMDS"
else
    assert "test plan, nextest unavailable: cargo-test fallback lines carry NO --config-file (nextest-only flag)" \
        bash -c 'printf "%s\n" "$1" | grep -q "cargo test" && ! printf "%s\n" "$1" | grep -q -- "--config-file"' \
        _ "$PLAN_TEST_CMDS"
fi
assert ".config/nextest.toml pins occt max-threads=24" \
    grep -qE 'occt = \{ max-threads = 24 \}' "$REPO_ROOT/.config/nextest.toml"
assert "gen-nextest-config.sh resolves occt cap to 24 (workstation-class injection NPROC=32/MEM=128: min(24,32,64)=24)" \
    bash -c '_p=$(REIFY_OCCT_NPROC=32 REIFY_OCCT_MEMTOTAL_GIB=128 env -u REIFY_OCCT_NEXTEST_MAX_THREADS -u REIFY_OCCT_NEXTEST_HARD_CAP -u REIFY_OCCT_GIB_PER_THREAD bash "$1/scripts/gen-nextest-config.sh"); rc=0; grep -qE "^occt = \{ max-threads = 24 \}" "$_p" || rc=1; rm -f "$_p"; exit $rc' \
    _ "$REPO_ROOT"
assert "gen-nextest-config.sh resolves occt cap to 16 (laptop-class injection NPROC=16/MEM=32: min(24,16,16)=16)" \
    bash -c '_p=$(REIFY_OCCT_NPROC=16 REIFY_OCCT_MEMTOTAL_GIB=32 env -u REIFY_OCCT_NEXTEST_MAX_THREADS -u REIFY_OCCT_NEXTEST_HARD_CAP -u REIFY_OCCT_GIB_PER_THREAD bash "$1/scripts/gen-nextest-config.sh"); rc=0; grep -qE "^occt = \{ max-threads = 16 \}" "$_p" || rc=1; rm -f "$_p"; exit $rc' \
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
assert "all plan: every test run line BETWEEN acquire and release markers (task 4862 revert: build inside slot)" \
    bash -c '
        ACQ=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*ACQUIRE" | head -1 | cut -d: -f1)
        REL=$(printf "%s\n" "$1" | grep -n "test-run semaphore.*RELEASE" | head -1 | cut -d: -f1)
        # All test passes are inside the slot; no --no-run filter needed (post-4862 revert).
        FIRST=$(printf "%s\n" "$1" | grep -nE "cargo (test|nextest run)" | head -1 | cut -d: -f1)
        LAST=$(printf "%s\n" "$1" | grep -nE "cargo (test|nextest run)" | tail -1 | cut -d: -f1)
        [ -n "$ACQ" ] && [ -n "$REL" ] && [ -n "$FIRST" ] && [ -n "$LAST" ]
        [ "$FIRST" -gt "$ACQ" ] && [ "$LAST" -lt "$REL" ]
    ' _ "$PLAN_ALL_FULL"
# Vacuity hardening (task 5599): the bare `cargo nextest run` form made this
# grep match nothing on a nextest-less host, so it passed while asserting
# nothing. No --no-run line exists in any plan post-4862 on either host.
assert "all plan: NO 'cargo (test|nextest run) ... --no-run' line before acquire marker (task 4862 revert: build inside slot)" \
    bash -c '! printf "%s\n" "$1" | grep -qE "cargo (test|nextest run).*--no-run"' _ "$PLAN_ALL_FULL"

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
    _wait_for_holder_ready "$_ready" "$(_load_scaled_deadline 30 180)"  # R-technique: causally guarantees holder holds flock -x

    # Launch verify.sh in the BACKGROUND so we can poll its stderr for clock
    # markers while the holder still holds the slot.  Anti-hang guard: load-scaled
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
        DF_VERIFY_ROLE=task timeout "$(_load_scaled_deadline 180 900)" bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) 2>"$F_ERR" & _run_pid=$!
    set +m  # restore default job control state

    # Causal handshake (R-technique): poll F_ERR for CLOCK_STOP then
    # CLOCK_HEARTBEAT.  CLOCK_STOP is emitted when the acquire first blocks
    # (proves verify.sh entered the contended wait while the holder holds);
    # CLOCK_HEARTBEAT proves >=1 heartbeat interval elapsed inside the wait.
    # Both must appear while the holder still holds the slot.  These marker
    # assertions are the strictly stronger, load-independent proof of a real wait
    # (no wall-clock discriminator needed).
    _wait_for_marker "$F_ERR" '@@REIFY_CLOCK_STOP@@' "$(_load_scaled_deadline 120 600)" \
        || { echo "  [F] ERROR: CLOCK_STOP not observed within the load-scaled marker-wait deadline; aborting" >&2
             kill -- -"$_run_pid" 2>/dev/null || kill "$_run_pid" 2>/dev/null || true
             kill "$_holder_pid" 2>/dev/null || true
             wait "$_run_pid" "$_holder_pid" 2>/dev/null || true
             rm -f "${_lock}.slot-1"; F_RC=99; return 1; }
    _wait_for_marker "$F_ERR" '@@REIFY_CLOCK_HEARTBEAT@@' "$(_load_scaled_deadline 120 600)" \
        || { echo "  [F] ERROR: CLOCK_HEARTBEAT not observed within the load-scaled marker-wait deadline; aborting" >&2
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
if [ "$_TS_READY" = "1" ]; then
    run_unlimited_wait_with_slot_held
    assert "F1: unlimited-wait verify.sh exits 0 when slot eventually freed (got ${F_RC})" \
        test "$F_RC" -eq 0
    # Mode-4 observability (task 5144): surface the nested run's captured
    # stderr when the exit-code assertion above fails.
    if [ "$F_RC" -ne 0 ]; then
        _dump_captured_stderr "Section F1 unlimited-wait run (rc=${F_RC})" "$F_ERR"
    fi
    assert_marker "F1: F_ERR captured the CLOCK_STOP marker (reason=test_slot_starvation)" \
        "$F_ERR" '@@REIFY_CLOCK_STOP@@ reason=test_slot_starvation'
    assert_marker "F1: F_ERR captured the CLOCK_HEARTBEAT marker" \
        "$F_ERR" '@@REIFY_CLOCK_HEARTBEAT@@'
    assert_marker "F1: F_ERR captured the CLOCK_START marker (reason=test_slot_starvation)" \
        "$F_ERR" '@@REIFY_CLOCK_START@@ reason=test_slot_starvation'
else
    assert "Section F1 SKIPPED: tree-sitter artifacts not ready — cannot run execute-mode e2e sections (see readiness diagnostic above)" \
        false
fi

# F2: print-plan ACQUIRE annotation must reference the clock-stop region.
# Captures the ACQUIRE # comment line from --print-plan and asserts it contains
# "REIFY_CLOCK", "clock-stop", or "dark_factory:1916".
#
# Mode-4 fix (task 5144, 5111 "silent FAIL"): the original bare
# `F2_PLAN="$(bash ... --print-plan 2>/dev/null)"` assignment had no `|| ...`
# fallback — unlike every other nested-run capture in this file (RC1/RC2,
# MERGE_RC, C_RC, F_RC all use the `|| _rc=$?` idiom). Under concurrent load a
# nested print-plan can transiently return non-zero; with no `||` to
# neutralize it, `set -e` aborted the WHOLE suite right there, and
# `2>/dev/null` erased the reason — a FAIL with no assert line and no
# evidence. _run_capturing (the same helper backing the step-7
# _dump_captured_stderr unit tests) makes the capture set-e-safe; on
# non-zero rc or an empty plan this now FAILs loudly via a single guarded
# assert and dumps the captured output via _dump_captured_stderr instead of
# dying silently.
#
# Stream split (amendment, reviewer finding): _run_capturing folds its
# wrapped command's stdout+stderr together (`>outfile 2>&1`), so piping
# verify.sh straight through it would fold any stderr noise into F2_PLAN too
# — a behavioral change from the original capture, which was stdout-only
# (`2>/dev/null` dropped stderr entirely). _f2_capture_print_plan below
# diverts verify.sh's OWN stderr to its own file (F2_ERR) via an inner
# redirect that takes effect BEFORE _run_capturing's outer `2>&1` ever sees
# it, so only the plan text reaches F2_OUT/F2_PLAN — matching the original
# stdout-only capture exactly — while F2_ERR (genuinely stderr-only, unlike
# the old merged file of the same name) is still there to dump on failure.
_f2_capture_print_plan() {
    bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan 2>"$1"
}

_F2_TMPDIR="$(mktemp -d)"
_TMPDIRS+=("$_F2_TMPDIR")
F2_OUT="$_F2_TMPDIR/f2_stdout.txt"
F2_ERR="$_F2_TMPDIR/f2_stderr.txt"
_F2_RC=0
_run_capturing "$F2_OUT" _f2_capture_print_plan "$F2_ERR" || _F2_RC=$?
F2_PLAN="$(cat "$F2_OUT" 2>/dev/null || true)"

if [ "$_F2_RC" -eq 0 ] && [ -n "$F2_PLAN" ]; then
    F2_ACQ_LINE="$(printf '%s\n' "$F2_PLAN" | grep 'test-run semaphore.*ACQUIRE' | head -1)"
    echo "  [F2] ACQUIRE annotation: ${F2_ACQ_LINE}" >&2
    assert "F2: ACQUIRE annotation is a # comment (not a bare timeout/exec command)" \
        bash -c 'printf "%s\n" "$1" | grep -q "^#"' _ "$F2_ACQ_LINE"
    assert "F2: ACQUIRE annotation references clock-stop region (REIFY_CLOCK / clock-stop / dark_factory:1916)" \
        bash -c 'printf "%s\n" "$1" | grep -qE "REIFY_CLOCK|clock-stop|dark_factory:1916"' _ "$F2_ACQ_LINE"
else
    assert "F2: print-plan capture succeeded (rc 0, non-empty plan) — got rc=${_F2_RC} (5111 silent-FAIL guard; see dumped output below)" \
        false
    _dump_captured_stderr "F2 print-plan stdout" "$F2_OUT"
    _dump_captured_stderr "F2 print-plan stderr" "$F2_ERR"
fi

# run_hermetic_compile_gate_capture
# Drives verify.sh compile-gate (execute-only entry) under apply_hermetic_env and
# captures stderr to E_ERR.  Sets E_RC (expected 0: gate disabled by hermetic env).
# Section E proves apply_hermetic_env exports REIFY_COMPILE_GATE_DISABLE=1, causing
# cpu-admit.sh to emit "verify.sh: compile-gate disabled" to stderr and return 0.
# Lighter than the full test --scope all path: no make_stub_bin (compile-gate
# dispatched at verify.sh:449-452 BEFORE cargo/npm/tree-sitter pipeline) — near-instant.
run_hermetic_compile_gate_capture() {
    local _tmpdir _stubdir _lock
    _tmpdir="$(mktemp -d)"
    _TMPDIRS+=("$_tmpdir")
    _stubdir="$_tmpdir/stubs"
    _lock="$_tmpdir/sem.lock"
    mkdir -p "$_stubdir"

    E_ERR="$_tmpdir/e_err.txt"
    touch "$E_ERR"

    E_RC=0
    (
        apply_hermetic_env "$_stubdir" "$_lock"
        DF_VERIFY_ROLE=task timeout "$(_load_scaled_deadline 30 120)" bash "$REPO_ROOT/scripts/verify.sh" compile-gate
    ) 2>"$E_ERR" || E_RC=$?
}

# ===========================================================================
# Section E: compile-gate neutralized in hermetic env (load-robustness root-cause guard)
# ===========================================================================
# S-technique structural proof: apply_hermetic_env must export
# REIFY_COMPILE_GATE_DISABLE=1, causing cpu-admit.sh to emit the fixed marker
# "verify.sh: compile-gate disabled" to stderr and exit 0.  This is the
# load-independent proof that the task-4853 compile-gate is neutralized in
# every hermetic execute section — the root-cause guard against esc-4288-206.
# Uses `verify.sh compile-gate` (execute-only entry, dispatched before the
# cargo/npm/tree-sitter pipeline) so the run is near-instant; make_stub_bin
# overhead is gone.  Removing apply_hermetic_env's DISABLE export → RED (both
# marker and exit-0 assertions fail).
echo ""
echo "--- Section E: compile-gate neutralized in hermetic env (load-robustness root-cause guard) ---"

E_RC=0
E_ERR=""
run_hermetic_compile_gate_capture
assert "Section E structural: stderr contains compile-gate disabled marker (verify.sh: compile-gate disabled)" \
    grep -qF 'verify.sh: compile-gate disabled' "$E_ERR"
assert "Section E: verify.sh compile-gate exits 0 (execute-only entry, gate disabled)" \
    test "$E_RC" -eq 0

# run_compile_gate_psi_capture <psi_proc_path> <mem_proc_path> <timeout_secs> [err_path]
# Drives `verify.sh compile-gate` (execute-only entry, dispatched before the
# cargo/npm/tree-sitter pipeline — no make_stub_bin needed) under an injected
# PSI fixture.  Unlike run_hermetic_compile_gate_capture, this does NOT use
# apply_hermetic_env (which unconditionally sets REIFY_COMPILE_GATE_DISABLE=1)
# — this harness exercises the REAL gate.  Captures stderr to G_ERR; G_RC is
# the `timeout`-wrapped exit code (124 when still holding at the bound).
# Mirrors run_hermetic_compile_gate_capture's shape (task 4920: proves the
# admit-mode hold + clock-stop markers on the verify.sh compile-gate path).
# [err_path] (optional, task 5839): capture the child's stderr to exactly this
# path and set G_ERR to it, instead of to a private mktemp path.
run_compile_gate_psi_capture() {
    local _psi="$1" _mem="$2" _bound="$3" _err="${4:-}"
    local _tmpdir
    _tmpdir="$(mktemp -d)"
    _TMPDIRS+=("$_tmpdir")

    # Optional explicit capture path (task 5839): Section G2's causal updater
    # must poll the SAME file the gate writes, and it forks BEFORE this
    # function runs — at which point $G_ERR still holds the PREVIOUS section's
    # capture file, which legitimately contains its own HEARTBEAT marker.
    # Letting the caller pre-create and pass the path removes that stale-marker
    # hazard.  Omitted → the original private mktemp path (G1's call site is
    # unchanged).
    if [ -n "$_err" ]; then
        G_ERR="$_err"
    else
        G_ERR="$_tmpdir/g_err.txt"
    fi
    # Idempotent on an already-created file; preserves the "capture file always
    # exists before the child starts" guarantee _wait_for_marker's
    # `grep … 2>/dev/null` relies on.
    touch "$G_ERR"

    G_RC=0
    (
        export REIFY_COMPILE_GATE_PROC_PATH="$_psi"
        export REIFY_COMPILE_GATE_MEM_PROC_PATH="$_mem"
        export REIFY_COMPILE_GATE_MAX_WAIT=2
        export REIFY_COMPILE_GATE_POLL=1
        export REIFY_CLOCK_HEARTBEAT_SECS=1
        timeout "$_bound" bash "$REPO_ROOT/scripts/verify.sh" compile-gate
    ) 2>"$G_ERR" || G_RC=$?
}

# run_g2_admit_on_drop_scenario <startup_delay_s> <mem_proc_path> <bound>
# The G2 "admit-on-drop" arrangement, made causal (task 5839).  Seeds a stuck
# avg10=99 PSI fixture, spawns an updater that clears it to avg10=10 ONLY once
# the gate has proven it observed pressure, optionally delays the gate's start
# by <startup_delay_s> to simulate load-induced startup latency, runs the gate,
# and reaps the updater.  Sets G_RC / G_ERR.  The updater's own deadline is
# <bound> + <startup_delay_s>, so it is guaranteed to outlive the gate's
# `timeout` bound however large the simulated latency gets.
#
# The old form used a FIXED `sleep 2` before clearing.  That raced a load-scaled
# deadline: under heavy load the gate's bash + verify.sh-preamble startup
# exceeded 2s, so its FIRST poll read an ALREADY-cleared fixture and
# cpu-admit.sh admitted immediately with _ca_waited=0 — rc=0 and NONE of
# @@REIFY_CLOCK_{STOP,HEARTBEAT,START}@@ (Section G0's T5a/T5b root-cause guard
# pins that behaviour directly).  Observed at task/5644 HEAD=3ae7db94f0, load
# avg 242.36 on 32 cores: 62/65, with exactly the three G2 marker asserts red
# and the neighbouring rc==0 assert green.
#
# Scaling the sleep with load_tolerance_factor would only WIDEN the window — the
# same whack-a-mole load_tolerance_lib.sh's own header exists to end.  The delay
# is a wall-clock guess about ANOTHER process's startup latency, so it can
# always be lost on a hot enough box; only a causal wait removes the race.
#
# Synchronising on @@REIFY_CLOCK_HEARTBEAT@@ (not STOP) is deliberate:
# lib_clock_stop.sh emits STOP from clock_enter_wait and only then stamps
# last_hb, so a HEARTBEAT can only ever follow a STOP on the same stream —
# observing it banks BOTH markers before the fixture is touched, leaving only
# START causally dependent on the clear.
run_g2_admit_on_drop_scenario() {
    local _startup_delay="$1" _mem="$2" _bound="$3"
    # Normalise the delay once: it feeds BOTH the updater's deadline arithmetic
    # and the sleep below, and a non-numeric value must degrade to "no delay"
    # rather than abort the suite inside $(( … )) under `set -e`.
    local _delay="${_startup_delay:-0}"
    case "$_delay" in ''|*[!0-9]*) _delay=0 ;; esac
    local _tmpdir
    _tmpdir="$(mktemp -d)"
    _TMPDIRS+=("$_tmpdir")
    local _psi="$_tmpdir/psi" _err="$_tmpdir/g2_err.txt"
    printf 'some avg10=99.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        > "$_psi"
    # Pre-create so the updater's poll and the gate's capture are provably the
    # SAME file from the start — $G_ERR still holds the PREVIOUS section's
    # capture file at this point, and that file legitimately contains its own
    # HEARTBEAT marker (G1 asserts exactly that), so an updater polling $G_ERR
    # would match a stale marker and silently reinstate the race being fixed.
    : > "$_err"

    # The updater's deadline is the gate's bound PLUS the simulated startup
    # latency, because the updater's clock starts here while the gate's `timeout`
    # only starts after the sleep below.  Sharing one bound would silently shrink
    # the updater's headroom as _delay grows, and a caller passing a delay near
    # the bound would get an updater that gave up while the gate was still
    # holding — surfacing as an opaque rc=124 instead of the helper's own
    # diagnostic.  Offsetting makes the updater outlive the gate's bound by
    # construction, at ANY delay, so the invariant is enforced rather than
    # documented as a _delay << _bound precondition.
    local _updater_deadline=$(( _bound + _delay ))
    _clear_psi_fixture_on_marker "$_err" '@@REIFY_CLOCK_HEARTBEAT@@' \
        "$_updater_deadline" "$_psi" 10.00 &
    local _updater=$!

    # Simulated gate-startup latency (0 in production use).  It cannot affect
    # correctness: the updater cannot clear before the gate emits a heartbeat,
    # and the gate cannot emit one before it starts — so the arrangement holds
    # at ANY startup latency, which is the whole point of the fix.  Written as
    # an explicit `if` rather than `[ … ] && sleep …` so a zero delay can never
    # make this the function's non-zero-returning last statement under `set -e`.
    if [ "$_delay" -gt 0 ]; then
        sleep "$_delay"
    fi

    run_compile_gate_psi_capture "$_psi" "$_mem" "$_bound" "$_err"

    kill "$_updater" 2>/dev/null || true
    wait "$_updater" 2>/dev/null || true
}

# ===========================================================================
# Section G0: G2 harness/updater contracts (task 5839)
# ===========================================================================
# Pins the harness contracts Section G2's causal updater depends on (T3a-T3c),
# the root cause that forces the updater to be causal at all (T5a/T5b), and a
# late-start regression witness for the fix itself (T5c-T5f).  The whole section
# costs TWO gate spawns plus one real hold: T3a/T3b and T3c each drive an
# ALREADY-QUIET PSI fixture and admit on the first poll (~0.3s apiece), T5a/T5b
# re-read the T3a/T3b run rather than repeating it, and only the T5c-T5f witness
# holds — bounded by the same load-scaled deadline Section G2 uses.
#
# Why the explicit-capture-path contract is a CORRECTNESS requirement, not
# ergonomics: run_compile_gate_psi_capture assigns G_ERR INSIDE the call, but
# G2 must fork its updater BEFORE that call — at which point $G_ERR still holds
# the PREVIOUS section's capture file, which legitimately contains its own
# HEARTBEAT marker (G1 asserts exactly that).  An updater polling $G_ERR would
# match G1's stale marker, fire immediately, and silently reinstate the very
# race being fixed — a fix that LOOKS causal but is not.  Pre-creating the path
# and passing it in makes the updater and the gate provably agree on one file.
echo ""
echo "--- Section G0: G2 harness/updater contracts (task 5839) ---"

_G0_TMPDIR="$(mktemp -d)"
_TMPDIRS+=("$_G0_TMPDIR")
_G0_MEM_QUIET="$_G0_TMPDIR/mem-quiet"
printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
    > "$_G0_MEM_QUIET"
# Below the gate's threshold (85), so the gate admits on its FIRST poll.
_G0_PSI_QUIET="$_G0_TMPDIR/psi-quiet"
printf 'some avg10=10.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
    > "$_G0_PSI_QUIET"

# --- T3a/T3b: the optional 4th [err_path] argument is honoured ---
_G0_EXPLICIT="$_G0_TMPDIR/g0_explicit_err.txt"
: > "$_G0_EXPLICIT"

G_RC=0
G_ERR=""
run_compile_gate_psi_capture "$_G0_PSI_QUIET" "$_G0_MEM_QUIET" \
    "$(_load_scaled_deadline 30 120)" "$_G0_EXPLICIT"
# Saved before T3c overwrites G_RC — T5a's root-cause guard reads THIS run's
# exit code rather than paying for a third identical spawn (see T5a/T5b).
_G0_EXPLICIT_RC="$G_RC"

assert "T3a: explicit [err_path] is honoured (G_ERR is the caller's pre-created path, not a private mktemp)" \
    test "$G_ERR" = "$_G0_EXPLICIT"
assert "T3b: quiet fixture admits on the first poll (exit 0; got ${G_RC})" \
    test "$G_RC" -eq 0
assert "T3b: the caller's explicit capture file exists after the run" \
    test -f "$_G0_EXPLICIT"

# --- T3c: 3-arg back-compat — G1's untouched call site keeps a PRIVATE file ---
G_RC=0
G_ERR=""
run_compile_gate_psi_capture "$_G0_PSI_QUIET" "$_G0_MEM_QUIET" \
    "$(_load_scaled_deadline 30 120)"

assert "T3c: 3-arg form still assigns a capture path (G_ERR non-empty)" \
    test -n "$G_ERR"
assert "T3c: 3-arg form's capture file exists" \
    test -f "$G_ERR"
assert "T3c: 3-arg form's capture file is PRIVATE (not the explicit path from T3a)" \
    test "$G_ERR" != "$_G0_EXPLICIT"

# --- T5a/T5b: ROOT-CAUSE GUARD — the exact 5839 failure signature ---
# The load-independent proof of WHY the fix must be causal rather than a longer
# sleep, shaped like Section E's own "load-robustness root-cause guard" below.
# A gate whose FIRST poll reads a below-threshold fixture takes cpu-admit.sh's
# uncontended fast path with _ca_waited=0: clock_enter_wait (the sole STOP
# emitter) and clock_maybe_heartbeat are never reached, and clock_exit_wait
# emits START only when WAITED=1 — so it exits 0 having emitted NO clock marker
# at all.  The architect measured exactly this at base HEAD=c89bac9267 under
# G2's env (MAX_WAIT=2, POLL=1, CLOCK_HEARTBEAT_SECS=1): rc=0, zero markers,
# empty stderr.  That is precisely what a wall-clock updater which WINS the
# race hands to Section G2 — the rc==0 assert green, all three marker asserts
# red — i.e. the whole bug, in one pair of asserts.
#
# NO EXTRA SPAWN: this guard reads the T3a/T3b run — byte-identical fixtures,
# env and bound — rather than starting a third `verify.sh compile-gate` child.
# The rc comes from _G0_EXPLICIT_RC (saved before T3c clobbered G_RC) and the
# stderr from $_G0_EXPLICIT, which already holds exactly the text T5b greps.
# The load-independence argument is unchanged: cpu_admit reads only the injected
# PSI/mem fixtures, never a loadavg axis, so the observation is the same whoever
# made it.  This suite is re-run wholesale by
# test_verify_nextest_absent_suites.sh's S4, so a redundant spawn is paid twice.
assert "T5a: G0 root-cause: an already-cleared PSI fixture admits on the first poll (exit 0; got ${_G0_EXPLICIT_RC}) — same run as T3b" \
    test "$_G0_EXPLICIT_RC" -eq 0
assert "T5b: G0 root-cause: that admit emits NO clock marker at all (the 5839 failure signature)" \
    bash -c '! grep -qF "@@REIFY_CLOCK_" "$1"' _ "$_G0_EXPLICIT"

# --- T5c-T5f: LATE-START REGRESSION WITNESS ---
# Drives the G2 arrangement with the gate's start deliberately delayed, so the
# updater has every chance to clear the fixture first.  The delay is NOT a tuned
# threshold: it is strictly greater than the 2s the old fixed-sleep updater
# waited, and per T5b ANY gate start after that clear deterministically yields
# rc=0 with zero markers.  This scenario therefore fails 3 of its 4 asserts
# against the old arrangement and passes against the causal one — a real
# regression witness, not a timing coincidence.  Being a simulated startup
# latency rather than an anti-hang deadline it stays FIXED and un-load-scaled,
# matching this file's HOLD_S / C_HOLD_S convention (header lines 55-63).
_G0_LATE_START_S=4

G_RC=0
G_ERR=""
# `|| G_RC=$?` covers BOTH states: with run_g2_admit_on_drop_scenario still
# undefined, bash's command-not-found 127 lands in G_RC — so T5c is genuinely
# RED rather than spuriously green on the pre-reset 0 — and `set -e` is
# neutralised so T5d-T5f still run instead of aborting the suite.  Once the
# helper exists it returns 0 and G_RC keeps the value the harness assigned.
run_g2_admit_on_drop_scenario "$_G0_LATE_START_S" "$_G0_MEM_QUIET" \
    "$(_load_scaled_deadline 30 120)" || G_RC=$?

assert "T5c: G0-late: admits once PSI drops despite a ${_G0_LATE_START_S}s-delayed gate start (exit 0; got ${G_RC})" \
    test "$G_RC" -eq 0
assert_marker "T5d: G0-late: G_ERR captured the CLOCK_STOP marker (reason=psi_pressure)" \
    "$G_ERR" '@@REIFY_CLOCK_STOP@@ reason=psi_pressure'
assert_marker "T5e: G0-late: G_ERR captured the CLOCK_HEARTBEAT marker" \
    "$G_ERR" '@@REIFY_CLOCK_HEARTBEAT@@'
assert_marker "T5f: G0-late: G_ERR captured the CLOCK_START marker (reason=psi_pressure)" \
    "$G_ERR" '@@REIFY_CLOCK_START@@ reason=psi_pressure'

# ===========================================================================
# Section G: compile-gate PSI hold + admit-on-drop (execution; task 4920)
# ===========================================================================
# Section E proves compile-gate is DISABLED in the hermetic harness (via
# apply_hermetic_env's REIFY_COMPILE_GATE_DISABLE=1). This section instead
# drives the REAL gate under an injected PSI fixture to prove admit mode is
# now IN clock-stop scope (task 4920): compile_gate() sets
# _ca_clock_reason=psi_pressure and no longer admits-on-timeout — it HOLDS
# until PSI drops, emitting @@REIFY_CLOCK_{STOP,HEARTBEAT,START}@@.
# G1 (hold): stuck avg10=99 fixture, `timeout` bound → rc==124 (killed while
#     still holding, well past the old MAX_WAIT=2s), stderr has STOP+HEARTBEAT,
#     never an admit/fairness-floor message (removed) nor START.
# G2 (admit-on-drop): same stuck start, a background updater clears the
#     fixture to a low avg10 once the gate's FIRST heartbeat proves it observed
#     the pressure (task 5839 — causal, never on wall-clock) → exit 0,
#     STOP+HEARTBEAT+START all present (balanced), no fairness message.
# RED today: compile_gate() sets no _ca_clock_reason → admits-on-timeout at
# MAX_WAIT=2s → the bounded `timeout` never fires, no STOP marker at all.
# ===========================================================================
echo ""
echo "--- Section G: compile-gate PSI hold + admit-on-drop (execution) ---"

_G_TMPDIR="$(mktemp -d)"
_TMPDIRS+=("$_G_TMPDIR")
G_MEM_QUIET="$_G_TMPDIR/mem-quiet"
printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
    > "$G_MEM_QUIET"

# G1: the hold — fixture stays stuck at avg10=99 for the whole bound.
G_PSI_1="$_G_TMPDIR/psi-g1"
printf 'some avg10=99.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
    > "$G_PSI_1"

G_RC=0
G_ERR=""
run_compile_gate_psi_capture "$G_PSI_1" "$G_MEM_QUIET" "$(_load_scaled_deadline 8 30)"

assert "G1: avg10=99 sustained → timeout kills it (rc==124; HELD past old MAX_WAIT=2s)" \
    test "$G_RC" -eq 124
assert_marker "G1: G_ERR captured the CLOCK_STOP marker (reason=psi_pressure)" \
    "$G_ERR" '@@REIFY_CLOCK_STOP@@ reason=psi_pressure'
assert_marker "G1: G_ERR captured the CLOCK_HEARTBEAT marker" \
    "$G_ERR" '@@REIFY_CLOCK_HEARTBEAT@@'
assert "G1: stderr does NOT match admit/fairness-floor message (removed — never admits-on-timeout)" \
    bash -c '! grep -qiE "admit|fairness|proceeding under load|sustained pressure" "$1"' _ "$G_ERR"
assert "G1: stderr does NOT contain the CLOCK_START marker (never admitted)" \
    bash -c '! grep -qF "@@REIFY_CLOCK_START@@" "$1"' _ "$G_ERR"

# G2: same stuck-fixture start; the updater clears it to avg10=10 only once the
# gate has PROVEN it observed pressure (task 5839 — the old fixed `sleep 2`
# raced the load-scaled deadline).  The hold therefore admits on the PSI drop,
# not on a timeout, at any gate-startup latency.
G_RC=0
G_ERR=""
run_g2_admit_on_drop_scenario 0 "$G_MEM_QUIET" "$(_load_scaled_deadline 30 120)"

assert "G2: admits once PSI drops (exit 0; got ${G_RC})" \
    test "$G_RC" -eq 0
assert_marker "G2: G_ERR captured the CLOCK_STOP marker (reason=psi_pressure)" \
    "$G_ERR" '@@REIFY_CLOCK_STOP@@ reason=psi_pressure'
assert_marker "G2: G_ERR captured the CLOCK_HEARTBEAT marker" \
    "$G_ERR" '@@REIFY_CLOCK_HEARTBEAT@@'
assert_marker "G2: G_ERR captured the CLOCK_START marker (reason=psi_pressure)" \
    "$G_ERR" '@@REIFY_CLOCK_START@@ reason=psi_pressure'
assert "G2: stderr does NOT match fairness-floor admit-on-timeout message" \
    bash -c '! grep -qiE "admit|fairness|proceeding under load|sustained pressure" "$1"' _ "$G_ERR"

# run_background_verify_check_no_bypass
# Runs a single DF_VERIFY_ROLE=background verify.sh test with the slot FREE
# (no external holder — proving the gated path ran needs no contention, just
# an event-log record) and REIFY_SLOT_EVENT_LOG set. Sets H_RC/H_ERR/H_EVENTLOG.
# Mirrors run_merge_while_task_slot_held's hermetic-harness idiom (stub dir,
# apply_hermetic_env inside a subshell so exports don't leak to the outer
# shell), but asserts the OPPOSITE outcome: background must actually acquire
# the slot, not bypass it.
# REIFY_INFRA_SUITE_ACTIVE=1 (task 5210, mirroring task 5125's Section-B
# idiom): step-8 of this task makes background run the wholesale run_all.sh
# infra pool (like merge), so a nested background verify inside THIS infra
# suite would otherwise recurse (run_all -> this test -> background verify ->
# run_all -> ...). Scoped narrowly to this one spawn, not broadcast, for the
# same ambient-isolation reason documented at the run_all.sh pool guard.
run_background_verify_check_no_bypass() {
    local _tmpdir _stubdir _lock
    _tmpdir="$(mktemp -d)"
    _TMPDIRS+=("$_tmpdir")
    _stubdir="$_tmpdir/stubs"
    _lock="$_tmpdir/sem.lock"
    mkdir -p "$_stubdir"
    # Stub cargo with SLEEP=0: free-slot run should be fast (instant nextest pass).
    REIFY_E2E_CARGO_SLEEP=0 make_stub_bin "$_stubdir"

    local _eventlog
    _eventlog="$_tmpdir/events.log"
    : > "$_eventlog"
    H_EVENTLOG="$_eventlog"

    H_ERR="$_tmpdir/background_err.txt"
    touch "$H_ERR"

    H_RC=0
    (
        apply_hermetic_env "$_stubdir" "$_lock" 30
        export REIFY_SLOT_EVENT_LOG="$_eventlog"
        REIFY_INFRA_SUITE_ACTIVE=1 DF_VERIFY_ROLE=background bash "$REPO_ROOT/scripts/verify.sh" test --scope all
    ) 2>"$H_ERR" || H_RC=$?
}

# ===========================================================================
# Section H: background non-exemption (execute mode; task 5210)
# ===========================================================================
# Contrasts Section B's merge exemption: DF_VERIFY_ROLE=background must NOT
# bypass test_semaphore_acquire — lib_test_semaphore.sh:91 is a strict
# `= "merge"` check, so background falls through to the real acquire path
# unchanged. Proven two ways: the event log (R-technique, load-independent)
# records a real ACQUIRE, and stderr (S-technique) LACKS the merge-bypass
# marker. Unlike Section B, no external holder is needed — a free-slot
# acquire is sufficient proof the gated code path executed.
echo ""
echo "--- Section H: background non-exemption (execute mode, task 5210) ---"

H_RC=0
H_ERR=""
H_EVENTLOG=""
if [ "$_TS_READY" = "1" ]; then
    run_background_verify_check_no_bypass
    assert "background-role verify.sh test succeeds with a free slot (exit 0, got ${H_RC})" \
        test "$H_RC" -eq 0
    # Mode-4 observability (task 5144 idiom): surface captured stderr when the
    # exit-code assertion above fails.
    if [ "$H_RC" -ne 0 ]; then
        _dump_captured_stderr "Section H background-role run (rc=${H_RC})" "$H_ERR"
    fi
    # --- Section H causal assertion (R-technique): background actually ACQUIRES the slot ---
    H_ACQ_COUNT=$(awk '$3 == "ACQUIRE"' "$H_EVENTLOG" | wc -l | tr -d ' ')
    assert "Section H causal: background recorded an ACQUIRE event (slot actually taken, count=${H_ACQ_COUNT})" \
        test "$H_ACQ_COUNT" -ge 1
    # --- Section H structural assertion (S-technique): merge-bypass marker ABSENT ---
    assert "Section H structural: stderr LACKS the merge-bypass marker (background is non-exempt, contrast Section B)" \
        bash -c '! grep -qF "lib_test_semaphore.sh: bypass (role=merge)" "$1"' _ "$H_ERR"
else
    assert "Section H SKIPPED: tree-sitter artifacts not ready — cannot run execute-mode e2e sections (see readiness diagnostic above)" \
        false
fi

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
