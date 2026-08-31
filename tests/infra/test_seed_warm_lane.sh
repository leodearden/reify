#!/usr/bin/env bash
# tests/infra/test_seed_warm_lane.sh
# Hermetic tests for scripts/seed-warm-lane.sh.
#
# PATH-stubs: cp/find/touch/git (record argv to CALLS_FILE).
# Env-driven stub behaviour:
#   REIFY_TEST_REFLINK_OK    — cp stub: "1" → exit 0; else print error + exit 1
#   REIFY_TEST_GIT_DIFF_FILES — git stub: emitted as output of diff --name-only
#   REIFY_TEST_GIT_HEAD      — git stub: emitted as output of rev-parse HEAD
#   REIFY_TEST_TRASH_GLOB_LEGACY/_SIBLING — (task #5633) selective rm stubs'
#     trash-path match patterns; sourced once from _TRASH_GLOB_LEGACY/
#     _TRASH_GLOB_SIBLING and threaded into run_helper_real's PIN/SLEEP
#     stubs and Block T's T4 stub via this env var pair (each stub also
#     carries a literal default equal to the current value, so a caller that
#     does not thread it is unaffected).
#
# run_helper captures STDOUT and STDERR SEPARATELY:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#   REAL_STUB_DIR — (run_helper_real only, task #5633) this invocation's
#                    minted PATH stub dir; survives the call (reclaimed by
#                    the suite's EXIT trap, see _REAL_STUB_ROOT) rather than
#                    being unlinked eagerly, so a detached grandchild of the
#                    script under test can still resolve stubs via PATH.
#
# Blocks:
#   A — CLI guard (step-1 / step-2)
#   B — RUSTFLAGS guard / B5 (step-3 / step-4)
#   C — reflink clone + fail-closed / S2 (step-5 / step-6)
#   D — fresh-checkout mtime / D5 (step-7 / step-8)
#   E — reset-in-place / no bulk stamp (step-9 / step-10)
#   F — invocation fingerprint guard / S1 (step-11 / step-12)
#   G — --record-base writer (step-13 / step-14)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/seed-warm-lane.sh hermetic tests (task 4660) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
_BGPIDS=()

# _SHARED_TRASH_DIR / _note_shared_trash_use / _assert_no_shared_trash_use:
# the shared-trash runtime detector for task 5590's invariant — no seed
# invocation may write into the machine-shared /tmp/.reseed-trash. PROMOTED
# into tests/infra/test_helpers.sh (task 5612), which this file sources above;
# the full rationale — why the trailing `return 0` is mandatory, why the state
# is an append-only file rather than a bash array, why the case pattern quotes
# the variable, and why _SHARED_TRASH_DIR must stay overridable for R2's
# positive control — lives with the code there. Block R's R7 pins that the
# ACTIVE definitions still come from the library, so a reintroduced local copy
# cannot silently shadow them.
#
# _note_real_lane: structural companion to the R1 behavioural detector —
# logs EVERY run_helper_real lane arg ($2) to _REAL_LANES_FILE (defined below
# near _LANE_ROOT), regardless of whether THIS run actually triggered a
# rename-to-trash (defence-in-depth for lanes that don't reach that path
# today but could after a future fixture tweak, and the source of R4's
# end-to-end coverage signal). Every run_helper_real call site passes
# <base_dir> <lane_dir> [flags...] positionally, so $2 is always the lane.
# The bare-/tmp filtering itself is deliberately NOT done here — it moves into
# the _assert_no_bare_tmp_lanes checker in Block R, which runs in the main
# shell (via `assert`'s no-subshell "$@" invocation) and so can safely
# aggregate into a local array. _note_helper_lane below is the analogous
# recorder for plain run_helper (task 5609): unlike run_helper_real, whose
# every call site passes <base> <lane> positionally with no exception,
# run_helper has 4 flag-first call sites with no lane arg at all (--help,
# --unknown-flag-xyz, --fresh-checkout, --record-base "$G_BASE" — note $2
# there is a base, not a lane), so it cannot log $2 unconditionally the way
# _note_real_lane does. It instead applies a filter keyed on the shape of
# $1/$2 (see _note_helper_lane's own comment below for the full audit) rather
# than an enumerated allowlist of exempt sites, so it keeps classifying
# correctly as call sites are added or removed. I_SC_BASE_PARENT (Block
# I11-I12's self-clobber fixture, the one exception task 5590 deferred this
# wiring over) is NO LONGER a false-positive risk: task 5609 migrated it off
# bare /tmp onto make_isolated_lane, which is what made this wiring safe.
# As with _note_shared_trash_use, the trailing `return 0` is mandatory (bare
# unguarded statement), and the state is file-backed for the same
# subshell-visibility reason documented above.
_note_real_lane() {
    printf '%s\n' "${2:-}" >> "$_REAL_LANES_FILE"
    return 0
}

# _note_helper_lane: analogous to _note_real_lane above, but for plain
# run_helper (task 5609), whose call sites are NOT uniformly
# <base> <lane> [flags...] — a positional-shape filter is required to
# identify the lane arg reliably instead of logging $2 unconditionally.
#
# Audit (task 5609, exhaustive over every run_helper call site in this
# file): exactly 4 are flag-first with no lane arg at all — --help,
# --unknown-flag-xyz, --fresh-checkout, --record-base "$G_BASE" (there the
# only non-flag arg is $G_BASE, a base, not a lane) — and every other call
# site passes <base_dir> <lane_dir> [flags...] positionally, sometimes with
# a further non-flag positional AFTER the lane too (e.g. --base-commit shaX).
# Cited here by flag text rather than line number, which does not rot as
# lines are inserted above (an amendment to this same task had to repair a
# set of numeric citations that had already gone stale mid-task).
#
# The filter walks every arg of the call, collects the ones that do NOT
# start with "-" (in order), and — only if at least two were collected —
# records the SECOND one as the lane. This is shape-based over the WHOLE
# call, not just $1/$2: unlike an earlier version keyed only on whether $1
# itself looked like a flag, this also classifies correctly for a
# flag-FIRST call that still carries a lane further along, e.g. a
# hypothetical `run_helper --lane-lock "$BASE" "$LANE"` (--lane-lock and
# --assume-lane-lock-held are real scripts/seed-warm-lane.sh flags, not
# currently used at any run_helper call site in this file, but the script's
# own CLI already makes that shape plausible for a future call site). Verified
# against all 44 current call sites plus both hypothetical flag-first-with-lane
# shapes: --record-base "$G_BASE" contributes exactly ONE non-flag arg
# ($G_BASE itself), so it never reaches the two-collected threshold and needs
# no separate exclusion. Being shape-based rather than an enumerated
# allowlist, it keeps classifying correctly as call sites are added or
# removed, unlike a hardcoded list which would silently rot.
#
# As with _note_shared_trash_use/_note_real_lane, the trailing `return 0` is
# mandatory: this runs as a bare unguarded statement at the top of
# run_helper, and a nonzero return here would abort the whole suite under
# set -euo pipefail. State is file-backed (_HELPER_LANES_FILE, defined below
# near _LANE_ROOT) for the same subshell-visibility reason documented above.
_note_helper_lane() {
    local a=() x
    for x in "$@"; do
        case "$x" in
            -*) ;;
            *) a+=("$x") ;;
        esac
    done
    if [ "${#a[@]}" -ge 2 ]; then
        printf '%s\n' "${a[1]}" >> "$_HELPER_LANES_FILE"
    fi
    return 0
}

cleanup() {
    for pid in "${_BGPIDS[@]+${_BGPIDS[@]}}"; do
        kill "$pid" 2>/dev/null || true
    done
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# Mint $_LANE_ROOT — the single per-run grandparent for every lane fixture this
# file creates via make_isolated_lane — plus $_TRASH_HITS_FILE and the
# shared-trash snapshot. Both the root and the helpers were promoted into
# tests/infra/test_helpers.sh (task 5612); the rationale for the per-run root,
# and for why make_isolated_lane must not register anything itself (its body
# runs in a command-substitution subshell, where an array append is discarded),
# lives with the code there.
#
# Sited HERE, immediately after `trap cleanup EXIT`: init_isolated_lane_root
# appends its root to _TMPDIRS, so it must run after this file's `_TMPDIRS=()`
# — a call placed before it would register into an array that assignment then
# wipes, leaking the root. The helper refuses to run if _TMPDIRS is undeclared,
# so that ordering mistake is an error rather than a silent leak.
#
# The stem is what makes any litter this suite does produce attributable to it:
# seed names a trash entry "<lane-basename>.<pid>", and every lane minted under
# this root carries the stem.
init_isolated_lane_root test-seed

STUB_DIR="$(mktemp -d /tmp/test-seed-warm-lane-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

# _REAL_LANES_FILE / _HELPER_LANES_FILE: append-only detector state for
# _note_real_lane / _note_helper_lane above. These stay LOCAL — unlike
# $_TRASH_HITS_FILE (promoted, and minted by init_isolated_lane_root), they are
# fed by this file's own run_helper/run_helper_real wrappers, machinery no
# sibling suite has. Nested directly under $_LANE_ROOT (a sibling of each lane's
# own private parent, never inside one), so the existing cleanup() EXIT trap's
# `rm -rf "$_LANE_ROOT"` reclaims them with no new _TMPDIRS entry and no extra
# top-level /tmp entry, and R0c's "parent contains the lane and nothing else"
# check (which inspects a lane's own private parent, not _LANE_ROOT itself) is
# unaffected. _HELPER_LANES_FILE is kept SEPARATE from _REAL_LANES_FILE (task
# 5609) rather than merged into it: R3's offender message names the call
# convention ("run_helper_real lane dir was bare /tmp"), and R4 asserts specific
# run_helper_real lanes are present in _REAL_LANES_FILE as an H5d/H9
# subshell-visibility coverage signal — merging the two streams would make R3's
# message wrong for half its inputs and let a run_helper lane silently satisfy
# R4's coverage check.
_REAL_LANES_FILE="$_LANE_ROOT/.real-lanes"
_HELPER_LANES_FILE="$_LANE_ROOT/.helper-lanes"
: > "$_REAL_LANES_FILE"
: > "$_HELPER_LANES_FILE"

# _REAL_STUB_ROOT: a single per-run parent for every per-invocation PATH stub
# dir run_helper_real mints (task #5633). WHY a per-run root registered here,
# rather than `_TMPDIRS+=("$real_stub_dir")` inside run_helper_real itself:
# two call sites — H5d (Q_LANE8) and H9 (Q_LANE11) — invoke run_helper_real
# from inside a backgrounded ( ... ) & subshell, where a bash array append is
# discarded the instant the subshell exits — the exact hazard already
# documented above for _LANE_ROOT (and, for file-backed state, for
# _TRASH_HITS_FILE et al.). Anchoring every per-call stub dir under ONE root
# registered here in the main shell mirrors _LANE_ROOT exactly: it is
# subshell-safe by construction and keeps the suite's /tmp footprint at
# exactly one extra top-level entry, reclaimed by cleanup()'s EXIT trap.
#
# WHY the stub dir must be reclaimed by cleanup() at all, instead of each
# invocation unlinking its own dir the instant its direct child exits (the
# pre-#5633 behaviour): scripts/seed-warm-lane.sh's trash rm is a DETACHED
# GRANDCHILD (`{ rm -rf "$RESEED_TRASH" ...; } 9<&- &`,
# scripts/seed-warm-lane.sh:1133, and the orphan sweep at :792) that is
# forked before seed exits but resolves `rm` through PATH at an arbitrary
# LATER instant. An eager per-invocation unlink races that lookup; deferring
# every stub dir's reclaim to this file's existing EXIT trap closes the race
# for every run_helper_real caller at once. See run_helper_real below and
# Block T (task #5633) for the full mechanism and the regression guard.
#
# Naming (task #5633 code review): deliberately "warm-lane-stubroot", NOT a
# "real-stub-root"/"real-stub" prefix extension -- the RETIRED pre-#5633
# per-invocation dirs were named /tmp/test-seed-real-stub-XXXXXX, and any
# stray survivors of an aborted old run are still glob-matched by
# /tmp/test-seed-real-stub-*. A root name sharing that prefix would let an
# operator's (or janitor's) `rm -rf` of the retired pattern also delete a
# concurrently-running suite's live stub root -- reintroducing the same
# PATH-lookup race this task fixes, but for every invocation in the run at
# once. This name matches the existing test-seed-warm-lane-* family used by
# STUB_DIR/CALLS_FILE/ERR_FILE/OUT_FILE instead.
_REAL_STUB_ROOT="$(mktemp -d /tmp/test-seed-warm-lane-stubroot-XXXXXX)"
_TMPDIRS+=("$_REAL_STUB_ROOT")

CALLS_FILE="$(mktemp /tmp/test-seed-warm-lane-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-seed-warm-lane-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

OUT_FILE="$(mktemp /tmp/test-seed-warm-lane-out-XXXXXX)"
_TMPDIRS+=("$OUT_FILE")

# ── PATH stubs ────────────────────────────────────────────────────────────────

# cp stub: record argv; REIFY_TEST_REFLINK_OK=1 → exit 0, else error + exit 1
cat > "$STUB_DIR/cp" << 'STUB_EOF'
#!/usr/bin/env bash
echo "cp $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    # When REIFY_TEST_CP_CREATE_DEST=1, physically create the destination dir+file
    # so that mtime tests can assert on target/ contents.
    if [ "${REIFY_TEST_CP_CREATE_DEST:-}" = "1" ]; then
        dest="${*: -1}"
        mkdir -p "$dest/debug"
        echo "artifact" > "$dest/debug/artifact.a"
    fi
    exit 0
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# find stub: record argv, exit 0 (no-op; Block D uses real find)
cat > "$STUB_DIR/find" << 'STUB_EOF'
#!/usr/bin/env bash
echo "find $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/find"

# touch stub: record argv, exit 0 (no-op; Block D uses real touch)
cat > "$STUB_DIR/touch" << 'STUB_EOF'
#!/usr/bin/env bash
echo "touch $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/touch"

# git stub: record argv; controlled diff/rev-parse output via env vars.
# REIFY_TEST_GIT_DIFF_FAIL=1 makes diff --name-only exit non-zero (for fail-closed RED).
cat > "$STUB_DIR/git" << 'STUB_EOF'
#!/usr/bin/env bash
echo "git $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
# Detect diff --name-only and emit controlled file list (or fail)
for arg in "$@"; do
    if [ "$arg" = "--name-only" ]; then
        if [ "${REIFY_TEST_GIT_DIFF_FAIL:-0}" = "1" ]; then
            echo "git diff failed: simulated error" >&2
            exit 1
        fi
        if [ -n "${REIFY_TEST_GIT_DIFF_FILES:-}" ]; then
            printf "%s\n" "${REIFY_TEST_GIT_DIFF_FILES}"
        fi
        exit 0
    fi
done
# Detect rev-parse HEAD and emit controlled sha
for arg in "$@"; do
    if [ "$arg" = "rev-parse" ]; then
        echo "${REIFY_TEST_GIT_HEAD:-abc1234}"
        exit 0
    fi
done
exit 0
STUB_EOF
chmod +x "$STUB_DIR/git"

# _TRASH_GLOB_LEGACY / _TRASH_GLOB_SIBLING (task #5633 code review): the two
# case-pattern shapes that mark a reseed-trash path -- legacy in-lane
# (pre-#4896) and pool-level sibling (post-#4896). Defined ONCE here, rather
# than hand-copied at each call site, and threaded into every place that
# needs to recognise a trash path: the PIN and SLEEP rm stubs inside
# run_helper_real below (via the REIFY_TEST_TRASH_GLOB_* env vars, read at
# stub RUNTIME with a literal default equal to these values, so a call path
# that does not thread the env var is byte-for-byte unaffected), the T4
# synthetic rm stub (Block T), and _calls_file_has_trash_rm's line match
# (below) -- so these previously-independent copies of the same pattern
# cannot silently drift apart if the trash layout changes again the way
# #4896 changed it.
_TRASH_GLOB_LEGACY='*target.reseed-trash.*'
_TRASH_GLOB_SIBLING='*/.reseed-trash/*'

# ── run_helper ────────────────────────────────────────────────────────────────
# Invokes the script under the stub PATH.
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    _note_helper_lane "$@"
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        PATH="$STUB_DIR:$PATH" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    _note_shared_trash_use "$@"
    RC=$rc
}

# run_helper_real: like run_helper but without stubbing find/touch — for Block D
# which asserts actual mtime changes on a real fixture tree.
run_helper_real() {
    local rc=0
    _note_real_lane "$@"
    > "$ERR_FILE"
    # Only stub cp and git; let find/touch be real binaries
    local real_stub_dir
    # Minted under $_REAL_STUB_ROOT (task #5633), not bare /tmp: the per-run
    # root is reclaimed once by the suite's EXIT trap instead of this
    # invocation unlinking its own dir eagerly — see REAL_STUB_DIR below and
    # the _REAL_STUB_ROOT comment above for why an eager unlink races the
    # seed's detached trash-rm grandchild.
    real_stub_dir="$(mktemp -d "$_REAL_STUB_ROOT/stub-XXXXXX")"
    # REAL_STUB_DIR: publish this invocation's stub dir as a plain global,
    # like OUT/ERR_OUT/RC below — set immediately after mktemp so it is
    # correct even if a later assertion in this function body were to abort.
    # Only observable for calls made from the MAIN shell; H5d/H9's
    # backgrounded ( ... ) & calls discard it like any other plain-variable
    # write made inside a subshell (same hazard as _TMPDIRS, documented
    # above). Task #5633 code review: that also means a plain unset/reset
    # placed HERE could never make a subshell call "look unset" to the main
    # shell either -- a subshell's writes (including unsetting) never
    # propagate out, so the two backgrounded call sites instead reset
    # REAL_STUB_DIR to empty in the MAIN shell, immediately before
    # backgrounding (see H5d/H9 below), which is the only place such a reset
    # can actually take effect.
    REAL_STUB_DIR="$real_stub_dir"
    # cp stub that physically copies src to dest (no --reflink needed for tests)
    cat > "$real_stub_dir/cp" << 'REAL_STUB_EOF'
#!/usr/bin/env bash
echo "cp $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
# REIFY_TEST_FD9_SQUATTER=1 (task #5705, H4c): fork a long-lived child that
# DELIBERATELY inherits every descriptor open in this stub -- including seed's
# lane-lock FD 9 -- and never closes any of them. That is the one thing seed's
# own `{ rm -rf ...; } 9<&- &` jobs only do TRANSIENTLY (their 9<&- runs in the
# child, microseconds after the fork), so it turns the fork-window race into a
# GUARANTEED live dup-holder at probe time: hermetic, no scheduling, no load.
# The squatter's PID goes to REIFY_TEST_FD9_SQUATTER_PIDFILE so the caller can
# prove it is alive and really holds the lock FD (readlink /proc/<pid>/fd/9),
# then reap it. Forked BEFORE the copy so it exists on the cp-failure path too.
# Callers that do not set this var are byte-for-byte unaffected.
if [ "${REIFY_TEST_FD9_SQUATTER:-}" = "1" ]; then
    sleep "${REIFY_TEST_FD9_SQUATTER_SECS:-30}" &
    echo "$!" > "${REIFY_TEST_FD9_SQUATTER_PIDFILE:-/dev/null}"
fi
if [ "${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    # Physically copy src→dest using plain cp -a (test environment is non-XFS)
    # Parse out: cp -a --reflink=always <src> <dest>
    src=""
    dest=""
    for arg in "$@"; do
        case "$arg" in
            -a|--reflink=always) ;;
            -*) ;;
            *) [ -z "$src" ] && src="$arg" || dest="$arg" ;;
        esac
    done
    if [ -n "$src" ] && [ -n "$dest" ]; then
        /bin/cp -a "$src" "$dest"
    fi
    exit 0
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
REAL_STUB_EOF
    chmod +x "$real_stub_dir/cp"
    cp "$STUB_DIR/git" "$real_stub_dir/git"
    # Selective rm stub: only active when REIFY_TEST_PIN_RESEED_TRASH=1.
    # Records argv, exits 0 (no-op, pins trash on disk) for any arg matching
    # *target.reseed-trash.* (in-lane, pre-#4896) OR */.reseed-trash/* (pool-level
    # sibling, post-#4896) so callers can assert on the pinned trash dir.
    # All other rm calls exec /bin/rm to stay real (build-dir invalidation, etc.).
    # Callers that do not set REIFY_TEST_PIN_RESEED_TRASH are byte-for-byte unchanged.
    if [ "${REIFY_TEST_PIN_RESEED_TRASH:-}" = "1" ]; then
        cat > "$real_stub_dir/rm" << 'REAL_RM_STUB_EOF'
#!/usr/bin/env bash
echo "rm $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
for arg in "$@"; do
    case "$arg" in
        ${REIFY_TEST_TRASH_GLOB_LEGACY:-*target.reseed-trash.*}|${REIFY_TEST_TRASH_GLOB_SIBLING:-*/.reseed-trash/*}) exit 0 ;;
    esac
done
exec /bin/rm "$@"
REAL_RM_STUB_EOF
        chmod +x "$real_stub_dir/rm"
    elif [ "${REIFY_TEST_SLEEP_RESEED_TRASH_RM:-}" = "1" ]; then
        # Selective SLEEPING rm stub: only active when
        # REIFY_TEST_SLEEP_RESEED_TRASH_RM=1. For the trash path (same glob as
        # the PIN stub above), sleeps ~2s before exiting 0 (no-op, like PIN) --
        # long enough that a caller probing immediately after the seed script
        # itself has exited (RC returned to run_helper_real) can deterministically
        # observe a detached background `rm &` that is STILL RUNNING, e.g. to
        # assert it did/didn't leak an inherited FD. All other rm calls exec
        # /bin/rm to stay real. Callers that do not set this var are unchanged.
        cat > "$real_stub_dir/rm" << 'REAL_SLEEP_RM_STUB_EOF'
#!/usr/bin/env bash
echo "rm $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
for arg in "$@"; do
    case "$arg" in
        ${REIFY_TEST_TRASH_GLOB_LEGACY:-*target.reseed-trash.*}|${REIFY_TEST_TRASH_GLOB_SIBLING:-*/.reseed-trash/*})
            sleep 2
            exit 0
            ;;
    esac
done
exec /bin/rm "$@"
REAL_SLEEP_RM_STUB_EOF
        chmod +x "$real_stub_dir/rm"
    fi
    # NOTE: stdout is captured via a real FILE (OUT_FILE), NOT command
    # substitution ($(...)). A file redirect has no EOF-on-all-writers
    # semantics: this returns as soon as the direct `bash "$SCRIPT"` child
    # exits, like a plain `wait`, letting H4 observe seed's own exit
    # independently of ANY detached grandchild this script forks -- present
    # or future. $(...) does not have that property: a backgrounded child of
    # $SCRIPT that inherited stdout (fd 1) would keep a pipe's write end
    # open, so $(...) would block until THAT descendant also exited (the
    # classic bash "command substitution hangs on a background job"
    # pitfall) -- masking exactly the FD-hygiene bug Block Q/H4 exists to
    # catch: with a pipe, the probe below would never run until the leaking
    # background rm had already exited on its own, so the lock would always
    # read back FREE regardless of whether seed itself leaked FD 9.
    # (Task #6219: seed's own trash rm jobs -- e.g. the reseed-trash `rm &`,
    # formerly cited here as the live example -- no longer hold fd 1/fd 2 at
    # all, so that example is now stale; see Block V above. The choice of a
    # file over $(...) stays load-bearing regardless, against any future
    # detached child that DOES inherit them.)
    > "$OUT_FILE"
    REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
    REIFY_TEST_TRASH_GLOB_LEGACY="$_TRASH_GLOB_LEGACY" \
    REIFY_TEST_TRASH_GLOB_SIBLING="$_TRASH_GLOB_SIBLING" \
    PATH="$real_stub_dir:$PATH" \
        bash "$SCRIPT" "$@" >"$OUT_FILE" 2>"$ERR_FILE" || rc=$?
    OUT="$(cat "$OUT_FILE")"
    ERR_OUT="$(cat "$ERR_FILE")"
    _note_shared_trash_use "$@"
    RC=$rc
    # Deliberately NOT `rm -rf "$real_stub_dir"` here (task #5633; a pre-#5633
    # eager unlink lived on this line). WHY: scripts/seed-warm-lane.sh's trash
    # rm is a DETACHED GRANDCHILD (`{ rm -rf "$RESEED_TRASH" ...; } 9<&- &`,
    # scripts/seed-warm-lane.sh:1133, and the orphan sweep at :792) that is
    # forked before seed exits but does its PATH lookup at an arbitrary LATER
    # instant. Unlinking real_stub_dir the moment the DIRECT child (`bash
    # "$SCRIPT"` above) exits races that lookup — when the unlink won, the
    # grandchild resolved the real /bin/rm instead of the
    # REIFY_TEST_PIN_RESEED_TRASH/REIFY_TEST_SLEEP_RESEED_TRASH_RM stub and
    # genuinely deleted the trash the stub exists to pin (the intermittent
    # I14g / M-setup failures #5633 fixes).
    # WHY NOT wait for the grandchild instead: H4 (below) and Block Q/H5d/H9
    # deliberately require this helper to return the instant the DIRECT child
    # exits, as already documented at the OUT_FILE-vs-command-substitution
    # NOTE above — waiting here would serialise every caller on a detached
    # background rm those blocks are specifically testing around.
    # LIFETIME: real_stub_dir now lives under $_REAL_STUB_ROOT (see mktemp
    # above), reclaimed once for every invocation by this file's existing
    # cleanup() EXIT trap — see Block T (task #5633) for the regression guard.
}

reset_calls() {
    > "$CALLS_FILE"
}

# _wait_for_reader_lock <ready-marker> <deadline-seconds>
# Causal ordering (technique R, docs/prds/infra-test-wallclock-deflake.md,
# task #4847): polls for the READY marker file in 0.05s ticks instead of a
# fixed sleep, so a background flock holder's acquisition is causally
# guaranteed complete before the caller's next statement runs -- a fixed
# sleep races the holder under CPU/IO load. Mirrors
# tests/infra/test_thin_warm_lane.sh's and tests/infra/test_warm_lane_gc.sh's
# identically-named helper.
_wait_for_reader_lock() {
    local ready_marker="$1"
    local deadline_s="$2"
    local max_ticks=$(( deadline_s * 20 ))
    local tick=0
    while [ "$tick" -lt "$max_ticks" ]; do
        [ -f "$ready_marker" ] && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
    return 1
}

# _wait_until <deadline-seconds> <predicate-cmd...> (task #5633 code review)
# Generic bounded causal poll (technique R, docs/prds/infra-test-wallclock-deflake.md):
# runs "$@" every 0.05s until it exits 0 or <deadline-seconds> elapses,
# returning 0 the instant the predicate first succeeds and 1 on timeout. Same
# tick cadence and deadline arithmetic as _wait_for_reader_lock above,
# factored out so every OTHER bounded wait in this file shares one skeleton
# instead of hand-copying it (this file's third and fourth near-identical
# copies of that skeleton, pre-refactor). _wait_for_reader_lock itself is
# deliberately NOT rebased onto this helper: its own docstring documents an
# intentional identical-shape mirror with tests/infra/test_thin_warm_lane.sh
# and tests/infra/test_warm_lane_gc.sh, and this task holds no lock on either
# sibling file to keep them in sync with a rebase made only here.
_wait_until() {
    local deadline_s="$1"
    shift
    local max_ticks=$(( deadline_s * 20 ))
    local tick=0
    while [ "$tick" -lt "$max_ticks" ]; do
        "$@" && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
    return 1
}

# _calls_file_has_trash_rm <calls-file> (task #5633 code review)
# _wait_until predicate: true as soon as <calls-file> contains a line that
# both starts with "rm " and mentions a trash path -- matching
# _TRASH_GLOB_LEGACY/_TRASH_GLOB_SIBLING above, the SAME source the PIN/
# SLEEP/T4 rm stubs read via REIFY_TEST_TRASH_GLOB_*, so the recorder and
# this predicate cannot drift apart. Called as `_wait_until <deadline>
# _calls_file_has_trash_rm "$CALLS_FILE"` ahead of I14g and M-setup's
# trash-presence checks below, and exercised directly by Block T's T3
# discrimination control. A timeout is a REAL signal, not noise: it means
# the detached rm was NOT the stub -- a real /bin/rm records nothing --
# which is exactly the #5633 failure mode.
_calls_file_has_trash_rm() {
    local calls_file="$1" line
    [ -f "$calls_file" ] || return 1
    while IFS= read -r line; do
        case "$line" in
            "rm "$_TRASH_GLOB_LEGACY|"rm "$_TRASH_GLOB_SIBLING) return 0 ;;
        esac
    done < "$calls_file"
    return 1
}

# ─────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0 with usage on stderr
reset_calls
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits non-zero
reset_calls
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits non-zero" test "$RC" -ne 0

# A3: missing positional args (only mode flag, no base/lane dirs) exits non-zero
reset_calls
run_helper --fresh-checkout
assert "A3: missing positional args exits non-zero" test "$RC" -ne 0

# A4: neither --fresh-checkout nor --reset-in-place exits non-zero
reset_calls
A_BASE="$(mktemp -d /tmp/test-seed-A-base-XXXXXX)"
A_LANE="$(make_isolated_lane A-lane)"
_TMPDIRS+=("$A_BASE")
run_helper "$A_BASE" "$A_LANE"
assert "A4: neither mode flag exits non-zero" test "$RC" -ne 0

# A5: both --fresh-checkout and --reset-in-place exits non-zero
reset_calls
run_helper "$A_BASE" "$A_LANE" --fresh-checkout --reset-in-place
assert "A5: both mode flags exits non-zero" test "$RC" -ne 0

# ─────────────────────────────────────────────────────────────────────────────
# Block B — RUSTFLAGS guard (B5 / D4)
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: RUSTFLAGS guard (B5) ---"

# Fixture: a base dir with a sidecar recording RUSTFLAGS
B_BASE_PARENT="$(mktemp -d /tmp/test-seed-B-parent-XXXXXX)"
B_BASE="$B_BASE_PARENT/target"
B_LANE="$(make_isolated_lane B-lane)"
_TMPDIRS+=("$B_BASE_PARENT")
mkdir -p "$B_BASE"
# Write sidecar with recorded RUSTFLAGS=old-flags
cat > "$B_BASE_PARENT/.warm-base-meta" <<'SIDECAR_EOF'
RUSTFLAGS=old-flags
INVOCATION=
SIDECAR_EOF

# B1: RUSTFLAGS mismatch → non-zero exit
reset_calls
RUSTFLAGS="new-flags" run_helper "$B_BASE" "$B_LANE" --fresh-checkout
assert "B1: RUSTFLAGS mismatch exits non-zero" test "$RC" -ne 0

# B2: stderr names the RUSTFLAGS mismatch (actionable message)
assert "B2: stderr names RUSTFLAGS mismatch" \
    bash -c 'printf "%s\n" "$1" | grep -qi "RUSTFLAGS"' _ "$ERR_OUT"

# B3: STDOUT is EMPTY on mismatch (fail-closed: no path emitted)
assert "B3: STDOUT is EMPTY on RUSTFLAGS mismatch (fail-closed)" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# B4: cp was NEVER invoked (guard fires before clone)
assert "B4: cp NEVER invoked on RUSTFLAGS mismatch" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# B5: matching RUSTFLAGS (recorded "old-flags" == env "old-flags") → guard passes → cp IS called
B_LANE2="$(make_isolated_lane B-lane2)"
reset_calls
RUSTFLAGS="old-flags" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$B_BASE" "$B_LANE2" --fresh-checkout
assert "B5: matching RUSTFLAGS passes guard → cp invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

# B6: also test: no sidecar → recorded RUSTFLAGS defaults to "" → empty-env RUSTFLAGS matches
B_BASE2_PARENT="$(mktemp -d /tmp/test-seed-B2-parent-XXXXXX)"
B_BASE2="$B_BASE2_PARENT/target"
B_LANE3="$(make_isolated_lane B-lane3)"
_TMPDIRS+=("$B_BASE2_PARENT")
mkdir -p "$B_BASE2"
# No sidecar: recorded defaults to ""
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$B_BASE2" "$B_LANE3" --fresh-checkout
assert "B6: no sidecar + empty RUSTFLAGS matches default → cp invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

# ─────────────────────────────────────────────────────────────────────────────
# Block C — reflink clone + fail-closed (S2)
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: reflink clone + fail-closed (S2) ---"

# Shared fixture: a base dir (with empty sidecar → guards pass) + a fresh lane dir
C_BASE_PARENT="$(mktemp -d /tmp/test-seed-C-parent-XXXXXX)"
C_BASE="$C_BASE_PARENT/target"
_TMPDIRS+=("$C_BASE_PARENT")
mkdir -p "$C_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$C_BASE_PARENT/.warm-base-meta"

# C1: cp invoked with --reflink=always and destination <lane_dir>/target
C_LANE1="$(make_isolated_lane C-lane1)"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$C_BASE" "$C_LANE1" --fresh-checkout
C1_OUT="$OUT"  # save before subsequent run_helpers overwrite OUT
assert "C1: cp invoked with --reflink=always" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# C2: cp NEVER invoked with --reflink=auto (always=always, not auto)
assert "C2: cp NEVER invoked with --reflink=auto" \
    bash -c '! grep "^cp" "$1" | grep -q -- "--reflink=auto"' _ "$CALLS_FILE"

# C3: destination is <lane_dir>/target
assert "C3: cp destination is <lane_dir>/target" \
    bash -c 'grep "^cp" "$1" | grep -qF "'"$C_LANE1/target"'"' _ "$CALLS_FILE"

# C4: cp failure (non-reflink FS) → script exits non-zero with EMPTY stdout (fail-closed)
C_LANE2="$(make_isolated_lane C-lane2)"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=0 \
    run_helper "$C_BASE" "$C_LANE2" --fresh-checkout
assert "C4: cp failure exits non-zero" test "$RC" -ne 0
assert "C4: STDOUT is EMPTY on cp failure (S2 fail-closed)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "C4: stderr names reflink failure" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "reflink|Operation not supported"' _ "$ERR_OUT"

# C5: --fresh-checkout + pre-existing NON-EMPTY <lane_dir>/target → replaced (task 4715)
# D10 replace semantics: non-empty target is replaced, NOT refused.
C_LANE3="$(make_isolated_lane C-lane3)"
mkdir -p "$C_LANE3/target"
echo "existing artifact" > "$C_LANE3/target/artifact.a"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$C_BASE" "$C_LANE3" --fresh-checkout
assert "C5: --fresh-checkout non-empty target exits 0 (replace semantics)" \
    test "$RC" -eq 0
assert "C5: --fresh-checkout replace: STDOUT is <lane_dir>/target" \
    bash -c '[ "$1" = "'"$C_LANE3/target"'" ]' _ "$OUT"
assert "C5: --fresh-checkout replace: cp IS invoked with --reflink=always" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# C6: on success STDOUT is exactly the resolved <lane_dir>/target path
assert "C6: STDOUT is exactly <lane_dir>/target on success" \
    bash -c '[ "$1" = "'"$C_LANE1/target"'" ]' _ "$C1_OUT"

# ─────────────────────────────────────────────────────────────────────────────
# Block D — fresh-checkout mtime normalization (D5)
# Uses run_helper_real (real find + touch; stub cp + git).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: fresh-checkout mtime (D5) ---"

# Epoch for the bulk stamp: 2020-01-01T00:00:00 UTC
EPOCH_2020=1577836800

# Fixture: a base_target_dir + a lane_dir with real source files and target/ + .git/
D_BASE_PARENT="$(mktemp -d /tmp/test-seed-D-parent-XXXXXX)"
D_BASE="$D_BASE_PARENT/target"
D_LANE="$(make_isolated_lane D-lane)"
_TMPDIRS+=("$D_BASE_PARENT")
mkdir -p "$D_BASE"
# Seed a build artifact so run_helper_real's /bin/cp -a propagates it to
# $D_LANE/target; allows D4 to assert that target/ files keep their mtime.
mkdir -p "$D_BASE/debug"
echo "artifact" > "$D_BASE/debug/artifact.a"
# Sidecar: no RUSTFLAGS/INVOCATION recorded (defaults "")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$D_BASE_PARENT/.warm-base-meta"
# Source files in lane_dir (these should be stamped to 2020-01-01)
mkdir -p "$D_LANE/src"
echo 'fn main() {}' > "$D_LANE/src/main.rs"
echo 'pub fn lib() {}' > "$D_LANE/src/lib.rs"
# .git/ files in lane_dir (pruned — must NOT be stamped)
mkdir -p "$D_LANE/.git"
echo '[core]' > "$D_LANE/.git/config"
# delta source file (will be passed via --touch; must be stamped to now)
D_DELTA="$D_LANE/src/changed.rs"
echo 'pub fn changed() {}' > "$D_DELTA"

# Record mtime of .git/config BEFORE the run (should be ~now; definitely > 2020)
D_GIT_MTIME_BEFORE="$(stat -c '%Y' "$D_LANE/.git/config")"

# A small sleep so "before" and "after" mtimes are distinguishable
sleep 1

# Run --fresh-checkout with real find/touch; pass D_DELTA via --touch
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$D_BASE" "$D_LANE" --fresh-checkout --touch "$D_DELTA"
assert "D0: script exits 0 (fresh-checkout succeeds)" test "$RC" -eq 0

# D1: source files are stamped to 2020-01-01 epoch
D1_MTIME_SRC="$(stat -c '%Y' "$D_LANE/src/main.rs")"
assert "D1: src/main.rs mtime == 2020-01-01 epoch ($EPOCH_2020)" \
    test "$D1_MTIME_SRC" -eq "$EPOCH_2020"

D1_MTIME_LIB="$(stat -c '%Y' "$D_LANE/src/lib.rs")"
assert "D1: src/lib.rs mtime == 2020-01-01 epoch ($EPOCH_2020)" \
    test "$D1_MTIME_LIB" -eq "$EPOCH_2020"

# D2: files under .git/ keep their original mtime (pruned — NOT stamped)
D2_GIT_MTIME_AFTER="$(stat -c '%Y' "$D_LANE/.git/config")"
assert "D2: .git/config mtime unchanged (pruned from bulk stamp)" \
    test "$D2_GIT_MTIME_AFTER" -eq "$D_GIT_MTIME_BEFORE"

# D3: delta file (--touch) is stamped to ~now (mtime > 2020-01-01 epoch)
D3_DELTA_MTIME="$(stat -c '%Y' "$D_DELTA")"
assert "D3: --touch delta file mtime > 2020-01-01 (stamped to now)" \
    test "$D3_DELTA_MTIME" -gt "$EPOCH_2020"

# D4: files under target/ keep their pre-clone mtime — find prunes target/ entirely.
# $D_BASE/debug/artifact.a was seeded above; run_helper_real's /bin/cp -a
# propagates it to $D_LANE/target/debug/artifact.a.
D4_TARGET_MTIME="$(stat -c '%Y' "$D_LANE/target/debug/artifact.a")"
assert "D4: target/debug/artifact.a mtime > 2020-01-01 (pruned from bulk stamp)" \
    test "$D4_TARGET_MTIME" -gt "$EPOCH_2020"

# ─────────────────────────────────────────────────────────────────────────────
# Block E — reset-in-place: NO bulk 2020-01-01 stamp (stub find+touch)
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: reset-in-place (no bulk stamp) ---"

# Fixture: a fresh base (sidecar with no RUSTFLAGS/INVOCATION) + a lane dir
E_BASE_PARENT="$(mktemp -d /tmp/test-seed-E-parent-XXXXXX)"
E_BASE="$E_BASE_PARENT/target"
E_LANE="$(make_isolated_lane E-lane)"
_TMPDIRS+=("$E_BASE_PARENT")
mkdir -p "$E_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$E_BASE_PARENT/.warm-base-meta"
mkdir -p "$E_LANE/src"
echo 'fn main() {}' > "$E_LANE/src/main.rs"

# E1: --reset-in-place exits 0
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$E_BASE" "$E_LANE" --reset-in-place
assert "E1: --reset-in-place exits 0" test "$RC" -eq 0

# E2: find was NOT invoked with a 2020-01-01 bulk stamp
# (the stub records every find call; if reset-in-place skips the bulk stamp,
# no find call with "2020-01-01" should appear)
assert "E2: find NOT called with 2020-01-01 bulk stamp (reset-in-place skips it)" \
    bash -c '! grep "^find" "$1" | grep -q "2020"' _ "$CALLS_FILE"

# E3: STDOUT is exactly <lane_dir>/target (success contract preserved)
assert "E3: STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$E_LANE/target"'" ]' _ "$OUT"

# ─────────────────────────────────────────────────────────────────────────────
# Block F — invocation fingerprint guard (S1)
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: invocation fingerprint guard (S1) ---"

# Fixture: base with sidecar recording a specific invocation fingerprint
F_BASE_PARENT="$(mktemp -d /tmp/test-seed-F-parent-XXXXXX)"
F_BASE="$F_BASE_PARENT/target"
_TMPDIRS+=("$F_BASE_PARENT")
mkdir -p "$F_BASE"
cat > "$F_BASE_PARENT/.warm-base-meta" <<'SIDECAR_EOF'
RUSTFLAGS=
INVOCATION=my-invocation-fingerprint
SIDECAR_EOF

# F1: invocation mismatch → non-zero exit
F_LANE1="$(make_isolated_lane F-lane1)"
reset_calls
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="wrong-invocation" \
    run_helper "$F_BASE" "$F_LANE1" --fresh-checkout
assert "F1: invocation mismatch exits non-zero" test "$RC" -ne 0

# F2: stderr names the invocation mismatch (actionable)
assert "F2: stderr names invocation mismatch" \
    bash -c 'printf "%s\n" "$1" | grep -qi "invocation"' _ "$ERR_OUT"

# F3: STDOUT is EMPTY on mismatch (fail-closed)
assert "F3: STDOUT is EMPTY on invocation mismatch" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# F4: cp NEVER invoked (guard fires before clone)
assert "F4: cp NEVER invoked on invocation mismatch" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# F5: matching invocation → guard passes → cp IS called
F_LANE2="$(make_isolated_lane F-lane2)"
reset_calls
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="my-invocation-fingerprint" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$F_BASE" "$F_LANE2" --fresh-checkout
assert "F5: matching invocation passes guard → cp invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

# F6: no sidecar recorded invocation → defaults "" → empty env matches
F_BASE2_PARENT="$(mktemp -d /tmp/test-seed-F2-parent-XXXXXX)"
F_BASE2="$F_BASE2_PARENT/target"
F_LANE3="$(make_isolated_lane F-lane3)"
_TMPDIRS+=("$F_BASE2_PARENT")
mkdir -p "$F_BASE2"
# No sidecar → recorded invocation defaults to ""
reset_calls
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$F_BASE2" "$F_LANE3" --fresh-checkout
assert "F6: no sidecar + empty invocation matches default → cp invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

# ─────────────────────────────────────────────────────────────────────────────
# Block G — --record-base writer
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: --record-base writer ---"

# Fixture: a base_target_dir to record provenance for
G_BASE_PARENT="$(mktemp -d /tmp/test-seed-G-parent-XXXXXX)"
G_BASE="$G_BASE_PARENT/target"
_TMPDIRS+=("$G_BASE_PARENT")
mkdir -p "$G_BASE"

EXPECTED_SIDECAR="$G_BASE_PARENT/.warm-base-meta"

# G1: --record-base exits 0
reset_calls
RUSTFLAGS="my-rustflags" REIFY_WARM_LANE_INVOCATION="my-invocation" \
    run_helper --record-base "$G_BASE"
assert "G1: --record-base exits 0" test "$RC" -eq 0

# G2: sidecar file was created beside the base target dir
assert "G2: sidecar created at $(dirname $G_BASE)/.warm-base-meta" \
    test -f "$EXPECTED_SIDECAR"

# G3: sidecar records RUSTFLAGS
assert "G3: sidecar records RUSTFLAGS=my-rustflags" \
    bash -c 'grep -q "^RUSTFLAGS=my-rustflags$" "$1"' _ "$EXPECTED_SIDECAR"

# G4: sidecar records INVOCATION
assert "G4: sidecar records INVOCATION=my-invocation" \
    bash -c 'grep -q "^INVOCATION=my-invocation$" "$1"' _ "$EXPECTED_SIDECAR"

# G5: STDOUT is the sidecar path (exactly)
assert "G5: STDOUT is exactly the sidecar path" \
    bash -c '[ "$1" = "'"$EXPECTED_SIDECAR"'" ]' _ "$OUT"

# G6: round-trip — a subsequent seed against the recorded base passes the guards
G_LANE="$(make_isolated_lane G-lane)"
reset_calls
RUSTFLAGS="my-rustflags" REIFY_WARM_LANE_INVOCATION="my-invocation" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$G_BASE" "$G_LANE" --fresh-checkout
assert "G6: round-trip: matching env passes both guards → cp invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

# G7: round-trip mismatch — different RUSTFLAGS is still refused after record-base
G_LANE2="$(make_isolated_lane G-lane2)"
reset_calls
RUSTFLAGS="wrong-flags" REIFY_WARM_LANE_INVOCATION="my-invocation" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$G_BASE" "$G_LANE2" --fresh-checkout
assert "G7: round-trip: mismatched RUSTFLAGS still refused after record-base" \
    test "$RC" -ne 0

# ─────────────────────────────────────────────────────────────────────────────
# Block H — build-script output-dir invalidation (non-relocatable absolute paths)
# Uses run_helper_real (real cp/find/touch + stub git).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: build-script output-dir invalidation (tauri-* + reify-gui-*) ---"

# Fixture: a base target/ with build dirs under debug/build and release/build.
# The sidecar has empty RUSTFLAGS/INVOCATION so guards pass.
H_BASE_PARENT="$(mktemp -d /tmp/test-seed-H-parent-XXXXXX)"
H_BASE="$H_BASE_PARENT/target"
H_LANE="$(make_isolated_lane H-lane)"
_TMPDIRS+=("$H_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H_BASE_PARENT/.warm-base-meta"

# Build dirs under two profiles:
#   debug/build:   tauri-AAAA, tauri-plugin-fs-BBBB, reify-gui-CCCC, serde-DDDD
#   release/build: tauri-EEEE, serde-FFFF
# Each dir contains an 'output' file (non-empty, as cargo would produce).
mkdir -p "$H_BASE/debug/build/tauri-AAAA"
mkdir -p "$H_BASE/debug/build/tauri-plugin-fs-BBBB"
mkdir -p "$H_BASE/debug/build/reify-gui-CCCC"
mkdir -p "$H_BASE/debug/build/serde-DDDD"
mkdir -p "$H_BASE/release/build/tauri-EEEE"
mkdir -p "$H_BASE/release/build/serde-FFFF"
echo "out" > "$H_BASE/debug/build/tauri-AAAA/output"
echo "out" > "$H_BASE/debug/build/tauri-plugin-fs-BBBB/output"
echo "out" > "$H_BASE/debug/build/reify-gui-CCCC/output"
echo "out" > "$H_BASE/debug/build/serde-DDDD/output"
echo "out" > "$H_BASE/release/build/tauri-EEEE/output"
echo "out" > "$H_BASE/release/build/serde-FFFF/output"

# H1: seed with --fresh-checkout; real cp physically copies dirs to the lane.
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H_BASE" "$H_LANE" --fresh-checkout

# H1c: success contract — exit 0, stdout == <lane>/target
assert "H1c: --fresh-checkout exits 0 (build-dir invalidation)" test "$RC" -eq 0
assert "H1c: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "'"$H_LANE/target"'" ]' _ "$OUT"

# H1a: allow-listed dirs REMOVED across both profiles
assert "H1a: debug/build/tauri-AAAA GONE (allow-listed tauri-*)" \
    bash -c '[ ! -e "'"$H_LANE/target/debug/build/tauri-AAAA"'" ]'
assert "H1a: debug/build/tauri-plugin-fs-BBBB GONE (allow-listed tauri-*)" \
    bash -c '[ ! -e "'"$H_LANE/target/debug/build/tauri-plugin-fs-BBBB"'" ]'
assert "H1a: debug/build/reify-gui-CCCC GONE (allow-listed reify-gui-*)" \
    bash -c '[ ! -e "'"$H_LANE/target/debug/build/reify-gui-CCCC"'" ]'
assert "H1a: release/build/tauri-EEEE GONE (allow-listed tauri-*)" \
    bash -c '[ ! -e "'"$H_LANE/target/release/build/tauri-EEEE"'" ]'

# H1b: unlisted dirs PRESERVED (warmth retained for non-offending crates)
assert "H1b: debug/build/serde-DDDD PRESERVED (not allow-listed)" \
    test -d "$H_LANE/target/debug/build/serde-DDDD"
assert "H1b: release/build/serde-FFFF PRESERVED (not allow-listed)" \
    test -d "$H_LANE/target/release/build/serde-FFFF"

# H1d: info line reports the correct non-zero invalidated count.
# This locks in that the matcher FIRED (dirs absent could also mean cp failed),
# and would catch a silent 0-count regression caused by the assignment-time-glob
# bug (unquoted 'tauri-*' in array assignment expanding against the CWD and
# replacing the intended literal patterns with CWD matches → 0 dirs found).
# Expected count: 4 dirs removed (debug/build: tauri-AAAA + tauri-plugin-fs-BBBB
# + reify-gui-CCCC; release/build: tauri-EEEE).
assert "H1d: info line reports Invalidated 4 non-relocatable dirs (matcher fired)" \
    bash -c 'printf "%s\n" "$1" | grep -q "Invalidated 4 "' _ "$ERR_OUT"

# ── H3a: --reset-in-place does NOT invalidate (scope guard) ──────────────────
# The invalidation block must live entirely inside `if [ -n "$FRESH_CHECKOUT" ]`.
H3a_BASE_PARENT="$(mktemp -d /tmp/test-seed-H3a-parent-XXXXXX)"
H3a_BASE="$H3a_BASE_PARENT/target"
H3a_LANE="$(make_isolated_lane H3a-lane)"
_TMPDIRS+=("$H3a_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H3a_BASE_PARENT/.warm-base-meta"
mkdir -p "$H3a_BASE/debug/build/tauri-XXXX"
echo "out" > "$H3a_BASE/debug/build/tauri-XXXX/output"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H3a_BASE" "$H3a_LANE" --reset-in-place
assert "H3a: --reset-in-place exits 0" test "$RC" -eq 0
assert "H3a: debug/build/tauri-XXXX PRESERVED under --reset-in-place (scope guard)" \
    test -d "$H3a_LANE/target/debug/build/tauri-XXXX"

# ── H3b: clean no-op when nothing matches (set -euo pipefail safe) ───────────
# Case 1: build/ exists but contains only unlisted dirs (serde-YYYY)
H3b1_BASE_PARENT="$(mktemp -d /tmp/test-seed-H3b1-parent-XXXXXX)"
H3b1_BASE="$H3b1_BASE_PARENT/target"
H3b1_LANE="$(make_isolated_lane H3b1-lane)"
_TMPDIRS+=("$H3b1_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H3b1_BASE_PARENT/.warm-base-meta"
mkdir -p "$H3b1_BASE/debug/build/serde-YYYY"
echo "out" > "$H3b1_BASE/debug/build/serde-YYYY/output"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H3b1_BASE" "$H3b1_LANE" --fresh-checkout
assert "H3b: no-match (only unlisted dirs) exits 0" test "$RC" -eq 0
assert "H3b: no-match: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "'"$H3b1_LANE/target"'" ]' _ "$OUT"

# Case 2: target/ has NO build/ dir at all
H3b2_BASE_PARENT="$(mktemp -d /tmp/test-seed-H3b2-parent-XXXXXX)"
H3b2_BASE="$H3b2_BASE_PARENT/target"
H3b2_LANE="$(make_isolated_lane H3b2-lane)"
_TMPDIRS+=("$H3b2_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H3b2_BASE_PARENT/.warm-base-meta"
# Only a deps/ dir — no build/ dir at all
mkdir -p "$H3b2_BASE/debug/deps"
echo "libserde.rlib" > "$H3b2_BASE/debug/deps/libserde.rlib"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H3b2_BASE" "$H3b2_LANE" --fresh-checkout
assert "H3b: no-build-dir exits 0" test "$RC" -eq 0
assert "H3b: no-build-dir: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "'"$H3b2_LANE/target"'" ]' _ "$OUT"

# ── H3c: sibling non-build dirs untouched (deps/, .fingerprint/ preserved) ───
H3c_BASE_PARENT="$(mktemp -d /tmp/test-seed-H3c-parent-XXXXXX)"
H3c_BASE="$H3c_BASE_PARENT/target"
H3c_LANE="$(make_isolated_lane H3c-lane)"
_TMPDIRS+=("$H3c_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H3c_BASE_PARENT/.warm-base-meta"
mkdir -p "$H3c_BASE/debug/build/tauri-ZZZZ"
mkdir -p "$H3c_BASE/debug/deps"
mkdir -p "$H3c_BASE/debug/.fingerprint"
echo "out" > "$H3c_BASE/debug/build/tauri-ZZZZ/output"
echo "libserde.rlib" > "$H3c_BASE/debug/deps/libserde.rlib"
echo "fp" > "$H3c_BASE/debug/.fingerprint/serde-abc123"

# REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=1 (task 5632): this fixture resolves no
# delta-touch base (no BASE_COMMIT in the sidecar, no ${H3c_BASE}.basecommit, no
# --base-commit) AND the debug/.fingerprint dir it deliberately creates is
# exactly the evidence §9.5 inv.13 refuses on, so the seed would otherwise abort
# before reaching the build-dir invalidation sweep this arm exists to measure.
# The knob is the semantically correct annotation rather than a workaround: the
# .fingerprint dir is LOAD-BEARING for what H3c asserts (that the sweep
# preserves non-build siblings), so it cannot be dropped; and passing
# --base-commit instead would divert H3c onto the _touch_git_delta stub-git path
# it does not otherwise exercise. Block U's U1 covers the refusal itself.
reset_calls
REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=1 \
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H3c_BASE" "$H3c_LANE" --fresh-checkout
assert "H3c: --fresh-checkout exits 0 (sibling dirs preserved)" test "$RC" -eq 0
assert "H3c: debug/deps PRESERVED (non-build sibling untouched)" \
    test -d "$H3c_LANE/target/debug/deps"
assert "H3c: debug/.fingerprint PRESERVED (non-build sibling untouched)" \
    test -d "$H3c_LANE/target/debug/.fingerprint"
assert "H3c: debug/build/tauri-ZZZZ GONE (allow-listed)" \
    bash -c '[ ! -e "'"$H3c_LANE/target/debug/build/tauri-ZZZZ"'" ]'

# ── H3d: -maxdepth 3 — nested build/ dirs inside output dirs are NOT walked ──
# Protects the -maxdepth 3 boundary in the find command.  Build dirs at depth 4+
# from LANE_TARGET are nested inside build-script out/ subdirs, not cargo profile
# build dirs; they must NOT be invalidated (false-invalidation risk).
H3d_BASE_PARENT="$(mktemp -d /tmp/test-seed-H3d-parent-XXXXXX)"
H3d_BASE="$H3d_BASE_PARENT/target"
H3d_LANE="$(make_isolated_lane H3d-lane)"
_TMPDIRS+=("$H3d_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H3d_BASE_PARENT/.warm-base-meta"
# Depth-2 profile build dir: target/debug/build/tauri-OUTER — should be invalidated
mkdir -p "$H3d_BASE/debug/build/tauri-OUTER"
echo "out" > "$H3d_BASE/debug/build/tauri-OUTER/output"
# Depth-5 build dir nested inside a build-script output dir:
#   target/debug/build/some-crate-hash/out/build/tauri-NESTED
# This is NOT a cargo profile build dir.  With -maxdepth 3 the find does not
# descend to it, so tauri-NESTED should be PRESERVED.
mkdir -p "$H3d_BASE/debug/build/some-crate-hash/out/build/tauri-NESTED"
echo "nested" > "$H3d_BASE/debug/build/some-crate-hash/out/build/tauri-NESTED/output"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H3d_BASE" "$H3d_LANE" --fresh-checkout
assert "H3d: --fresh-checkout exits 0 (maxdepth boundary)" test "$RC" -eq 0
# Depth-2 tauri-OUTER is invalidated (expected, within maxdepth)
assert "H3d: depth-2 tauri-OUTER GONE (in allow-list, within maxdepth)" \
    bash -c '[ ! -e "'"$H3d_LANE/target/debug/build/tauri-OUTER"'" ]'
# Depth-5 tauri-NESTED is preserved (not found by -maxdepth 3)
assert "H3d: depth-5 tauri-NESTED PRESERVED (nested in out/build/, outside maxdepth)" \
    test -d "$H3d_LANE/target/debug/build/some-crate-hash/out/build/tauri-NESTED"

# ─────────────────────────────────────────────────────────────────────────────
# Block I — replace-existing reset (--fresh-checkout, task 4715)
# I1-I3:  hermetic (run_helper, stub cp, RESEED_TRASH_SYNC=1 implied via I4-I7)
# I3b:    async-branch smoke (run_helper, stub cp, no RESEED_TRASH_SYNC)
# I4-I7:  real-fs (run_helper_real, REIFY_WARM_LANE_RESEED_TRASH_SYNC=1)
# I8-I13: misuse guards
# I14:    deterministic prune check (async, REIFY_TEST_PIN_RESEED_TRASH=1, no SYNC)
# I15:    async large-trash smoke (no SYNC, no rm stub, 200+ files)
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block I: replace-existing reset (--fresh-checkout) ---"

# Shared base fixture: base with empty sidecar so RUSTFLAGS+invocation guards pass.
I_BASE_PARENT="$(mktemp -d /tmp/test-seed-I-parent-XXXXXX)"
I_BASE="$I_BASE_PARENT/target"
_TMPDIRS+=("$I_BASE_PARENT")
mkdir -p "$I_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$I_BASE_PARENT/.warm-base-meta"

# ── I1-I3: hermetic replace assertions (stub cp, REIFY_TEST_REFLINK_OK=1) ────
# Lane has a pre-existing NON-EMPTY target (stale content from prior lane use).
I_LANE1="$(make_isolated_lane I-lane1)"
mkdir -p "$I_LANE1/target"
echo "stale artifact" > "$I_LANE1/target/stale.a"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$I_BASE" "$I_LANE1" --fresh-checkout
I1_OUT="$OUT"  # save before subsequent run_helpers overwrite OUT

# I1: --fresh-checkout with non-empty target must exit 0 (replace, not refuse)
assert "I1: --fresh-checkout non-empty target exits 0 (replace semantics)" \
    test "$RC" -eq 0

# I2: cp invoked with --reflink=always (thin CoW clone behavioral proxy)
assert "I2: cp invoked with --reflink=always" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# I3: STDOUT is exactly <lane_dir>/target (success contract)
assert "I3: STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$I_LANE1/target"'" ]' _ "$I1_OUT"

# ── I3b: async-branch smoke ──────────────────────────────────────────────────
# Same hermetic harness as I1-I3 but WITHOUT REIFY_WARM_LANE_RESEED_TRASH_SYNC.
# Purpose: confirm the production async-rm path (rm -rf & with warning on
# failure) executes without a synchronous error; a regression that breaks the
# async branch (e.g. syntax error in the subshell after &) would be invisible
# to I4-I7 which always force SYNC=1.
# No trash-leak assertion (async cleanup is inherently race-conditional).
I_LANE_ASYNC="$(make_isolated_lane I-async)"
mkdir -p "$I_LANE_ASYNC/target"
echo "stale artifact" > "$I_LANE_ASYNC/target/stale.a"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$I_BASE" "$I_LANE_ASYNC" --fresh-checkout

assert "I3b: async-branch: exit 0 (async rm path executes without synchronous error)" \
    test "$RC" -eq 0
assert "I3b: async-branch: cp invoked with --reflink=always (async path reached cp)" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# ── I4-I7: real-fs replace + trash-cleanup assertions ────────────────────────
# run_helper_real: real find/touch + physically-copying cp stub.
# REIFY_WARM_LANE_RESEED_TRASH_SYNC=1: force synchronous trash removal so the
# no-trash-leak assertion is race-free (no background rm -rf &).
I_BASE_REAL_PARENT="$(mktemp -d /tmp/test-seed-I-real-parent-XXXXXX)"
I_BASE_REAL="$I_BASE_REAL_PARENT/target"
# I_LANE_REAL is created via make_isolated_lane (task 5590), NOT bare /tmp
# (task 5384 amendment; same rationale as I14 below): I7b's sibling-trash
# assertion resolves dirname(I_LANE_REAL)/.reseed-trash, so a bare-/tmp lane
# would put it at the machine-shared /tmp/.reseed-trash.
I_LANE_REAL="$(make_isolated_lane I-real-lane)"
_TMPDIRS+=("$I_BASE_REAL_PARENT")
# Seed base with a known artifact so we can verify it appears after the clone.
mkdir -p "$I_BASE_REAL/debug"
echo "base artifact" > "$I_BASE_REAL/debug/base_artifact.a"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$I_BASE_REAL_PARENT/.warm-base-meta"
# Seed the lane with a non-empty target containing divergent content.
mkdir -p "$I_LANE_REAL/target/debug"
echo "stale content" > "$I_LANE_REAL/target/OLD_DIVERGENT.txt"
echo "stale artifact" > "$I_LANE_REAL/target/debug/stale.a"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 \
    run_helper_real "$I_BASE_REAL" "$I_LANE_REAL" --fresh-checkout

# I4: exit 0 (replace succeeds with a real non-empty target)
assert "I4: real-fs --fresh-checkout with non-empty target exits 0" \
    test "$RC" -eq 0

# I5: divergent sentinel file GONE from the new target (old content was replaced)
assert "I5: OLD_DIVERGENT.txt GONE from <lane>/target after reseed" \
    bash -c '[ ! -e "'"$I_LANE_REAL/target/OLD_DIVERGENT.txt"'" ]'

# I6: base content IS present in the new target (clone from base succeeded)
assert "I6: base_artifact.a IS present in <lane>/target after reseed" \
    test -f "$I_LANE_REAL/target/debug/base_artifact.a"

# I7: NO target.reseed-trash.* left in lane (synchronous rm completed; no in-lane leak)
# Regression guard: a leaking trash dir re-introduces the unbounded-growth bug.
assert "I7: NO target.reseed-trash.* left in lane (trash fully reclaimed, no leak)" \
    bash -c '[ -z "$(find "'"$I_LANE_REAL"'" -maxdepth 1 -name "target.reseed-trash.*" -print -quit 2>/dev/null)" ]'

# I7b: sibling .reseed-trash/ has no leftover <lane>.* entry after SYNC rm.
# Intent-preservation (#4896): post-fix the trash lives in the sibling; the sync rm
# must also clean it up completely.  Pre-fix: sibling never created (trivially passes).
_I7_SIBLING_TRASH_DIR="$(dirname "$I_LANE_REAL")/.reseed-trash"
assert "I7b: sibling .reseed-trash/ has no leftover <lane>.* entry after SYNC rm" \
    bash -c '[ -z "$(find "'"$_I7_SIBLING_TRASH_DIR"'" -maxdepth 1 -name "'"$(basename "$I_LANE_REAL")"'.*" -print -quit 2>/dev/null)" ]'

# ── I8-I13: misuse guard (retained-refusal cases, task 4715 step-3) ───────────
# These cases must be refused BEFORE the rename-to-trash (cp never reached).
# Fixture: a shared mount root for the under-mount / outside-mount tests.
I_MOUNT="$(mktemp -d /tmp/test-seed-I-mount-XXXXXX)"
_TMPDIRS+=("$I_MOUNT")

# I8-I10b: OUTSIDE-MOUNT — REIFY_WARM_LANE_MOUNT set, LANE_DIR NOT under it.
# Empty target: no rename-to-trash fires, so only the mount check can refuse.
I_OUTSIDE_BASE_PARENT="$(mktemp -d /tmp/test-seed-I-out-parent-XXXXXX)"
I_OUTSIDE_BASE="$I_OUTSIDE_BASE_PARENT/target"
I_OUTSIDE_LANE="$(make_isolated_lane I-out-lane)"
_TMPDIRS+=("$I_OUTSIDE_BASE_PARENT")
mkdir -p "$I_OUTSIDE_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$I_OUTSIDE_BASE_PARENT/.warm-base-meta"
# I_OUTSIDE_LANE is a make_isolated_lane tmpdir, NOT under I_MOUNT (a
# different /tmp tmpdir) — that's the only property this fixture needs.
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_MOUNT="$I_MOUNT" \
    run_helper "$I_OUTSIDE_BASE" "$I_OUTSIDE_LANE" --fresh-checkout

assert "I8: outside-mount: exit 1 (misuse guard refuses)" \
    test "$RC" -ne 0
assert "I9: outside-mount: STDOUT is EMPTY (fail-closed, no path emitted)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "I10: outside-mount: cp NEVER invoked (refused before clone)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"
assert "I10b: outside-mount: stderr names mount/misuse" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "mount|misuse"' _ "$ERR_OUT"

# I11-I12: SELF-CLOBBER — LANE_DIR = dirname(BASE_TARGET_DIR) so LANE_TARGET == BASE_TARGET_DIR.
# Catastrophic: would rename the base target to trash and clone it onto itself.
#
# I_SC_BASE_PARENT is created via make_isolated_lane (task 5609), NOT bare
# /tmp: this is a DIFFERENT hazard than the dirname()/.reseed-trash collision
# that motivated _LANE_ROOT for other lanes (this fixture's misuse guard
# refuses before ever reaching the rename-to-trash path, so R1 never sees
# it). The hazard here is a SIBLING lane-lock: scripts/seed-warm-lane.sh
# computes LANE_LOCK="${LANE_DIR}.lock" (:522) — a sibling of LANE_DIR, not a
# child — and creates it (:563, `exec 9>"$LANE_LOCK"`) BEFORE the
# self-clobber misuse guard refuses (I11), so the lock is created even
# though RC != 0. A bare-/tmp
# I_SC_BASE_PARENT would strand that sibling lock at top-level /tmp, outside
# every _TMPDIRS entry — the sibling is not INSIDE the registered directory,
# so cleanup()'s `rm -rf` never sees it (the leak I12b/I12c above pin).
# Nesting under $_LANE_ROOT via make_isolated_lane makes the sibling lock a
# descendant of $_LANE_ROOT instead, which cleanup() DOES reclaim.
I_SC_BASE_PARENT="$(make_isolated_lane I-sc-parent)"
I_SC_BASE="$I_SC_BASE_PARENT/target"
mkdir -p "$I_SC_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$I_SC_BASE_PARENT/.warm-base-meta"
# LANE_DIR = I_SC_BASE_PARENT → LANE_TARGET = I_SC_BASE_PARENT/target = I_SC_BASE = BASE_TARGET_DIR
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$I_SC_BASE" "$I_SC_BASE_PARENT" --fresh-checkout

assert "I11: self-clobber: exit 1 (misuse guard refuses)" \
    test "$RC" -ne 0
assert "I12: self-clobber: cp NEVER invoked (refused before clone)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# I12b/I12c: pin the sibling lane-lock leak (task 5609). scripts/seed-warm-lane.sh
# unconditionally enters its lane-lock path for --fresh-checkout (:510) and
# computes LANE_LOCK="${LANE_DIR}.lock" — a SIBLING of LANE_DIR, not a child —
# creating it (:563, `exec 9>"$LANE_LOCK"`) BEFORE the self-clobber misuse
# guard above refuses. So the lock exists even though RC != 0 (I12b), and it
# must be nested under $_LANE_ROOT so the existing cleanup() EXIT trap's
# `rm -rf "$_LANE_ROOT"` reclaims it as a child of I_SC_BASE_PARENT's own
# private parent (i.e. a sibling of I_SC_BASE_PARENT itself) (I12c).
assert "I12b: self-clobber: sibling lane-lock \${I_SC_BASE_PARENT}.lock exists (seed-warm-lane.sh:563's exec 9>\"\$LANE_LOCK\" creates it before the misuse guard refuses)" \
    bash -c '[ -n "$1" ] && [ -e "$1.lock" ]' _ "$I_SC_BASE_PARENT"
assert "I12c: self-clobber: the sibling lane-lock is nested under \$_LANE_ROOT (so cleanup()'s rm -rf reclaims it)" \
    bash -c '[ -n "$1" ] || exit 1; case "$1" in "$2"/*) exit 0 ;; *) exit 1 ;; esac' _ "${I_SC_BASE_PARENT}.lock" "${_LANE_ROOT:-/nonexistent}"

# I13: POSITIVE CONTROL UNDER MOUNT — lane IS under REIFY_WARM_LANE_MOUNT, non-empty target.
# The replace path must still succeed when the mount check passes.
# NOT migrated to make_isolated_lane (task 5590): I_UNDER_LANE must stay nested
# under $I_MOUNT because REIFY_WARM_LANE_MOUNT="$I_MOUNT" is set below and the
# mount guard would refuse a lane outside it — a later sweep should not "fix" this.
I_UNDER_LANE="$(mktemp -d "$I_MOUNT/test-seed-I-under-XXXXXX")"
# I_UNDER_LANE is inside I_MOUNT, so the cleanup trap picks it up via I_MOUNT.
mkdir -p "$I_UNDER_LANE/target"
echo "stale artifact" > "$I_UNDER_LANE/target/stale.a"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_MOUNT="$I_MOUNT" \
    run_helper "$I_BASE" "$I_UNDER_LANE" --fresh-checkout

assert "I13: positive-control-under-mount: exit 0 (mount check passes, replace runs)" \
    test "$RC" -eq 0
assert "I13b: positive-control-under-mount: cp IS invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

# ── I14: deterministic structural relocation guard (REIFY_TEST_PIN_RESEED_TRASH=1, no SYNC) ──
# Proves that after --fresh-checkout the trash is at the pool-level sibling (.reseed-trash/),
# NOT under the lane dir. The selective rm stub (REIFY_TEST_PIN_RESEED_TRASH=1) pins the trash
# on disk so the location can be inspected after the seed completes.
# Before fix (in-lane trash, pre-#4896): I14d finds trash under lane → FAILS (RED);
#                                         I14g: sibling dir absent → FAILS (RED).
# After fix (#4896, pool-level sibling):  I14d: no in-lane trash → PASSES (GREEN);
#                                         I14g: trash in sibling → PASSES (GREEN).
#
# I14_LANE is created via make_isolated_lane (task 5590), NOT bare /tmp
# (task 5384; esc-5354-6): the script under test computes RESEED_TRASH_DIR as
# dirname(LANE_DIR)/.reseed-trash, so a bare-/tmp lane would put I14_SIBLING_TRASH_DIR
# at the machine-shared /tmp/.reseed-trash — vulnerable to a flaky I14g on hosts running
# many concurrent agents/test runs. Same pattern I13 already uses for I_MOUNT above.
I14_BASE_PARENT="$(mktemp -d /tmp/test-seed-I14-parent-XXXXXX)"
I14_BASE="$I14_BASE_PARENT/target"
I14_LANE="$(make_isolated_lane I14-lane)"
# Set I14_SIBLING_TRASH_DIR early so the I14g assertion below references the
# same computed path. Because make_isolated_lane's private parent is a fresh
# per-run mktemp'd dir, this sibling trash dir is private to this run and is
# guaranteed not to exist yet, so no pre-clean of stale entries is needed.
I14_SIBLING_TRASH_DIR="$(dirname "$I14_LANE")/.reseed-trash"
_TMPDIRS+=("$I14_BASE_PARENT")
mkdir -p "$I14_BASE/debug"
echo "base artifact" > "$I14_BASE/debug/base_artifact.a"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$I14_BASE_PARENT/.warm-base-meta"
# Source file so find has real work to stamp (confirms find actually ran)
mkdir -p "$I14_LANE/src"
echo "fn main() {}" > "$I14_LANE/src/main.rs"
# Lane target: non-empty so the rename path triggers (seed renames it to trash)
mkdir -p "$I14_LANE/target"
echo "stale" > "$I14_LANE/target/stale.a"
echo "sentinel content" > "$I14_LANE/target/TRASH_SENTINEL.txt"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_TEST_PIN_RESEED_TRASH=1 \
    run_helper_real "$I14_BASE" "$I14_LANE" --fresh-checkout

# I14: seed did not abort
assert "I14: relocation: exit 0 (seed did not abort)" test "$RC" -eq 0
assert "I14b: relocation: STDOUT is <lane>/target" \
    bash -c '[ "$1" = "'"$I14_LANE/target"'" ]' _ "$OUT"

# I14c: base artifact present (clone succeeded)
assert "I14c: relocation: base_artifact.a present in <lane>/target" \
    test -f "$I14_LANE/target/debug/base_artifact.a"

# I14d: NO target.reseed-trash.* directly under the lane dir (trash was relocated).
# Pre-fix (in-lane): find returns the in-lane trash → I14_IN_LANE_TRASH is non-empty → FAILS (RED).
# Post-fix (#4896):  trash at pool-level sibling → find returns nothing → PASSES (GREEN).
I14_IN_LANE_TRASH="$(find "$I14_LANE" -maxdepth 1 -name 'target.reseed-trash.*' -print -quit 2>/dev/null)"
assert "I14d: relocation: NO target.reseed-trash.* directly under lane dir" \
    bash -c '[ -z "$1" ]' _ "$I14_IN_LANE_TRASH"

# I14e: src/main.rs stamped to 2020 (confirms the find walk actually ran)
I14_SRC_MTIME="$(stat -c '%Y' "$I14_LANE/src/main.rs")"
assert "I14e: relocation: src/main.rs stamped to 2020 (find walk ran)" \
    test "$I14_SRC_MTIME" -eq "$EPOCH_2020"

# Wait for the detached trash rm to have provably run (task #5633 code
# review) before checking presence below. The PIN stub outliving the
# invocation (Block T below) makes this resolve via the stub near-instantly
# today, but without this wait a FUTURE regression that reintroduced an
# eager stub-dir teardown would race this check exactly the way it raced
# pre-#5633 -- sometimes observing the trash before the real rm won,
# sometimes after. Waiting first means such a regression instead times out
# here (a real /bin/rm records nothing) with the trash already actually
# gone, so I14g fails for the right, deterministic reason every time instead
# of flaking on timing. `|| true`: a timeout must fall through to the
# assertion below, not abort the suite via set -e.
_wait_until 10 _calls_file_has_trash_rm "$CALLS_FILE" || true

# I14g: trash IS under the pool-level sibling .reseed-trash/ dir for THIS run's lane.
# Pre-fix (in-lane): sibling dir absent → find on non-existent dir → empty → FAILS (RED).
# Post-fix (#4896):  sibling dir has an entry matching THIS lane's name → PASSES (GREEN).
# Scoped to $(basename "$I14_LANE").* (not just -mindepth 1) so a stale entry from a
# *prior* run's different lane cannot produce a false GREEN on a non-pristine machine.
# Safe from the run_helper_real stub-dir race (task #5633, the assertion
# reported flaky) both structurally (the stub dir now outlives the
# invocation, Block T below -- do not "tidy up" that deferred teardown back
# into an eager `rm -rf "$real_stub_dir"`) and causally (the wait above
# orders this check after the detached rm, whichever binary it resolved to,
# has already run).
assert "I14g: relocation: trash IS under pool-level sibling .reseed-trash/ for this lane" \
    bash -c '[ -n "$(find "'"$I14_SIBLING_TRASH_DIR"'" -maxdepth 1 -name "'"$(basename "$I14_LANE")"'.*" -print -quit 2>/dev/null)" ]'

# ── I15: real async large-trash smoke (no SYNC, no rm stub, 200+ files) ─────────────────────
# Smoke test: with trash relocated outside the lane (#4896), no race between the seed's
# find walk and the background rm is possible (trash is structurally invisible to the lane-
# rooted walker).  Confirms exit 0 and correct cloning under real async conditions.
I15_BASE_PARENT="$(mktemp -d /tmp/test-seed-I15-parent-XXXXXX)"
I15_BASE="$I15_BASE_PARENT/target"
# I15_LANE is created via make_isolated_lane (task 5590), NOT bare /tmp (task
# 5384 amendment; same rationale as I14 above): the script under test
# mkdir -p's dirname(LANE_DIR)/.reseed-trash for trash relocation, so a bare-/tmp
# lane would write into the machine-shared /tmp/.reseed-trash.
I15_LANE="$(make_isolated_lane I15-lane)"
_TMPDIRS+=("$I15_BASE_PARENT")
mkdir -p "$I15_BASE/debug"
echo "base artifact" > "$I15_BASE/debug/base_artifact.a"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$I15_BASE_PARENT/.warm-base-meta"
# Source file so find has real work that overlaps with the background rm
mkdir -p "$I15_LANE/src"
echo "fn main() {}" > "$I15_LANE/src/main.rs"
# Large trash tree (210 dirs × 1 file = 210 files) so real rm can overlap find
mkdir -p "$I15_LANE/target"
for _i15 in $(seq 1 210); do
    mkdir -p "$I15_LANE/target/dir_${_i15}"
    echo "content ${_i15}" > "$I15_LANE/target/dir_${_i15}/file_${_i15}.txt"
done

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$I15_BASE" "$I15_LANE" --fresh-checkout

assert "I15: async-large-trash: exit 0 (seed does not abort under concurrent rm)" \
    test "$RC" -eq 0
assert "I15b: async-large-trash: STDOUT is <lane>/target" \
    bash -c '[ "$1" = "'"$I15_LANE/target"'" ]' _ "$OUT"
assert "I15c: async-large-trash: base_artifact.a present in new target" \
    test -f "$I15_LANE/target/debug/base_artifact.a"

# ─────────────────────────────────────────────────────────────────────────────
# Block M — git-clean TOCTOU acceptance (pool-level trash relocation, task #4896)
# ─────────────────────────────────────────────────────────────────────────────
# Acceptance for esc-4892-99: after --fresh-checkout the trash must be INVISIBLE to
# `git clean -xfd -e target` rooted at the lane.
#
# M1 uses a `git clean -n` (dry-run) to check structural visibility deterministically:
#   PRE-FIX (in-lane trash, target.reseed-trash.PID):
#     git clean -n output INCLUDES "Would remove target.reseed-trash.PID/" → non-empty
#     → assertion FAILS (RED) — the walker CAN see the trash (TOCTOU root cause).
#   POST-FIX (#4896, pool-level sibling .reseed-trash/lane.PID):
#     trash is outside the lane-rooted walk → git clean -n output is EMPTY → PASSES (GREEN).
#
# Note: git clean itself tolerates ENOENT (concurrent rm does not make it fail), so a
# pure exit-code check is not a reliable RED-first lever.  The dry-run visibility check
# is the deterministic RED-first guard; M1b confirms the actual clean exits 0 post-fix.
#
# Uses a REAL git repo lane (git init + commit) and REAL git clean.  The seed is
# run via run_helper_real with REIFY_TEST_PIN_RESEED_TRASH=1 (trash pinned on disk).
# No --base-commit is passed → seed makes zero git calls → git stub never invoked.
echo ""
echo "--- Block M: git-clean TOCTOU acceptance (relocation, task #4896) ---"

# M: Build a real git-repo lane in a pool dir so dirname(LANE) == pool dir.
M_POOL="$(mktemp -d /tmp/test-seed-M-pool-XXXXXX)"
M_LANE="$M_POOL/test_lane_M"
_TMPDIRS+=("$M_POOL")
mkdir -p "$M_LANE"
git -C "$M_LANE" init -q
git -C "$M_LANE" config user.email "test@reify.test"
git -C "$M_LANE" config user.name "Test"
printf 'target\n' > "$M_LANE/.gitignore"
git -C "$M_LANE" add .gitignore
git -C "$M_LANE" commit -q -m "init"

# Base: real artifact so the cp stub has something to clone.
M_BASE_PARENT="$(mktemp -d /tmp/test-seed-M-base-XXXXXX)"
M_BASE="$M_BASE_PARENT/target"
_TMPDIRS+=("$M_BASE_PARENT")
mkdir -p "$M_BASE/debug"
echo "base artifact" > "$M_BASE/debug/base_artifact.a"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$M_BASE_PARENT/.warm-base-meta"

# Pre-create a large non-empty target (300+ files) to trigger the rename-to-trash path.
mkdir -p "$M_LANE/target"
for _im in $(seq 1 300); do
    mkdir -p "$M_LANE/target/dir_${_im}"
    echo "content ${_im}" > "$M_LANE/target/dir_${_im}/file_${_im}.txt"
done

# Run seed: REIFY_TEST_PIN_RESEED_TRASH=1 pins the trash on disk (rm stub no-op).
# No --base-commit → zero git calls → git stub never invoked.
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_TEST_PIN_RESEED_TRASH=1 \
    run_helper_real "$M_BASE" "$M_LANE" --fresh-checkout

assert "M0: seed exits 0 (fixture sanity)" test "$RC" -eq 0
assert "M0b: stdout is <lane>/target (fixture sanity)" \
    bash -c '[ "$1" = "'"$M_LANE/target"'" ]' _ "$OUT"

# Wait for the detached trash rm to have provably run (task #5633 code
# review), BEFORE resolving M_TRASH below. Unlike I14g, the assertion below
# only checks that M_TRASH is a non-empty STRING captured once by the finds
# that follow -- it does not re-check the path still exists at assert time --
# so here the ORDER is load-bearing: running the finds before the detached
# rm has resolved could still capture a path a real /bin/rm deletes moments
# later, with nothing downstream to catch it. See the I14g wait above for
# the full #5633 rationale. `|| true`: a timeout must fall through to the
# finds below, not abort the suite via set -e.
_wait_until 10 _calls_file_has_trash_rm "$CALLS_FILE" || true

# Locate the pinned trash dir T (in-lane pre-fix; sibling post-fix).
M_TRASH_IN_LANE="$(find "$M_LANE" -maxdepth 1 -name 'target.reseed-trash.*' -print -quit 2>/dev/null)"
M_TRASH_SIBLING="$(find "$M_POOL/.reseed-trash" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null || true)"
if [ -n "$M_TRASH_IN_LANE" ]; then
    M_TRASH="$M_TRASH_IN_LANE"
elif [ -n "$M_TRASH_SIBLING" ]; then
    M_TRASH="$M_TRASH_SIBLING"
else
    M_TRASH=""
fi
# Safe from the run_helper_real stub-dir race (task #5633) both structurally
# (the stub dir now outlives the invocation, Block T below -- do not "tidy
# up" that deferred teardown back into an eager `rm -rf "$real_stub_dir"`)
# and causally (the wait above orders the finds after the detached rm has
# already run).
assert "M-setup: trash dir was pinned on disk (rm stub worked)" \
    bash -c '[ -n "$1" ]' _ "$M_TRASH"

# M1: structural visibility — git clean -xfdn -e target must list no entries for removal.
# The -e target exclusion mirrors DF's _reset_warm_lane call (preserves the cloned target/).
# PRE-FIX: in-lane trash is visible → output includes "Would remove target.reseed-trash.PID/"
#           → non-empty → assertion FAILS (RED).
# POST-FIX (#4896): sibling trash outside lane → git clean sees nothing to remove → PASSES (GREEN).
M_GIT_CLEAN_DRY="$(git -C "$M_LANE" clean -xfdn -e target 2>&1)"
assert "M1: git clean -xfdn -e target output empty (trash invisible to lane-rooted walker)" \
    bash -c '[ -z "$1" ]' _ "$M_GIT_CLEAN_DRY"

# M1b: actual git clean exits 0 and the fresh target/ survived (-e target preserved it).
# (git clean now has nothing to remove post-fix; target/ is excluded by -e target.)
M_GIT_CLEAN_RC=0
git -C "$M_LANE" clean -xfd -e target >/dev/null 2>&1 || M_GIT_CLEAN_RC=$?
assert "M1b: git clean -xfd -e target exits 0 (nothing left to clean in lane)" \
    test "$M_GIT_CLEAN_RC" -eq 0
assert "M1c: target/debug/base_artifact.a survived git clean (-e target preserved it)" \
    test -f "$M_LANE/target/debug/base_artifact.a"

# ─────────────────────────────────────────────────────────────────────────────
# Block J — authoritative base-commit resolution (esc-3468-75)
# Priority: CLI --base-commit > <base_target_dir>.basecommit (authoritative,
#   refresh-written, gen-bound) > .warm-base-meta BASE_COMMIT (legacy/fallback).
# Uses run_helper (stubbed git so CALLS_FILE records the SHA passed to diff).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block J: authoritative base-commit resolution ---"

# Shared: a lane dir for all J fixtures (git diff is stubbed, so we only need
# the directory to exist; no real git repo needed)
J_LANE="$(make_isolated_lane J-lane)"
# Create a file that the stubbed git diff "reports as changed" so we can assert
# the touch stub was called for it (REIFY_TEST_GIT_DIFF_FILES).
mkdir -p "$J_LANE/src"
echo 'fn main() {}' > "$J_LANE/src/diag.rs"

# ── J1: .basecommit present, no .warm-base-meta BASE_COMMIT, no CLI --base-commit ──
# seed must use .basecommit (shaAUTH) for the git diff call.
J1_BASE_PARENT="$(mktemp -d /tmp/test-seed-J1-parent-XXXXXX)"
J1_BASE="$J1_BASE_PARENT/target"
_TMPDIRS+=("$J1_BASE_PARENT")
mkdir -p "$J1_BASE"
# Sidecar has no BASE_COMMIT entry (only guards)
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$J1_BASE_PARENT/.warm-base-meta"
# Authoritative .basecommit sibling of the resolved gen dir (BASE_TARGET_DIR)
printf 'shaAUTH' > "${J1_BASE}.basecommit"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_TEST_GIT_DIFF_FILES="src/diag.rs" \
    run_helper "$J1_BASE" "$J_LANE" --fresh-checkout
assert "J1: seed with .basecommit exits 0" test "$RC" -eq 0
assert "J1: git diff called with shaAUTH (authoritative .basecommit used)" \
    bash -c 'grep "^git" "$1" | grep -q "diff" | true; grep "^git" "$1" | grep "diff" | grep -q "shaAUTH"' _ "$CALLS_FILE"
assert "J1: touch stub called for diff file (delta-touch ran)" \
    bash -c 'grep "^touch" "$1" | grep -q "src/diag.rs"' _ "$CALLS_FILE"

# ── J2: PRIORITY — .basecommit=shaAUTH beats .warm-base-meta BASE_COMMIT=shaMETA ──
J2_BASE_PARENT="$(mktemp -d /tmp/test-seed-J2-parent-XXXXXX)"
J2_BASE="$J2_BASE_PARENT/target"
J2_LANE="$(make_isolated_lane J2-lane)"
_TMPDIRS+=("$J2_BASE_PARENT")
mkdir -p "$J2_BASE"
# Sidecar records a DIVERGENT BASE_COMMIT=shaMETA (legacy, should lose priority)
printf 'RUSTFLAGS=\nINVOCATION=\nBASE_COMMIT=shaMETA\n' > "$J2_BASE_PARENT/.warm-base-meta"
# Authoritative stamp: shaAUTH (must win)
printf 'shaAUTH' > "${J2_BASE}.basecommit"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$J2_BASE" "$J2_LANE" --fresh-checkout
assert "J2: seed exits 0" test "$RC" -eq 0
assert "J2: git diff used shaAUTH NOT shaMETA (.basecommit beats .warm-base-meta)" \
    bash -c 'grep "^git" "$1" | grep "diff" | grep -q "shaAUTH"' _ "$CALLS_FILE"
assert "J2: git diff did NOT use shaMETA (legacy sidecar ignored)" \
    bash -c '! grep "^git" "$1" | grep "diff" | grep -q "shaMETA"' _ "$CALLS_FILE"

# ── J3: CLI --base-commit shaCLI beats .basecommit (highest priority) ──
J3_BASE_PARENT="$(mktemp -d /tmp/test-seed-J3-parent-XXXXXX)"
J3_BASE="$J3_BASE_PARENT/target"
J3_LANE="$(make_isolated_lane J3-lane)"
_TMPDIRS+=("$J3_BASE_PARENT")
mkdir -p "$J3_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$J3_BASE_PARENT/.warm-base-meta"
# .basecommit present but CLI wins
printf 'shaAUTH' > "${J3_BASE}.basecommit"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$J3_BASE" "$J3_LANE" --fresh-checkout --base-commit shaCLI
assert "J3: seed with --base-commit exits 0" test "$RC" -eq 0
assert "J3: git diff used shaCLI (CLI --base-commit highest priority)" \
    bash -c 'grep "^git" "$1" | grep "diff" | grep -q "shaCLI"' _ "$CALLS_FILE"
assert "J3: git diff did NOT use shaAUTH (CLI beats .basecommit)" \
    bash -c '! grep "^git" "$1" | grep "diff" | grep -q "shaAUTH"' _ "$CALLS_FILE"

# ── J4: legacy fallback — no .basecommit, .warm-base-meta has BASE_COMMIT=shaLEGACY ──
J4_BASE_PARENT="$(mktemp -d /tmp/test-seed-J4-parent-XXXXXX)"
J4_BASE="$J4_BASE_PARENT/target"
J4_LANE="$(make_isolated_lane J4-lane)"
_TMPDIRS+=("$J4_BASE_PARENT")
mkdir -p "$J4_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\nBASE_COMMIT=shaLEGACY\n' > "$J4_BASE_PARENT/.warm-base-meta"
# No .basecommit file: fallback to .warm-base-meta

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$J4_BASE" "$J4_LANE" --fresh-checkout
assert "J4: seed exits 0" test "$RC" -eq 0
assert "J4: git diff used shaLEGACY (legacy .warm-base-meta fallback)" \
    bash -c 'grep "^git" "$1" | grep "diff" | grep -q "shaLEGACY"' _ "$CALLS_FILE"

# ── J5: SYMLINK case — BASE_TARGET_DIR is a symlink to the concrete gen dir ──
# Pins the D8 "caller must resolve" contract: _read_basecommit_stamp looks for
# ${BASE_TARGET_DIR}.basecommit; when BASE_TARGET_DIR is a symlink the stamp
# file is beside the CONCRETE gen dir (not beside the symlink), so it is NOT
# found.  Seed silently falls back to legacy sidecar and emits a diagnosable
# warn — the exact signal that surfaced a mis-wired D8 caller produces.
J5_PARENT="$(mktemp -d /tmp/test-seed-J5-parent-XXXXXX)"
J5_GEN="$J5_PARENT/target.gen.1"
J5_SYMLINK="$J5_PARENT/target"   # symlink → target.gen.1
J5_LANE="$(make_isolated_lane J5-lane)"
_TMPDIRS+=("$J5_PARENT")
mkdir -p "$J5_GEN"
ln -s target.gen.1 "$J5_SYMLINK"
# Authoritative stamp is on the CONCRETE gen sibling — NOT reachable via symlink
printf 'shaAUTH' > "${J5_GEN}.basecommit"
# Legacy sidecar beside the symlink's parent (dirname J5_SYMLINK = J5_PARENT)
printf 'RUSTFLAGS=\nINVOCATION=\nBASE_COMMIT=shaLEGACY\n' > "$J5_PARENT/.warm-base-meta"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$J5_SYMLINK" "$J5_LANE" --fresh-checkout
assert "J5: seed exits 0 (degrades to legacy fallback when symlink passed)" \
    test "$RC" -eq 0
assert "J5: git diff used shaLEGACY (legacy sidecar, .basecommit not found via symlink)" \
    bash -c 'grep "^git" "$1" | grep "diff" | grep -q "shaLEGACY"' _ "$CALLS_FILE"
assert "J5: git diff did NOT use shaAUTH (authoritative stamp unreachable via symlink)" \
    bash -c '! grep "^git" "$1" | grep "diff" | grep -q "shaAUTH"' _ "$CALLS_FILE"
assert "J5: warn emitted noting authoritative stamp absent (D8 mis-wiring diagnosable)" \
    bash -c 'printf "%s\n" "$1" | grep -q "authoritative stamp absent"' _ "$ERR_OUT"

# ─────────────────────────────────────────────────────────────────────────────
# Block K — fail-closed delta-touch (esc-3468-75)
# A git diff non-zero exit must abort the seed (exit NON-ZERO, STDOUT EMPTY).
# An empty diff output (zero changed files) is a legitimate zero-change result
# and must succeed (exit 0).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block K: fail-closed delta-touch ---"

# Shared fixture: a base dir + lane for all K cases
K_BASE_PARENT="$(mktemp -d /tmp/test-seed-K-parent-XXXXXX)"
K_BASE="$K_BASE_PARENT/target"
_TMPDIRS+=("$K_BASE_PARENT")
mkdir -p "$K_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$K_BASE_PARENT/.warm-base-meta"

# ── K1: git diff fails → seed exits NON-ZERO, STDOUT EMPTY (fail-closed) ──
K1_LANE="$(make_isolated_lane K1-lane)"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_TEST_GIT_DIFF_FAIL=1 \
    run_helper "$K_BASE" "$K1_LANE" --fresh-checkout --base-commit shaX
assert "K1: git-diff failure causes seed to exit non-zero (fail-closed)" \
    test "$RC" -ne 0
assert "K1: STDOUT is EMPTY on git-diff failure (no path emitted)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "K1: stderr names the git-diff failure" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "git.?diff|diff.*fail"' _ "$ERR_OUT"

# ── K2: empty diff output (zero changed files) exits 0 (not an error) ──
# REIFY_TEST_GIT_DIFF_FILES="" + REIFY_TEST_GIT_DIFF_FAIL unset → diff returns ""
K2_LANE="$(make_isolated_lane K2-lane)"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_TEST_GIT_DIFF_FILES="" \
    run_helper "$K_BASE" "$K2_LANE" --fresh-checkout --base-commit shaX
assert "K2: empty diff output exits 0 (zero-change seed is not a failure)" \
    test "$RC" -eq 0
assert "K2: STDOUT is <lane>/target on empty-diff success" \
    bash -c '[ "$1" = "'"$K2_LANE/target"'" ]' _ "$OUT"

# ─────────────────────────────────────────────────────────────────────────────
# Block L — seed-time stale-stamp post-condition (_assert_no_stale_delta_stamp)
# After the delta-touch, no file listed by git diff --name-only that exists in
# the lane may still carry the 2020-01-01 bulk-stamp epoch.
#
# L1: RED via run_helper (stubbed touch no-ops) — a lane file pre-stamped to
#     2020 stays at 2020 after the no-op touch → post-condition fires → exit
#     NON-ZERO, STDOUT EMPTY.
# L2: GREEN control via run_helper_real (real touch) — same setup but real
#     touch re-stamps the file to now → post-condition passes → exit 0.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block L: seed-time stale-stamp post-condition ---"

# Compute the bulk-stamp epoch as the post-condition does (TZ-robust)
STALE_EPOCH="$(date -d '2020-01-01T00:00:00' +%s)"

# Shared base fixture
L_BASE_PARENT="$(mktemp -d /tmp/test-seed-L-parent-XXXXXX)"
L_BASE="$L_BASE_PARENT/target"
_TMPDIRS+=("$L_BASE_PARENT")
mkdir -p "$L_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$L_BASE_PARENT/.warm-base-meta"

# ── L1: stubbed run_helper — touch no-ops → file stays at 2020 → RED ──────
L1_LANE="$(make_isolated_lane L1-lane)"
mkdir -p "$L1_LANE/src"
# Pre-stamp diagnostics.rs to 2020-01-01T00:00:00 (the bulk-stamp epoch)
touch -d "2020-01-01T00:00:00" "$L1_LANE/src/diagnostics.rs"
L1_MTIME="$(stat -c '%Y' "$L1_LANE/src/diagnostics.rs")"
# Confirm the pre-stamp matches the expected epoch
assert "L1: pre-stamp: diagnostics.rs mtime == stale epoch (test fixture check)" \
    test "$L1_MTIME" -eq "$STALE_EPOCH"

# git diff reports diagnostics.rs as changed; stubbed touch no-ops so it stays at 2020
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    REIFY_TEST_GIT_DIFF_FILES="src/diagnostics.rs" \
    run_helper "$L_BASE" "$L1_LANE" --fresh-checkout --base-commit shaX
assert "L1: stubbed-touch seed exits NON-ZERO (stale-stamp post-condition fired)" \
    test "$RC" -ne 0
assert "L1: STDOUT is EMPTY (fail-closed, no path emitted)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "L1: stderr names the violating stale path" \
    bash -c 'printf "%s\n" "$1" | grep -q "diagnostics.rs"' _ "$ERR_OUT"

# ── L2: run_helper_real — real touch re-stamps → post-condition passes → GREEN ──
L2_LANE="$(make_isolated_lane L2-lane)"
# Seed base with a real file so run_helper_real's /bin/cp -a propagates something
mkdir -p "$L_BASE/debug"
echo "artifact" > "$L_BASE/debug/artifact.a"
mkdir -p "$L2_LANE/src"
# Pre-stamp to 2020 (real touch will bring it to now during delta-touch)
touch -d "2020-01-01T00:00:00" "$L2_LANE/src/diagnostics.rs"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    REIFY_TEST_GIT_DIFF_FILES="src/diagnostics.rs" \
    run_helper_real "$L_BASE" "$L2_LANE" --fresh-checkout --base-commit shaX
assert "L2: real-touch seed exits 0 (post-condition passes after real touch)" \
    test "$RC" -eq 0
assert "L2: STDOUT is <lane>/target (success contract)" \
    bash -c '[ "$1" = "'"$L2_LANE/target"'" ]' _ "$OUT"
# Verify diagnostics.rs was actually re-stamped to now (> stale epoch)
L2_MTIME="$(stat -c '%Y' "$L2_LANE/src/diagnostics.rs")"
assert "L2: diagnostics.rs mtime > stale epoch after real touch (not 2020 anymore)" \
    test "$L2_MTIME" -gt "$STALE_EPOCH"

# ─────────────────────────────────────────────────────────────────────────────
# Block N — env!()-baked-path test relink on buildroot mismatch (task 4983)
# Uses run_helper_real (real find/touch; stub cp/git) + the Block-D EPOCH_2020
# fixture pattern.  Detects env!() path-baking macros (CARGO_MANIFEST_DIR et al)
# in seeded tests/ and benches/ sources and relinks (touches to now) ONLY those
# when the recorded build-worktree (<base>.buildroot) differs from (or is absent
# for) the consuming lane — so cargo recompiles+relinks just the affected test/
# bench binaries, re-baking the lane's own CARGO_MANIFEST_DIR (esc-4906-57).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block N: env!()-baked-path test relink on buildroot mismatch ---"

# N: DIFFER fixture — recorded .buildroot points at a DIFFERENT worktree than
# the consuming lane, so the relink must fire.
N_BASE_PARENT="$(mktemp -d /tmp/test-seed-N-parent-XXXXXX)"
N_BASE="$N_BASE_PARENT/target"
N_LANE="$(make_isolated_lane N-lane)"
_TMPDIRS+=("$N_BASE_PARENT")
mkdir -p "$N_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$N_BASE_PARENT/.warm-base-meta"
# Recorded build-worktree is a DIFFERENT (nonexistent) path — the differ branch.
printf '%s' "/nonexistent/other-build-worktree" > "${N_BASE}.buildroot"

# Fixture sources: an env!()-bearing tests/ file (must relink), a plain tests/
# file with no macro (must stay untouched — macro-only scope), and a src/ file
# WITH the macro (must stay untouched — no lib rlib cascade).
mkdir -p "$N_LANE/crates/foo/tests" "$N_LANE/crates/foo/src"
cat > "$N_LANE/crates/foo/tests/env_probe.rs" <<'RS_EOF'
#[test]
fn probe() {
    let _ = env!("CARGO_MANIFEST_DIR");
}
RS_EOF
cat > "$N_LANE/crates/foo/tests/plain.rs" <<'RS_EOF'
#[test]
fn plain() {
    assert_eq!(1 + 1, 2);
}
RS_EOF
cat > "$N_LANE/crates/foo/src/lib.rs" <<'RS_EOF'
pub const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
RS_EOF
mkdir -p "$N_LANE/target" "$N_LANE/.git"

# Snapshot byte content of env_probe.rs BEFORE the seed (N5 asserts mtime-only
# touch — no content diff, so git stays clean).
N_SNAPSHOT="$(mktemp /tmp/test-seed-N-snapshot-XXXXXX)"
_TMPDIRS+=("$N_SNAPSHOT")
cp "$N_LANE/crates/foo/tests/env_probe.rs" "$N_SNAPSHOT"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$N_BASE" "$N_LANE" --fresh-checkout

# N1: exit 0
assert "N1: differ-buildroot seed exits 0" test "$RC" -eq 0

# N2: env!()-bearing tests/ file was RELINKED (mtime > 2020 epoch)
N2_MTIME="$(stat -c '%Y' "$N_LANE/crates/foo/tests/env_probe.rs")"
assert "N2: tests/env_probe.rs mtime > 2020 epoch (relinked on buildroot differ)" \
    test "$N2_MTIME" -gt "$EPOCH_2020"

# N3: plain tests/ file (no macro) stays at the bulk-stamp epoch (scope: macro-only)
N3_MTIME="$(stat -c '%Y' "$N_LANE/crates/foo/tests/plain.rs")"
assert "N3: tests/plain.rs mtime == 2020 epoch (no macro -> untouched)" \
    test "$N3_MTIME" -eq "$EPOCH_2020"

# N4: src/ file WITH the macro stays at the bulk-stamp epoch (scope: tests/+benches/ only)
N4_MTIME="$(stat -c '%Y' "$N_LANE/crates/foo/src/lib.rs")"
assert "N4: src/lib.rs mtime == 2020 epoch (src/ pruned -> no lib rlib cascade)" \
    test "$N4_MTIME" -eq "$EPOCH_2020"

# N5: byte content of the relinked file is IDENTICAL (mtime-only touch, git stays clean)
assert "N5: tests/env_probe.rs byte content unchanged (mtime-only touch)" \
    cmp -s "$N_SNAPSHOT" "$N_LANE/crates/foo/tests/env_probe.rs"

# N: EQUAL fixture — recorded .buildroot content == realpath(consuming lane),
# so the relink must be SKIPPED (base was already built under this lane's own
# worktree path — re-baking would be a no-op).
N_EQ_BASE_PARENT="$(mktemp -d /tmp/test-seed-N-eq-parent-XXXXXX)"
N_EQ_BASE="$N_EQ_BASE_PARENT/target"
N_EQ_LANE="$(make_isolated_lane N-eq-lane)"
_TMPDIRS+=("$N_EQ_BASE_PARENT")
mkdir -p "$N_EQ_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$N_EQ_BASE_PARENT/.warm-base-meta"
# Recorded build-worktree EQUALS the consuming lane's own realpath.
printf '%s' "$(realpath -m "$N_EQ_LANE")" > "${N_EQ_BASE}.buildroot"

mkdir -p "$N_EQ_LANE/crates/foo/tests"
cat > "$N_EQ_LANE/crates/foo/tests/env_probe.rs" <<'RS_EOF'
#[test]
fn probe() {
    let _ = env!("CARGO_MANIFEST_DIR");
}
RS_EOF
mkdir -p "$N_EQ_LANE/target" "$N_EQ_LANE/.git"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$N_EQ_BASE" "$N_EQ_LANE" --fresh-checkout

# N6: exit 0
assert "N6: equal-buildroot seed exits 0" test "$RC" -eq 0

# N7: env!()-bearing tests/ file stays at 2020 epoch (skip branch — recorded
# buildroot equals the consuming lane, so relink is skipped).
N7_MTIME="$(stat -c '%Y' "$N_EQ_LANE/crates/foo/tests/env_probe.rs")"
assert "N7: tests/env_probe.rs mtime == 2020 epoch (buildroot equal -> skip)" \
    test "$N7_MTIME" -eq "$EPOCH_2020"

# N: ABSENT fixture — NO .buildroot sidecar present at all (pre-stamp / initial
# base). Fail-safe: relink still fires — uncertain provenance is treated the
# same as a confirmed mismatch.
N_ABS_BASE_PARENT="$(mktemp -d /tmp/test-seed-N-abs-parent-XXXXXX)"
N_ABS_BASE="$N_ABS_BASE_PARENT/target"
N_ABS_LANE="$(make_isolated_lane N-abs-lane)"
_TMPDIRS+=("$N_ABS_BASE_PARENT")
mkdir -p "$N_ABS_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$N_ABS_BASE_PARENT/.warm-base-meta"
# No ${N_ABS_BASE}.buildroot written.

mkdir -p "$N_ABS_LANE/crates/foo/tests"
cat > "$N_ABS_LANE/crates/foo/tests/env_probe.rs" <<'RS_EOF'
#[test]
fn probe() {
    let _ = env!("CARGO_MANIFEST_DIR");
}
RS_EOF
mkdir -p "$N_ABS_LANE/target" "$N_ABS_LANE/.git"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$N_ABS_BASE" "$N_ABS_LANE" --fresh-checkout

# N8: exit 0
assert "N8: absent-buildroot seed exits 0" test "$RC" -eq 0

# N9: env!()-bearing tests/ file IS relinked (mtime > 2020 epoch) even though no
# .buildroot sidecar exists — absent-stamp fail-safe still relinks.
N9_MTIME="$(stat -c '%Y' "$N_ABS_LANE/crates/foo/tests/env_probe.rs")"
assert "N9: tests/env_probe.rs mtime > 2020 epoch (buildroot absent -> fail-safe relink)" \
    test "$N9_MTIME" -gt "$EPOCH_2020"

# ─────────────────────────────────────────────────────────────────────────────
# Block O — base-absent vs reflink-unsupported discrimination (task 4989,
# esc-triaged 2026-07-03)
#
# The 2026-07-03 outage's real failure was a MISSING CoW base (absent dir /
# removed-or-dangling base symlink) misdiagnosed as a reflink-capability fault
# because the clone block printed the same "does not support reflinks" message
# for ANY non-zero cp exit. A missing base must instead fail fast, before cp is
# ever invoked, with a distinct exit code (76) and an accurate message — while a
# base that IS present but whose cp genuinely fails a reflink check must still
# hit the original "does not support reflinks" message + exit 1 (Block C's C4).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block O: base-absent vs reflink-unsupported discrimination (task 4989) ---"

# ── O1-O5: base target dir ABSENT (never created) ────────────────────────────
O_BASE_PARENT="$(mktemp -d /tmp/test-seed-O-parent-XXXXXX)"
O_BASE="$O_BASE_PARENT/target"     # NEVER created — absent base
O_LANE="$(make_isolated_lane O-lane)"
_TMPDIRS+=("$O_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$O_BASE_PARENT/.warm-base-meta"

reset_calls
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
    run_helper "$O_BASE" "$O_LANE" --fresh-checkout

assert "O1: base-absent exits 76 (distinct code)" test "$RC" -eq 76
assert "O2a: base-absent: stderr says base is missing" \
    bash -c 'printf "%s\n" "$1" | grep -qi "base is missing"' _ "$ERR_OUT"
assert "O2b: base-absent: stderr says NOT a reflink-capability fault" \
    bash -c 'printf "%s\n" "$1" | grep -qi "NOT a reflink-capability fault"' _ "$ERR_OUT"
assert "O3: base-absent: stderr does NOT contain the reflink-unsupported message" \
    bash -c '! printf "%s\n" "$1" | grep -qi "does not support reflinks"' _ "$ERR_OUT"
assert "O4: base-absent: STDOUT is EMPTY (fail-closed)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "O5: base-absent: cp NEVER invoked (guard fires before clone)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# ── O6: base target dir is a BROKEN/DANGLING SYMLINK (removed-base scenario) ──
O_BASE2_PARENT="$(mktemp -d /tmp/test-seed-O2-parent-XXXXXX)"
O_BASE2="$O_BASE2_PARENT/target"
O_LANE2="$(make_isolated_lane O-lane2)"
_TMPDIRS+=("$O_BASE2_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$O_BASE2_PARENT/.warm-base-meta"
ln -s /nonexistent/xyz "$O_BASE2"

reset_calls
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
    run_helper "$O_BASE2" "$O_LANE2" --fresh-checkout
assert "O6: broken-symlink base exits 76 (same discrimination as absent dir)" \
    test "$RC" -eq 76

# ── O7: discrimination lock — base PRESENT + genuine reflink failure → exit 1 (NOT 76) ──
# Mirrors Block C's C4. Pins that a base which EXISTS but whose cp genuinely
# fails the reflink capability check still yields the ORIGINAL "does not
# support reflinks" message + exit 1 — the new guard must not shadow this path.
O_BASE3_PARENT="$(mktemp -d /tmp/test-seed-O3-parent-XXXXXX)"
O_BASE3="$O_BASE3_PARENT/target"
O_LANE3="$(make_isolated_lane O-lane3)"
_TMPDIRS+=("$O_BASE3_PARENT")
mkdir -p "$O_BASE3"     # base PRESENT this time
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$O_BASE3_PARENT/.warm-base-meta"

reset_calls
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" REIFY_TEST_REFLINK_OK=0 \
    run_helper "$O_BASE3" "$O_LANE3" --fresh-checkout
assert "O7a: base-present + reflink-fail: exits 1 (not 76 — discrimination lock)" \
    test "$RC" -eq 1
assert "O7b: base-present + reflink-fail: RC is NOT 76" \
    test "$RC" -ne 76
assert "O7c: base-present + reflink-fail: stderr still names reflink failure (path unchanged)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "reflink|Operation not supported"' _ "$ERR_OUT"

# ── O8-O12: full-teardown ordering lock (base parent + sidecar BOTH absent,
# non-empty env RUSTFLAGS/invocation) ────────────────────────────────────────
# The base-absent guard MUST run before the sidecar read / RUSTFLAGS / invocation
# guards. If it ran after, a fully-torn-down base parent (sidecar gone too, not
# just the target dir) would make _sidecar_read default both recorded values to
# "" — and under a typical NON-EMPTY env RUSTFLAGS/invocation, the RUSTFLAGS (or
# invocation) guard would fire first with a misleading mismatch message instead
# of the true "base is missing" cause, exactly the wrong signal for DF's
# BASE_ABSENT discriminant.
O_GONE_ROOT="$(mktemp -d /tmp/test-seed-O-gone-XXXXXX)"
O_GONE_PARENT="$O_GONE_ROOT/base-parent-never-created"    # parent (+ sidecar) never created
O_GONE_BASE="$O_GONE_PARENT/target"                        # never created
O_GONE_LANE="$(make_isolated_lane O-gone-lane)"
_TMPDIRS+=("$O_GONE_ROOT")
# Deliberately no .warm-base-meta sidecar anywhere — simulates the base's entire
# parent directory having been torn down, not just the target dir.

reset_calls
RUSTFLAGS="-C target-cpu=native" REIFY_WARM_LANE_INVOCATION="some-fingerprint" \
    run_helper "$O_GONE_BASE" "$O_GONE_LANE" --fresh-checkout

assert "O8: full-teardown + non-empty env RUSTFLAGS/invocation still exits 76 (not 1)" \
    test "$RC" -eq 76
assert "O9: full-teardown: stderr says base is missing" \
    bash -c 'printf "%s\n" "$1" | grep -qi "base is missing"' _ "$ERR_OUT"
assert "O10: full-teardown: stderr does NOT contain the RUSTFLAGS-mismatch message" \
    bash -c '! printf "%s\n" "$1" | grep -qi "RUSTFLAGS mismatch"' _ "$ERR_OUT"
assert "O11: full-teardown: stderr does NOT contain the Invocation-mismatch message" \
    bash -c '! printf "%s\n" "$1" | grep -qi "Invocation mismatch"' _ "$ERR_OUT"
assert "O12: full-teardown: cp NEVER invoked (guard fires before clone)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# ─────────────────────────────────────────────────────────────────────────────
# Block P — non-relocatable links-metadata/OUT_DIR path relocation (esc-5052)
# Uses run_helper_real (real cp/find/touch + stub git), mirroring Block H.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block P: non-relocatable links-metadata path relocation (esc-5052) ---"

# P: DIFFER fixture — recorded .buildroot points at a FOREIGN worktree root that
# is baked into build-script `output` files (links-metadata absolute paths
# consumed as file paths by DEPENDENT build scripts, e.g. cxx's CXXBRIDGE_DIR0
# and a sys-crate's lib_dir). Both cxx-* and reify-kernel-occt-* are OUTSIDE the
# tauri-*/reify-gui-* allow-list (Block H), so the existing deletion sweep never
# touches them — relocation must rewrite the baked prefix in place instead.
P_FOREIGN="/tmp/foreign-wt"

P_BASE_PARENT="$(mktemp -d /tmp/test-seed-P-parent-XXXXXX)"
P_BASE="$P_BASE_PARENT/target"
P_LANE="$(make_isolated_lane P-lane)"
_TMPDIRS+=("$P_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$P_BASE_PARENT/.warm-base-meta"
printf '%s' "$P_FOREIGN" > "${P_BASE}.buildroot"

mkdir -p "$P_BASE/debug/build/cxx-AAAA" "$P_BASE/debug/build/reify-kernel-occt-BBBB"
cat > "$P_BASE/debug/build/cxx-AAAA/output" <<EOF
cargo:CXXBRIDGE_DIR0=$P_FOREIGN/target/debug/build/reify-kernel-occt-BBBB/out/cxxbridge/include
EOF
cat > "$P_BASE/debug/build/reify-kernel-occt-BBBB/output" <<EOF
cargo:lib_dir=$P_FOREIGN/target/debug/build/reify-kernel-occt-BBBB/out/lib
EOF

# root-output records the build script's OUT_DIR (replayed to set OUT_DIR for
# the crate compile) — a distinct ENOENT class from `output`'s links-metadata:
# include!(concat!(env!("OUT_DIR"), ...)) opens the FOREIGN out dir baked here.
mkdir -p "$P_BASE/debug/build/ahash-CCCC"
printf '%s' "$P_FOREIGN/target/debug/build/ahash-CCCC/out" > "$P_BASE/debug/build/ahash-CCCC/root-output"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$P_BASE" "$P_LANE" --fresh-checkout

P_LANE_RP="$(realpath -m "$P_LANE")"

# P1: success contract — exit 0, stdout == <lane>/target
assert "P1: --fresh-checkout exits 0 (links-metadata relocation)" test "$RC" -eq 0
assert "P1: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "'"$P_LANE/target"'" ]' _ "$OUT"

# P1a/b: cxx-AAAA/output rewritten to the lane root; no FOREIGN substring left
assert "P1a: build/cxx-AAAA/output contains the LANE root prefix" \
    bash -c 'grep -qF "$1" "$2"' _ "$P_LANE_RP" "$P_LANE/target/debug/build/cxx-AAAA/output"
assert "P1b: build/cxx-AAAA/output no longer contains the FOREIGN root" \
    bash -c '! grep -qF "$1" "$2"' _ "$P_FOREIGN" "$P_LANE/target/debug/build/cxx-AAAA/output"

# P1c/d: reify-kernel-occt-BBBB/output rewritten to the lane root; no FOREIGN left
assert "P1c: build/reify-kernel-occt-BBBB/output contains the LANE root prefix" \
    bash -c 'grep -qF "$1" "$2"' _ "$P_LANE_RP" "$P_LANE/target/debug/build/reify-kernel-occt-BBBB/output"
assert "P1d: build/reify-kernel-occt-BBBB/output no longer contains the FOREIGN root" \
    bash -c '! grep -qF "$1" "$2"' _ "$P_FOREIGN" "$P_LANE/target/debug/build/reify-kernel-occt-BBBB/output"

# P2: build/ahash-CCCC/root-output (OUT_DIR class) rewritten to the exact lane
# path, byte-for-byte — this file's ENTIRE content is the OUT_DIR value cargo
# replays verbatim, so the assertion pins the full expected string, not just
# a substring.
assert "P2: build/ahash-CCCC/root-output reads the lane's OUT_DIR (no FOREIGN substring)" \
    bash -c '[ "$(cat "$1")" = "'"$P_LANE_RP"'/target/debug/build/ahash-CCCC/out" ]' \
    _ "$P_LANE/target/debug/build/ahash-CCCC/root-output"

# P-abs: buildroot stamp ABSENT. Unlike the env!()-relink (which fails safe by
# relinking even when the stamp is absent — a cheap, idempotent mtime touch),
# relocation is a content rewrite: an unguarded/empty search prefix would match
# every byte and corrupt the file, so the fail-safe direction is inverted here
# — skip, and say so on stderr, rather than guess.
P_ABS_BASE_PARENT="$(mktemp -d /tmp/test-seed-P-abs-parent-XXXXXX)"
P_ABS_BASE="$P_ABS_BASE_PARENT/target"
P_ABS_LANE="$(make_isolated_lane P-abs-lane)"
_TMPDIRS+=("$P_ABS_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$P_ABS_BASE_PARENT/.warm-base-meta"
# Deliberately no ${P_ABS_BASE}.buildroot written.

mkdir -p "$P_ABS_BASE/debug/build/cxx-AAAA"
cat > "$P_ABS_BASE/debug/build/cxx-AAAA/output" <<EOF
cargo:CXXBRIDGE_DIR0=$P_FOREIGN/target/debug/build/cxx-AAAA/out/cxxbridge/include
EOF

P_ABS_SNAPSHOT="$(mktemp /tmp/test-seed-P-abs-snapshot-XXXXXX)"
_TMPDIRS+=("$P_ABS_SNAPSHOT")
cp "$P_ABS_BASE/debug/build/cxx-AAAA/output" "$P_ABS_SNAPSHOT"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$P_ABS_BASE" "$P_ABS_LANE" --fresh-checkout

assert "P-abs: absent-buildroot seed exits 0" test "$RC" -eq 0
assert "P-abs: output file BYTE-UNCHANGED (no corruption from an empty-prefix match)" \
    cmp -s "$P_ABS_SNAPSHOT" "$P_ABS_LANE/target/debug/build/cxx-AAAA/output"
assert "P-abs: stderr names the absent buildroot stamp (actionable warn)" \
    bash -c 'printf "%s\n" "$1" | grep -qi "buildroot stamp absent"' _ "$ERR_OUT"

# P-eq: buildroot EQUALS the lane (genuine in-place build) — the baked path
# already reflects the lane's own root, so relocation must be a pure no-op.
P_EQ_BASE_PARENT="$(mktemp -d /tmp/test-seed-P-eq-parent-XXXXXX)"
P_EQ_BASE="$P_EQ_BASE_PARENT/target"
P_EQ_LANE="$(make_isolated_lane P-eq-lane)"
_TMPDIRS+=("$P_EQ_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$P_EQ_BASE_PARENT/.warm-base-meta"
P_EQ_LANE_RP="$(realpath -m "$P_EQ_LANE")"
printf '%s' "$P_EQ_LANE_RP" > "${P_EQ_BASE}.buildroot"

mkdir -p "$P_EQ_BASE/debug/build/cxx-AAAA"
cat > "$P_EQ_BASE/debug/build/cxx-AAAA/output" <<EOF
cargo:CXXBRIDGE_DIR0=$P_EQ_LANE_RP/target/debug/build/cxx-AAAA/out/cxxbridge/include
EOF

P_EQ_SNAPSHOT="$(mktemp /tmp/test-seed-P-eq-snapshot-XXXXXX)"
_TMPDIRS+=("$P_EQ_SNAPSHOT")
cp "$P_EQ_BASE/debug/build/cxx-AAAA/output" "$P_EQ_SNAPSHOT"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$P_EQ_BASE" "$P_EQ_LANE" --fresh-checkout

assert "P-eq: equal-buildroot seed exits 0" test "$RC" -eq 0
assert "P-eq: output file unchanged (no-op; buildroot equals lane)" \
    cmp -s "$P_EQ_SNAPSHOT" "$P_EQ_LANE/target/debug/build/cxx-AAAA/output"
assert "P-eq: no relocation count > 0 reported on stderr" \
    bash -c '! printf "%s\n" "$1" | grep -qE "Relocated [1-9][0-9]* "' _ "$ERR_OUT"

# P-mismatch: buildroot DIFFERS from the lane (relocation is applicable per the
# gate) but the baked `output` file's actual bytes do NOT carry the recorded
# buildroot's prefix — simulating a canonicalization drift between the
# .buildroot stamp and what actually got baked (e.g. a symlinked vs. realpath
# worktree root at build time). grep -qF then matches nothing on the only
# candidate; the fix must warn instead of silently reporting a bare
# "Relocated 0" so the drift is visible rather than reading as success.
P_MISMATCH_BASE_PARENT="$(mktemp -d /tmp/test-seed-P-mismatch-parent-XXXXXX)"
P_MISMATCH_BASE="$P_MISMATCH_BASE_PARENT/target"
P_MISMATCH_LANE="$(make_isolated_lane P-mismatch-lane)"
_TMPDIRS+=("$P_MISMATCH_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$P_MISMATCH_BASE_PARENT/.warm-base-meta"
printf '%s' "$P_FOREIGN" > "${P_MISMATCH_BASE}.buildroot"

mkdir -p "$P_MISMATCH_BASE/debug/build/cxx-AAAA"
cat > "$P_MISMATCH_BASE/debug/build/cxx-AAAA/output" <<EOF
cargo:CXXBRIDGE_DIR0=/tmp/some-other-uncanonicalized-wt/target/debug/build/cxx-AAAA/out/cxxbridge/include
EOF

P_MISMATCH_SNAPSHOT="$(mktemp /tmp/test-seed-P-mismatch-snapshot-XXXXXX)"
_TMPDIRS+=("$P_MISMATCH_SNAPSHOT")
cp "$P_MISMATCH_BASE/debug/build/cxx-AAAA/output" "$P_MISMATCH_SNAPSHOT"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$P_MISMATCH_BASE" "$P_MISMATCH_LANE" --fresh-checkout

assert "P-mismatch: differing-buildroot-but-no-candidate-match seed exits 0" test "$RC" -eq 0
assert "P-mismatch: output file BYTE-UNCHANGED (no candidate matched the recorded prefix)" \
    cmp -s "$P_MISMATCH_SNAPSHOT" "$P_MISMATCH_LANE/target/debug/build/cxx-AAAA/output"
assert "P-mismatch: stderr warns of the canonicalization-drift possibility (not a bare Relocated-0)" \
    bash -c 'printf "%s\n" "$1" | grep -qi "canonicali"' _ "$ERR_OUT"

# P3a: --reset-in-place does NOT relocate (scope guard, mirrors Block H's H3a).
# The relocation sweep must live entirely inside `if [ -n "$FRESH_CHECKOUT" ]`.
P3A_BASE_PARENT="$(mktemp -d /tmp/test-seed-P3a-parent-XXXXXX)"
P3A_BASE="$P3A_BASE_PARENT/target"
P3A_LANE="$(make_isolated_lane P3a-lane)"
_TMPDIRS+=("$P3A_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$P3A_BASE_PARENT/.warm-base-meta"
printf '%s' "$P_FOREIGN" > "${P3A_BASE}.buildroot"

mkdir -p "$P3A_BASE/debug/build/cxx-AAAA"
cat > "$P3A_BASE/debug/build/cxx-AAAA/output" <<EOF
cargo:CXXBRIDGE_DIR0=$P_FOREIGN/target/debug/build/cxx-AAAA/out/cxxbridge/include
EOF

P3A_SNAPSHOT="$(mktemp /tmp/test-seed-P3a-snapshot-XXXXXX)"
_TMPDIRS+=("$P3A_SNAPSHOT")
cp "$P3A_BASE/debug/build/cxx-AAAA/output" "$P3A_SNAPSHOT"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$P3A_BASE" "$P3A_LANE" --reset-in-place

assert "P3a: --reset-in-place exits 0" test "$RC" -eq 0
assert "P3a: build/cxx-AAAA/output UNCHANGED under --reset-in-place (scope guard)" \
    cmp -s "$P3A_SNAPSHOT" "$P3A_LANE/target/debug/build/cxx-AAAA/output"

# P3b/c: warmth + safety guards, same --fresh-checkout fixture.
#   P3b — a sibling compiled artifact (out/libfoo.a) and the build dir itself
#         are NOT deleted by relocation (relocation only rewrites file content).
#   P3c — only files NAMED output/root-output are rewritten: a `.d` depfile and
#         a binary stub that ALSO contain the FOREIGN byte-string are left
#         BYTE-UNCHANGED (filename-scoped match, not a content scan).
P3BC_BASE_PARENT="$(mktemp -d /tmp/test-seed-P3bc-parent-XXXXXX)"
P3BC_BASE="$P3BC_BASE_PARENT/target"
P3BC_LANE="$(make_isolated_lane P3bc-lane)"
P3BC_D_SNAPSHOT="$(mktemp /tmp/test-seed-P3bc-d-snapshot-XXXXXX)"
P3BC_A_SNAPSHOT="$(mktemp /tmp/test-seed-P3bc-a-snapshot-XXXXXX)"
_TMPDIRS+=("$P3BC_BASE_PARENT" "$P3BC_D_SNAPSHOT" "$P3BC_A_SNAPSHOT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$P3BC_BASE_PARENT/.warm-base-meta"
printf '%s' "$P_FOREIGN" > "${P3BC_BASE}.buildroot"

mkdir -p "$P3BC_BASE/debug/build/cxx-AAAA/out"
cat > "$P3BC_BASE/debug/build/cxx-AAAA/output" <<EOF
cargo:CXXBRIDGE_DIR0=$P_FOREIGN/target/debug/build/cxx-AAAA/out/cxxbridge/include
EOF
cat > "$P3BC_BASE/debug/build/cxx-AAAA/build_script_build.d" <<EOF
$P_FOREIGN/target/debug/build/cxx-AAAA/build_script_build: $P_FOREIGN/crates/cxx/build.rs
EOF
printf 'ELF-stub-bytes %s\n' "$P_FOREIGN/target/debug/build/cxx-AAAA/out" \
    > "$P3BC_BASE/debug/build/cxx-AAAA/out/libfoo.a"

cp "$P3BC_BASE/debug/build/cxx-AAAA/build_script_build.d" "$P3BC_D_SNAPSHOT"
cp "$P3BC_BASE/debug/build/cxx-AAAA/out/libfoo.a" "$P3BC_A_SNAPSHOT"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$P3BC_BASE" "$P3BC_LANE" --fresh-checkout

assert "P3b/c: --fresh-checkout exits 0 (warmth/safety guards)" test "$RC" -eq 0

assert "P3b: build/cxx-AAAA/out/libfoo.a PRESERVED (warmth; not deleted)" \
    test -e "$P3BC_LANE/target/debug/build/cxx-AAAA/out/libfoo.a"
assert "P3b: build/cxx-AAAA build dir PRESERVED (not deleted, only rewritten)" \
    test -d "$P3BC_LANE/target/debug/build/cxx-AAAA"

assert "P3c: build_script_build.d BYTE-UNCHANGED (.d not in scope; filename-only match)" \
    cmp -s "$P3BC_D_SNAPSHOT" "$P3BC_LANE/target/debug/build/cxx-AAAA/build_script_build.d"
assert "P3c: out/libfoo.a BYTE-UNCHANGED (binary; never rewritten regardless of content)" \
    cmp -s "$P3BC_A_SNAPSHOT" "$P3BC_LANE/target/debug/build/cxx-AAAA/out/libfoo.a"

# Sanity: confirm relocation actually ran in this same fixture, so the guards
# above aren't vacuously true because relocation never fired at all.
P3BC_LANE_RP="$(realpath -m "$P3BC_LANE")"
assert "P3c: build/cxx-AAAA/output WAS relocated in this same run (guards non-vacuous)" \
    bash -c 'grep -qF "$1" "$2"' _ "$P3BC_LANE_RP" "$P3BC_LANE/target/debug/build/cxx-AAAA/output"

# ─────────────────────────────────────────────────────────────────────────────
# Block Q — acquisition-time lane-lock exclusivity (task #5223, PRD
# docs/prds/warm-lane-pool-cow-seeding.md §9.5 inv.11)
#
# The lane lock is an exclusive flock on the sibling-path ${LANE_DIR}.lock (the
# SAME convention thin-warm-lane.sh T3 / warm-lane-gc.sh's live-consumer probe
# already use) held across the destructive replace+clone, so a reseed never
# clobbers a lane a live consumer still holds (inv.2). Acquired by DEFAULT under
# --fresh-checkout (esc-5214/task 5354 fail-safe flip: the #5223 --lane-lock
# guard was opt-in and thus bypassable by a caller that simply omitted it — the
# exact esc-5214 acquire-path clobber). --lane-lock stays accepted (implied under
# --fresh-checkout; still the explicit opt-in for the --reset-in-place control arm).
#
# Uses run_helper_real (real fixture: a non-empty <lane_dir>/target containing
# a sentinel file) so the mv/clobber actually executes or is actually refused.
# The held-lock scenarios use a BACKGROUNDED subshell lock-holder + the
# _wait_for_reader_lock causal handshake (mirrors test_thin_warm_lane.sh B3),
# not an fd inherited in this test shell, so contention is against a genuinely
# separate process -- deterministic RED/GREEN, no fixed-sleep race.
#
# H1/H2/H3 (step-1/step-2). H4 (step-3/step-4). H5 (step-5/step-6).
# H5d, H6a/H6b (task #5223 amend, reviewer_comprehensive test-coverage
# findings): H5d exercises the previously-untested WAIT="unlimited"
# block-until-acquired branch (mixed-case, via a backgrounded seed run +
# done-marker file); H6a/H6b confirm the lock block genuinely spans
# --reset-in-place too, not just the --fresh-checkout path every case above
# exercises.
# H3 (task 5354, REWRITTEN) + H9 (task 5354, NEW) pin the fail-safe DEFAULT:
# with the lock HELD and --fresh-checkout but NO --lane-lock, seed REFUSES
# (H3, WAIT=0 → exit 75) or QUEUEs (H9, WAIT=unlimited → blocks then reseeds)
# rather than clobbering the live consumer (the esc-5214 acquire-path regression).
# H7/H8 (task 5354, NEW) pin the --assume-lane-lock-held opt-out that a caller
# already holding ${LANE_DIR}.lock (thin --reseed FD 9, gc reclaim FD 8) uses to
# skip the default acquire without self-refusing: H7 = the opt-out is honored
# (seed reseeds instead of refusing), H8 = --assume-lane-lock-held + --lane-lock
# is a contradiction (usage error, exit 2).
# H10 (task 5354, NEW) pins the complementary SCOPING property: the fail-safe
# default acquire is gated on --fresh-checkout || --lane-lock and does NOT extend
# to the bare --reset-in-place control arm — a held lock is ignored there (exit 0,
# not 75), the property H6a/H6b (reset-in-place WITH --lane-lock) and E1/H3a
# (reset-in-place, no held lock) leave unpinned.
# H11/H12/H13 (task 5568, NEW) pin the lane-lock refusal's own discriminant.
# The normative statement — why 75 is the wrong code, why the flag is opt-in
# rather than an unconditional flip, and the dark-factory arm — lives in ONE
# place: docs/prds/warm-lane-pool-cow-seeding.md §9.5 inv.11, deliberately NOT
# restated here (G7 no-lockstep-duplication; same discipline as Block S/inv.12).
# What each case pins:
#   H11 — --distinct-lock-refusal-rc on the flock -n arm: 77 with the flag
#         (a), 75 without it (b, the COMPAT PIN and (a)'s differential
#         partner: same fixture, same held lock, the flag the only
#         difference), plus the paths where the flag must be inert — success
#         (c), --assume-lane-lock-held (d), --record-base (e),
#         WAIT=unlimited (f).
#   H12 — the SAME rc contract on the flock -w N queue-timeout arm, with its
#         own compat pin: both arms share ONE rc (one cause, one remediation).
#   H13 — the LANE_LOCK_CONTENDED stderr token on both arms, UNCONDITIONAL
#         (a/b with the flag absent, c with it present). Its non-vacuity
#         control is H11c's token-absent assertion on the success path.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block Q: acquisition-time lane-lock exclusivity (--lane-lock) ---"

# Shared base fixture: base with empty sidecar so RUSTFLAGS+invocation guards pass.
Q_BASE_PARENT="$(mktemp -d /tmp/test-seed-Q-parent-XXXXXX)"
Q_BASE="$Q_BASE_PARENT/target"
_TMPDIRS+=("$Q_BASE_PARENT")
mkdir -p "$Q_BASE/debug"
echo "base artifact" > "$Q_BASE/debug/base_artifact.a"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$Q_BASE_PARENT/.warm-base-meta"

# ── H1: lock HELD by a live consumer → --lane-lock refuses (EX_TEMPFAIL 75),
# does NOT clobber the lane (sentinel survives), cp never invoked. ───────────
Q_LANE1="$(make_isolated_lane Q-lane1)"
mkdir -p "$Q_LANE1/target"
echo "sentinel content" > "$Q_LANE1/target/SENTINEL.txt"

Q_LOCK1="${Q_LANE1}.lock"
_TMPDIRS+=("$Q_LOCK1")
Q_READY1="${Q_LOCK1}.ready-marker"
_TMPDIRS+=("$Q_READY1")
touch "$Q_LOCK1"
# Causal handshake: the subshell touches Q_READY1 AFTER acquiring flock -x, so
# the run below only fires once the lock is provably held by this OTHER process.
( flock -x 9 && touch "$Q_READY1" && sleep 300 ) 9>"$Q_LOCK1" &
Q_LOCK1_PID=$!
_BGPIDS+=("$Q_LOCK1_PID")
_wait_for_reader_lock "$Q_READY1" 30

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_LANE1" --fresh-checkout --lane-lock

assert "H1: lock held by live consumer → exit 75 (EX_TEMPFAIL)" test "$RC" -eq 75
assert "H1: stderr mentions the lock/live-consumer refusal" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "lock|consumer"' _ "$ERR_OUT"
assert "H1: STDOUT is EMPTY (fail-closed, no path emitted)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "H1: sentinel file in <lane>/target still present (lane NOT clobbered)" \
    test -f "$Q_LANE1/target/SENTINEL.txt"
assert "H1: cp NEVER invoked (refused before clone)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

kill "$Q_LOCK1_PID" 2>/dev/null || true
wait "$Q_LOCK1_PID" 2>/dev/null || true

# ── H2: lock FREE → --lane-lock succeeds (RC 0, stdout resolved <lane>/target,
# target replaced from base). ─────────────────────────────────────────────────
Q_LANE2="$(make_isolated_lane Q-lane2)"
mkdir -p "$Q_LANE2/target"
echo "sentinel content" > "$Q_LANE2/target/SENTINEL.txt"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_LANE2" --fresh-checkout --lane-lock

assert "H2: lock free → exit 0" test "$RC" -eq 0
assert "H2: STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$Q_LANE2/target"'" ]' _ "$OUT"
assert "H2: sentinel file GONE (target was replaced from base)" \
    bash -c '[ ! -e "'"$Q_LANE2/target/SENTINEL.txt"'" ]'
assert "H2: base_artifact.a IS present in <lane>/target (clone from base succeeded)" \
    test -f "$Q_LANE2/target/debug/base_artifact.a"
assert "H2: cp invoked with --reflink=always" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# ── H3: fail-safe DEFAULT — lock HELD, --fresh-checkout but NO --lane-lock →
# seed acquires the lane lock BY DEFAULT (esc-5214/task 5354) and REFUSES
# (EX_TEMPFAIL 75) rather than clobbering a live consumer's lane. This is the
# task-required regression: the #5223 guard is no longer bypassable by a caller
# that simply forgets --lane-lock (the exact esc-5214 acquire-path clobber).
# Pairs with H9 (queue instead of refuse under WAIT=unlimited). ───────────────
Q_LANE3="$(make_isolated_lane Q-lane3)"
mkdir -p "$Q_LANE3/target"
echo "sentinel content" > "$Q_LANE3/target/SENTINEL.txt"

Q_LOCK3="${Q_LANE3}.lock"
_TMPDIRS+=("$Q_LOCK3")
Q_READY3="${Q_LOCK3}.ready-marker"
_TMPDIRS+=("$Q_READY3")
touch "$Q_LOCK3"
( flock -x 9 && touch "$Q_READY3" && sleep 300 ) 9>"$Q_LOCK3" &
Q_LOCK3_PID=$!
_BGPIDS+=("$Q_LOCK3_PID")
_wait_for_reader_lock "$Q_READY3" 30

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_LANE3" --fresh-checkout
# NOTE: no --lane-lock above -- the lock is acquired by DEFAULT under --fresh-checkout.

assert "H3: fail-safe default: lock held, no --lane-lock → exit 75 (EX_TEMPFAIL)" \
    test "$RC" -eq 75
assert "H3: fail-safe default: stderr mentions the lock/live-consumer refusal" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "lock|consumer"' _ "$ERR_OUT"
assert "H3: fail-safe default: STDOUT is EMPTY (fail-closed, no path emitted)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "H3: fail-safe default: sentinel survives (lane NOT clobbered)" \
    test -f "$Q_LANE3/target/SENTINEL.txt"
assert "H3: fail-safe default: cp NEVER invoked (refused before clone)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

kill "$Q_LOCK3_PID" 2>/dev/null || true
wait "$Q_LOCK3_PID" 2>/dev/null || true

# ── H4: FD-hygiene — a detached background trash-rm must NOT inherit the held
# lane-lock FD 9, so the lock is fully released the instant seed itself exits
# (not only once the orphaned background rm eventually finishes). ───────────
# REIFY_WARM_LANE_RESEED_TRASH_SYNC left UNSET (default async path) and a
# real non-empty target so a background trash rm is actually spawned; the
# sleeping rm stub (REIFY_TEST_SLEEP_RESEED_TRASH_RM=1) keeps that background
# process alive for ~2s so the immediate lock re-probe below is deterministic.
Q_LANE4="$(make_isolated_lane Q-lane4)"
mkdir -p "$Q_LANE4/target"
echo "stale artifact" > "$Q_LANE4/target/stale.a"
Q_LOCK4="${Q_LANE4}.lock"
_TMPDIRS+=("$Q_LOCK4")

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_TEST_SLEEP_RESEED_TRASH_RM=1 \
    run_helper_real "$Q_BASE" "$Q_LANE4" --fresh-checkout --lane-lock

assert "H4: seed exits 0 (async trash rm spawned)" test "$RC" -eq 0
assert "H4: lane lock is re-acquirable immediately after seed exits (no FD-9 leak to background rm)" \
    bash -c 'exec 8>"$1"; flock -n 8' _ "$Q_LOCK4"

# ── H4b (task #5705): the DETERMINISTIC, load-independent companion to H4.
#
# H4 above probes an OBSERVABLE TIMING PROPERTY: it asks whether the lock reads
# back free the instant seed exits. That is the right property, but it can only
# ever fail PROBABILISTICALLY — pre-#5705 it went RED in roughly 2-8% of runs
# under host load and passed the rest of the time, because the window it catches
# is the scheduling gap between a background job's fork() and that child's
# close(9). H4b instead asserts the STRUCTURAL fact that makes the timing
# property true (technique S, docs/prds/infra-test-wallclock-deflake.md §2):
# seed must have executed its explicit `flock -u 9` release path, which it
# announces on stderr. That marker is emitted or it is not — no load, no
# scheduling, no flake. Keeping BOTH is deliberate: H4 is the end-to-end
# observation, H4b is the regression guard that cannot silently degrade into a
# coin flip.
#
# Reuses the $ERR_OUT captured by the H4 run above — no second seed invocation.
assert "H4b: seed emits the explicit lane-lock release marker on stderr (the LOCK_UN path actually ran)" \
    bash -c 'printf "%s\n" "$1" | grep -q "explicit flock -u before exit"' _ "$ERR_OUT"

# ── H4c (task #5705 code review): the BEHAVIOURAL guard. H4/H4b/H4e between
# them prove the release path RAN; none of them proves it had the intended
# EFFECT. ─────────────────────────────────────────────────────────────────────
#
# THE COVERAGE HOLE THIS CLOSES, established by mutation: delete
# `trap _release_lane_lock EXIT` from scripts/seed-warm-lane.sh and exactly two
# asserts go red — H4b's marker grep and H4e's marker grep. BOTH of the
# "lock is re-acquirable immediately after seed exits" asserts still PASS,
# because the defect they target is a scheduling race that seed's own detached
# child almost always loses. So the only permanent guards were structural
# greps: they would still pass for a release that ran but did nothing — a
# typo'd `flock -u 8`, or a `flock -u` reordered after a close of FD 9 (which
# leaves the OFD locked while the marker still prints). The one behavioural
# check of the real semantics lived in test_seed_lane_lock_release_soak.sh,
# which is default-SKIPPING and therefore never runs in the gate.
#
# WHY H4's EXISTING KNOB CANNOT FILL IT: REIFY_TEST_SLEEP_RESEED_TRASH_RM=1
# keeps a background rm alive past the probe, but that child is spawned as
# `{ ...; } 9<&- &` — the redirection is applied by the child BEFORE it execs
# the sleeping rm stub, so the sleeper holds NO FD 9 and squats on nothing.
#
# THE FIXTURE: REIFY_TEST_FD9_SQUATTER=1 makes run_helper_real's cp stub fork a
# plain `sleep` that inherits FD 9 and never closes it. It outlives seed by
# construction, so at probe time a live process provably holds a dup of the OFD
# carrying seed's exclusive flock. The lock can then only read back FREE if
# seed's LOCK_UN really dropped it for the WHOLE open file description rather
# than merely for seed's own descriptor — which is the central claim of #5705,
# and is exactly what closing a descriptor cannot do.
#
# NON-VACUITY is structural, not a second timing observation: the three
# H4c-fixture asserts pin that the squatter exists, is still alive at probe
# time, and that /proc/<pid>/fd/9 really resolves to THIS lane's lock file. If
# the fixture ever stops reproducing that shape it fails loudly instead of
# quietly degrading into "the lock was free because nothing held it".
Q_LANE4C="$(make_isolated_lane Q-lane4c)"
mkdir -p "$Q_LANE4C/target"
echo "stale artifact" > "$Q_LANE4C/target/stale.a"
Q_LOCK4C="${Q_LANE4C}.lock"
_TMPDIRS+=("$Q_LOCK4C")
Q_SQPID_FILE4C="$(mktemp /tmp/test-seed-warm-lane-fd9squatter-XXXXXX)"
_TMPDIRS+=("$Q_SQPID_FILE4C")

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    REIFY_TEST_FD9_SQUATTER=1 REIFY_TEST_FD9_SQUATTER_PIDFILE="$Q_SQPID_FILE4C" \
    run_helper_real "$Q_BASE" "$Q_LANE4C" --fresh-checkout --lane-lock

Q_SQPID4C="$(cat "$Q_SQPID_FILE4C" 2>/dev/null || true)"

assert "H4c: seed exits 0 (the squatter fixture does not disturb the reseed itself)" \
    test "$RC" -eq 0
assert "H4c-fixture: the cp stub recorded a squatter PID (the FD-9 dup-holder was really forked)" \
    bash -c '[ -n "$1" ]' _ "$Q_SQPID4C"
assert "H4c-fixture: that squatter is STILL ALIVE now that seed has exited (it outlives seed by construction)" \
    kill -0 "$Q_SQPID4C"
assert "H4c-fixture: ... and it really holds a dup of seed's lane-lock FD 9 (/proc/<pid>/fd/9 resolves to THIS lane's lock)" \
    bash -c '[ "$(readlink "/proc/$1/fd/9" 2>/dev/null)" = "$(realpath "$2")" ]' _ "$Q_SQPID4C" "$Q_LOCK4C"
assert "H4c: the lane lock is FREE anyway — LOCK_UN dropped it for the whole OFD, not just for seed's own descriptor" \
    bash -c 'exec 8>"$1"; flock -n 8' _ "$Q_LOCK4C"

kill "$Q_SQPID4C" 2>/dev/null || true

# ── H4e (task #5705): the release must cover seed's FAILURE paths, not just its
# success tail. A release written as a trailing statement is UNREACHABLE the
# moment seed aborts after acquiring the lock — and seed has plenty of such
# aborts downstream of acquisition: the hard reflink error, the same-FS check,
# and every `set -euo pipefail` abort in the mid-run find walks and the
# _assert_delta_newer_than_build_outputs post-condition. All of those happen
# AFTER the orphan sweep has already forked its detached `{ rm -rf ...; } 9<&- &`
# child, so the identical fork-window race applies on every one of them.
#
# The fixture drives the reflink hard-error path — REIFY_TEST_REFLINK_OK is
# deliberately left UNSET, so run_helper_real's cp stub prints
# "cp: failed to clone: Operation not supported" and exits 1, and seed aborts
# with exit 1 having already acquired the lane lock.
#
# A pre-seeded ORPHANED trash entry (<lane-basename>.<pid> under the lane's own
# private dirname(LANE)/.reseed-trash) makes the orphan sweep fire, so a real
# detached child is forked before the abort; REIFY_TEST_SLEEP_RESEED_TRASH_RM=1
# keeps that child alive past the probe below. An H4e-fixture assert pins that
# the sweep really ran, so the shape being tested cannot silently stop
# reproducing if the sweep's entry naming ever changes.
#
# HONEST SCOPE, as with H4/H4b: the MARKER assert is the deterministic RED here
# — it fails outright while the release is a tail statement. The lock-reacquire
# assert is the end-to-end companion and can only ever fail probabilistically
# (the detached child usually wins the race to its own close(9) during the
# mv+rmdir+cp work that precedes the abort). Both are kept for the same reason
# H4 and H4b both are.
Q_LANE4E="$(make_isolated_lane Q-lane4e)"
mkdir -p "$Q_LANE4E/target"
echo "stale artifact" > "$Q_LANE4E/target/stale.a"
Q_LOCK4E="${Q_LANE4E}.lock"
_TMPDIRS+=("$Q_LOCK4E")

Q_TRASH4E="$(dirname "$Q_LANE4E")/.reseed-trash"
mkdir -p "$Q_TRASH4E/$(basename "$Q_LANE4E").999999"
echo "orphan artifact" > "$Q_TRASH4E/$(basename "$Q_LANE4E").999999/orphan.a"

reset_calls
RUSTFLAGS="" REIFY_TEST_SLEEP_RESEED_TRASH_RM=1 \
    run_helper_real "$Q_BASE" "$Q_LANE4E" --fresh-checkout --lane-lock

assert "H4e: seed ABORTS with exit 1 on the hard reflink error (downstream of a successful lane-lock acquire)" \
    test "$RC" -eq 1
assert "H4e: ... and it really was the reflink hard-error path (stderr names the aborted clone)" \
    bash -c 'printf "%s\n" "$1" | grep -q "Reflink clone FAILED"' _ "$ERR_OUT"
assert "H4e-fixture: the orphan sweep really forked a detached child before that abort (non-vacuity)" \
    bash -c 'printf "%s\n" "$1" | grep -q "Sweeping orphaned trash entry"' _ "$ERR_OUT"
assert "H4e: seed emits the lane-lock release marker on the FAILURE path too (release is not tail-only)" \
    bash -c 'printf "%s\n" "$1" | grep -q "explicit flock -u before exit"' _ "$ERR_OUT"
assert "H4e: lane lock is re-acquirable immediately after the ABORTED seed exits" \
    bash -c 'exec 8>"$1"; flock -n 8' _ "$Q_LOCK4E"

# ── H5: bounded-wait "queue" via REIFY_WARM_LANE_LANE_LOCK_WAIT ─────────────
# A refused acquirer of the SINGLETON _merge-verify lane has no alternate
# FREE lane to fall back to, so the WAIT knob lets --lane-lock QUEUE (bounded
# flock -w N) instead of refusing instantly (flock -n, the WAIT-unset
# default from H1-H4 above).
#
# H5a: lock HELD (same backgrounded flock -x holder + _wait_for_reader_lock
# causal handshake as H1) + WAIT=1 -> still refuses (75), but only AFTER the
# bounded wait elapses -- i.e. it queued, it did not refuse at 0s like H1.
# SECONDS is a plain lower-bound check (-ge, not -le/-lt) so it cannot be a
# flaky wall-clock UPPER bound (tests/infra/test_no_new_wallclock_upper_bounds.sh
# only flags -le/-lt time comparisons; a slower CI host only ever makes an
# elapsed-queued wait LONGER, never shorter, so -ge 1 cannot flake high).
Q_LANE5="$(make_isolated_lane Q-lane5)"
mkdir -p "$Q_LANE5/target"
echo "sentinel content" > "$Q_LANE5/target/SENTINEL.txt"

Q_LOCK5="${Q_LANE5}.lock"
_TMPDIRS+=("$Q_LOCK5")
Q_READY5="${Q_LOCK5}.ready-marker"
_TMPDIRS+=("$Q_READY5")
touch "$Q_LOCK5"
( flock -x 9 && touch "$Q_READY5" && sleep 300 ) 9>"$Q_LOCK5" &
Q_LOCK5_PID=$!
_BGPIDS+=("$Q_LOCK5_PID")
_wait_for_reader_lock "$Q_READY5" 30

reset_calls
SECONDS=0
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_LANE_LOCK_WAIT=1 \
    run_helper_real "$Q_BASE" "$Q_LANE5" --fresh-checkout --lane-lock
Q_H5A_WAITED_S=$SECONDS

assert "H5a: lock held + WAIT=1 -> exit 75 (EX_TEMPFAIL) after the bounded wait" \
    test "$RC" -eq 75
assert "H5a: bounded wait actually elapsed (queued, not an instant refuse like H1)" \
    test "$Q_H5A_WAITED_S" -ge 1
assert "H5a: sentinel file in <lane>/target still present (lane NOT clobbered)" \
    test -f "$Q_LANE5/target/SENTINEL.txt"
assert "H5a: cp NEVER invoked (refused before clone)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

kill "$Q_LOCK5_PID" 2>/dev/null || true
wait "$Q_LOCK5_PID" 2>/dev/null || true

# H5b: lock FREE + WAIT=1 -> succeeds exactly as H2 (the knob only changes
# behavior when the lock is contended; an uncontended flock -w N acquires on
# the very first try, same as flock -n or a bare blocking flock).
Q_LANE6="$(make_isolated_lane Q-lane6)"
mkdir -p "$Q_LANE6/target"
echo "sentinel content" > "$Q_LANE6/target/SENTINEL.txt"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_LANE_LOCK_WAIT=1 \
    run_helper_real "$Q_BASE" "$Q_LANE6" --fresh-checkout --lane-lock

assert "H5b: lock free + WAIT=1 -> exit 0" test "$RC" -eq 0
assert "H5b: STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$Q_LANE6/target"'" ]' _ "$OUT"
assert "H5b: sentinel file GONE (target was replaced from base)" \
    bash -c '[ ! -e "'"$Q_LANE6/target/SENTINEL.txt"'" ]'

# H5c: invalid WAIT knob value ("abc") -> usage error (exit 64), no target
# mutation -- rejected before the flock is even attempted.
Q_LANE7="$(make_isolated_lane Q-lane7)"
mkdir -p "$Q_LANE7/target"
echo "sentinel content" > "$Q_LANE7/target/SENTINEL.txt"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_LANE_LOCK_WAIT=abc \
    run_helper_real "$Q_BASE" "$Q_LANE7" --fresh-checkout --lane-lock

assert "H5c: invalid WAIT knob ('abc') -> exit 64 (usage error)" test "$RC" -eq 64
assert "H5c: stderr names the invalid knob (REIFY_WARM_LANE_LANE_LOCK_WAIT)" \
    bash -c 'printf "%s\n" "$1" | grep -q "REIFY_WARM_LANE_LANE_LOCK_WAIT"' _ "$ERR_OUT"
assert "H5c: sentinel file in <lane>/target still present (no target mutation)" \
    test -f "$Q_LANE7/target/SENTINEL.txt"
assert "H5c: cp NEVER invoked (rejected before any mutation)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# ── H5d: bounded-wait "unlimited" (mixed-case) -> blocks until acquired,
# never refuses. Exercises the bare blocking `flock 9` branch
# (seed-warm-lane.sh's _llw_unlimited=1 path) -- the exact path the
# SINGLETON _merge-verify lane is documented to rely on (queue forever
# rather than refuse, since it has no alternate FREE lane to fall back to).
# Mixed-case "UnLiMiTeD" also covers the case-insensitive glob match. ───────
Q_LANE8="$(make_isolated_lane Q-lane8)"
mkdir -p "$Q_LANE8/target"
echo "sentinel content" > "$Q_LANE8/target/SENTINEL.txt"

Q_LOCK8="${Q_LANE8}.lock"
_TMPDIRS+=("$Q_LOCK8")
Q_READY8="${Q_LOCK8}.ready-marker"
_TMPDIRS+=("$Q_READY8")
touch "$Q_LOCK8"
( flock -x 9 && touch "$Q_READY8" && sleep 300 ) 9>"$Q_LOCK8" &
Q_LOCK8_PID=$!
_BGPIDS+=("$Q_LOCK8_PID")
_wait_for_reader_lock "$Q_READY8" 30

# Run seed itself in the BACKGROUND -- with WAIT=unlimited it must block for
# as long as the holder above lives. Completion signal is a done-marker file
# (touched only after run_helper_real returns), NOT the subshell PID's
# liveness: a finished-but-unreaped background job is a zombie, and `kill -0`
# on a zombie PID still succeeds, so PID liveness alone cannot distinguish
# "still blocked" from "done but not yet wait(1)-ed".
Q_DONE8="${Q_LANE8}.done-marker"
_TMPDIRS+=("$Q_DONE8" "${Q_DONE8}.rc" "${Q_DONE8}.out")

# REAL_STUB_DIR staleness guard (task #5633 code review): run_helper_real
# below runs inside the backgrounded subshell that follows, so any write it
# makes to REAL_STUB_DIR is confined to the subshell's own copy and is
# discarded when the subshell exits -- the same fork-local-write hazard
# already documented above for _TMPDIRS/_HELPER_LANES_FILE (and it is why
# RC/OUT are instead round-tripped through the .rc/.out files below). A
# reset placed inside run_helper_real itself cannot fix this: an unset made
# by the subshell is just as invisible to the main shell as a set would be.
# Clearing it HERE, in the MAIN shell, before backgrounding, is the only
# place a reset can actually take effect -- it makes staleness loud (a plain
# assert failure on an empty value) instead of plausible (a stale-but-still-
# valid directory left over from whichever run_helper_real call last ran in
# the main shell).
REAL_STUB_DIR=""

reset_calls
(
    RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_LANE_LOCK_WAIT=UnLiMiTeD \
        run_helper_real "$Q_BASE" "$Q_LANE8" --fresh-checkout --lane-lock
    printf '%s' "$RC" > "${Q_DONE8}.rc"
    printf '%s' "$OUT" > "${Q_DONE8}.out"
    touch "$Q_DONE8"
) &
Q_SEED8_PID=$!
_BGPIDS+=("$Q_SEED8_PID")

# Brief settle so the backgrounded job has actually forked/reached the flock
# call; this is NOT a wall-clock upper-bound assertion -- the "not done yet"
# check below can only be a false failure (never a false pass), since the
# holder genuinely holds the lock until killed below.
sleep 0.3
assert "H5d: 'unlimited' (mixed-case) is still blocked while the lock is held (no done-marker yet)" \
    bash -c '[ ! -e "$1" ]' _ "$Q_DONE8"
assert "H5d: sentinel file in <lane>/target still present while blocked (no clobber yet)" \
    test -f "$Q_LANE8/target/SENTINEL.txt"

# Release the holder; the queued seed should now acquire the lock and run.
kill "$Q_LOCK8_PID" 2>/dev/null || true
wait "$Q_LOCK8_PID" 2>/dev/null || true

_wait_for_reader_lock "$Q_DONE8" 30
wait "$Q_SEED8_PID" 2>/dev/null || true

Q_H5D_RC="$(cat "${Q_DONE8}.rc" 2>/dev/null || echo "unset")"
Q_H5D_OUT="$(cat "${Q_DONE8}.out" 2>/dev/null || echo "")"

assert "H5d: after the holder releases, 'unlimited' seed completes with exit 0 (got '${Q_H5D_RC}')" \
    test "$Q_H5D_RC" -eq 0
assert "H5d: STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$Q_LANE8/target"'" ]' _ "$Q_H5D_OUT"
assert "H5d: sentinel file GONE (target was replaced from base after unblocking)" \
    bash -c '[ ! -e "'"$Q_LANE8/target/SENTINEL.txt"'" ]'
assert "H5d: base_artifact.a IS present in <lane>/target (clone from base succeeded)" \
    test -f "$Q_LANE8/target/debug/base_artifact.a"

# ── H6: --lane-lock also guards --reset-in-place, not just --fresh-checkout ──
# The lock block runs BEFORE the mode-split (seed-warm-lane.sh comment: "so it
# guards BOTH --fresh-checkout and --reset-in-place"), but H1-H5 above only
# ever exercise --fresh-checkout. A future refactor that moved the lock block
# below the mode-split would silently drop reset-in-place protection with no
# failing test above -- H6 pins the ordering directly against real fixtures.

# H6a: lock HELD + --reset-in-place + --lane-lock, on a NON-EMPTY lane target
# (which --reset-in-place's OWN clobber guard would otherwise refuse with
# exit 1) -> exit 75, not 1: proves the lock check runs and refuses BEFORE
# the clobber guard gets a chance to run its own (different) refusal.
Q_LANE9="$(make_isolated_lane Q-lane9)"
mkdir -p "$Q_LANE9/target"
echo "sentinel content" > "$Q_LANE9/target/SENTINEL.txt"

Q_LOCK9="${Q_LANE9}.lock"
_TMPDIRS+=("$Q_LOCK9")
Q_READY9="${Q_LOCK9}.ready-marker"
_TMPDIRS+=("$Q_READY9")
touch "$Q_LOCK9"
( flock -x 9 && touch "$Q_READY9" && sleep 300 ) 9>"$Q_LOCK9" &
Q_LOCK9_PID=$!
_BGPIDS+=("$Q_LOCK9_PID")
_wait_for_reader_lock "$Q_READY9" 30

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_LANE9" --reset-in-place --lane-lock

assert "H6a: reset-in-place + lock held -> exit 75 (lock check precedes the clobber guard, not exit 1)" \
    test "$RC" -eq 75
assert "H6a: stderr mentions the lock/live-consumer refusal (not the clobber-guard message)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "lock|consumer"' _ "$ERR_OUT"
assert "H6a: sentinel file in <lane>/target still present (lane NOT touched)" \
    test -f "$Q_LANE9/target/SENTINEL.txt"
assert "H6a: cp NEVER invoked" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

kill "$Q_LOCK9_PID" 2>/dev/null || true
wait "$Q_LOCK9_PID" 2>/dev/null || true

# H6b: lock FREE + --reset-in-place + --lane-lock, on an EMPTY lane (no
# pre-existing target/ at all, mirroring Block E's E1 fixture) -> exit 0:
# confirms the lock genuinely spans --reset-in-place's uncontended success
# path too, not just its held-lock refusal path in H6a.
Q_LANE10="$(make_isolated_lane Q-lane10)"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_LANE10" --reset-in-place --lane-lock

assert "H6b: reset-in-place + lock free + empty lane -> exit 0" test "$RC" -eq 0
assert "H6b: STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$Q_LANE10/target"'" ]' _ "$OUT"
assert "H6b: base_artifact.a IS present in <lane>/target (clone from base succeeded)" \
    test -f "$Q_LANE10/target/debug/base_artifact.a"
assert "H6b: cp invoked with --reflink=always" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# ── H7: --assume-lane-lock-held opt-out is HONORED — a caller that already holds
# ${LANE_DIR}.lock passes --assume-lane-lock-held so seed SKIPS its own default
# acquisition and does NOT self-refuse. The held-lock fixture (H1's backgrounded
# flock -x holder) simulates the caller's own hold; with the opt-out seed reseeds
# normally (RC 0, target replaced) instead of refusing (75). This is the
# mechanism thin --reseed (FD 9) and gc reclaim (FD 8) rely on to avoid the
# flock-non-reentrancy self-refuse under the fail-safe default. ───────────────
Q_LANE12="$(make_isolated_lane Q-lane12)"
mkdir -p "$Q_LANE12/target"
echo "sentinel content" > "$Q_LANE12/target/SENTINEL.txt"

Q_LOCK12="${Q_LANE12}.lock"
_TMPDIRS+=("$Q_LOCK12")
Q_READY12="${Q_LOCK12}.ready-marker"
_TMPDIRS+=("$Q_READY12")
touch "$Q_LOCK12"
( flock -x 9 && touch "$Q_READY12" && sleep 300 ) 9>"$Q_LOCK12" &
Q_LOCK12_PID=$!
_BGPIDS+=("$Q_LOCK12_PID")
_wait_for_reader_lock "$Q_READY12" 30

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_LANE12" --fresh-checkout --assume-lane-lock-held

assert "H7: --assume-lane-lock-held + lock held → exit 0 (seed skipped its own acquire, no self-refuse)" \
    test "$RC" -eq 0
assert "H7: STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$Q_LANE12/target"'" ]' _ "$OUT"
assert "H7: sentinel GONE (target replaced from base — the reseed actually ran)" \
    bash -c '[ ! -e "'"$Q_LANE12/target/SENTINEL.txt"'" ]'
assert "H7: base_artifact.a present in <lane>/target (clone from base succeeded)" \
    test -f "$Q_LANE12/target/debug/base_artifact.a"
assert "H7: cp invoked with --reflink=always" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

kill "$Q_LOCK12_PID" 2>/dev/null || true
wait "$Q_LOCK12_PID" 2>/dev/null || true

# ── H7b (task #5705): --assume-lane-lock-held must leave the CALLER's lock
# ALONE. seed now drops its lane lock with an explicit `flock -u 9` before
# exiting (the FD-9 fork-window fix); that release is guarded on
# _should_acquire_lane_lock, i.e. it runs ONLY on the branch where seed opened
# FD 9 itself. Without that guard an `--assume-lane-lock-held` run would unlock
# a descriptor it INHERITED — releasing the caller's lock and re-opening the
# very inv.2 clobber window --lane-lock exists to close. That is strictly worse
# than the bug being fixed, so it gets its own pinned test.
#
# WHY H7 ABOVE DOES NOT COVER THIS: H7 holds the lock in a BACKGROUNDED
# SUBSHELL (`( flock -x 9 ... ) 9>"$LOCK" &`), so its FD 9 belongs to that
# subshell and seed never inherits it — an unguarded `flock -u 9` inside seed
# would find FD 9 unopened and change nothing observable. The REAL shape of
# thin --reseed / gc reclaim is the caller holding the lock on FD 9 in the very
# process that execs seed, so H7b holds it HERE, in the test shell, and lets
# run_helper_real's plain `bash "$SCRIPT"` child inherit it exactly as thin's
# would.
#
# The probe MUST be a separate process: flock is per-open-file-description, so
# a probe run inside this shell would see our own lock and succeed regardless.
#
# REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 (foreground trash rm) is load-bearing for
# DETERMINISM here, and is not the property under test: with the default async
# rm, seed forks `{ rm -rf ...; } 9<&- &`, and that detached child transiently
# holds a dup of THIS SHELL's OFD 9 — so the final "probe now succeeds" control
# below would race it exactly the way #5705's production bug races acquire_lane.
# Forcing the rm foreground leaves zero detached children, so the control is a
# clean two-way detector rather than a second copy of the race.
Q_LANE12B="$(make_isolated_lane Q-lane12b)"
mkdir -p "$Q_LANE12B/target"
echo "sentinel content" > "$Q_LANE12B/target/SENTINEL.txt"

Q_LOCK12B="${Q_LANE12B}.lock"
_TMPDIRS+=("$Q_LOCK12B")
touch "$Q_LOCK12B"

# _h7b_probe_refused / _h7b_probe_acquired: the H4 probe form, wrapped so the
# assert lines stay readable. Both spawn a SEPARATE process (see above).
_h7b_probe_acquired() { bash -c 'exec 8>"$1"; flock -n 8' _ "$1"; }
_h7b_probe_refused()  { ! _h7b_probe_acquired "$1"; }

exec 9>"$Q_LOCK12B"
assert "H7b-setup: the TEST SHELL itself holds the lane lock on FD 9 (thin --reseed's real shape, unlike H7's backgrounded subshell)" \
    flock -n 9

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 \
    run_helper_real "$Q_BASE" "$Q_LANE12B" --fresh-checkout --assume-lane-lock-held

assert "H7b: --assume-lane-lock-held with the caller's lock INHERITED on FD 9 → exit 0 (seed skipped its own acquire)" \
    test "$RC" -eq 0
assert "H7b: seed does NOT announce a lane-lock release it does not own (marker absent from stderr)" \
    bash -c '! printf "%s\n" "$1" | grep -q "explicit flock -u before exit"' _ "$ERR_OUT"
assert "H7b: the CALLER'S LOCK SURVIVED seed — a separate-process probe still cannot acquire it" \
    _h7b_probe_refused "$Q_LOCK12B"

exec 9>&-
assert "H7b: ... and that same probe SUCCEEDS once the test shell drops FD 9 (a detector, not a constant refusal)" \
    _h7b_probe_acquired "$Q_LOCK12B"

# ── H8: --assume-lane-lock-held + --lane-lock is a CONTRADICTION → usage error
# (exit 2). --lane-lock says "acquire the lane lock yourself"; --assume-lane-lock-held
# says "the caller already holds it, do NOT acquire" — passing both is caller
# confusion, rejected loudly (naming BOTH flags) rather than silently picking a
# precedence. No held lock needed; rejected before any target mutation. ───────
Q_LANE13="$(make_isolated_lane Q-lane13)"
mkdir -p "$Q_LANE13/target"
echo "sentinel content" > "$Q_LANE13/target/SENTINEL.txt"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_LANE13" --fresh-checkout --lane-lock --assume-lane-lock-held

assert "H8: --lane-lock + --assume-lane-lock-held → exit 2 (usage error)" \
    test "$RC" -eq 2
assert "H8: stderr names --lane-lock (the contradiction names both flags)" \
    bash -c 'printf "%s\n" "$1" | grep -q -- "--lane-lock"' _ "$ERR_OUT"
assert "H8: stderr names --assume-lane-lock-held (the contradiction names both flags)" \
    bash -c 'printf "%s\n" "$1" | grep -q -- "--assume-lane-lock-held"' _ "$ERR_OUT"
assert "H8: sentinel file in <lane>/target still present (no target mutation)" \
    test -f "$Q_LANE13/target/SENTINEL.txt"
assert "H8: cp NEVER invoked (rejected before any mutation)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# ── H9: fail-safe DEFAULT queues under WAIT=unlimited — lock HELD,
# --fresh-checkout but NO --lane-lock + REIFY_WARM_LANE_LANE_LOCK_WAIT=unlimited
# → the default-acquired lane lock BLOCKS until the live consumer releases
# (never refuses), then reseeds. Pairs with H3 (refuse at WAIT=0): together they
# pin "refuse OR queue rather than clobber" for the no-`--lane-lock` acquire path
# (the esc-5214 regression). Mirrors H5d's backgrounded-seed + done-marker
# pattern, but WITHOUT --lane-lock (the lock is now default-on). ──────────────
Q_LANE11="$(make_isolated_lane Q-lane11)"
mkdir -p "$Q_LANE11/target"
echo "sentinel content" > "$Q_LANE11/target/SENTINEL.txt"

Q_LOCK11="${Q_LANE11}.lock"
_TMPDIRS+=("$Q_LOCK11")
Q_READY11="${Q_LOCK11}.ready-marker"
_TMPDIRS+=("$Q_READY11")
touch "$Q_LOCK11"
( flock -x 9 && touch "$Q_READY11" && sleep 300 ) 9>"$Q_LOCK11" &
Q_LOCK11_PID=$!
_BGPIDS+=("$Q_LOCK11_PID")
_wait_for_reader_lock "$Q_READY11" 30

# Run seed in the BACKGROUND -- with WAIT=unlimited and the lock held it must
# block until the holder below is killed. Completion signal is a done-marker
# file (touched only after run_helper_real returns), NOT PID liveness (a
# finished-but-unreaped bg job is a zombie whose PID `kill -0` still succeeds).
Q_DONE11="${Q_LANE11}.done-marker"
_TMPDIRS+=("$Q_DONE11" "${Q_DONE11}.rc" "${Q_DONE11}.out")

# REAL_STUB_DIR staleness guard (task #5633 code review) -- see the
# identical comment at H5d/Q_LANE8 above: a reset made inside
# run_helper_real cannot reach the main shell from inside this backgrounded
# subshell, so it has to happen here instead.
REAL_STUB_DIR=""

reset_calls
(
    RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_LANE_LOCK_WAIT=unlimited \
        run_helper_real "$Q_BASE" "$Q_LANE11" --fresh-checkout
    printf '%s' "$RC" > "${Q_DONE11}.rc"
    printf '%s' "$OUT" > "${Q_DONE11}.out"
    touch "$Q_DONE11"
) &
Q_SEED11_PID=$!
_BGPIDS+=("$Q_SEED11_PID")

# Brief settle so the backgrounded job has reached the flock call. NOT a
# wall-clock upper bound -- the "not done yet" check can only false-fail, never
# false-pass, since the holder genuinely holds the lock until killed below.
sleep 0.3
assert "H9: WAIT=unlimited + no --lane-lock is still BLOCKED while the lock is held (no done-marker yet)" \
    bash -c '[ ! -e "$1" ]' _ "$Q_DONE11"
assert "H9: sentinel survives while blocked (no clobber yet)" \
    test -f "$Q_LANE11/target/SENTINEL.txt"

# Release the holder; the queued seed should now acquire the lock and reseed.
kill "$Q_LOCK11_PID" 2>/dev/null || true
wait "$Q_LOCK11_PID" 2>/dev/null || true

_wait_for_reader_lock "$Q_DONE11" 30
wait "$Q_SEED11_PID" 2>/dev/null || true

Q_H9_RC="$(cat "${Q_DONE11}.rc" 2>/dev/null || echo "unset")"
Q_H9_OUT="$(cat "${Q_DONE11}.out" 2>/dev/null || echo "")"

assert "H9: after the holder releases, the queued seed completes with exit 0 (got '${Q_H9_RC}')" \
    test "$Q_H9_RC" -eq 0
assert "H9: STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$Q_LANE11/target"'" ]' _ "$Q_H9_OUT"
assert "H9: sentinel GONE (target replaced from base after unblocking, queued-then-clobbered)" \
    bash -c '[ ! -e "'"$Q_LANE11/target/SENTINEL.txt"'" ]'
assert "H9: base_artifact.a present in <lane>/target (clone from base succeeded)" \
    test -f "$Q_LANE11/target/debug/base_artifact.a"

# ── H10: scoping guard — the fail-safe default acquire is SCOPED to
# --fresh-checkout (|| explicit --lane-lock) and DOES NOT extend to the bare
# --reset-in-place control arm (seed-warm-lane.sh:510 gates on
# `[ -n "$FRESH_CHECKOUT" ] || [ -n "$LANE_LOCK_OPT" ]`, deliberately NOT
# $RESET_IN_PLACE; the PRD keeps --lane-lock the explicit opt-in for the
# reset-in-place arm). H6a/H6b exercise reset-in-place WITH --lane-lock, and
# E1/H3a exercise reset-in-place with NO held lock — so a leak of the default-on
# acquire into reset-in-place would silently pass every case above. H10 pins it
# directly: a live consumer HOLDS ${LANE}.lock (H7's backgrounded flock -x holder
# fixture) and seed runs --reset-in-place WITHOUT --lane-lock/--assume-lane-lock-held
# on an EMPTY lane (H6b's success-path fixture — reset-in-place refuses a
# non-empty target). Because reset-in-place does NOT default-acquire, the held
# lock is simply ignored and seed proceeds (RC 0, cp ran); if the default acquire
# leaked into reset-in-place this would instead refuse with exit 75. ───────────
Q_LANE14="$(make_isolated_lane Q-lane14)"

Q_LOCK14="${Q_LANE14}.lock"
_TMPDIRS+=("$Q_LOCK14")
Q_READY14="${Q_LOCK14}.ready-marker"
_TMPDIRS+=("$Q_READY14")
touch "$Q_LOCK14"
# Same causal handshake as H1/H7: the holder touches Q_READY14 only AFTER
# acquiring flock -x, so the run below fires with the lock provably held.
( flock -x 9 && touch "$Q_READY14" && sleep 300 ) 9>"$Q_LOCK14" &
Q_LOCK14_PID=$!
_BGPIDS+=("$Q_LOCK14_PID")
_wait_for_reader_lock "$Q_READY14" 30

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_LANE14" --reset-in-place
# NOTE: no --lane-lock and no --assume-lane-lock-held -- the bare reset-in-place
# control arm must NOT default-acquire, so the held lock above is ignored.

assert "H10: reset-in-place + lock HELD + no --lane-lock → exit 0 (default acquire is NOT scoped to reset-in-place; not 75)" \
    test "$RC" -eq 0
assert "H10: STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$Q_LANE14/target"'" ]' _ "$OUT"
assert "H10: base_artifact.a present in <lane>/target (reset-in-place clone from base ran despite the held lock)" \
    test -f "$Q_LANE14/target/debug/base_artifact.a"
assert "H10: cp invoked with --reflink=always (reset-in-place proceeded, not refused)" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

kill "$Q_LOCK14_PID" 2>/dev/null || true
wait "$Q_LOCK14_PID" 2>/dev/null || true

# ── H11: --distinct-lock-refusal-rc — the lane-lock refusal gets its OWN exit
# code, distinct from EX_TEMPFAIL 75 (task 5568). ───────────────────────────
#
# The contract and its rationale live in ONE place —
# docs/prds/warm-lane-pool-cow-seeding.md §9.5 inv.11 ("Refusal signal") — and
# are deliberately not restated here (G7). These cases pin the runtime
# BEHAVIOUR: the exit code with and without the flag, on both refusal arms,
# and the paths where the flag must be inert.
#
# The flag string is spelled once here and reused by every case below. In
# seed's own arg parser it must stay a LITERAL, because dark-factory's
# capability probe text-greps the lane's copy of the script for it.
Q_H11_FLAG="--distinct-lock-refusal-rc"
# The stderr marker H13 pins, hoisted here because H11c doubles as H13's
# non-vacuity control (the token must be ABSENT on a success run).
Q_CONTENDED_TOKEN="LANE_LOCK_CONTENDED"

# _q_hold_lane_lock <tag> — mint an isolated lane carrying a sentinel file,
# start a BACKGROUNDED flock -x holder on ${LANE}.lock, and block on the
# _wait_for_reader_lock causal handshake so the lock is provably held before
# returning (a fixed sleep would race the holder under load). Contention is
# therefore against a genuinely separate process, as in H1. Sets Q_HELD_LANE /
# Q_HELD_LOCK_PID; release with _q_release_lane_lock.
#
# Factored because H11/H12/H13 need EIGHT near-identical contended fixtures,
# and hand-copying the holder block is exactly where a dropped
# _wait_for_reader_lock turns into a flaky test.
_q_hold_lane_lock() {
    local tag="$1"
    Q_HELD_LANE="$(make_isolated_lane "Q-lane-$tag")"
    mkdir -p "$Q_HELD_LANE/target"
    echo "sentinel content" > "$Q_HELD_LANE/target/SENTINEL.txt"
    local lock="${Q_HELD_LANE}.lock"
    local ready="${lock}.ready-marker"
    _TMPDIRS+=("$lock" "$ready")
    touch "$lock"
    ( flock -x 9 && touch "$ready" && sleep 300 ) 9>"$lock" &
    Q_HELD_LOCK_PID=$!
    _BGPIDS+=("$Q_HELD_LOCK_PID")
    _wait_for_reader_lock "$ready" 30
}
_q_release_lane_lock() {
    kill "$Q_HELD_LOCK_PID" 2>/dev/null || true
    wait "$Q_HELD_LOCK_PID" 2>/dev/null || true
}

# H11a: lock HELD + the flag → 77, and the refusal is otherwise IDENTICAL to
# H1 (fail-closed stdout, lane not clobbered, no clone attempted). The flag
# selects a code; it must not weaken the guard the code reports.
_q_hold_lane_lock h11a
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_HELD_LANE" --fresh-checkout --lane-lock "$Q_H11_FLAG"

assert "H11a: lock held + --distinct-lock-refusal-rc → exit 77 (lock contention), NOT 75 (disk pressure)" \
    test "$RC" -eq 77
assert "H11a: STDOUT is EMPTY (fail-closed, no path emitted — the flag does not weaken the guard)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "H11a: sentinel file in <lane>/target still present (lane NOT clobbered)" \
    test -f "$Q_HELD_LANE/target/SENTINEL.txt"
assert "H11a: cp NEVER invoked (refused before clone)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"
_q_release_lane_lock

# H11b: COMPAT PIN — the SAME fixture and the SAME held lock as H11a, with the
# flag ABSENT → 75, unchanged. This assertion must stay green FOREVER: it is
# what keeps an unpatched dark-factory on today's exact behaviour, and hence
# what makes either landing order safe. Read as a differential against H11a,
# it also proves the flag is the SOLE cause of the rc change (nothing else in
# this invocation differs).
_q_hold_lane_lock h11b
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_HELD_LANE" --fresh-checkout --lane-lock
# NOTE: no --distinct-lock-refusal-rc above -- that omission IS the test.

assert "H11b: COMPAT PIN — lock held, flag ABSENT → exit 75 unchanged (an unpatched dark-factory sees today's behaviour)" \
    test "$RC" -eq 75
assert "H11b: COMPAT PIN — sentinel survives (default refusal path otherwise unchanged)" \
    test -f "$Q_HELD_LANE/target/SENTINEL.txt"
_q_release_lane_lock

# H11c: the flag NEVER perturbs the success path — lock FREE + the flag → 0
# and the ordinary resolved-path stdout, exactly as H2. A code that only names
# a refusal must be invisible when there is no refusal.
#
# This case also carries H13's NON-VACUITY control: the LANE_LOCK_CONTENDED
# token must be ABSENT here. Without it, every token assertion in H13 would
# still pass if the token were printed unconditionally at startup, which would
# make `grep LANE_LOCK_CONTENDED` a constant rather than a detector. Same
# fixture, same invocation — one extra assertion rather than a second lane.
Q_LANE17="$(make_isolated_lane Q-lane17)"
mkdir -p "$Q_LANE17/target"
echo "sentinel content" > "$Q_LANE17/target/SENTINEL.txt"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_LANE17" --fresh-checkout --lane-lock "$Q_H11_FLAG"

assert "H11c: flag + lock FREE → exit 0 (the flag never perturbs the success path)" \
    test "$RC" -eq 0
assert "H11c: flag + lock FREE → STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$Q_LANE17/target"'" ]' _ "$OUT"
assert "H11c: flag + lock FREE → clone from base actually ran" \
    test -f "$Q_LANE17/target/debug/base_artifact.a"
assert "H11c: NON-VACUITY for H13 — the $Q_CONTENDED_TOKEN token is ABSENT on the success path (a detector, not a constant)" \
    bash -c '! printf "%s\n" "$2" | grep -q "$1"' _ "$Q_CONTENDED_TOKEN" "$ERR_OUT"

# H11d/H11e/H11f: the flag is ACCEPTED-BUT-INERT — never a usage error —
# wherever seed acquires no lock and no refusal arm is reachable. The usage
# text and §9.5 inv.11 both invite dark-factory to pass it UNCONDITIONALLY
# rather than replicating seed's internal mode logic; these three cases pin
# each path that promise covers, so a future tightening cannot silently turn a
# DF invocation into a hard exit-2 fault.
#
# (Contrast H8: --assume-lane-lock-held + --lane-lock IS a usage error, because
# those two make CONTRADICTORY assertions about who holds the lock; this flag
# only selects a code for an outcome that may not occur.)

# H11d: under --assume-lane-lock-held, with another process holding the lock.
# NOT a usage error (exit 2) and NOT a refusal (75/77): exit 0. The holder is
# BACKGROUNDED (H7's shape), so seed does not inherit FD 9 — and
# --assume-lane-lock-held makes seed skip its own acquire anyway.
_q_hold_lane_lock h11d
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_HELD_LANE" --fresh-checkout --assume-lane-lock-held "$Q_H11_FLAG"

assert "H11d: flag + --assume-lane-lock-held (lock held elsewhere) → exit 0 — accepted-but-inert, NOT a usage error (2) and NOT a refusal (75/77)" \
    test "$RC" -eq 0
assert "H11d: flag + --assume-lane-lock-held → STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$Q_HELD_LANE/target"'" ]' _ "$OUT"
assert "H11d: flag + --assume-lane-lock-held → stderr carries NO unknown-argument complaint" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "unknown|unrecognized"' _ "$ERR_OUT"
_q_release_lane_lock

# H11e: in --record-base mode, which has its OWN validation block rejecting
# positionals and the seed mode flags (exit 2). The riskiest of the inert
# paths: a future "record-base mode: seed-only flags are invalid here"
# tightening would convert an unconditional DF invocation into a hard fault.
Q_H11E_PARENT="$(mktemp -d /tmp/test-seed-Q-h11e-XXXXXX)"
_TMPDIRS+=("$Q_H11E_PARENT")
mkdir -p "$Q_H11E_PARENT/target"

reset_calls
RUSTFLAGS="" run_helper --record-base "$Q_H11E_PARENT/target" "$Q_H11_FLAG"

assert "H11e: --record-base + the flag → exit 0 (inert, NOT a record-base usage error)" \
    test "$RC" -eq 0
assert "H11e: --record-base + the flag → STDOUT is exactly the sidecar path" \
    bash -c '[ "$1" = "'"$Q_H11E_PARENT/.warm-base-meta"'" ]' _ "$OUT"

# H11f: under REIFY_WARM_LANE_LANE_LOCK_WAIT=unlimited, where seed blocks until
# acquired and neither refusal arm exists — so $LANE_LOCK_REFUSAL_RC is
# computed but never reachable.
Q_LANE_H11F="$(make_isolated_lane Q-lane-h11f)"
mkdir -p "$Q_LANE_H11F/target"
echo "sentinel content" > "$Q_LANE_H11F/target/SENTINEL.txt"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_LANE_LOCK_WAIT=unlimited \
    run_helper_real "$Q_BASE" "$Q_LANE_H11F" --fresh-checkout --lane-lock "$Q_H11_FLAG"

assert "H11f: WAIT=unlimited + the flag, lock FREE → exit 0 (inert; no refusal arm is reachable)" \
    test "$RC" -eq 0
assert "H11f: WAIT=unlimited + the flag → STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$Q_LANE_H11F/target"'" ]' _ "$OUT"

# ── H12: the SAME rc contract on the BOUNDED-QUEUE timeout arm (task 5568).
# H11 covers the flock -n immediate refusal; this covers flock -w N timing
# out. Both arms share ONE rc — identical cause, identical remediation; see
# §9.5 inv.11 for why splitting them would be wrong. ───────────────────────

# H12a: lock HELD + WAIT=1 + the flag → 77 after the bounded wait elapsed.
# The SECONDS check is a plain LOWER bound (-ge, never -le/-lt): it proves the
# run queued rather than refusing instantly like H11a, and it cannot flake,
# because a slower host only ever makes an elapsed queued wait LONGER.
# (tests/infra/test_no_new_wallclock_upper_bounds.sh flags -le/-lt time
# comparisons for exactly this reason — same idiom and rationale as H5a.)
_q_hold_lane_lock h12a
reset_calls
SECONDS=0
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_LANE_LOCK_WAIT=1 \
    run_helper_real "$Q_BASE" "$Q_HELD_LANE" --fresh-checkout --lane-lock "$Q_H11_FLAG"
Q_H12A_WAITED_S=$SECONDS

assert "H12a: queue timeout + --distinct-lock-refusal-rc → exit 77 (same rc as the flock -n arm; one cause, one code)" \
    test "$RC" -eq 77
assert "H12a: bounded wait actually elapsed (queued, not an instant refuse like H11a)" \
    test "$Q_H12A_WAITED_S" -ge 1
assert "H12a: sentinel file in <lane>/target still present (lane NOT clobbered)" \
    test -f "$Q_HELD_LANE/target/SENTINEL.txt"
assert "H12a: cp NEVER invoked (refused before clone)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"
_q_release_lane_lock

# H12b: COMPAT PIN for the queue arm — same fixture, same WAIT=1, flag ABSENT
# → 75 unchanged. H11b's guarantee, extended to the second refusal arm: both
# arms must keep today's default so an unpatched dark-factory is unaffected.
_q_hold_lane_lock h12b
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_LANE_LOCK_WAIT=1 \
    run_helper_real "$Q_BASE" "$Q_HELD_LANE" --fresh-checkout --lane-lock
# NOTE: no --distinct-lock-refusal-rc above -- that omission IS the test.

assert "H12b: COMPAT PIN — queue timeout, flag ABSENT → exit 75 unchanged" \
    test "$RC" -eq 75
assert "H12b: COMPAT PIN — sentinel survives (default queue-refusal path otherwise unchanged)" \
    test -f "$Q_HELD_LANE/target/SENTINEL.txt"
_q_release_lane_lock

# ── H13: the OPERATOR-facing half of the discriminant — the stable, machine-
# greppable LANE_LOCK_CONTENDED marker on stderr, emitted by BOTH refusal arms
# UNCONDITIONALLY (task 5568). Why the token exists and why it is ungated:
# §9.5 inv.11, not restated here (G7). ─────────────────────────────────────
#
# NOT redundant with H11/H12, which own the rc contract: these cases assert
# only the token, on the two arms x flag-absent/flag-present. The token also
# UNIFIES the arms, whose prose otherwise diverges ("held by a live consumer"
# vs "still held ... after waiting Ns") with no shared grep. Non-vacuity — the
# token must be ABSENT on a success run — is pinned by H11c.

# H13a: flock -n refusal, flag ABSENT → the token is present. This is the case
# that matters most: it is the UNPATCHED-FLEET configuration, where the rc is
# still the ambiguous 75 and the token is the ONLY discriminant.
_q_hold_lane_lock h13a
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_HELD_LANE" --fresh-checkout --lane-lock

assert "H13a: flock -n refusal, flag ABSENT → stderr carries the $Q_CONTENDED_TOKEN marker (the ONLY discriminant on an unpatched fleet)" \
    bash -c 'printf "%s\n" "$2" | grep -q "$1"' _ "$Q_CONTENDED_TOKEN" "$ERR_OUT"
_q_release_lane_lock

# H13b: flock -w timeout refusal, flag ABSENT → the SAME token, so one grep
# catches both arms despite their divergent prose.
_q_hold_lane_lock h13b
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_LANE_LOCK_WAIT=1 \
    run_helper_real "$Q_BASE" "$Q_HELD_LANE" --fresh-checkout --lane-lock

assert "H13b: queue-timeout refusal emits the SAME $Q_CONTENDED_TOKEN token (one grep catches BOTH arms)" \
    bash -c 'printf "%s\n" "$2" | grep -q "$1"' _ "$Q_CONTENDED_TOKEN" "$ERR_OUT"
_q_release_lane_lock

# H13c: both arms WITH the flag → the token is STILL present, i.e. it is
# unconditional rather than an artifact of the new code path. (The rc under
# the flag is H11a's/H12a's contract, not re-asserted here.)
_q_hold_lane_lock h13c1
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$Q_BASE" "$Q_HELD_LANE" --fresh-checkout --lane-lock "$Q_H11_FLAG"

assert "H13c: flock -n refusal WITH the flag → the $Q_CONTENDED_TOKEN token is STILL present (unconditional, not gated on the flag)" \
    bash -c 'printf "%s\n" "$2" | grep -q "$1"' _ "$Q_CONTENDED_TOKEN" "$ERR_OUT"
_q_release_lane_lock

_q_hold_lane_lock h13c2
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_LANE_LOCK_WAIT=1 \
    run_helper_real "$Q_BASE" "$Q_HELD_LANE" --fresh-checkout --lane-lock "$Q_H11_FLAG"

assert "H13c: queue-timeout refusal WITH the flag → the $Q_CONTENDED_TOKEN token is STILL present on the queue arm too" \
    bash -c 'printf "%s\n" "$2" | grep -q "$1"' _ "$Q_CONTENDED_TOKEN" "$ERR_OUT"
_q_release_lane_lock

# ─────────────────────────────────────────────────────────────────────────────
# Block S — relocation sweep must not advance build-script freshness references
# (task 5630). Uses run_helper_real (real cp/find/touch + stub git), mirroring
# Block P's fixture recipe and Block L's "pre-stamp to an explicit timestamp,
# assert the exact mtime after the run" technique.
#
# Contract under test: no seed step may leave a build-script replay file
# (target/**/build/<pkg>-<hash>/output) at or after a delta-touched source's
# mtime. The normative statement, the measured cargo repro behind it, and the
# operator remedy live in ONE place — docs/prds/warm-lane-pool-cow-seeding.md
# §9.5 inv.12 — deliberately NOT restated here (G7 no-lockstep-duplication).
#
#   Placed BEFORE Block R deliberately: per Block R's ordering note, a block
#   appended after R2/R5 gets no shared-trash-litter coverage from R1.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block S: relocation sweep preserves build-script freshness refs (task 5630) ---"

S_FOREIGN="/tmp/foreign-buildroot-S5630"

S_BASE_PARENT="$(mktemp -d /tmp/test-seed-S-parent-XXXXXX)"
S_BASE="$S_BASE_PARENT/target"
S_LANE="$(make_isolated_lane S-lane)"
_TMPDIRS+=("$S_BASE_PARENT")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$S_BASE_PARENT/.warm-base-meta"
printf '%s' "$S_FOREIGN" > "${S_BASE}.buildroot"

# A `cc`/`cxx_build`-shaped build dir: `output` carries an OUT_DIR-absolute
# rustc-link-search (which is why occt/openvdb/eval are rewritten in a real
# lane, while gmsh/conformance — emitting only /opt/reify-deps paths — are not)
# alongside the rerun-if-changed line whose staleness `output`'s mtime gates.
mkdir -p "$S_BASE/debug/build/fakecc-1111"
cat > "$S_BASE/debug/build/fakecc-1111/output" <<EOF
cargo:rerun-if-changed=crates/k/cpp/wrapper.cpp
cargo:rustc-link-search=native=$S_FOREIGN/target/debug/build/fakecc-1111/out
EOF
printf '%s' "$S_FOREIGN/target/debug/build/fakecc-1111/out" \
    > "$S_BASE/debug/build/fakecc-1111/root-output"

# Pre-stamp both replay files to a fixed PAST timestamp and capture it at FULL
# precision. This makes every assertion below an exact comparison — no sleep, no
# dependence on wall-clock granularity (Block L's technique). /bin/cp -a in the
# run_helper_real cp stub preserves sub-second mtimes, so the lane copy inherits
# this stamp exactly and only the sed can move it.
#
# The .123456789 fraction is LOAD-BEARING, not decoration: cargo compares mtimes
# at nanosecond resolution, so a restore that truncates to whole seconds
# (stat '%Y' + touch -d @epoch) must FAIL S1c. On a zero-nanosecond fixture such
# a restore would pass unnoticed, leaving half the contract unpinned.
touch -d "2024-06-01 00:00:00.123456789" "$S_BASE/debug/build/fakecc-1111/output" \
                                         "$S_BASE/debug/build/fakecc-1111/root-output"
S_OUTPUT_MTIME="$(stat -c '%y' "$S_BASE/debug/build/fakecc-1111/output")"
S_ROOT_OUTPUT_MTIME="$(stat -c '%y' "$S_BASE/debug/build/fakecc-1111/root-output")"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$S_BASE" "$S_LANE" --fresh-checkout

S_LANE_RP="$(realpath -m "$S_LANE")"
S_LANE_OUTPUT="$S_LANE/target/debug/build/fakecc-1111/output"
S_LANE_ROOT_OUTPUT="$S_LANE/target/debug/build/fakecc-1111/root-output"

# S1a: success contract
assert "S1a: --fresh-checkout exits 0 (relocation sweep ran)" test "$RC" -eq 0

# S1b: REGRESSION PIN — the task-5126 relocation itself must still work. Without
# this, "preserve the mtime" could be trivially satisfied by not rewriting at all.
assert "S1b: build/fakecc-1111/output contains the LANE root prefix (relocation still works)" \
    bash -c 'grep -qF "$1" "$2"' _ "$S_LANE_RP" "$S_LANE_OUTPUT"
assert "S1b: build/fakecc-1111/output no longer contains the FOREIGN root" \
    bash -c '! grep -qF "$1" "$2"' _ "$S_FOREIGN" "$S_LANE_OUTPUT"
assert "S1b: build/fakecc-1111/root-output reads the lane's OUT_DIR (no FOREIGN substring)" \
    bash -c '[ "$(cat "$1")" = "'"$S_LANE_RP"'/target/debug/build/fakecc-1111/out" ]' \
    _ "$S_LANE_ROOT_OUTPUT"

# S1c: THE NEW CONTRACT — the rewrite must NOT advance the freshness reference.
# Compared at FULL precision (stat '%y', nanoseconds included), never '%Y': a
# whole-second-truncated restore still leaves cargo mis-gating inside that second.
S1C_OUTPUT_MTIME="$(stat -c '%y' "$S_LANE_OUTPUT")"
assert "S1c: rewritten output KEEPS its pre-sed mtime to the NANOSECOND ($S_OUTPUT_MTIME) — sed must not advance cargo's freshness reference" \
    bash -c '[ "$1" = "$2" ]' _ "$S1C_OUTPUT_MTIME" "$S_OUTPUT_MTIME"

S1C_ROOT_OUTPUT_MTIME="$(stat -c '%y' "$S_LANE_ROOT_OUTPUT")"
assert "S1c: rewritten root-output KEEPS its pre-sed mtime to the NANOSECOND ($S_ROOT_OUTPUT_MTIME)" \
    bash -c '[ "$1" = "$2" ]' _ "$S1C_ROOT_OUTPUT_MTIME" "$S_ROOT_OUTPUT_MTIME"

# ── S2: fail-closed post-condition — a build-script `output` that is NEWER than
# a delta-touched tracked source must ABORT the seed, not ship a lane cargo will
# mis-gate. This deliberately SYNTHESISES the inversion (base `output` stamped
# into the future) so the guard itself is tested rather than the happy path;
# mtime preservation alone cannot catch an inversion that was already baked into
# the base. Same precedent as _assert_no_stale_delta_stamp, which shipped
# alongside the _touch_git_delta fix in esc-3468-75: a silently-wrong mtime is a
# FALSE GREEN, the failure class no downstream test can catch by construction.
# ─────────────────────────────────────────────────────────────────────────────
_s_make_fixture() {
    # <prefix> <output_touch_stamp> — echoes "<base>|<lane>|<delta_path>".
    #
    # The base parent is minted via make_isolated_lane (i.e. under $_LANE_ROOT),
    # NOT bare /tmp, for the SUBSHELL reason documented above _LANE_ROOT: every
    # call site reads this function's stdout via command substitution
    # (IFS='|' read ... <<< "$(_s_make_fixture ...)"), so a `_TMPDIRS+=(...)`
    # here would be silently discarded when the subshell exits and every base
    # parent would leak into bare /tmp — measured litter, the same hazard tasks
    # 5590/5609 hardened the rest of this file against (and the same resolution
    # I_SC_BASE_PARENT uses at ~L1140). Anchoring on $_LANE_ROOT, which is
    # already ONE _TMPDIRS entry reclaimed by the cleanup() EXIT trap, needs no
    # registration from inside the subshell at all.
    local prefix="$1" stamp="$2" parent base lane delta
    parent="$(make_isolated_lane "$prefix-base")"
    base="$parent/target"
    lane="$(make_isolated_lane "$prefix-lane")"
    printf 'RUSTFLAGS=\nINVOCATION=\n' > "$parent/.warm-base-meta"
    printf '%s' "$S_FOREIGN" > "${base}.buildroot"
    mkdir -p "$base/debug/build/fakecc-1111"
    cat > "$base/debug/build/fakecc-1111/output" <<EOF
cargo:rerun-if-changed=crates/k/cpp/wrapper.cpp
cargo:rustc-link-search=native=$S_FOREIGN/target/debug/build/fakecc-1111/out
EOF
    touch -d "$stamp" "$base/debug/build/fakecc-1111/output"
    # The delta: a tracked source the caller passes via --touch, i.e. exactly
    # the `cargo:rerun-if-changed` path baked into `output` above.
    delta="$lane/crates/k/cpp/wrapper.cpp"
    mkdir -p "$(dirname "$delta")"
    echo '// wrapper' > "$delta"
    printf '%s|%s|%s' "$base" "$lane" "$delta"
}

# S2 RED arm: base `output` pre-stamped one hour into the FUTURE.
IFS='|' read -r S2_BASE S2_LANE S2_DELTA \
    <<< "$(_s_make_fixture S2 "$(date -d 'now + 1 hour' '+%Y-%m-%d %H:%M:%S')")"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$S2_BASE" "$S2_LANE" --fresh-checkout --touch "$S2_DELTA"

assert "S2a: seed exits NON-zero when a build-script output is newer than a delta-touched source" \
    test "$RC" -ne 0
assert "S2b: STDOUT is EMPTY on the freshness-inversion abort (caller falls back to a cold rebuild)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
# Both path assertions are scoped to [error]-level lines. The guard's SUCCESS
# info line names the very same two paths, so an unscoped grep over all of stderr
# passes even when the guard did not fire at all (measured against a mutant).
assert "S2c: an [error] line names the offending build-script output path" \
    bash -c 'printf "%s\n" "$1" | grep "\[error\]" | grep -qF "$2"' _ "$ERR_OUT" \
    "$S2_LANE/target/debug/build/fakecc-1111/output"
assert "S2c: an [error] line names the delta path that is not newer than it" \
    bash -c 'printf "%s\n" "$1" | grep "\[error\]" | grep -qF "$2"' _ "$ERR_OUT" "$S2_DELTA"

# S2 POSITIVE CONTROL: identical fixture except `output` carries the ordinary
# past (base-build) stamp. Proves the guard does not fire on the normal path —
# without this, S2a-c would also pass if the seed simply always aborted.
IFS='|' read -r S2P_BASE S2P_LANE S2P_DELTA \
    <<< "$(_s_make_fixture S2p "2024-06-01T00:00:00")"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$S2P_BASE" "$S2P_LANE" --fresh-checkout --touch "$S2P_DELTA"

assert "S2d: positive control: past-stamped output + same delta exits 0 (guard does not over-fire)" \
    test "$RC" -eq 0
assert "S2d: positive control: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$S2P_LANE/target"

# ── S2e/S2f: the TIE and SUB-SECOND boundaries — the one behavioural difference
# that motivates comparing with bash's `-nt` instead of `stat -c '%Y'` integers.
# S2a-d above sit an hour / two years away from the boundary, so an integer-%Y
# refactor would keep them all green while re-opening exactly the same-second and
# sub-second inversion holes cargo still mis-gates on (§9.5 inv.12).
#
# WHY the delta path here is itself a replay file: the seed stamps every delta
# path to NOW (`touch "${TOUCH_PATHS[@]}"`, no -d), so a fixture CANNOT pre-arrange
# a tie against a base-stamped `output` — the pre-stamp is overwritten during the
# run. Passing the lane's own `output` via --touch makes the oldest delta and the
# newest `output` the SAME inode, which is a tie by construction and needs no
# wall-clock luck. Artificial as a delta path, exact as an operator pin.
IFS='|' read -r S2T_BASE S2T_LANE S2T_DELTA \
    <<< "$(_s_make_fixture S2t "2024-06-01 00:00:00.123456789")"
S2T_OUTPUT="$S2T_LANE/target/debug/build/fakecc-1111/output"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$S2T_BASE" "$S2T_LANE" --fresh-checkout --touch "$S2T_OUTPUT"

assert "S2e: EXACT TIE (oldest delta IS the newest output) aborts — cargo's comparison is strict >, so equal is NOT newer" \
    test "$RC" -ne 0
assert "S2e: tie abort leaves STDOUT empty (cold-rebuild fallback)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "S2e: tie abort names the offending build-script output path on an [error] line" \
    bash -c 'printf "%s\n" "$1" | grep "\[error\]" | grep -qF "$2"' _ "$ERR_OUT" "$S2T_OUTPUT"

# S2f: STRICT SUB-SECOND inversion between two DISTINCT files — output newer
# than the delta by MILLISECONDS, not seconds. Built from the seed's own two
# separate touch phases rather than one multi-file `touch`: coreutils resolves
# UTIME_NOW once per `touch` invocation, so `touch a b` stamps BOTH paths with
# the identical nanosecond value (measured) — a tie, i.e. S2e again. Instead the
# source goes through the earlier explicit --touch phase and the replay file
# through the LATER git-delta phase (a `git diff` subprocess apart), so
# output > delta strictly, inside the same second.
# This is the arm an integer-%Y comparison would wave through (equal seconds)
# while `-nt` aborts. The phase ORDER makes the abort unconditional: if the two
# phases straddle a second boundary the arm merely degenerates into S2a's
# whole-second inversion and still aborts, so it can never flake.
IFS='|' read -r S2S_BASE S2S_LANE S2S_DELTA \
    <<< "$(_s_make_fixture S2s "2024-06-01 00:00:00.123456789")"
S2S_OUTPUT="$S2S_LANE/target/debug/build/fakecc-1111/output"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    REIFY_TEST_GIT_DIFF_FILES="target/debug/build/fakecc-1111/output" \
    run_helper_real "$S2S_BASE" "$S2S_LANE" --fresh-checkout \
        --touch "$S2S_DELTA" --base-commit shaS2s

# Log the measured pair so a reader can see WHICH regime the arm exercised on
# this run: same second + strictly later output ns = the discriminating case;
# identical ns = the S2e tie; different seconds = the degenerate S2a case. All
# three must abort. Diagnostic only — never gating.
echo "    S2f measured: delta=$(stat -c '%y' "$S2S_DELTA" 2>/dev/null || echo '<gone>') output=$(stat -c '%y' "$S2S_OUTPUT" 2>/dev/null || echo '<gone>')"

assert "S2f: SUB-SECOND inversion (delta touched MILLISECONDS before the output) aborts" \
    test "$RC" -ne 0
assert "S2f: sub-second abort leaves STDOUT empty (cold-rebuild fallback)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "S2f: sub-second abort names BOTH the output and the delta path on [error] lines" \
    bash -c 'e="$(printf "%s\n" "$1" | grep "\[error\]")"
             printf "%s\n" "$e" | grep -qF "$2" && printf "%s\n" "$e" | grep -qF "$3"' \
    _ "$ERR_OUT" "$S2S_OUTPUT" "$S2S_DELTA"

# ── S3: a --fresh-checkout that resolves NO delta-touch base must SAY SO. All
# three tiers of the resolution (--base-commit / <base>.basecommit /
# .warm-base-meta BASE_COMMIT) coming back empty means nothing is delta-touched,
# so every tracked source keeps the 2020-01-01 bulk stamp and cannot out-date
# any cloned build artifact — the same freshness inversion S1/S2 target, reached
# by a different route. Today that condition is COMPLETELY SILENT: the
# `if [ -n "$EFFECTIVE_BASE_COMMIT" ]` block simply has no else.
#
# Scope note: this asserts a warn ONLY, and S3b pins that the warn stays purely
# additive. The ruling that question was awaiting landed as §9.5 inv.13 (task
# 5632): a no-base seed now REFUSES — but only when the clone carries recorded
# prior compilations for the bulk stamp to wrongly re-Freshen. S3's fixture
# deliberately carries NO .fingerprint dir, so it stays BELOW inv.13's hazard
# gate and keeps asserting the accept path unchanged. That makes S3a/S3b the
# below-threshold CONTROL for Block U's U1; see Block U for the refusal arms.
# ─────────────────────────────────────────────────────────────────────────────
S3_BASE_PARENT="$(mktemp -d /tmp/test-seed-S3-parent-XXXXXX)"
S3_BASE="$S3_BASE_PARENT/target"
S3_LANE="$(make_isolated_lane S3-lane)"
_TMPDIRS+=("$S3_BASE_PARENT")
# Sidecar deliberately carries NO BASE_COMMIT line; no ${S3_BASE}.basecommit
# stamp is written; and no --base-commit flag is passed below. All three tiers
# resolve empty.
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$S3_BASE_PARENT/.warm-base-meta"
mkdir -p "$S3_BASE/debug"
echo "artifact" > "$S3_BASE/debug/artifact.a"
mkdir -p "$S3_LANE/src"
echo 'pub fn tracked() {}' > "$S3_LANE/src/tracked.rs"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$S3_BASE" "$S3_LANE" --fresh-checkout

# S3a: the condition is diagnosable from stderr. Deliberately ONE assertion on
# the behavioural contract ("this condition is no longer silent") — the earlier
# revision also pinned the tier names, the literal 2020-01-01 and a
# case-insensitive "fresh" in the message body, which locked cosmetic wording:
# any reword of the remediation sentence broke the suite with no behaviour change
# and caught no failure mode this line misses.
assert "S3a: STDERR carries a [warn] line about the unresolved delta-touch base" \
    bash -c 'printf "%s\n" "$1" | grep -q "\[warn\].*delta-touch base"' _ "$ERR_OUT"

# S3b: REGRESSION PIN — the warn is purely ADDITIVE. Exit code, stdout and the
# bulk stamp itself are all unchanged (the D5 contract Block D's D1 and
# test_warm_lane_pool.sh:398 both depend on).
assert "S3b: no-base seed still exits 0 (warn is additive, not a new failure)" \
    test "$RC" -eq 0
assert "S3b: STDOUT is still exactly <lane>/target" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$S3_LANE/target"
S3B_SRC_MTIME="$(stat -c '%Y' "$S3_LANE/src/tracked.rs")"
assert "S3b: tracked source still carries the 2020-01-01 bulk stamp ($EPOCH_2020) — D5 unchanged" \
    test "$S3B_SRC_MTIME" -eq "$EPOCH_2020"

# ─────────────────────────────────────────────────────────────────────────────
# Block T: run_helper_real stub-dir lifetime guard (task #5633) — the
# per-invocation PATH stub dir run_helper_real mints (real_stub_dir, minted
# just above the REAL_STUB_DIR assignment in that function) must outlive the
# invocation, not just the DIRECT `bash "$SCRIPT"` child. scripts/seed-warm-lane.sh's
# trash rm is a DETACHED GRANDCHILD — `{ rm -rf "$RESEED_TRASH" ...; } 9<&- &`
# (scripts/seed-warm-lane.sh:1133, and the orphan sweep at :792) — forked
# before seed exits but resolving `rm` through PATH at an arbitrary LATER
# instant. If run_helper_real unlinks its stub dir the moment the direct
# child exits, that grandchild's PATH lookup can miss the no-op
# REIFY_TEST_PIN_RESEED_TRASH stub and find the real /bin/rm instead, which
# genuinely deletes the trash the PIN stub exists to pin on disk — the
# intermittent I14g / M-setup failure mode (see the causal waits added ahead
# of each, above, per the #5633 code review).
#
# Placement note: inserted AFTER Block S and BEFORE Block R (not appended at
# EOF). Block R's own ordering note below states that a block appended AFTER
# Block R gets no R1 shared-trash / R3 bare-/tmp detector coverage unless it
# re-asserts them itself — inserting here keeps this block's run_helper_real
# call inside the existing sweep, with no duplicated checker.
#
# T2 (code review amendment): an earlier revision of this block also carried
# a "T2" assertion that reused this fixture to re-prove the same trash-
# survives-the-detached-rm property, purely within Block T. Once I14g and
# M-setup each gained their own causal wait (above), T2 became a pure
# duplicate of what those two now already prove end-to-end on their own
# fixtures, so it was removed rather than kept as a third copy of the same
# check. T1/T1b (this block's own lifetime contract) and T3/T4 (below, which
# validate the wait helper and the failure mode themselves) are unaffected.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block T: run_helper_real stub-dir lifetime guard (task #5633) ---"

# Fixture: direct copy of I14's recipe (L1199-1221) — a base with a real
# artifact + .warm-base-meta, and a lane with a NON-EMPTY target so seed
# takes the rename-to-trash path and REIFY_TEST_PIN_RESEED_TRASH has
# something to pin.
T_BASE_PARENT="$(mktemp -d /tmp/test-seed-T-parent-XXXXXX)"
T_BASE="$T_BASE_PARENT/target"
_TMPDIRS+=("$T_BASE_PARENT")
mkdir -p "$T_BASE/debug"
echo "base artifact" > "$T_BASE/debug/base_artifact.a"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$T_BASE_PARENT/.warm-base-meta"

T_LANE="$(make_isolated_lane T-lane)"
mkdir -p "$T_LANE/src"
echo "fn main() {}" > "$T_LANE/src/main.rs"
# Lane target: non-empty so the rename path triggers (seed renames it to trash).
mkdir -p "$T_LANE/target"
echo "stale" > "$T_LANE/target/stale.a"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_TEST_PIN_RESEED_TRASH=1 \
    run_helper_real "$T_BASE" "$T_LANE" --fresh-checkout

# T0/T0b: fixture sanity, mirroring I14/I14b.
assert "T0: seed exits 0 (fixture sanity, mirrors I14)" test "$RC" -eq 0
assert "T0b: STDOUT is <lane>/target (fixture sanity, mirrors I14b)" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$T_LANE/target"

# T1: AFTER run_helper_real has returned, the invocation's stub dir must
# still exist and still be usable — this is the exact contract the seed's
# DETACHED trash rm depends on (see the Block T header comment above).
# Pre-#5633, run_helper_real unlinked real_stub_dir eagerly, as soon as the
# direct child exited, so REAL_STUB_DIR would be unset and this would FAIL.
# "${REAL_STUB_DIR:-}" passed as a positional arg keeps this a plain assert
# FAILURE rather than a set -u abort while the global does not exist yet.
assert "T1: run_helper_real's stub dir survives the invocation and still holds executable cp/rm stubs" \
    bash -c '[ -n "$1" ] && [ -d "$1" ] && [ -x "$1/cp" ] && [ -x "$1/rm" ]' _ "${REAL_STUB_DIR:-}"

# T1b: the surviving stub dir is parented at the per-run root, and that root
# is itself registered in _TMPDIRS — i.e. teardown is DEFERRED to cleanup()'s
# EXIT trap, not simply skipped. Mirrors R4's ${_REAL_LANES_FILE:-/nonexistent}
# defensive idiom below and cleanup()'s own "${_TMPDIRS[@]+${_TMPDIRS[@]}}"
# expansion near the top of the file, so a not-yet-implemented $_REAL_STUB_ROOT
# is a plain assert FAILURE, not a set -u abort of the whole suite.
assert "T1b: the stub dir is parented at _REAL_STUB_ROOT, which is registered in _TMPDIRS for EXIT-trap reclaim" \
    bash -c '
        root="$1"; stub="$2"; shift 2
        [ -n "$stub" ] || exit 1
        [ "$(dirname "$stub")" = "$root" ] || exit 1
        for t in "$@"; do
            [ "$t" = "$root" ] && exit 0
        done
        exit 1
    ' _ "${_REAL_STUB_ROOT:-/nonexistent}" "${REAL_STUB_DIR:-/nonexistent-stub}" "${_TMPDIRS[@]+${_TMPDIRS[@]}}"

# T3: discrimination positive control (R6b spirit) for _calls_file_has_trash_rm
# -- proves the predicate the I14g/M-setup causal waits above poll on cannot
# pass vacuously if the recorder or the trash glob is ever broken. Synthetic
# calls files under $_LANE_ROOT (no real seed run needed): one with only
# non-trash rm lines, one with a genuine trash line.
T3_NONTRASH_CALLS="$_LANE_ROOT/.t3-nontrash-calls"
T3_TRASH_CALLS="$_LANE_ROOT/.t3-trash-calls"
printf 'rm -rf /some/other/path\ncp -a /a /b\n' > "$T3_NONTRASH_CALLS"
printf 'rm -rf /some/other/path\nrm -rf /pool-sibling/.reseed-trash/T3-lane.12345\n' > "$T3_TRASH_CALLS"
_t3_discriminates() {
    local nontrash="$1" trash="$2"
    # Deadline kept short (1s) so this control stays fast -- it is EXPECTED
    # to time out against the non-trash log.
    _wait_until 1 _calls_file_has_trash_rm "$nontrash" && return 1
    _wait_until 1 _calls_file_has_trash_rm "$trash"
}
assert "T3: _calls_file_has_trash_rm discriminates -- times out on a calls file with no trash rm line, succeeds on one that has one" \
    _t3_discriminates "$T3_NONTRASH_CALLS" "$T3_TRASH_CALLS"

# T4/T4b: two-armed mechanism control (R2/R5 spirit; task #5633 code review
# amendment). Proves T1 guards a REAL failure mode, not a hypothetical, AND
# that the stub dir's LIFETIME specifically -- not some other incidental
# difference between the two runs -- is the causal variable. Both arms
# reproduce the pre-#5633 shape end-to-end with synthetic/throwaway state: a
# stub dir holding the SAME no-op-on-trash-paths rm stub the PIN mode
# installs (via the shared REIFY_TEST_TRASH_GLOB_* env vars, same source as
# run_helper_real's PIN/SLEEP stubs above), then a detached grandchild --
# launched BEFORE either arm's teardown decision, exactly like seed's own
# trash rm -- whose PATH lookup is held back by a GO marker until AFTER that
# decision has taken effect (a causal ordering, not a sleep race), followed
# by a DONE marker written the instant the grandchild's rm attempt returns
# (whichever binary it resolved to), so both arms can be asserted
# deterministically instead of one of them racing a timeout. All state lives
# under $_LANE_ROOT (reclaimed by the EXIT trap); nothing here touches a
# real seed invocation or $CALLS_FILE.
#
#   _t4_control unlink: unlinks the stub dir EAGERLY (the pre-#5633
#     teardown) before releasing the grandchild -- its PATH lookup then
#     finds only the real /bin/rm, which genuinely deletes the trash.
#     Expected: trash GONE.
#   _t4_control retain: leaves the stub dir in place past the release -- the
#     grandchild's PATH lookup instead finds the no-op stub. Expected:
#     trash SURVIVES.
#
# Without the retain arm, the unlink arm alone cannot discriminate a working
# stub from a broken/empty/never-executed one: "the trash is gone" is also
# just what /bin/rm -rf does on its own, so only the pair proves the stub
# dir's lifetime is the variable actually being exercised, not incidental.
#
# Known, deliberately unexercised gap: seed's REAL grandchild is forked from
# WITHIN its own already-running bash process (a plain `{ ... } 9<&- &`),
# which can inherit a warm command-hash-table entry for `rm` from an earlier
# real invocation in that SAME process (e.g. the foreground build-dir
# invalidation rm, scripts/seed-warm-lane.sh:949) -- whereas this control's
# grandchild is a fresh `bash -c`, which always does a cold PATH search.
# Both are genuine PATH-resolution hazards this control's shape can trigger;
# the warm-hash variant is instead exercised incidentally by the real seed
# invocations above (I14, M, H4) rather than reproduced synthetically here.
_t4_control() {
    local mode="$1"
    local stub area trash go pid_file done_marker
    stub="$(mktemp -d "$_LANE_ROOT/t4-${mode}-stub-XXXXXX")"
    cat > "$stub/rm" << 'T4_RM_STUB_EOF'
#!/usr/bin/env bash
echo "rm $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
for arg in "$@"; do
    case "$arg" in
        ${REIFY_TEST_TRASH_GLOB_LEGACY:-*target.reseed-trash.*}|${REIFY_TEST_TRASH_GLOB_SIBLING:-*/.reseed-trash/*}) exit 0 ;;
    esac
done
exec /bin/rm "$@"
T4_RM_STUB_EOF
    chmod +x "$stub/rm"

    area="$(mktemp -d "$_LANE_ROOT/t4-${mode}-area-XXXXXX")"
    trash="$area/.reseed-trash/T4-${mode}-lane.99999"
    mkdir -p "$trash"
    echo "sentinel" > "$trash/SENTINEL.txt"

    go="$_LANE_ROOT/.t4-${mode}-go-marker"
    pid_file="$_LANE_ROOT/.t4-${mode}-child-pid"
    done_marker="$_LANE_ROOT/.t4-${mode}-done-marker"

    # Detached grandchild, same shape as scripts/seed-warm-lane.sh:1133:
    # forked NOW, inside this synchronous subshell (before the subshell
    # itself returns), but blocks on the GO marker so its PATH lookup
    # provably happens AFTER this function's teardown decision below.
    # `echo $! > "$pid_file"` runs inside the subshell, right after
    # backgrounding, so the grandchild's PID is visible to the caller via
    # the file even though $! itself does not survive the subshell exiting.
    ( PATH="$stub:$PATH" \
      REIFY_TEST_TRASH_GLOB_LEGACY="$_TRASH_GLOB_LEGACY" \
      REIFY_TEST_TRASH_GLOB_SIBLING="$_TRASH_GLOB_SIBLING" \
      bash -c 'while [ ! -f "$1" ]; do sleep 0.02; done; rm -rf "$2"; : > "$3"' \
          _ "$go" "$trash" "$done_marker" & echo $! > "$pid_file" )

    if [ "$mode" = "unlink" ]; then
        # The pre-#5633 teardown: unlink the stub dir the instant the
        # "direct child" (the subshell above, which has already returned)
        # has exited.
        /bin/rm -rf "$stub"
    fi
    # NOW let the grandchild proceed. mode=unlink: its PATH lookup happens
    # after the stub dir is already gone, so `rm` can only resolve to the
    # real /bin/rm. mode=retain: the stub dir is untouched (still reachable
    # via PATH, and not reclaimed until this file's EXIT trap), so `rm`
    # resolves to the no-op stub.
    : > "$go"

    if [ -s "$pid_file" ]; then
        _BGPIDS+=("$(cat "$pid_file")")
    fi

    # Deterministic completion signal for EITHER arm -- see the DONE-marker
    # rationale in the header comment above.
    _wait_until 10 test -f "$done_marker" || return 1

    case "$mode" in
        unlink) [ ! -e "$trash" ] ;;
        retain) [ -d "$trash" ] && [ -f "$trash/SENTINEL.txt" ] ;;
    esac
}

assert "T4: mechanism positive control — a detached grandchild whose stub dir is gone before its PATH lookup really does delete the trash (proves T1 guards a real failure mode)" \
    _t4_control unlink

assert "T4b: counterfactual control — the SAME detached grandchild whose stub dir is instead RETAINED past its PATH lookup does NOT delete the trash (proves the stub dir's lifetime, not incidental timing, is the causal variable T1 guards)" \
    _t4_control retain

# ─────────────────────────────────────────────────────────────────────────────
# Block U — a --fresh-checkout that resolves NO delta-touch base must REFUSE,
# not return a lane whose freshness claim it cannot substantiate (task 5632).
#
# Contract under test, its reasoning, the causal `.fingerprint` gate below and
# the opt-out knob all live in ONE place — docs/prds/warm-lane-pool-cow-seeding.md
# §9.5 inv.13 — deliberately NOT restated here or in the arm comments below
# (G7 no-lockstep-duplication).
# Block S's S3a/S3b are the BELOW-THRESHOLD control for U1: S3's fixture carries
# no .fingerprint dir, so it stays under inv.13's hazard gate and keeps asserting
# the accept path unchanged.
#
# The arms are built as two-sided pairs, so that no single-sided edit to the
# guard can stay green: U1/U1b straddle the causal .fingerprint gate AND the
# guard's call position relative to the invalidation sweep; U1b/U1e straddle
# the probe's find-traversal outcome itself (succeeds vs FAILS with EACCES on
# an unreadable subdirectory); U1c/U1d straddle the -maxdepth 3 probe bound;
# U2/U2b straddle what the guard keys on (base resolution — NOT an empty delta
# set, and NOT an explicit --touch list); U3/U3b/U3c straddle the opt-out
# knob's exact-"1" contract; U4/U4b/U4c straddle `absent` vs `present but
# empty` in the refusal's per-tier attribution, with U4b/U4d straddling the
# .basecommit trailing-whitespace trim that the same distinction rests on.
#
#   Placed AFTER the task-#5633 Block T and BEFORE Block R, for the same reason
#   Block S is: per Block R's ordering note, a block appended after R2/R5 gets
#   no shared-trash-litter coverage from R1.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block U: no delta-touch base ⇒ seed refuses (task 5632) ---"

_u_make_fixture() {
    # <prefix> <with_fingerprint:0|1> [fingerprint_parent_relpath] —
    # echoes "<base>|<lane>".
    #
    # Base parent and lane are BOTH minted via make_isolated_lane (i.e. under
    # $_LANE_ROOT), NOT bare /tmp, for the SUBSHELL reason documented above
    # _s_make_fixture and _LANE_ROOT: every call site reads this function's
    # stdout via command substitution, so a `_TMPDIRS+=(...)` here would be
    # silently discarded when the subshell exits and every base parent would
    # leak into bare /tmp.
    #
    # The optional 3rd arg is the .fingerprint dir's PARENT, relative to
    # <base>, and defaults to the depth-2 `debug` shape. U1c/U1d override it to
    # drive the guard's -maxdepth 3 bound from both sides; nothing else should
    # need it.
    local prefix="$1" with_fingerprint="$2" fp_parent="${3:-debug}" parent base lane
    parent="$(make_isolated_lane "$prefix-base")"
    base="$parent/target"
    lane="$(make_isolated_lane "$prefix-lane")"
    # All three base-resolution tiers deliberately empty: the sidecar carries NO
    # BASE_COMMIT line, no ${base}.basecommit stamp is written, and no
    # --base-commit flag is passed (except by U2, which is the discriminator).
    printf 'RUSTFLAGS=\nINVOCATION=\n' > "$parent/.warm-base-meta"
    mkdir -p "$base/debug"
    echo "artifact" > "$base/debug/artifact.a"
    # ORDERING WITNESS for the guard's documented call position (before the
    # non-relocatable build-dir invalidation sweep). debug/build/tauri-TTTT
    # matches the sweep's `tauri-*` allow-list glob, so it survives IFF the
    # guard aborted first and is GONE on every path where the seed proceeded —
    # a two-sided pin (U1 asserts survival, U1b asserts deletion) of a property
    # that is otherwise only stated in a comment. The marker file is
    # deliberately NOT named `output`: that is the inv.12 guard's freshness
    # reference, and this fixture must exercise inv.13 alone.
    mkdir -p "$base/debug/build/tauri-TTTT"
    echo "marker" > "$base/debug/build/tauri-TTTT/marker"
    if [ "$with_fingerprint" = "1" ]; then
        # Default fp_parent=debug ⇒ target/debug/.fingerprint = depth 2 from
        # LANE_TARGET, inside the guard's probe bound. Reaches the lane via
        # run_helper_real's cp stub (/bin/cp -a), the same propagation mechanism
        # Block D's D4 relies on for debug/artifact.a. Named lib-somepkg, NOT
        # `output`, so this fixture stays clear of the inv.12 guard and only
        # inv.13 is under test here.
        mkdir -p "$base/$fp_parent/.fingerprint/somepkg-1111"
        echo "fp" > "$base/$fp_parent/.fingerprint/somepkg-1111/lib-somepkg"
    fi
    mkdir -p "$lane/src"
    echo 'pub fn tracked() {}' > "$lane/src/tracked.rs"
    printf '%s|%s' "$base" "$lane"
}

# ── U1: THE RED ARM — no base resolves AND the clone carries recorded prior
# compilations for the bulk stamp to wrongly re-Freshen ⇒ refuse.
IFS='|' read -r U1_BASE U1_LANE <<< "$(_u_make_fixture U1 1)"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U1_BASE" "$U1_LANE" --fresh-checkout

assert "U1: seed exits NON-zero when no delta-touch base resolves and the clone carries recorded compilations" \
    test "$RC" -ne 0
assert "U1: STDOUT is EMPTY on the unsubstantiated-base abort (caller falls back to a cold rebuild)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
# Both message assertions are scoped to [error]-level lines. The RETAINED no-base
# [warn] names the very same condition, so an unscoped grep over all of stderr
# passes even when the guard never fired at all — the same measured hazard S2c
# documents for the inv.12 guard.
assert "U1: an [error] line names the unresolved delta-touch base condition" \
    bash -c 'printf "%s\n" "$1" | grep "\[error\]" | grep -q "delta-touch base"' _ "$ERR_OUT"
assert "U1: an [error] line names the .fingerprint evidence found under the lane target" \
    bash -c 'printf "%s\n" "$1" | grep "\[error\]" | grep -qF "$2"' _ "$ERR_OUT" \
    "$U1_LANE/target/debug/.fingerprint"
# ORDERING PIN (positive half; U1b holds the negative half). The guard is
# documented as running BEFORE the non-relocatable build-dir invalidation sweep,
# the inv.8 relocation sweep and the env!() relink, so a doomed seed pays none
# of those walks. debug/build/tauri-TTTT matches the sweep's allow-list glob, so
# its SURVIVAL is direct evidence the abort preceded the sweep. Without this,
# moving the call after the sweep keeps every other U-arm green.
assert "U1: the tauri-* build dir SURVIVES the abort ⇒ the guard ran before the invalidation sweep" \
    test -d "$U1_LANE/target/debug/build/tauri-TTTT"

# ── U1b: POSITIVE CONTROL (green before AND after) — byte-identical fixture
# MINUS the .fingerprint dir. Without this arm U1 would also pass against an
# implementation that simply always aborts on a missing base; this is the arm
# that pins the CAUSAL .fingerprint gate, and it is what keeps Block D and Block
# S's S3 below the threshold by construction rather than by exemption.
IFS='|' read -r U1B_BASE U1B_LANE <<< "$(_u_make_fixture U1b 0)"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U1B_BASE" "$U1B_LANE" --fresh-checkout

assert "U1b: no .fingerprint under the clone ⇒ nothing to mis-gate ⇒ seed still exits 0" \
    test "$RC" -eq 0
assert "U1b: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$U1B_LANE/target"
U1B_SRC_MTIME="$(stat -c '%Y' "$U1B_LANE/src/tracked.rs")"
assert "U1b: tracked source still carries the 2020-01-01 bulk stamp ($EPOCH_2020) — accept path unchanged" \
    test "$U1B_SRC_MTIME" -eq "$EPOCH_2020"
# ORDERING PIN (negative half — the control that gives U1's survival assertion
# its meaning). On a path where the guard does NOT abort, the sweep runs and the
# allow-listed dir is gone; so U1's surviving dir cannot be explained by the
# sweep simply never touching this fixture.
assert "U1b: the tauri-* build dir is GONE when the seed proceeds ⇒ the sweep does run on this fixture" \
    bash -c '[ ! -e "$1" ]' _ "$U1B_LANE/target/debug/build/tauri-TTTT"

# ── U1c/U1d: the guard's -maxdepth 3 probe bound, driven from BOTH sides.
# Why the bound is load-bearing and why both edges are pinned: §9.5 inv.13.
# Local: every other .fingerprint arm in this block (U1/U1b/U2/U3) uses the
# depth-2 `debug/.fingerprint` shape and would stay green under a narrowed
# -maxdepth 2 — without this pair, nothing here fails on a narrowed bound.

# U1c: depth 3 — the cross-compile <triple>/<profile>/.fingerprint shape, which
# is INSIDE the bound and must still be refused.
IFS='|' read -r U1C_BASE U1C_LANE <<< "$(_u_make_fixture U1c 1 x86_64-unknown-linux-gnu/debug)"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U1C_BASE" "$U1C_LANE" --fresh-checkout

assert "U1c: a depth-3 cross-compile <triple>/debug/.fingerprint is INSIDE the probe bound ⇒ still refuses" \
    test "$RC" -ne 0
assert "U1c: depth-3 abort still leaves STDOUT empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "U1c: the [error] line names the depth-3 .fingerprint path it found" \
    bash -c 'printf "%s\n" "$1" | grep "\[error\]" | grep -qF "$2"' _ "$ERR_OUT" \
    "$U1C_LANE/target/x86_64-unknown-linux-gnu/debug/.fingerprint"

# U1d: depth 4 — a .fingerprint nested inside a build-script output dir, OUTSIDE
# the bound. It is not a cargo profile fingerprint dir, so it is deliberately
# not probed and the seed must still proceed. This is the arm a WIDENED bound
# would fail.
IFS='|' read -r U1D_BASE U1D_LANE <<< "$(_u_make_fixture U1d 1 debug/build/somepkg-2222)"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U1D_BASE" "$U1D_LANE" --fresh-checkout

assert "U1d: a depth-4 .fingerprint nested in a build-script out dir is OUTSIDE the bound ⇒ seed proceeds" \
    test "$RC" -eq 0
assert "U1d: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$U1D_LANE/target"

# ── U1e: the probe's FIND-TRAVERSAL-FAILURE branch (outcome 1 of
# _assert_delta_touch_base_substantiated's `if ! fingerprint_dir="$(find ...)"`
# wrapper) — an EACCES on a subdirectory under LANE_TARGET is a DIFFERENT
# outcome from "no .fingerprint anywhere" (U1b, outcome 2) and from "base
# unsubstantiated" (U1/U1c/U1d, outcome 3): only a TRAVERSAL failure exits the
# probe non-zero WITHOUT ever assigning fingerprint_dir, which is exactly the
# distinction the guard's own `if !` wrapper (rather than a bare assignment
# under set -e) exists to attribute. None of U1/U1b/U1c/U1d/U2 reach it. U1b is
# this arm's byte-identical POSITIVE CONTROL: same fixture, minus the one
# unreadable subdirectory, exits 0 with stdout "<lane>/target" — so exit status
# alone discriminates outcome (1) from outcome (2), with no .fingerprint
# anywhere to make the arm readdir-order-dependent.
#
# Mode 000 is not a barrier for root, so the whole arm is skipped there — idiom
# copied from tests/infra/test_warm_lane_audit.sh's L9 arm and
# tests/infra/test_warm_lane_lock_guard.sh's D4 arm, including the "restore
# before any assert" ordering both of those document: an aborting assertion
# must never leave cleanup()'s rm -rf stuck on an untraversable dir.
if [ "$(id -u)" -ne 0 ]; then
    IFS='|' read -r U1E_BASE U1E_LANE <<< "$(_u_make_fixture U1e 0)"
    # DEPTH IS LOAD-BEARING: target/debug/<name> is depth 2 from LANE_TARGET,
    # INSIDE the probe's `-maxdepth 3` bound, so find descends into it and gets
    # EACCES. At depth 3 (like U1c/U1d's .fingerprint placements) find would
    # stat the entry but never READ it, and the probe would succeed — silently
    # degrading this arm into a duplicate of U1b. Deliberately named
    # "untraversable", not "output" or "locked", to stay clear of the inv.12
    # guard's own freshness-reference name.
    mkdir -p "$U1E_BASE/debug/untraversable"
    chmod 000 "$U1E_BASE/debug/untraversable"

    reset_calls
    RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
        run_helper_real "$U1E_BASE" "$U1E_LANE" --fresh-checkout

    # Capture the non-vacuity precondition BEFORE restoring the mode, with an
    # explicit `if ...; then VAR=1; fi` — NOT `[ -r x ] && VAR=1`, whose
    # errexit exemption is positional (only inside an if/while condition; as a
    # bare statement a false `[ -r x ]` would itself trip this file's set -e).
    # The condition requires existence AND unreadability, not bare `! -r`:
    # `[ -r path ]` is ALSO false when path does not exist at all, so a bare
    # `[ ! -r "$U1E_CLONE_DIR" ]` capture would satisfy this precondition for
    # the WRONG reason if a future GNU cp stopped creating the destination
    # dir when it can't read the source — silently degrading this arm into a
    # U1b duplicate while the non-vacuity check meant to catch exactly that
    # degrade kept printing PASS.
    U1E_CLONE_DIR="$U1E_LANE/target/debug/untraversable"
    U1E_CLONE_WAS_UNTRAVERSABLE=""
    if [ -d "$U1E_CLONE_DIR" ] && [ ! -r "$U1E_CLONE_DIR" ]; then U1E_CLONE_WAS_UNTRAVERSABLE=1; fi
    # Restore BOTH copies — the base fixture's dir AND the clone's — before any
    # assert runs, so an aborting assertion can never leave cleanup()'s rm -rf
    # stuck on an untraversable path.
    chmod 0755 "$U1E_BASE/debug/untraversable" || true
    chmod 0755 "$U1E_CLONE_DIR" || true

    # WHY the mode-000 dir reaches the lane at all, MEASURED (not assumed):
    # run_helper_real's cp stub exit-0s regardless of `/bin/cp -a`'s own status
    # (see the stub, above), and GNU cp still CREATES the destination directory
    # carrying the source's mode even though it could not read the source's
    # entries to populate it — `/bin/cp: cannot access '<src>/debug/untraversable':
    # Permission denied`, dest created as `d---------`. The next assertion is the
    # guard that turns a future change in either of those two behaviours into an
    # attributable red instead of a silent degrade into a U1b duplicate.
    assert "U1e: NON-VACUITY — the clone's copy of the mode-000 dir existed AND was untraversable before the restore (else this arm silently degrades into a duplicate of U1b)" \
        test -n "$U1E_CLONE_WAS_UNTRAVERSABLE"
    assert "U1e: seed exits NON-zero — the traversal-failure probe is an abort, not a return-0 fall-through" \
        test "$RC" -ne 0
    assert "U1e: STDOUT is EMPTY on the traversal-failure abort (caller falls back to a cold rebuild)" \
        bash -c '[ -z "$1" ]' _ "$OUT"
    assert "U1e: an [error] line names the probe traversal failure AND the lane target path" \
        bash -c 'printf "%s\n" "$1" | grep "\[error\]" | grep -qF "probe FAILED to walk $2"' _ "$ERR_OUT" \
        "$U1E_LANE/target"
    # DISCRIMINATOR: outcome (3)'s refusal wording is structurally impossible on
    # a fixture with no .fingerprint anywhere — so a future change that routed a
    # traversal failure into the refusal path (rather than its own attributed
    # abort) goes red here.
    assert "U1e: no [error] line carries the outcome-(3) unsubstantiated-base refusal wording" \
        bash -c '! printf "%s\n" "$1" | grep "\[error\]" | grep -q "Unsubstantiated delta-touch base"' _ "$ERR_OUT"
else
    echo "  SKIP: U1e (running as root — mode 000 does not make a directory untraversable)"
fi

# ── U2: DISCRIMINATOR (green before AND after) — .fingerprint present, but a
# base RESOLVES via --base-commit while the stub git returns an EMPTY
# `diff --name-only` (REIFY_TEST_GIT_DIFF_FILES deliberately left unset, the
# Block J3/K1 shape). Pins that the guard keys on BASE RESOLUTION, not on the
# delta set being empty: a lane legitimately sitting AT the base commit has an
# empty delta and must still seed. This is the arm that fails against the
# plausible-but-wrong implementation keyed on an empty _DELTA_PATHS.
IFS='|' read -r U2_BASE U2_LANE <<< "$(_u_make_fixture U2 1)"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U2_BASE" "$U2_LANE" --fresh-checkout --base-commit shaU2

assert "U2: base RESOLVES (empty delta) ⇒ seed exits 0 even with .fingerprint present" \
    test "$RC" -eq 0
assert "U2: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$U2_LANE/target"

# ── U2b: the OTHER half of the same discriminator — an explicit --touch list is
# NOT substantiation. Together with U2 this pins the guard's key exactly: BASE
# RESOLUTION, neither more nor less. Why --touch does not substantiate, and why
# the `non-empty _DELTA_PATHS ⇒ substantiated` refactor is the wrong one:
# §9.5 inv.13.
IFS='|' read -r U2B_BASE U2B_LANE <<< "$(_u_make_fixture U2b 1)"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U2B_BASE" "$U2B_LANE" --fresh-checkout \
        --touch "$U2B_LANE/src/tracked.rs"

assert "U2b: a non-empty --touch list does NOT substantiate a missing base ⇒ still refuses" \
    test "$RC" -ne 0
assert "U2b: --touch abort still leaves STDOUT empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# ── U3: the narrow opt-out knob, honoured. REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=1
# downgrades the refusal to a warn and reaches the accept path VERBATIM (same
# exit 0, same stdout, same 2020-01-01 bulk stamp U1b asserts) — not a new third
# behaviour. The warn is level-scoped so an honoured downgrade is visible in a
# production log.
#
# Polarity rationale (default-ON; the INVERSE of esc-5214): §9.5 inv.13.
IFS='|' read -r U3_BASE U3_LANE <<< "$(_u_make_fixture U3 1)"

reset_calls
REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=1 \
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U3_BASE" "$U3_LANE" --fresh-checkout

assert "U3: REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=1 downgrades the refusal ⇒ exits 0" \
    test "$RC" -eq 0
assert "U3: knob-honoured seed returns exactly <lane>/target on STDOUT" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$U3_LANE/target"
U3_SRC_MTIME="$(stat -c '%Y' "$U3_LANE/src/tracked.rs")"
assert "U3: tracked source still carries the 2020-01-01 bulk stamp ($EPOCH_2020) — accept path reached verbatim" \
    test "$U3_SRC_MTIME" -eq "$EPOCH_2020"
assert "U3: a [warn] line names the knob, so an honoured downgrade is visible in a production log" \
    bash -c 'printf "%s\n" "$1" | grep "\[warn\]" | grep -qF "REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT"' \
    _ "$ERR_OUT"

# ── U3b/U3c: the EXACT-"1" contract. Without these, a truthy-ish comparison
# (-n) would silently let =0, a stray empty-string export, or any unrecognised
# value bypass a default-ON safety guard. Mirrors the REIFY_WARM_LANE_RESEED_TRASH_SYNC
# = "1" idiom already used in scripts/seed-warm-lane.sh.
#
# Deliberately NO usage/exit-64 validation arm for the knob: unlike
# REIFY_WARM_LANE_LANE_LOCK_WAIT (whose bad value could reach a destructive
# flock/mv), an unrecognised value here fails SAFE — it simply leaves the
# default fail-closed abort in force, which is exactly what these two arms pin.
IFS='|' read -r U3B_BASE U3B_LANE <<< "$(_u_make_fixture U3b 1)"

reset_calls
REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=0 \
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U3B_BASE" "$U3B_LANE" --fresh-checkout

assert "U3b: REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=0 does NOT downgrade the guard (exact-1 match) ⇒ still exits non-zero" \
    test "$RC" -ne 0
assert "U3b: =0 abort still leaves STDOUT empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"

IFS='|' read -r U3C_BASE U3C_LANE <<< "$(_u_make_fixture U3c 1)"

reset_calls
REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=yes \
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U3C_BASE" "$U3C_LANE" --fresh-checkout

assert "U3c: REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=yes does NOT downgrade the guard (exact-1, not truthiness) ⇒ still exits non-zero" \
    test "$RC" -ne 0
assert "U3c: =yes abort still leaves STDOUT empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# ── U4/U4b/U4c/U4d: the no-base diagnostic's PER-TIER ATTRIBUTION and the
# .basecommit trailing-whitespace trim. Both are what an operator READS and ACTS
# ON when a lane refuses to seed, so "absent" (go hunt for a missing file) vs
# "present but empty" (go inspect the file's content) is the difference between
# the right next action and the wrong one. All four reuse _u_make_fixture WITH
# .fingerprint so inv.13's guard is live, and vary ONLY the stamp/sidecar state.
#
# The attribution is built ONCE at the resolve site and consumed by BOTH the
# retained no-base [warn] and inv.13's [error]. Each arm asserts the SAME
# substring on BOTH levels (via _u4_assert_attribution), so a future edit that
# splits the one string into two independently-maintained literals goes red on
# the first divergence rather than drifting silently.
#
# These pin RUNTIME behaviour — a running script's stderr, exit status, and the
# sha its git diff actually received — not comment prose.

# _u4_assert_attribution <arm> <substring> — the substring must appear on a
# [warn] line AND on an [error] line. Both greps are LEVEL-SCOPED for the reason
# U1 documents: the retained no-base warn names the same condition as the err,
# so an unscoped grep over all of stderr cannot tell the two consumers apart and
# would pass with either one missing.
_u4_assert_attribution() {
    local arm="$1" frag="$2"
    assert "$arm: the no-base [warn] attributes — $frag" \
        bash -c 'printf "%s\n" "$1" | grep "\[warn\]" | grep -qF "$2"' _ "$ERR_OUT" "$frag"
    assert "$arm: inv.13's [error] carries the SAME attribution — $frag" \
        bash -c 'printf "%s\n" "$1" | grep "\[error\]" | grep -qF "$2"' _ "$ERR_OUT" "$frag"
}

# U4: the BASELINE attribution — nothing present at either file-backed tier.
# Both must read `absent`, which is what makes U4b/U4c's `present but empty`
# a distinction rather than the only thing the diagnostic ever says.
IFS='|' read -r U4_BASE U4_LANE <<< "$(_u_make_fixture U4 1)"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U4_BASE" "$U4_LANE" --fresh-checkout

assert "U4: no .basecommit file and no BASE_COMMIT= line ⇒ seed refuses" \
    test "$RC" -ne 0
_u4_assert_attribution "U4" "${U4_BASE}.basecommit absent"
_u4_assert_attribution "U4" ".warm-base-meta BASE_COMMIT absent"

# U4b: tier 2 PRESENT BUT EMPTY — the shape a refresh-warm-base.sh Step-4b write
# truncated mid-`printf` leaves behind. Two things are pinned at once: the stamp
# does NOT resolve a base (the trim's NEGATIVE direction — without it `cat`
# yields "   ", which is non-empty and would sail past the caller's [ -n ] check
# as a falsely-resolved base and seed the lane), and the diagnostic says the
# file is there rather than sending an operator hunting for it.
IFS='|' read -r U4B_BASE U4B_LANE <<< "$(_u_make_fixture U4b 1)"
printf '   \n' > "${U4B_BASE}.basecommit"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U4B_BASE" "$U4B_LANE" --fresh-checkout

assert "U4b: a whitespace-only .basecommit resolves NO base ⇒ seed still refuses" \
    test "$RC" -ne 0
assert "U4b: whitespace-only-stamp abort still leaves STDOUT empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"
_u4_assert_attribution "U4b" "${U4B_BASE}.basecommit present but empty"
assert "U4b: the file EXISTS, so the diagnostic must NOT call tier 2 'absent'" \
    bash -c '! printf "%s\n" "$1" | grep -qF "$2"' _ "$ERR_OUT" "${U4B_BASE}.basecommit absent"
_u4_assert_attribution "U4b" ".warm-base-meta BASE_COMMIT absent"

# U4c: tier 3 PRESENT BUT EMPTY — the sidecar carries the BASE_COMMIT key with
# no value. Same presence-vs-empty distinction, one tier down; without it the
# attribution could be keyed on tier 2 alone and stay green on U4/U4b.
IFS='|' read -r U4C_BASE U4C_LANE <<< "$(_u_make_fixture U4c 1)"
printf 'RUSTFLAGS=\nINVOCATION=\nBASE_COMMIT=\n' > "$(dirname "$U4C_BASE")/.warm-base-meta"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$U4C_BASE" "$U4C_LANE" --fresh-checkout

assert "U4c: a valueless BASE_COMMIT= line resolves NO base ⇒ seed still refuses" \
    test "$RC" -ne 0
_u4_assert_attribution "U4c" ".warm-base-meta BASE_COMMIT present but empty"
_u4_assert_attribution "U4c" "${U4C_BASE}.basecommit absent"

# U4d: the trim's POSITIVE direction — a real sha must survive it intact and
# still resolve. The trailing character is a SPACE, not a newline: `$(cat)`
# already strips trailing newlines, so a newline-only fixture would be green
# with the trim deleted and pin nothing. With a trailing space, an untrimmed
# value reaches `git diff --name-only` as `shaTRIM ` — hence the `shaTRIM$`
# anchor on the recorded call (the sha is _touch_git_delta's LAST argument, so
# the stub's `echo "git $*"` line ends exactly where the sha does).
IFS='|' read -r U4D_BASE U4D_LANE <<< "$(_u_make_fixture U4d 1)"
printf 'shaTRIM \n' > "${U4D_BASE}.basecommit"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_TEST_GIT_DIFF_FILES="src/tracked.rs" \
    run_helper_real "$U4D_BASE" "$U4D_LANE" --fresh-checkout

assert "U4d: a sha padded with trailing whitespace still RESOLVES ⇒ seed exits 0" \
    test "$RC" -eq 0
assert "U4d: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$U4D_LANE/target"
assert "U4d: git diff received the TRIMMED sha (line ends at shaTRIM, no trailing pad)" \
    bash -c 'grep "^git" "$1" | grep "diff" | grep -q "shaTRIM$"' _ "$CALLS_FILE"
U4D_SRC_MTIME="$(stat -c '%Y' "$U4D_LANE/src/tracked.rs")"
assert "U4d: the resolved base really delta-touched the changed source off the 2020-01-01 stamp" \
    test "$U4D_SRC_MTIME" -ne "$EPOCH_2020"

# ─────────────────────────────────────────────────────────────────────────────
# Block V: detached trash-rm holds no caller FD (task #6219) — originally,
# the two backgrounded trash-rm jobs in scripts/seed-warm-lane.sh (the orphan
# sweep, the reseed-trash rm) closed only the lane-lock FD they knew about
# (`9<&-`) and did NOT redirect fd 1/fd 2 at all. Both are now fixed: fd 1/fd
# 2 are redirected to /dev/null, and every OTHER inherited descriptor -- not
# just a hardcoded FD 9 or FD 8 -- is closed generically via
# `_close_inherited_fds` (task #6219 amendment; see its doc-comment in
# scripts/seed-warm-lane.sh, next to the log helpers). A caller that captures
# the script's stdout through a PIPE rather than a file — scripts/warm-lane-gc.sh:648-649's
# `2>&1 | while IFS= read -r line; do warn "  [seed] $line"; done` is the live
# example — therefore blocks until that detached rm ALSO exits, defeating the
# whole point of detaching a possibly-large-tree rm from the acquire path.
#
# Method: a PATH-shimmed `rm` records its OWN fd 1/fd 2 targets for any
# `*/.reseed-trash/*` argument, then no-ops (exit 0) IMMEDIATELY — no sleep,
# no hang, no timing dependence (tests/infra/test_no_new_wallclock_upper_bounds.sh
# is a live static guard against new wall-clock upper-bound asserts, and this
# pool-bucket suite's C-P3 discipline requires load-independent verdicts). The
# shim opens its own log file so the probe cannot perturb what it measures.
#
# fd-PROBING MECHANIC: reading a descriptor via `$(readlink /proc/self/fd/N)`
# does NOT work here — command substitution itself runs in a subshell whose
# OWN fd 1 is the internal pipe bash uses to capture $(...)'s output, so
# `/proc/self/fd/1` inside that subshell always resolves to THAT capture
# pipe, never to the shim's real, inherited fd 1 (verified empirically while
# building this block: it read back `pipe:*` unconditionally, fix or no fix).
# The shim instead captures `$BASHPID` — its own real PID, stable across the
# fd redirects being probed — into a plain variable FIRST, then uses that
# fixed value in `/proc/$BASHPID/fd/N` from inside the command substitution.
# That decouples "what redirect does the probe's own output need" from "whose
# fd table am I inspecting", which a self-referential `/proc/self` can never
# do.
#
# The whole invocation deliberately mirrors the real blocking caller,
# scripts/warm-lane-gc.sh:648-649 — `bash "$SCRIPT" ... 2>&1 | consumer` — so
# BOTH fd 1 and fd 2 are the SAME pipe in one run, covering both descriptors.
# Neither run_helper nor run_helper_real fits: both capture stdout via a real
# FILE precisely to dodge this hang (see the NOTE at run_helper_real,
# :456-467), so this block invokes the script directly through its own
# bespoke pipe-connected stub dir instead.
#
# TIMING: pre-fix, the detached child still inherits the pipe, so THIS
# invocation blocks on it too — but only for the instant the shim needs to
# printf+exit (no sleep in the shim), so the test does not hang; the
# pipeline's own completion is then a valid, already-settled read of the log.
# Post-fix, the pipeline no longer waits on the grandchild at all, so the
# shim's log write can race the main shell's very next statement (confirmed
# empirically: an unconditional read immediately after the pipeline is
# sometimes empty post-fix). Every log read below therefore goes through
# `_wait_until` rather than a bare read, so the block is deterministic in
# BOTH the RED and the GREEN state, with no upper-bound timing assertion.
#
# Non-vacuity is the load-bearing part of the design — independent guards are
# required throughout, because a "does not hold a descriptor onto X" assert
# would otherwise pass trivially if the fixture never connected the resource
# or the site never fired:
#   V3/V7 (site-fired):  the shim log really carries an entry for the path.
#   V4    (pipe-wired):  REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 forces the
#         reseed-trash rm (only, its SYNC branch) into the FOREGROUND, where
#         inheriting the pipe is CORRECT — asserting THAT invocation's fd 1
#         IS `pipe:*` proves, in the same harness, that a pipe was genuinely
#         connected.
#   V12   (FD-8-wired): the same SYNC knob, wrapped in the gc-mirroring
#         exec-8/exec-9 lock harness (see V9-V13 below), proves the harness
#         genuinely holds FD 8 open across the (now-foreground) reseed-trash
#         rm — the FD-8 analogue of V4's role for the pipe.
#
# V9-V13 EXTEND this block with a SECOND caller descriptor: warm-lane-gc.sh's
# own EXCLUSIVE lane-lock FD (8, ${lane}.lock — scripts/warm-lane-gc.sh:559-
# 565 the FD-8 acquire, :645-649 the seed subshell + pipe, :716 the later
# release). See scripts/seed-warm-lane.sh's `_close_inherited_fds`
# doc-comment (next to the log helpers) and its LANE-LOCK RELEASE CONTRACT
# block for the full flock-is-attached-to-the-OPEN-FILE-DESCRIPTION argument
# for why a detached child inheriting a dup of FD 8 keeps gc's exclusive
# lane lock held for its entire orphaned rm -rf, even after gc's own
# `exec 8>&-` — making the lane read as locked to every later consumer
# against a process that is not a consumer at all (not restated here, G7).
# The rm shim below now records a FOURTH tab-separated field: the child's
# full /proc/$BASHPID/fd inventory (TAB-joined `<n>=><target>` items — see
# the shim comment below for why tab rather than space), so V9-V13 can
# target a SPECIFIC lock path rather than fd 1/fd 2 alone — a "no fd >= 3"
# assert would be flaky, a healthy inventory legitimately carries a
# transient pipe: entry from the probe's own $(readlink ...) plus
# 255=><shim path> and 0/1/2=>/dev/null. The V8 structural detector gains a
# matching second criterion (a DETACH line must call `_close_inherited_fds`,
# not just carry a stdout redirect), asserted
# separately as V13 so V8's own original, narrower assertion is unaffected
# and stays green throughout.
#
# V14-V18 (task #6219 amendment 2) close the block's remaining asymmetry: fd
# 1, fd 2 and the fd>=3 inventory were all measured, fd 0 never was -- and fd
# 0 was exactly the half of the header's PIPE-SAFE claim that was untrue,
# because it rested on bash's async-list /dev/null stdin default, which
# applies only "in the absence of any explicit redirections" and so never
# reached the orphan-sweep detach nested inside `while ... done < <(find
# ...)`. V14/V15 assert fd 0 for both sites off the same field-4 inventory
# the FD-8 arm already records (V15 was RED); V16 is their non-vacuity
# control, in the SYNC/foreground harness where inheriting the caller's stdin
# is CORRECT -- which is also why every invocation in the V0-V7 arms is now
# wired to a REAL stdin FILE rather than the runner's ambient stdin. V17 adds
# the matching structural criterion (a DETACH line must carry an explicit
# `</dev/null`), and V18 pins the shipped `_close_inherited_fds` body itself:
# its close-error suppression must scope to the eval builtin rather than
# permanently redirecting the shell's own stderr.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block V: detached trash-rm holds no caller FD (task #6219) ---"

# Fixture: direct copy of the I14/Block T recipe (:3627-3643) — a base with a
# real artifact + .warm-base-meta (RUSTFLAGS="" to match the sidecar), and a
# lane with a NON-EMPTY target so seed takes the rename-to-trash path. No
# .fingerprint/ dir anywhere in the base, so inv.13's delta-touch-base guard
# takes its vacuous skip (scripts/seed-warm-lane.sh:597-606) and neither
# --base-commit nor REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT is needed — same
# discipline as Block T's own fixture. One lane fixture serves both
# invocations below (the async default and the SYNC control arm): after the
# first invocation, target/ holds the freshly cloned base content, which is
# itself non-empty, so the second invocation takes the same rename-to-trash
# path again.
V_BASE_PARENT="$(mktemp -d /tmp/test-seed-V-parent-XXXXXX)"
V_BASE="$V_BASE_PARENT/target"
_TMPDIRS+=("$V_BASE_PARENT")
mkdir -p "$V_BASE/debug"
echo "base artifact" > "$V_BASE/debug/base_artifact.a"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$V_BASE_PARENT/.warm-base-meta"

V_LANE="$(make_isolated_lane V-lane)"
mkdir -p "$V_LANE/target"
echo "stale" > "$V_LANE/target/stale.a"

# Bespoke PATH stub dir (mirrors Block T's t4-*-stub pattern, :3739-3771):
# minted under $_LANE_ROOT so the suite's single EXIT trap reclaims it, with
# no new _TMPDIRS registration.
V_STUB="$(mktemp -d "$_LANE_ROOT/V-stub-XXXXXX")"

# cp: the reflink-OK body from run_helper_real (:396-410), trimmed to just
# the physical copy -- this block does not need the FD9-squatter arm.
cat > "$V_STUB/cp" << 'V_CP_STUB_EOF'
#!/usr/bin/env bash
src=""
dest=""
for arg in "$@"; do
    case "$arg" in
        -a|--reflink=always) ;;
        -*) ;;
        *) [ -z "$src" ] && src="$arg" || dest="$arg" ;;
    esac
done
if [ -n "$src" ] && [ -n "$dest" ]; then
    /bin/cp -a "$src" "$dest"
fi
exit 0
V_CP_STUB_EOF
chmod +x "$V_STUB/cp"

# git: copied verbatim from the suite's shared stub -- the .fingerprint-
# vacuous-skip fixture never consults REIFY_TEST_GIT_DIFF_FILES/HEAD.
cp "$STUB_DIR/git" "$V_STUB/git"

# rm: the fd-probing shim. Built on the existing PIN-stub shape (:420-431) --
# same */.reseed-trash/* glob, same exec /bin/rm passthrough for every other
# rm call (build-dir invalidation etc.) -- with the no-op arm extended to
# record its own fd 1/fd 2 targets before exiting. See the block header above
# for why $BASHPID (captured into a plain variable before the command
# substitution) stands in for /proc/self here. Written as a quoted heredoc
# into a mktemp'd dir, matching every other stub in this file.
#
# Fourth field (task #6219 FD-8 arm): the child's full fd inventory, built
# into a plain variable BEFORE the printf so the log write itself is never
# what gets measured -- TAB-joined "<n>=><target>" items read via
# /proc/$_v_mypid/fd (BASHPID, never /proc/self, same reason as fd 1/fd 2
# above). TAB rather than space (amendment, robustness): the outer record is
# itself tab-separated (`IFS=$'\t' read -r path fd1 fd2 fdset`, below), and
# `read` assigns any surplus fields AND their intervening delimiters
# verbatim to the LAST named variable, so joining inner items on the SAME
# delimiter the outer read already special-cases is free -- no escaping
# needed. A space-joined inventory, by contrast, breaks on any target path
# containing a space (word-splits one logical item into two) and is subject
# to pathname expansion wherever a target contains glob metacharacters (a
# healthy inventory legitimately contains one, "pipe:[12345]") when a
# consumer later does `for item in $fdset`; tab avoids the first hazard
# outright and _v_fdset_has_target below additionally disables globbing for
# the second. MEASURED while building this: each $(readlink ...) below --
# including the fd1/fd2 reads themselves -- transiently occupies a
# descriptor in THIS shim's own table, so a HEALTHY inventory legitimately
# contains a pipe: entry (observed at fd 3) plus 255=><this shim's path> and
# 0/1/2=>/dev/null (bash's own async-list stdin default) even post-fix.
# Callers must therefore target a SPECIFIC path, never assert "no fd >= 3" --
# that would be flaky by construction.
cat > "$V_STUB/rm" << 'V_RM_STUB_EOF'
#!/usr/bin/env bash
for arg in "$@"; do
    case "$arg" in
        */.reseed-trash/*)
            _v_mypid="$BASHPID"
            _v_fdset=""
            for _v_fdpath in "/proc/$_v_mypid/fd/"*; do
                _v_fdset="$_v_fdset"$'\t'"${_v_fdpath##*/}=>$(readlink "$_v_fdpath" 2>/dev/null || true)"
            done
            printf '%s\t%s\t%s\t%s\n' "$arg" \
                "$(readlink "/proc/$_v_mypid/fd/1")" \
                "$(readlink "/proc/$_v_mypid/fd/2")" \
                "${_v_fdset#$'\t'}" \
                >> "$REIFY_TEST_FDLOG"
            exit 0
            ;;
    esac
done
exec /bin/rm "$@"
V_RM_STUB_EOF
chmod +x "$V_STUB/rm"

V_TRASH_GLOB="*/.reseed-trash/$(basename "$V_LANE").*"

# _v_fdlog_has_entry <fdlog> <path-glob> [exclude-exact-path] -- _wait_until
# predicate: true once <fdlog> carries a line whose path (tab-separated
# field 1) matches <glob>, other than a line whose path is EXACTLY
# [exclude-exact-path] if given. Mirrors _calls_file_has_trash_rm's
# line-match shape (:560-569). The optional exclusion exists only for the
# V9-V13 arm below (task #6219 FD-8 arm): one seed invocation there fires
# BOTH detach sites into a SHARED fdlog, and the reseed-trash rename entry's
# PID suffix is unknown ahead of time and not safely distinguishable from
# the orphan-sweep entry's suffix by a glob (both are plain digit strings),
# so it is identified by eliminating the orphan's exact, test-planted path
# rather than by pattern or by log-write order (log-write order is never
# assumed in this file -- see the V4 comment above on the ambiguity that
# once flaked because of it).
_v_fdlog_has_entry() {
    local fdlog="$1" glob="$2" exclude="${3:-}" path
    [ -f "$fdlog" ] || return 1
    while IFS=$'\t' read -r path _ _ _; do
        [ -n "$exclude" ] && [ "$path" = "$exclude" ] && continue
        case "$path" in
            $glob) return 0 ;;
        esac
    done < "$fdlog"
    return 1
}

# _v_fdlog_field <fdlog> <path-glob> <1|2|4> [exclude-exact-path] -- prints
# the fd1 (field 2), fd2 (field 3) or fd-inventory (field 4, task #6219 FD-8
# arm -- the TAB-joined "<n>=><target>" list) recorded for the FIRST line
# whose path matches <glob>, honouring the same optional exact-path
# exclusion as _v_fdlog_has_entry. Callers only invoke this after
# _v_fdlog_has_entry has already proven a matching line exists.
_v_fdlog_field() {
    local fdlog="$1" glob="$2" field="$3" exclude="${4:-}" path fd1 fd2 fdset
    while IFS=$'\t' read -r path fd1 fd2 fdset; do
        [ -n "$exclude" ] && [ "$path" = "$exclude" ] && continue
        case "$path" in
            $glob)
                case "$field" in
                    1) printf '%s' "$fd1" ;;
                    2) printf '%s' "$fd2" ;;
                    4) printf '%s' "$fdset" ;;
                esac
                return 0
                ;;
        esac
    done < "$fdlog"
    return 1
}

# _v_field_not_pipe / _v_field_is_pipe <fdlog> <glob> <1|2> -- wait for the
# entry (bounded, deterministic -- see TIMING above), then compare its fd
# field against the pipe:* shape. A missing entry is a FAILURE for both --
# neither can pass vacuously off an absent line.
_v_field_not_pipe() {
    local fdlog="$1" glob="$2" field="$3" val
    _wait_until 10 _v_fdlog_has_entry "$fdlog" "$glob" || return 1
    val="$(_v_fdlog_field "$fdlog" "$glob" "$field")"
    [ -n "$val" ] || return 1
    case "$val" in
        pipe:*) return 1 ;;
        *) return 0 ;;
    esac
}
_v_field_is_pipe() {
    local fdlog="$1" glob="$2" field="$3" val
    _wait_until 10 _v_fdlog_has_entry "$fdlog" "$glob" || return 1
    val="$(_v_fdlog_field "$fdlog" "$glob" "$field")"
    case "$val" in
        pipe:*) return 0 ;;
        *) return 1 ;;
    esac
}

# _v_fdset_has_target <fdlog> <path-glob> <target-path> [exclude-exact-path]
# (task #6219 FD-8 arm) -- wait for the entry (bounded, deterministic -- see
# TIMING above), then return 0 iff its fd inventory (field 4) contains an
# "<n>=><target-path>" item whose target is EXACTLY <target-path>. A missing
# log entry is a FAILURE (return 1) -- same no-vacuous-pass discipline as
# _v_field_not_pipe/_v_field_is_pipe. Deliberately targeted at one specific
# path rather than "does the child hold any fd >= 3": see the fourth-field
# comment on the rm shim above for why a fd-count-based assertion would be
# flaky by construction.
# Robustness (amendment): the inventory is scanned with IFS locally narrowed
# to a bare TAB and globbing disabled, rather than a bare `for item in
# $fdset` under the caller's ambient IFS/glob settings. Two hazards
# otherwise: (a) the default IFS also splits on space, so a target path
# containing a space would split one logical item into two and never
# string-match whole, silently passing the "missing" polarity for the wrong
# reason; (b) an unquoted expansion is subject to pathname expansion, and a
# healthy inventory legitimately contains an item with glob metacharacters
# ("pipe:[12345]"). `local IFS` auto-restores the caller's value on return
# (including the early return below); `set -f`/`set +f` are NOT scoped by
# `local`, so every path out of the loop explicitly restores it before
# returning.
_v_fdset_has_target() {
    local fdlog="$1" glob="$2" target="$3" exclude="${4:-}" fdset item
    local IFS
    _wait_until 10 _v_fdlog_has_entry "$fdlog" "$glob" "$exclude" || return 1
    fdset="$(_v_fdlog_field "$fdlog" "$glob" 4 "$exclude")"
    IFS=$'\t'
    set -f
    for item in $fdset; do
        case "$item" in
            *"=>$target") set +f; return 0 ;;
        esac
    done
    set +f
    return 1
}

# _v_fdset_missing_target <fdlog> <path-glob> <target-path> [exclude-exact-path]
# -- the "does NOT contain" polarity. Deliberately NOT a bare
# `! _v_fdset_has_target ...`: negating would turn a missing log entry (site
# never fired) into a spurious PASS, defeating the non-vacuity discipline
# this whole block is built around. Both polarities independently gate on
# _v_fdlog_has_entry first, exactly like _v_field_not_pipe/_v_field_is_pipe
# do for fields 2/3.
_v_fdset_missing_target() {
    local fdlog="$1" glob="$2" target="$3" exclude="${4:-}"
    _wait_until 10 _v_fdlog_has_entry "$fdlog" "$glob" "$exclude" || return 1
    _v_fdset_has_target "$fdlog" "$glob" "$target" "$exclude" && return 1
    return 0
}

# _v_fdset_fd_target <fdlog> <path-glob> <fdnum> [exclude-exact-path]
# (task #6219 amendment 2) -- print the target of ONE specific descriptor
# <fdnum> out of the fd inventory (field 4), or return 1 if the entry is
# missing or carries no item for that descriptor. _v_fdset_has_target above
# deliberately matches a target on ANY descriptor, which cannot express "fd 0
# specifically": fds 1 and 2 legitimately point at /dev/null too, so
# `_v_fdset_has_target ... /dev/null` is green whether or not fd 0 is. Same
# IFS/glob hardening and the same no-vacuous-pass gate on _v_fdlog_has_entry
# as _v_fdset_has_target (see its comment; not restated, G7).
_v_fdset_fd_target() {
    local fdlog="$1" glob="$2" fdnum="$3" exclude="${4:-}" fdset item
    local IFS
    _wait_until 10 _v_fdlog_has_entry "$fdlog" "$glob" "$exclude" || return 1
    fdset="$(_v_fdlog_field "$fdlog" "$glob" 4 "$exclude")"
    IFS=$'\t'
    set -f
    for item in $fdset; do
        case "$item" in
            "$fdnum=>"*) set +f; printf '%s' "${item#"$fdnum=>"}"; return 0 ;;
        esac
    done
    set +f
    return 1
}

# _v_fd_target_is <fdlog> <path-glob> <fdnum> <expected> [exclude-exact-path]
# -- true iff descriptor <fdnum> of the recorded child points EXACTLY at
# <expected>. A missing entry or a missing descriptor is a FAILURE, never a
# vacuous pass -- same discipline as every other predicate in this block.
# Both polarities of the fd-0 assertions below go through this one predicate
# (detached: "/dev/null"; foreground control: the caller's own stdin file), so
# neither can be satisfied by the descriptor simply being absent.
_v_fd_target_is() {
    local fdlog="$1" glob="$2" fdnum="$3" expected="$4" exclude="${5:-}" val
    val="$(_v_fdset_fd_target "$fdlog" "$glob" "$fdnum" "$exclude")" || return 1
    [ "$val" = "$expected" ]
}

# The caller's stdin (task #6219 amendment 2 -- V14-V16). Every invocation in
# the V0-V7 arms below is wired to this REAL FILE rather than left to inherit
# whatever stdin the test runner happens to have (a tty interactively,
# /dev/null under some CI shapes) -- otherwise V16's control could not
# distinguish "the detached child's fd 0 was redirected to /dev/null by seed"
# from "the whole invocation's stdin was already /dev/null", and the fd-0
# assertions would be vacuous exactly where they matter. Resolved through
# readlink -f because /proc/PID/fd/0 reports the RESOLVED path, which would
# not string-compare equal if any component of $_LANE_ROOT were a symlink.
V_STDIN_FILE="$_LANE_ROOT/.v-caller-stdin"
printf 'caller stdin\n' > "$V_STDIN_FILE"
V_STDIN_REAL="$(readlink -f "$V_STDIN_FILE")"

# ── V0-V3: the async DEFAULT path (both trash-rm sites backgrounded) ────────
V_FDLOG="$_LANE_ROOT/.v-fdlog"
: > "$V_FDLOG"
V_SINK1="$_LANE_ROOT/.v-sink1"

set +e
( PATH="$V_STUB:$PATH" RUSTFLAGS="" REIFY_TEST_FDLOG="$V_FDLOG" \
    bash "$SCRIPT" "$V_BASE" "$V_LANE" --fresh-checkout ) \
    < "$V_STDIN_FILE" 2>&1 | cat > "$V_SINK1"
V0_RC="${PIPESTATUS[0]}"
set -e

assert "V0: seed exits 0 (fixture sanity, mirrors T0)" test "$V0_RC" -eq 0

assert "V3 (non-vacuity, site fired): the reseed-trash rm really ran and the shim recorded it" \
    _wait_until 10 _v_fdlog_has_entry "$V_FDLOG" "$V_TRASH_GLOB"

assert "V1 (RED, the defect): the detached reseed-trash rm's fd 1 is NOT the caller's pipe" \
    _v_field_not_pipe "$V_FDLOG" "$V_TRASH_GLOB" 1
assert "V2 (RED, the defect): the detached reseed-trash rm's fd 2 is NOT the caller's pipe" \
    _v_field_not_pipe "$V_FDLOG" "$V_TRASH_GLOB" 2

# V14 (task #6219 amendment 2): the THIRD standard descriptor. Block V
# measured fd 1, fd 2 and the fd>=3 inventory but never fd 0, which is
# precisely the half of the header's PIPE-SAFE claim that was untrue -- the
# claim leaned on bash's async-list /dev/null stdin default, which applies
# only "in the absence of any explicit redirections" and therefore does NOT
# reach the orphan-sweep detach nested inside `while ... done < <(find ...)`
# (V15 below is the arm that was RED). Both sites now redirect stdin
# EXPLICITLY; see `_close_inherited_fds`'s doc-comment in
# scripts/seed-warm-lane.sh for the argument and the measurement (G7, not
# restated). V16 is this assertion's non-vacuity control.
assert "V14: the detached RESEED-TRASH rm's fd 0 is /dev/null, not the caller's stdin" \
    _v_fd_target_is "$V_FDLOG" "$V_TRASH_GLOB" 0 "/dev/null"

# ── V4: the SYNC control arm — proves a pipe was genuinely connected ───────
# A FRESH, never-before-seeded lane (V_LANE_SYNC), NOT a second run on V_LANE:
# reusing V_LANE would leave run1's own leftover trash entry on disk (the fd
# shim no-ops rather than really deleting it) as a live orphan for THIS run to
# sweep, so once the orphan-sweep site is ALSO fixed (step-4) that entry's fd
# 1 is NOT pipe while the SYNC entry's IS -- and V_TRASH_GLOB's trailing `.*`
# matches either one, so which line _v_fdlog_field finds first becomes a race
# between the backgrounded orphan-sweep child and the foreground SYNC rm
# (caught empirically: this exact ambiguity made V4 flake red once step-4
# landed). A never-seeded lane has no pre-existing trash dir, so no orphan
# sweep can fire and only the SYNC entry ever reaches the log.
V_LANE_SYNC="$(make_isolated_lane V-lane-sync)"
mkdir -p "$V_LANE_SYNC/target"
echo "stale" > "$V_LANE_SYNC/target/stale.a"
V_TRASH_GLOB_SYNC="*/.reseed-trash/$(basename "$V_LANE_SYNC").*"

V_FDLOG_SYNC="$_LANE_ROOT/.v-fdlog-sync"
: > "$V_FDLOG_SYNC"
V_SINK2="$_LANE_ROOT/.v-sink2"

set +e
( PATH="$V_STUB:$PATH" RUSTFLAGS="" REIFY_TEST_FDLOG="$V_FDLOG_SYNC" \
    REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 \
    bash "$SCRIPT" "$V_BASE" "$V_LANE_SYNC" --fresh-checkout ) \
    < "$V_STDIN_FILE" 2>&1 | cat > "$V_SINK2"
V4_RC="${PIPESTATUS[0]}"
set -e

assert "V4-fixture: the SYNC control run exits 0" \
    test "$V4_RC" -eq 0
assert "V4 (non-vacuity, pipe really wired): under REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 the (now-foreground) reseed-trash rm's fd 1 IS the caller's pipe, proving this harness genuinely connects one" \
    _v_field_is_pipe "$V_FDLOG_SYNC" "$V_TRASH_GLOB_SYNC" 1

# V16 (task #6219 amendment 2) -- the fd-0 analogue of V4's role, in the same
# SYNC harness: with the rm forced into the FOREGROUND it legitimately
# inherits seed's own stdin, so its fd 0 is the caller's stdin FILE. That is
# what makes V14/V15's "/dev/null" verdict load-bearing rather than an
# artefact of the runner's stdin already being /dev/null.
assert "V16 (non-vacuity, caller stdin really wired): under REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 the (now-foreground) reseed-trash rm's fd 0 IS the caller's stdin file, proving this harness genuinely wires a non-/dev/null stdin" \
    _v_fd_target_is "$V_FDLOG_SYNC" "$V_TRASH_GLOB_SYNC" 0 "$V_STDIN_REAL"

# ── V5-V7: the orphan-sweep arm (:1083) ─────────────────────────────────────
# Reuses the H4e orphan-trash plant recipe (:2553-2565) — a FRESH lane with a
# non-empty target/ (a precondition: the orphan sweep lives inside the same
# `if [ -d "$LANE_TARGET" ] && [ -n "$(ls -A ...)" ]` block as the
# rename-to-trash it accompanies) plus a pre-planted `<lane>.999999` orphan
# entry under the lane's own private dirname(LANE)/.reseed-trash, naming a
# dead PID so seed's crash-recovery sweep treats it as reclaimable. Same pipe
# shape and fd-probing rm stub as V0-V4 above (V_STUB is reused verbatim).
V_LANE2="$(make_isolated_lane V-lane2)"
mkdir -p "$V_LANE2/target"
echo "stale" > "$V_LANE2/target/stale.a"

V_TRASH2="$(dirname "$V_LANE2")/.reseed-trash"
mkdir -p "$V_TRASH2/$(basename "$V_LANE2").999999"
echo "orphan artifact" > "$V_TRASH2/$(basename "$V_LANE2").999999/orphan.a"

V_ORPHAN_GLOB="*/.reseed-trash/$(basename "$V_LANE2").999999"

V_FDLOG2="$_LANE_ROOT/.v-fdlog2"
: > "$V_FDLOG2"
V_SINK3="$_LANE_ROOT/.v-sink3"

set +e
( PATH="$V_STUB:$PATH" RUSTFLAGS="" REIFY_TEST_FDLOG="$V_FDLOG2" \
    bash "$SCRIPT" "$V_BASE" "$V_LANE2" --fresh-checkout ) \
    < "$V_STDIN_FILE" 2>&1 | cat > "$V_SINK3"
V5_RC="${PIPESTATUS[0]}"
set -e

assert "V5-fixture: seed exits 0 (orphan-sweep fixture sanity)" test "$V5_RC" -eq 0

assert "V7 (non-vacuity, sweep really fired): captured output names the swept orphan entry (mirrors H4e's assert at :2571)" \
    grep -q "Sweeping orphaned trash entry" "$V_SINK3"

assert "V5 (RED, the defect): the detached orphan-sweep rm's fd 1 is NOT the caller's pipe" \
    _v_field_not_pipe "$V_FDLOG2" "$V_ORPHAN_GLOB" 1
assert "V6 (RED, the defect): the detached orphan-sweep rm's fd 2 is NOT the caller's pipe" \
    _v_field_not_pipe "$V_FDLOG2" "$V_ORPHAN_GLOB" 2

# V15 (RED, task #6219 amendment 2): the loop-nested site. Measured on the
# pre-amendment script, this entry's fd 0 read back `pipe:[...]` -- the
# `< <(find ...)` process substitution feeding the sweep loop -- while V14's
# non-loop site read /dev/null, which is exactly why the async-list default
# could not carry the invariant. Benign in itself (seed's own exhausted find
# pipe, not the caller's) but false as a general claim, and a future detach
# placed inside a construct whose stdin IS the caller's would have inherited
# it silently.
assert "V15 (RED, the defect): the detached ORPHAN-SWEEP rm's fd 0 is /dev/null, not the stdin of the loop it is nested in" \
    _v_fd_target_is "$V_FDLOG2" "$V_ORPHAN_GLOB" 0 "/dev/null"

# ── V8: structural regression guard — a behavioural test only pins the two
# sites that exist TODAY; a future third detached job in this script would
# silently reintroduce the identical defect. Borrows the DETACH definition
# already used by tests/infra/test_flock_detached_fork_guard.sh (a
# non-comment line whose final effective token is a bare `&`, i.e. not `&&`)
# rather than inventing a competing one. ─────────────────────────────────────

# _v8_detach_offenders <file> [required-substring]... -- prints
# "<lineno>:<reasons>: <line>" for every DETACH line in <file> that does NOT
# also carry a stdout-TARGET redirect (a `>` -- optionally `1>` -- whose
# very next character is not `&`, once a trailing `# comment` is stripped)
# OR is missing any of the given [required-substring] tokens (task #6219
# FD-8 arm's V13: pass `_close_inherited_fds` to additionally require the
# caller-lock close). <reasons> names which criterion/criteria fired
# (no-stdout-redirect, missing-<token>), so an operator can tell a missing
# redirect from a missing close. With no extra args this is exactly the
# original V8 check. Exits 1 if any offender was printed, 0 if none --
# mirrors _flock_fork_offenders' print+return-code shape
# (tests/infra/test_flock_detached_fork_guard.sh).
#
# Stdout-TARGET, not bare `>` (amendment, tightening): a bare search for any
# `>` character wrongly treats a stderr-only dup (`2>&1`) or an fd-1-via-
# fd-2 dup (`>&2`) as "stdout redirected" -- neither ever points fd 1 at a
# real target, so a detached child spelled with only one of those still
# holds fd 1 on the caller's pipe, exactly the failure this whole block
# exists to catch. `[^&]` immediately after the `>` excludes both: the
# character right after `>` in `2>&1` and in `>&2` is literally `&`.
_v8_detach_offenders() {
    local f="$1"
    shift
    local reqs="$*"
    awk -v reqs="$reqs" '
        function is_detach(s) {
            sub(/[[:space:]]+#.*$/, "", s)
            return (s ~ /[^&>|]&[[:space:]]*$/)
        }
        BEGIN { nreq = split(reqs, req, " ") }
        {
            t = $0
            sub(/^[[:space:]]+/, "", t)
            if (t == "" || t ~ /^#/) next
            body = t
            sub(/[[:space:]]+#.*$/, "", body)
            if (!is_detach(t)) next
            reasons = ""
            if (body !~ /(^|[[:space:]])1?>[^&]/) reasons = reasons " no-stdout-redirect"
            for (i = 1; i <= nreq; i++) {
                if (index(body, req[i]) == 0) reasons = reasons " missing-" req[i]
            }
            if (reasons != "") {
                printf "%d:%s: %s\n", NR, reasons, t
                bad = 1
            }
        }
        END { exit (bad ? 1 : 0) }
    ' "$f"
}

# _v8_assert_no_offenders <file> [required-substring]... -- the assert-shaped
# wrapper: prints any offenders (captured by assert's own on-FAIL dump, per
# its tmpfile mechanism) and returns non-zero iff at least one was found.
# Extra args forward to _v8_detach_offenders verbatim.
_v8_assert_no_offenders() {
    local f="$1"
    shift
    local out rc=0
    out="$(_v8_detach_offenders "$f" "$@")" || rc=$?
    if [ "$rc" -ne 0 ]; then
        printf '%s\n' "$out"
        return 1
    fi
    return 0
}

# Sensitivity pin (self-match-safety convention shared with
# test_flock_detached_fork_guard.sh / test_no_new_wallclock_upper_bounds.sh /
# test_reify_audit_ptodo.sh): the offending shape is assembled from shell
# variables into a mktemp'd file, NEVER written as a literal line in this
# file — tests/infra/test_flock_detached_fork_guard.sh:762-763 asserts this
# very file scans clean under its own detached-fork/FD-9 scanner, and a
# literal offending line here would put that at risk.
V8_FIXTURE_DIR="$(mktemp -d "$_LANE_ROOT/V8-fixture-XXXXXX")"
V8_OFFENDER="$V8_FIXTURE_DIR/offender.sh"
V8_CLEAN="$V8_FIXTURE_DIR/redirected-twin.sh"
_v8_amp='&'
_v8_close='9<&-'
printf '{ rm -rf "$X"; } %s %s\n' "$_v8_close" "$_v8_amp" > "$V8_OFFENDER"
printf '{ rm -rf "$X"; } %s >/dev/null 2>&1 %s\n' "$_v8_close" "$_v8_amp" > "$V8_CLEAN"

V8_OFF_RC=0
_v8_detach_offenders "$V8_OFFENDER" >/dev/null || V8_OFF_RC=$?
assert "V8-sensitivity: the detector FLAGS a synthetic offender (9<&- & with no stdout redirect) -- proves it is not vacuously green" \
    test "$V8_OFF_RC" -ne 0

V8_CLEAN_RC=0
_v8_detach_offenders "$V8_CLEAN" >/dev/null || V8_CLEAN_RC=$?
assert "V8-sensitivity: the detector does NOT flag the redirected twin (>/dev/null 2>&1 added)" \
    test "$V8_CLEAN_RC" -eq 0

# Tightening pins (amendment): the no-stdout-redirect criterion used to
# search for ANY `>` character in the line body, so a stderr-only dup
# (`2>&1`) or an fd-1-via-fd-2 dup (`>&2`) satisfied it despite leaving fd 1
# pointed at whatever it was inherited as -- the caller's pipe, in the real
# failure this block exists to prevent -- since neither shape ever redirects
# fd 1 to a real target. Verified empirically against the shipped awk body
# BEFORE tightening it (see _v8_detach_offenders's doc-comment above): both
# synthetic lines below returned rc=0 (wrongly clean). Reuses $_v8_close
# ('9<&-') as inert filler, same convention as $V8_OFFENDER/$V8_CLEAN above
# -- these two pins are purely about the stdout criterion, so no extra args
# are passed to _v8_detach_offenders.
V8_STDERR_ONLY="$V8_FIXTURE_DIR/stderr-only-twin.sh"
V8_DUP_ONLY="$V8_FIXTURE_DIR/dup-only-twin.sh"
printf '{ rm -rf "$X"; } %s 2>&1 %s\n' "$_v8_close" "$_v8_amp" > "$V8_STDERR_ONLY"
printf '{ rm -rf "$X"; } %s >&2 %s\n' "$_v8_close" "$_v8_amp" > "$V8_DUP_ONLY"

V8_STDERR_ONLY_RC=0
_v8_detach_offenders "$V8_STDERR_ONLY" >/dev/null || V8_STDERR_ONLY_RC=$?
assert "V8-sensitivity (tightening): the detector FLAGS a synthetic near-miss that dups fd2 onto fd1 (2>&1) without ever redirecting fd1 off the caller's pipe" \
    test "$V8_STDERR_ONLY_RC" -ne 0

V8_DUP_ONLY_RC=0
_v8_detach_offenders "$V8_DUP_ONLY" >/dev/null || V8_DUP_ONLY_RC=$?
assert "V8-sensitivity (tightening): the detector FLAGS a synthetic near-miss that dups fd1 onto fd2 (>&2) without ever redirecting fd1 off the caller's pipe" \
    test "$V8_DUP_ONLY_RC" -ne 0

assert "V8: scripts/seed-warm-lane.sh has ZERO detached (bare-&) jobs lacking a stdout redirect (structural regression guard)" \
    _v8_assert_no_offenders "$SCRIPT"

# ── V9-V13: the FD-8 lane-lock arm — replicates warm-lane-gc.sh's production
# invocation EXACTLY: an EXCLUSIVE flock on the per-lane mutex (FD 8,
# ${lane}.lock) held in the PARENT, a SHARED flock on the gen lock (FD 9)
# held in an inner subshell, and seed invoked with --assume-lane-lock-held so
# it never opens its own FD 9 (scripts/warm-lane-gc.sh:559-565 the FD-8
# acquire, :645-649 the seed subshell + pipe; scripts/seed-warm-lane.sh's
# "flock is NOT re-entrant" comment, next to --assume-lane-lock-held's own
# flag parsing, names the convention: "thin --reseed on FD 9, gc reclaim on
# FD 8"). One lane with a NON-EMPTY target/ AND a pre-planted `<lane>.999999`
# orphan (H4e recipe, :2553-2565) fires BOTH detach sites in ONE run,
# mirroring the real caller shape rather than two synthetic ones.
#
# Disambiguating the two entries in the SHARED fdlog: the orphan's suffix is
# the known literal `.999999`, but the reseed-trash rename entry's suffix is
# seed's own $$ at runtime — unknown ahead of time and not safely
# distinguishable from `.999999` by a numeric-range glob (both are plain
# digit strings). It is identified instead by eliminating the orphan's
# exact, test-planted path (the `exclude` parameter threaded through
# _v_fdlog_has_entry/_v_fdlog_field above), never by log-write order: this
# file already caught one flake from assuming order (see the V4 comment).
V_LANE3="$(make_isolated_lane V-lane3)"
mkdir -p "$V_LANE3/target"
echo "stale" > "$V_LANE3/target/stale.a"

V_ORPHAN_PATH3="$(dirname "$V_LANE3")/.reseed-trash/$(basename "$V_LANE3").999999"
mkdir -p "$V_ORPHAN_PATH3"
echo "orphan artifact" > "$V_ORPHAN_PATH3/orphan.a"

V_ORPHAN_GLOB3="*/.reseed-trash/$(basename "$V_LANE3").999999"
V_TRASH_GLOB3="*/.reseed-trash/$(basename "$V_LANE3").*"

# gc's ${WORKTREES_DIR}/${name}.lock == seed's own ${LANE_DIR}.lock
# (scripts/seed-warm-lane.sh's `LANE_LOCK="${LANE_DIR}.lock"` assignment) --
# the SAME file, minted fresh here exactly as make_isolated_lane's own
# docstring anticipates ("its sibling
# ${lane}.lock ... files"), reclaimed by the suite's one EXIT trap with no
# new _TMPDIRS registration.
V_LOCK3="${V_LANE3}.lock"
: > "$V_LOCK3"
V_GENLOCK3="$_LANE_ROOT/.v-genlock3"
: > "$V_GENLOCK3"

V_FDLOG3="$_LANE_ROOT/.v-fdlog3"
: > "$V_FDLOG3"
V_SINK4="$_LANE_ROOT/.v-sink4"

set +e
( exec 8>"$V_LOCK3"
  flock -n 8 || exit 90
  exec 9>"$V_GENLOCK3"
  flock -s 9
  PATH="$V_STUB:$PATH" RUSTFLAGS="" REIFY_TEST_FDLOG="$V_FDLOG3" \
      bash "$SCRIPT" "$V_BASE" "$V_LANE3" --fresh-checkout --assume-lane-lock-held
) 2>&1 | cat > "$V_SINK4"
V9_RC="${PIPESTATUS[0]}"
set -e

assert "V9-fixture: seed exits 0 under the gc-mirroring FD-8/FD-9 lock harness" \
    test "$V9_RC" -eq 0
assert "V9-fixture (site fired): the reseed-trash rename entry was logged" \
    _wait_until 10 _v_fdlog_has_entry "$V_FDLOG3" "$V_TRASH_GLOB3" "$V_ORPHAN_PATH3"
assert "V9-fixture (site fired): the orphan-sweep entry was logged" \
    _wait_until 10 _v_fdlog_has_entry "$V_FDLOG3" "$V_ORPHAN_GLOB3"
assert "V9-fixture (non-vacuity, sweep really fired): captured output names the swept orphan entry (mirrors H4e's assert at :2571)" \
    grep -q "Sweeping orphaned trash entry" "$V_SINK4"

assert "V9 (RED, the defect): the detached RESEED-TRASH rm's fd inventory holds no descriptor onto the caller's lane-lock file" \
    _v_fdset_missing_target "$V_FDLOG3" "$V_TRASH_GLOB3" "$V_LOCK3" "$V_ORPHAN_PATH3"
assert "V10 (RED, the defect): the detached ORPHAN-SWEEP rm's fd inventory holds no descriptor onto the caller's lane-lock file" \
    _v_fdset_missing_target "$V_FDLOG3" "$V_ORPHAN_GLOB3" "$V_LOCK3"

assert "V11 (regression pin): the detached RESEED-TRASH rm's fd inventory holds no descriptor onto the gen-lock file" \
    _v_fdset_missing_target "$V_FDLOG3" "$V_TRASH_GLOB3" "$V_GENLOCK3" "$V_ORPHAN_PATH3"
assert "V11 (regression pin): the detached ORPHAN-SWEEP rm's fd inventory holds no descriptor onto the gen-lock file" \
    _v_fdset_missing_target "$V_FDLOG3" "$V_ORPHAN_GLOB3" "$V_GENLOCK3"

# ── V12: the SYNC control arm — proves FD 8 was genuinely wired ────────────
# Same reasoning as V4: a FRESH, never-before-seeded lane (no orphan
# planted), wrapped in the SAME gc-mirroring exec-8/exec-9 harness, with
# REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 so the reseed-trash rm runs in the
# FOREGROUND and therefore legitimately inherits FD 8. Without this arm,
# every "does not contain" assert above (V9, V10, V11) would pass trivially
# if the harness never opened FD 8 at all.
V_LANE3_SYNC="$(make_isolated_lane V-lane3-sync)"
mkdir -p "$V_LANE3_SYNC/target"
echo "stale" > "$V_LANE3_SYNC/target/stale.a"
V_TRASH_GLOB3_SYNC="*/.reseed-trash/$(basename "$V_LANE3_SYNC").*"

V_LOCK3_SYNC="${V_LANE3_SYNC}.lock"
: > "$V_LOCK3_SYNC"
V_GENLOCK3_SYNC="$_LANE_ROOT/.v-genlock3-sync"
: > "$V_GENLOCK3_SYNC"

V_FDLOG3_SYNC="$_LANE_ROOT/.v-fdlog3-sync"
: > "$V_FDLOG3_SYNC"
V_SINK5="$_LANE_ROOT/.v-sink5"

set +e
( exec 8>"$V_LOCK3_SYNC"
  flock -n 8 || exit 90
  exec 9>"$V_GENLOCK3_SYNC"
  flock -s 9
  PATH="$V_STUB:$PATH" RUSTFLAGS="" REIFY_TEST_FDLOG="$V_FDLOG3_SYNC" \
      REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 \
      bash "$SCRIPT" "$V_BASE" "$V_LANE3_SYNC" --fresh-checkout --assume-lane-lock-held
) 2>&1 | cat > "$V_SINK5"
V12_RC="${PIPESTATUS[0]}"
set -e

assert "V12-fixture: the SYNC control run exits 0 under the gc-mirroring lock harness" \
    test "$V12_RC" -eq 0
assert "V12 (non-vacuity, FD 8 really wired): under REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 the (now-foreground) reseed-trash rm's fd inventory DOES hold a descriptor onto the caller's lane-lock file, proving this harness genuinely wires FD 8" \
    _v_fdset_has_target "$V_FDLOG3_SYNC" "$V_TRASH_GLOB3_SYNC" "$V_LOCK3_SYNC"

# ── V13: structural regression guard, generic-close criterion ───────────────
# Extends the V8 detector (above) with a second criterion via
# _v8_detach_offenders's optional [required-substring] args: a DETACH line
# must ALSO call `_close_inherited_fds` (task #6219 amendment), not just
# carry a stdout redirect. Kept as a SEPARATE assertion (not a rewrite of
# V8's own call) so V8's original, narrower check is untouched and stays
# green throughout this task.
#
# Mechanism note: scripts/seed-warm-lane.sh originally closed the caller's
# lock FD(s) by number (`8<&-`, `9<&-`) at each detach site -- an inverted
# dependency where the callee had to track every caller's private
# FD-locking convention, which had already produced the identical class of
# leak twice (FD 9 in #5705, FD 8 in #6219). It now calls a single shared
# `_close_inherited_fds` helper (its own doc-comment, next to the log
# helpers, is the single source of truth for the argument -- not restated
# here, G7) that closes every inherited descriptor generically, so V13's
# structural criterion checks for a CALL to that helper rather than a
# token-match on specific FD numbers.
#
# Sensitivity pins reuse $V8_FIXTURE_DIR (same self-match-safety convention
# noted above): a fresh twin has the stdout redirect V8 already requires but
# omits the `_close_inherited_fds` call, isolating this criterion from V8's.
V13_MISSING_CLOSE="$V8_FIXTURE_DIR/missing-close-twin.sh"
printf '{ rm -rf "$X"; } >/dev/null 2>&1 %s\n' "$_v8_amp" > "$V13_MISSING_CLOSE"
V13_CLEAN="$V8_FIXTURE_DIR/generic-close-twin.sh"
printf '{ _close_inherited_fds; rm -rf "$X"; } >/dev/null 2>&1 %s\n' "$_v8_amp" > "$V13_CLEAN"

V13_OFF_RC=0
_v8_detach_offenders "$V13_MISSING_CLOSE" '_close_inherited_fds' >/dev/null || V13_OFF_RC=$?
assert "V13-sensitivity: the extended detector FLAGS a synthetic offender that redirects stdout but never calls _close_inherited_fds -- proves the new criterion is not vacuously green" \
    test "$V13_OFF_RC" -ne 0

V13_CLEAN_RC=0
_v8_detach_offenders "$V13_CLEAN" '_close_inherited_fds' >/dev/null || V13_CLEAN_RC=$?
assert "V13-sensitivity: the extended detector does NOT flag the { _close_inherited_fds; rm ...; } >/dev/null 2>&1 & twin" \
    test "$V13_CLEAN_RC" -eq 0

assert "V13: scripts/seed-warm-lane.sh has ZERO detached (bare-&) jobs failing EITHER criterion (stdout redirect, _close_inherited_fds call) -- structural regression guard for the caller-lock-FD leak" \
    _v8_assert_no_offenders "$SCRIPT" '_close_inherited_fds'

# ── V17: structural regression guard, explicit-stdin criterion ─────────────
# (task #6219 amendment 2) Extends the detector with a THIRD criterion --
# a DETACH line must ALSO carry an explicit `</dev/null` -- reusing
# _v8_detach_offenders's existing [required-substring] mechanism verbatim, so
# no awk change is needed. Kept as a SEPARATE assertion for the same reason
# V13 was: V8's and V13's own narrower checks stay untouched and green.
#
# Why a structural criterion and not just V14/V15's behavioural pair: the
# behavioural asserts pin the two sites that exist TODAY, and the property
# being pinned is one bash grants CONDITIONALLY -- an async list gets
# /dev/null on stdin only "in the absence of any explicit redirections", so a
# future third detach placed inside any construct that redirects stdin (a
# `while ... done < <(...)` loop, a `... < file` block) silently loses it. The
# `_close_inherited_fds` sweep cannot cover the gap either: it deliberately
# starts at fd 3. See `_close_inherited_fds`'s doc-comment in
# scripts/seed-warm-lane.sh for the measurement (G7, not restated).
#
# Offender fixture: $V13_CLEAN is reused as-is -- it is the exact
# `{ _close_inherited_fds; rm -rf "$X"; } >/dev/null 2>&1 &` shape that is
# CLEAN under V8's and V13's criteria and must now be FLAGGED under this one,
# which isolates the new criterion from both of the older ones with no new
# fixture file. Same self-match-safety convention throughout (the shapes are
# assembled from $_v8_amp, never written as literal detach lines here).
V17_STDIN='</dev/null'
V17_CLEAN="$V8_FIXTURE_DIR/explicit-stdin-twin.sh"
printf '{ _close_inherited_fds; rm -rf "$X"; } %s >/dev/null 2>&1 %s\n' \
    "$V17_STDIN" "$_v8_amp" > "$V17_CLEAN"

V17_OFF_RC=0
_v8_detach_offenders "$V13_CLEAN" '_close_inherited_fds' "$V17_STDIN" >/dev/null || V17_OFF_RC=$?
assert "V17-sensitivity: the extended detector FLAGS a detach that closes inherited fds and redirects stdout but leaves stdin to bash's conditional async-list default -- proves the new criterion is not vacuously green" \
    test "$V17_OFF_RC" -ne 0

V17_CLEAN_RC=0
_v8_detach_offenders "$V17_CLEAN" '_close_inherited_fds' "$V17_STDIN" >/dev/null || V17_CLEAN_RC=$?
assert "V17-sensitivity: the extended detector does NOT flag the twin that adds the explicit </dev/null" \
    test "$V17_CLEAN_RC" -eq 0

assert "V17: scripts/seed-warm-lane.sh has ZERO detached (bare-&) jobs failing ANY of the three criteria (stdout redirect, _close_inherited_fds call, explicit </dev/null) -- structural regression guard for the inherited-stdin leak" \
    _v8_assert_no_offenders "$SCRIPT" '_close_inherited_fds' "$V17_STDIN"

# ── V18: _close_inherited_fds must not clobber the caller's stderr ──────────
# (task #6219 amendment 2) The helper suppressed close errors with
# `eval "exec ${_n}<&- 2>/dev/null"` -- the redirect INSIDE the eval'd string,
# where it is a redirection on an `exec` with NO command, which bash applies
# PERMANENTLY to the shell running it rather than to the close. The whole
# diagnostic channel scripts/seed-warm-lane.sh's header contract declares
# ("Stderr: all diagnostics, progress messages, and errors") therefore went to
# /dev/null for the rest of the process, err() before a fail-closed abort
# included. It was masked at both shipped call sites only because each wraps
# the helper in a group that is already `>/dev/null 2>&1`, so the effect was
# invisible until some future caller invoked the helper -- which its own
# doc-comment invites -- from a path that had not pre-redirected fd 2.
#
# Tested against the SHIPPED function body, extracted from the real script
# rather than retyped, so a reword of the helper cannot leave this pin
# silently testing a stale copy. The MUTATION CONTROL below re-inserts the
# defect literally (assembled by literal awk substring replacement, never a
# regex, so no shell/regex metacharacter in the fd spelling can misfire) and
# asserts the probe flips RED -- without it, "stderr survived" would pass for
# any reason at all, including the probe never exercising the helper.
V18_DIR="$(mktemp -d "$_LANE_ROOT/V18-XXXXXX")"
V18_FN="$V18_DIR/fn.sh"
sed -n '/^_close_inherited_fds() {$/,/^}$/p' "$SCRIPT" > "$V18_FN"

assert "V18-fixture: the shipped _close_inherited_fds body was extracted from scripts/seed-warm-lane.sh" \
    grep -q 'exec \${_n}<&-' "$V18_FN"

# Probe: source the extracted body, open one descriptor >= 3 so the loop has
# something real to close, call the helper, then write a sentinel to BOTH
# stdout and stderr. The caller captures the two streams into separate files,
# so "did the helper survive with fd 2 intact" is a plain grep, with no
# timing, no background job and no /proc self-reference subtleties.
V18_PROBE="$V18_DIR/probe.sh"
cat > "$V18_PROBE" << 'V18_PROBE_EOF'
#!/usr/bin/env bash
set -uo pipefail
. "$1"
exec 7>"$2"
_close_inherited_fds
echo "STDERR-ALIVE" >&2
if [ -e /proc/self/fd/7 ]; then echo "FD7-OPEN"; else echo "FD7-CLOSED"; fi
V18_PROBE_EOF
chmod +x "$V18_PROBE"

V18_OUT="$V18_DIR/out"
V18_ERR="$V18_DIR/err"
set +e
bash "$V18_PROBE" "$V18_FN" "$V18_DIR/squat" > "$V18_OUT" 2> "$V18_ERR"
V18_RC=$?
set -e

assert "V18-fixture: the probe exits 0" test "$V18_RC" -eq 0
assert "V18-fixture (helper really ran): the shipped body closed the fd 7 the probe opened for it" \
    grep -q '^FD7-CLOSED$' "$V18_OUT"
assert "V18: the shipped _close_inherited_fds leaves the caller's stderr intact -- the close-error suppression is scoped to the eval builtin, not applied to the shell" \
    grep -q '^STDERR-ALIVE$' "$V18_ERR"

# Mutation control: put the redirect back INSIDE the eval'd string.
_v18_close_ok='eval "exec ${_n}<&-" 2>/dev/null'
_v18_close_bad='eval "exec ${_n}<&- 2>/dev/null"'
V18_MUT="$V18_DIR/fn-mut.sh"
awk -v ok="$_v18_close_ok" -v bad="$_v18_close_bad" '{
        i = index($0, ok)
        if (i > 0) { $0 = substr($0, 1, i - 1) bad substr($0, i + length(ok)) }
        print
     }' "$V18_FN" > "$V18_MUT"

V18_MUT_DIFFERS=0
cmp -s "$V18_FN" "$V18_MUT" || V18_MUT_DIFFERS=1
assert "V18-sensitivity fixture: the mutation actually rewrote the shipped eval line (so the control below is not testing an unchanged copy)" \
    test "$V18_MUT_DIFFERS" -eq 1

V18_MUT_OUT="$V18_DIR/mut-out"
V18_MUT_ERR="$V18_DIR/mut-err"
set +e
bash "$V18_PROBE" "$V18_MUT" "$V18_DIR/squat-mut" > "$V18_MUT_OUT" 2> "$V18_MUT_ERR"
set -e

V18_MUT_STDERR_RC=0
grep -q '^STDERR-ALIVE$' "$V18_MUT_ERR" || V18_MUT_STDERR_RC=$?
assert "V18-sensitivity: the pre-amendment spelling SWALLOWS the caller's stderr -- proves V18 is not vacuously green" \
    test "$V18_MUT_STDERR_RC" -ne 0
assert "V18-sensitivity: the pre-amendment spelling still closed fd 7, so the two spellings differ ONLY in the stderr clobber" \
    grep -q '^FD7-CLOSED$' "$V18_MUT_OUT"

# ─────────────────────────────────────────────────────────────────────────────
# Block R: lane isolation guards (task 5590) — every lane created in this file
# must be nested under a private per-run parent, never bare /tmp, because
# scripts/seed-warm-lane.sh:663 computes RESEED_TRASH_DIR as
# dirname(LANE_DIR)/.reseed-trash: a bare-/tmp lane makes that the
# machine-shared /tmp/.reseed-trash, shared across every concurrent agent/test
# run on the host. R0 pins the contract of the `make_isolated_lane <prefix>`
# helper introduced to fix this; later sub-blocks verify the fix itself.
#
# Ordering note (task 5590 amend): R1/R3/R4 assert ONCE, before R2/R5 run.
# R2 and R5 both transiently mutate _SHARED_TRASH_DIR/_TRASH_HITS_FILE and
# restore/clear them when done, so by the end of this block the detector
# state is back to a clean slate — but R1 does NOT re-run against it. A new
# block appended AFTER Block R that calls run_helper/run_helper_real gets no
# shared-trash-litter coverage unless it is inserted BEFORE R2 (so the
# existing R1 assert still covers it) or it re-asserts
# _assert_no_shared_trash_use itself after R5.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block R: lane isolation guards ---"

# ── R0: make_isolated_lane <prefix> contract ─────────────────────────────────
# Guarded with `2>/dev/null || true`: make_isolated_lane does not exist yet, so
# without the guard the command substitution would still just yield rc=127
# captured by `|| true` (set -e does not abort on a substitution used in an
# assignment) — but the guard keeps a "command not found" line off the console
# and lets the not-yet-implemented state read as ordinary assert FAILures below.
R0_LANE_A="$(make_isolated_lane R0-a 2>/dev/null || true)"
R0_LANE_B="$(make_isolated_lane R0-a 2>/dev/null || true)"

assert "R0a: make_isolated_lane returns a path that exists and is a directory" \
    bash -c '[ -n "$1" ] && [ -d "$1" ]' _ "$R0_LANE_A"
assert "R0b: dirname of the returned lane is NOT bare /tmp (private parent)" \
    bash -c '[ -n "$1" ] || exit 1; [ "$(dirname "$1")" != "/tmp" ]' _ "$R0_LANE_A"
assert "R0c: the parent contains the lane and nothing else (private, freshly minted)" \
    bash -c '[ -n "$1" ] || exit 1; [ "$(ls -A "$(dirname "$1")")" = "$(basename "$1")" ]' _ "$R0_LANE_A"
assert "R0d: two calls with the same prefix return different parents" \
    bash -c '[ -n "$1" ] && [ -n "$2" ] || exit 1; [ "$(dirname "$1")" != "$(dirname "$2")" ]' _ "$R0_LANE_A" "$R0_LANE_B"
assert "R0e: dirname(<lane>)/.reseed-trash does not already exist (run-private)" \
    bash -c '[ -n "$1" ] || exit 1; [ ! -e "$(dirname "$1")/.reseed-trash" ]' _ "$R0_LANE_A"
assert "R0f: returned path is nested under the per-run lane root (_LANE_ROOT), which the EXIT trap reclaims" \
    bash -c '[ -n "$1" ] && [ -n "$2" ] || exit 1; case "$1" in "$2"/*) exit 0 ;; *) exit 1 ;; esac' _ "$R0_LANE_A" "${_LANE_ROOT:-}"

# ── R1: runtime detector — no seed invocation in this whole suite may write
# into the machine-shared /tmp/.reseed-trash. Fed by _note_shared_trash_use,
# called from inside run_helper/run_helper_real after every invocation
# throughout the file, so this observes every prior Block's fixtures too.
# State is file-backed (see _TRASH_HITS_FILE above) so appends made from
# inside a backgrounded subshell (H5d/H9) are not silently discarded. The
# checker itself is _assert_no_shared_trash_use, promoted into
# tests/infra/test_helpers.sh (task 5612) and pinned to that home by R7. ────
assert "R1: no seed invocation in this suite wrote into the machine-shared $_SHARED_TRASH_DIR" \
    _assert_no_shared_trash_use

# ── R3: structural guard — every run_helper_real lane dir had a private
# parent, never bare /tmp. Fed by _note_real_lane, called at the top of
# run_helper_real for every one of its call sites, which logs EVERY lane
# unconditionally — the bare-/tmp filtering happens here, reading the
# file-backed log line by line (this checker runs in the main shell via
# `assert`'s no-subshell "$@" invocation, so a local array is safe here even
# though the recorder itself cannot use one).
#
# _assert_no_bare_tmp_lanes <log-file> <label>: parameterized (task 5609) so
# R6 below can share this ONE implementation for run_helper's lanes instead
# of a near-duplicate checker — <label> names the call convention in the
# offender message (e.g. "run_helper_real" here, "run_helper" for R6). ─────
_assert_no_bare_tmp_lanes() {
    local log="$1" label="$2"
    local lane
    local offenders=()
    while IFS= read -r lane; do
        [ -n "$lane" ] || continue
        case "$lane" in
            /*) [ "$(dirname "$lane")" = "/tmp" ] && offenders+=("$lane") ;;
        esac
    done < "$log"
    [ "${#offenders[@]}" -eq 0 ] && return 0
    printf '%s lane dir was bare /tmp (no private parent): %s\n' \
        "$label" "${offenders[@]}"
    return 1
}
assert "R3: every run_helper_real lane dir had a private parent (never bare /tmp)" \
    _assert_no_bare_tmp_lanes "$_REAL_LANES_FILE" "run_helper_real"

# ── R4: end-to-end coverage — the structural detector must have observed the
# two run_helper_real invocations made from inside a backgrounded ( ... ) &
# subshell: H5d's Q_LANE8 and H9's Q_LANE11. These are the ONLY two such
# sites in the file — every other `( ... ) &` in Block Q is a
# `( flock -x 9 && touch ... && sleep 300 ) 9>"$LOCK" &` lock-holder, not a
# seed run. $Q_LANE8/$Q_LANE11 are assigned in the MAIN shell (Block Q, in
# the H5d/H9 fixture setup, before their respective backgrounded subshells)
# via make_isolated_lane, so both are in scope here. Asserted against
# _REAL_LANES_FILE, the observation log every run_helper_real invocation
# appends to via _note_real_lane (including from inside H5d/H9's backgrounded
# subshells, now that the log is file-backed rather than an array). The
# ${_REAL_LANES_FILE:-/nonexistent} fallback is defensive only — it keeps this
# a plain assert FAILURE rather than an unbound-variable abort under set -u if
# the variable were ever undefined. grep -Fxq matches the lane path literally
# and as a whole line. ───────────────────────────────────────────────────────
assert "R4: the structural detector observed the run_helper_real invocations made from inside a backgrounded subshell (H5d/H9)" \
    bash -c 'grep -Fxq -- "$2" "$1" && grep -Fxq -- "$3" "$1"' _ "${_REAL_LANES_FILE:-/nonexistent}" "$Q_LANE8" "$Q_LANE11"

# ── R6: regression guard — no run_helper (as opposed to run_helper_real)
# lane arg was bare /tmp either (task 5609). This pays off the follow-up the
# _note_real_lane comment above used to defer: run_helper's call sites are
# NOT uniformly <base> <lane> [flags...] (4 are flag-first with no lane arg
# at all), so wiring this in safely required auditing every call site's
# positional shape first — see _note_helper_lane's own comment for that
# audit. Shares the SAME _assert_no_bare_tmp_lanes checker R3 uses above,
# parameterized by <log-file, label> so R3 and R6 are one implementation,
# two callers, rather than a near-duplicate checker. ────────────────────────
assert "R6: no run_helper lane arg was bare /tmp" \
    _assert_no_bare_tmp_lanes "${_HELPER_LANES_FILE:-/nonexistent}" "run_helper"

# ── R6a: coverage signal for R6, mirroring R4's role for R3 — proves
# _note_helper_lane is actually wired in and recording, so R6 above cannot
# pass vacuously forever against an empty log. Checks for I_SC_BASE_PARENT
# specifically: task 5609's own I11-I12 fixture (the lane this whole task
# migrated off bare /tmp), which is a plain run_helper call and so must have
# been recorded once _note_helper_lane exists. The
# ${_HELPER_LANES_FILE:-/nonexistent} fallback is defensive only, mirroring
# R4's ${_REAL_LANES_FILE:-/nonexistent} idiom: it keeps this a plain assert
# FAILURE rather than a set -u abort before _HELPER_LANES_FILE is wired up.
# grep -Fxq matches the lane path literally and as a whole line. ───────────
assert "R6a: \$_HELPER_LANES_FILE exists, is non-empty, and recorded \$I_SC_BASE_PARENT" \
    bash -c '[ -s "$1" ] && grep -Fxq -- "$2" "$1"' _ "${_HELPER_LANES_FILE:-/nonexistent}" "$I_SC_BASE_PARENT"

# ── R6b: positive control for the shared _assert_no_bare_tmp_lanes checker —
# proves it actually flags a bare-/tmp entry (even amid a good entry in the
# same log) and passes a clean log, so R6/R3 cannot silently stop firing if
# the checker is ever refactored. Mirrors R2/R5's positive-control role for
# the other detector. Synthetic log files only (no real seed run needed):
# the checker only inspects path STRINGS line by line, never touches disk
# itself.
#
# The mixed-log branch also inspects the OFFENDER MESSAGE content, not just
# the exit status: <label> is the one new surface the task 5609 amendment
# added to _assert_no_bare_tmp_lanes (parameterizing it for R6's benefit), so
# an exit-status-only check would stay green through a refactor that broke
# the message (dropped the offenders expansion, or swapped the label/offender
# argument order between R3 and R6) — the exact regression this positive
# control exists to catch. ──────────────────────────────────────────────────
R6B_NESTED_LANE="$_LANE_ROOT/synthetic-nested-lane"
R6B_MIXED_LOG="$_LANE_ROOT/.r6b-mixed-log"
R6B_CLEAN_LOG="$_LANE_ROOT/.r6b-clean-log"
printf '%s\n%s\n' "/tmp/synthetic-bare-lane" "$R6B_NESTED_LANE" > "$R6B_MIXED_LOG"
printf '%s\n' "$R6B_NESTED_LANE" > "$R6B_CLEAN_LOG"
_r6b_positive_control() {
    local mixed_log="$1" clean_log="$2" label="$3"
    local out
    out="$(_assert_no_bare_tmp_lanes "$mixed_log" "$label" 2>&1)" && return 1
    case "$out" in
        *"$label"*"/tmp/synthetic-bare-lane"*) ;;
        *) return 1 ;;
    esac
    _assert_no_bare_tmp_lanes "$clean_log" "$label" >/dev/null 2>&1
}
assert "R6b: _assert_no_bare_tmp_lanes flags a bare-/tmp entry in a mixed log as an offender (message names both the label and the offending path) AND passes a clean nested-only log" \
    _r6b_positive_control "$R6B_MIXED_LOG" "$R6B_CLEAN_LOG" "R6b-synthetic"

# ── R7: duplicate-definition guard (task 5612) — make_isolated_lane and the
# shared-trash detector now live in tests/infra/test_helpers.sh, which this
# file sources near the top. The hazard the promotion creates is that a future
# edit reintroduces a LOCAL copy here: bash silently keeps the last definition,
# so the local copy would shadow the library's and this suite would drift back
# to its own private implementation with every assert still green.
#
# Pinned BEHAVIOURALLY rather than by grepping this file's source text: with
# `shopt -s extdebug`, `declare -F <fn>` prints "<name> <lineno> <file>", so
# this reads the ACTIVE definition's provenance straight out of the shell.
# A source-grep would go stale the moment the helper is mentioned in a comment
# or a heredoc; this cannot. The prior extdebug setting is restored so no later
# assert's behaviour changes.
#
# Placed here with the other structural guards, BEFORE R2/R5 mutate detector
# state (see the ordering note at the head of Block R). ──────────────────────
_assert_defined_in_test_helpers() {
    local fn="$1" src extdebug_was=off
    if shopt -q extdebug; then extdebug_was=on; fi
    shopt -s extdebug
    src="$(declare -F "$fn")"
    if [ "$extdebug_was" = off ]; then shopt -u extdebug; fi
    src="${src#* }"   # drop "<name> "
    src="${src#* }"   # drop "<lineno> ", leaving the defining file
    case "$src" in
        */tests/infra/test_helpers.sh) return 0 ;;
    esac
    printf '%s is defined in %s, not tests/infra/test_helpers.sh — a local duplicate shadows the promoted helper\n' \
        "$fn" "${src:-<undefined>}"
    return 1
}
for _r7_fn in make_isolated_lane _note_shared_trash_use _assert_no_shared_trash_use; do
    assert "R7: the active $_r7_fn definition comes from tests/infra/test_helpers.sh (no local duplicate shadows the promoted helper)" \
        _assert_defined_in_test_helpers "$_r7_fn"
done

# ── R2: positive control for R1 — proves the detector actually fires on a
# real rename, so R1 cannot silently pass forever if seed's rename message is
# ever reworded or moved. Redirects _SHARED_TRASH_DIR to an isolated lane's
# OWN private sibling .reseed-trash for one real seed run (rather than
# deliberately littering the real shared path once per suite run). ─────────
R2_BASE_PARENT="$(make_isolated_lane R2-base)"
R2_BASE="$R2_BASE_PARENT/target"
mkdir -p "$R2_BASE/debug"
echo "base artifact" > "$R2_BASE/debug/base_artifact.a"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$R2_BASE_PARENT/.warm-base-meta"

R2_LANE="$(make_isolated_lane R2-poscontrol)"
mkdir -p "$R2_LANE/target"
echo "stale artifact" > "$R2_LANE/target/stale.a"

# Redirect the detector at this lane's own sibling trash dir — exactly what
# seed-warm-lane.sh:663 will independently compute as dirname(LANE_DIR)/.reseed-trash.
# Save the pre-redirect value rather than re-typing the literal default at
# restore time below (task 5590 amend): if the canonical default near the top
# of the file is ever changed, restoring a hardcoded literal here would
# silently revert that change instead of tracking it, and every detector
# check after this point (R5 today, plus any block appended later) would
# match against a path seed never actually produces.
_SHARED_TRASH_DIR_SAVED="$_SHARED_TRASH_DIR"
_SHARED_TRASH_DIR="$(dirname "$R2_LANE")/.reseed-trash"
: > "$_TRASH_HITS_FILE"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$R2_BASE" "$R2_LANE" --fresh-checkout

assert "R2: positive control: redirected-trash seed run exits 0" \
    test "$RC" -eq 0
assert "R2: positive control: detector recorded exactly one hit against the redirected trash dir" \
    test "$(wc -l < "$_TRASH_HITS_FILE")" -eq 1

# Restore the real shared-path target and clear the positive-control hit
# before R1 (already asserted above) or any later block could see it. Restore
# FROM THE SAVED COPY, not a re-typed literal — see the save above.
_SHARED_TRASH_DIR="$_SHARED_TRASH_DIR_SAVED"
: > "$_TRASH_HITS_FILE"

# ── R5: mechanism guard — a recorder append made from inside a backgrounded
# ( ... ) & subshell must be visible to the parent shell. Drives the REAL
# _note_shared_trash_use recorder through the exact H5d/H9 shape (a subshell
# that sets ERR_OUT and calls the recorder, backgrounded, then waited on) with
# a synthetic ERR_OUT rather than a second real seed run: R2 above already
# covers "real seed → detector fires" end-to-end, so R5 only needs to prove
# storage survives a subshell. This is the durable companion to R4 — R4 could
# go vacuous if H5d/H9 were ever un-backgrounded (it would then silently test
# nothing while still passing), but R5 constructs its own subshell, so it
# always exercises the subshell-visibility property directly. ──────────────
: > "$_TRASH_HITS_FILE"
(
    ERR_OUT="Renaming non-empty /x/target → $_SHARED_TRASH_DIR before re-seed"
    _note_shared_trash_use R5-subshell-probe
) &
R5_PID=$!
wait "$R5_PID" 2>/dev/null || true

assert "R5: a shared-trash-use append made inside a backgrounded subshell is visible to the parent shell" \
    test "$(wc -l < "$_TRASH_HITS_FILE")" -eq 1

# Clear the probe's state so nothing leaks past test_summary.
: > "$_TRASH_HITS_FILE"

# ── R8: real-seed end-to-end proof for the LITTER guard (task 5612) — the
# filesystem-based companion to R2's ERR_OUT-based positive control.
#
# R2 proves the RECORDER fires on seed's stderr. It says nothing about whether
# _assert_no_shared_trash_litter — the guard the six sibling suites actually
# rely on, since none of them has a run_helper wrapper that sets $ERR_OUT —
# observes what seed WRITES. A synthetic entry (as the hermetic liveness control
# uses) proves the checker's logic but not that its matcher agrees with reality.
# R8 closes that gap: it drives the REAL seed and asserts the guard fires on the
# entry seed itself produced, whose basename is "<lane-basename>.<pid>".
#
# THE LANE IS MINTED BY make_isolated_lane AND CHECKED AGAINST THIS SUITE'S OWN
# $_LANE_LITTER_PREFIX — not a bespoke hand-named fixture. That is the whole
# point: it closes the attribution loop end-to-end, proving the name the library
# actually gives its lanes is the name the guard actually matches. A hand-minted
# lane would prove only that the matcher works on a name the fixture chose, and
# would still pass if make_isolated_lane were changed to a stemless
# "lane-XXXXXX" — the exact regression that would silently demote every lane in
# every wired suite to "unattributed" and leave the guard unable to fail.
# The whole fixture stays inside $_LANE_ROOT, so the real /tmp/.reseed-trash is
# never touched.
#
# REIFY_TEST_PIN_RESEED_TRASH=1 pins the trash on disk: seed's trash rm is a
# DETACHED GRANDCHILD (see Block T), so without the pin this assert would race
# it and flake. Same technique I14 already uses to inspect the trash location.
# ─────────────────────────────────────────────────────────────────────────────
R8_STEM="$_LANE_LITTER_PREFIX"
R8_BASE_PARENT="$(make_isolated_lane R8-base)"
R8_BASE="$R8_BASE_PARENT/target"
mkdir -p "$R8_BASE/debug"
echo "base artifact" > "$R8_BASE/debug/base_artifact.a"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$R8_BASE_PARENT/.warm-base-meta"

R8_LANE="$(make_isolated_lane R8)"
R8_PARENT="$(dirname "$R8_LANE")"
R8_LANE_BASE="$(basename "$R8_LANE")"
# Non-empty target/ so seed reaches the rename-into-trash path at all.
mkdir -p "$R8_LANE/target"
echo "stale" > "$R8_LANE/target/stale.a"

# Redirect the shared-path variable at the trash dir seed will independently
# compute as dirname(LANE_DIR)/.reseed-trash. Save the prior value and restore
# FROM THE SAVED COPY below, never a re-typed literal — see R2's note above for
# why re-typing would silently revert a future change to the canonical default.
_SHARED_TRASH_DIR_SAVED_R8="$_SHARED_TRASH_DIR"
_SHARED_TRASH_DIR="$R8_PARENT/.reseed-trash"
: > "$_TRASH_HITS_FILE"

# Snapshot BEFORE the run, exactly as init_isolated_lane_root does for a suite.
R8_SNAP="$_LANE_ROOT/.r8-snapshot"
_list_trash_entries "$_SHARED_TRASH_DIR" > "$R8_SNAP"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_TEST_PIN_RESEED_TRASH=1 \
    run_helper_real "$R8_BASE" "$R8_LANE" --fresh-checkout

R8_ENTRIES="$(_list_trash_entries "$_SHARED_TRASH_DIR" | tr '\n' ' ')"
R8_RC=0
R8_OUT="$(_assert_no_shared_trash_litter "$_SHARED_TRASH_DIR" "$R8_SNAP" "$R8_STEM" 2>&1)" || R8_RC=$?

assert "R8a: real-seed litter fixture: the seed run exits 0" \
    test "$RC" -eq 0
assert "R8a2: ATTRIBUTION LOOP — the lane make_isolated_lane minted carries this suite's own stem ($R8_STEM), so the guard is checking the name the library really produces" \
    bash -c 'case "$2" in "$1"*) exit 0 ;; esac; exit 1' _ "$R8_STEM" "$R8_LANE_BASE"
assert "R8b: NON-VACUITY — seed really did rename into dirname(LANE)/.reseed-trash, producing a <lane-basename>.<pid> entry (without this, R8c would be asserting against an empty dir)" \
    bash -c 'case "$1" in *"$2".*) exit 0 ;; esac; exit 1' _ "$R8_ENTRIES" "$R8_LANE_BASE"
assert "R8c: the litter guard FAILS against the entry a REAL seed run produced, and names it (proves the stem matcher agrees with seed's actual <lane>.<pid> naming, not just a hand-built string)" \
    bash -c '[ "$1" -ne 0 ] || exit 1; case "$2" in *"$3"*) exit 0 ;; esac; exit 1' \
        _ "$R8_RC" "$R8_OUT" "$R8_LANE_BASE"

# ... and PASSES once the entry is gone, so it is a detector and not a constant
# failure. Removed from the run-private trash dir only.
rm -rf "${_SHARED_TRASH_DIR:?}"
assert "R8d: ... and PASSES once that entry is gone (a detector, not a constant failure)" \
    _assert_no_shared_trash_litter "$_SHARED_TRASH_DIR" "$R8_SNAP" "$R8_STEM"

# Restore the real shared-path target and clear this control's hits, matching
# R2's discipline, so nothing leaks past test_summary.
_SHARED_TRASH_DIR="$_SHARED_TRASH_DIR_SAVED_R8"
: > "$_TRASH_HITS_FILE"

# ─────────────────────────────────────────────────────────────────────────────
# Block W — rerere disarm at lane cadence (task 6889, open item (c))
#
# scripts/seed-warm-lane.sh delegates to scripts/git-rerere-guard.sh arm at the
# tail of every --fresh-checkout seed, so the shared .git/config is re-pinned at
# ACQUIRE cadence rather than only at developer-setup cadence. This block pins
# WHEN that call happens, and that it can never break an acquire.
#
# THE DISCRIMINATOR, established by experiment rather than assumed: the guard's
# first two git calls are `rev-parse --is-inside-work-tree` and `rev-parse
# --git-common-dir`, while seed's OWN git use is only `diff --name-only` and
# `rev-parse HEAD`. So `--is-inside-work-tree` in CALLS_FILE is a precise "the
# guard ran" probe that no existing seed call can forge. Deliberately NOT
# `git config`: under this suite's stub `git` the guard bails at
# git-rerere-guard.sh:197 (`cd: abc1234`) and never reaches a config call.
#
# BUT THE PROBE IS DISPATCH-INDEPENDENT — do NOT re-derive it as an `arm` probe.
# That misreading is what left W1-W8 coarser than they read, and it is worth
# spelling out: `--is-inside-work-tree` is emitted at git-rerere-guard.sh:182,
# BEFORE the subcommand dispatch at :951-953, so it fires identically for `arm`
# and for `check`, and identically whatever target was passed. On its own it
# proves only "some guard-shaped call happened somewhere". Two asserts close the
# two halves it cannot reach:
#   * W9 — the ARGUMENT half: was the guard pointed at THIS lane?
#   * W10 — the SUBCOMMAND half: was it `arm` and not `check`? CALLS_FILE
#     structurally cannot reach this, since the guard's only observable git
#     calls precede its dispatch; W10 uses a guard SHIM instead.
#
# THE GUARD IS DELIBERATELY NOT STUBBED OUT. It runs for real against the stub
# `git`, where it exits 1 (measured: empty stdout, one stderr line, exactly the
# two rev-parse calls above). That is what makes W3 a genuine hostile-environment
# test of the fail-open path rather than a vacuous one.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block W: rerere disarm at lane cadence (task 6889) ---"

W_BASE_PARENT="$(mktemp -d /tmp/test-seed-W-parent-XXXXXX)"
W_BASE="$W_BASE_PARENT/target"
_TMPDIRS+=("$W_BASE_PARENT")
mkdir -p "$W_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$W_BASE_PARENT/.warm-base-meta"

# W1 (a): INVOKED on the --fresh-checkout path — the production ACQUIRE mode.
W_LANE1="$(make_isolated_lane W-fresh)"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$W_BASE" "$W_LANE1" --fresh-checkout
W1_RC="$RC"
W1_OUT="$OUT"

assert "W1: --fresh-checkout invokes git-rerere-guard.sh (--is-inside-work-tree seen)" \
    bash -c 'grep "^git" "$1" | grep -q -- "--is-inside-work-tree"' _ "$CALLS_FILE"

# W9: ...AND IT WAS POINTED AT THIS LANE. W1 above is the cheap presence check;
# W9 refines it from "some guard-shaped call happened somewhere" to "the guard
# was pointed at THIS lane". Without it, dropping or mis-deriving the
# `"$LANE_DIR"` argument leaves every one of W1-W8 green.
#
# MEASURED shape: the guard resolves TARGET from its first positional
# (git-rerere-guard.sh:175) and its first call is `git -C "$TARGET" rev-parse
# --is-inside-work-tree`, so the recorded line is literally
#     git -C <LANE_DIR> rev-parse --is-inside-work-tree
# Match the FIELD after `-C`, not a substring of the line: a substring grep for
# the lane path would also be satisfied by a call that passed a CHILD of the
# lane, and a grep for a parent would be satisfied by the lane itself.
#
# Reuses W1's already-captured CALLS_FILE rather than adding a fixture, so this
# adds no runtime.
_guard_target_from_calls() {
    awk '/--is-inside-work-tree/ {
             for (i = 1; i <= NF; i++)
                 if ($i == "-C") { print $(i + 1); exit }
         }' "$1"
}

# Asserted as a FUNCTION, not `bash -c`: assert runs "$@" directly in THIS shell
# (test_helpers.sh's no-subshell idiom), whereas a `bash -c` child would not
# inherit _guard_target_from_calls and the check would fail for the wrong reason.
_guard_targeted_lane() {   # <expected_lane> <calls_file>
    [ "$(_guard_target_from_calls "$2")" = "$1" ]
}

assert "W9: ...and the guard was pointed at THIS lane (the -C field is \$W_LANE1)" \
    _guard_targeted_lane "$W_LANE1" "$CALLS_FILE"

# W2 (b): NOT invoked on --reset-in-place, which pins the mode gate. That gate
# covers every TASK-lane acquire (dark-factory drives those through
# _seed_warm_lane(lane, '--fresh-checkout')), but NOT the merge-spec lane —
# measured 2026-08-30, acquire_spec_lane always passes --reset-in-place. That is
# harmless here because the pin is a property of the ONE shared .git/config, not
# of a lane: any acquire that pins it pins it for every lane. See the mode-gate
# comment in scripts/seed-warm-lane.sh for the full measurement.
# Same fixture shape as Block E's --reset-in-place run.
W_LANE2="$(make_isolated_lane W-reset)"
mkdir -p "$W_LANE2/src"
echo 'fn main() {}' > "$W_LANE2/src/main.rs"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$W_BASE" "$W_LANE2" --reset-in-place

assert "W2: --reset-in-place exits 0 (fixture sanity)" \
    test "$RC" -eq 0
assert "W2: --reset-in-place does NOT invoke git-rerere-guard.sh (mode gate)" \
    bash -c '! grep "^git" "$1" | grep -q -- "--is-inside-work-tree"' _ "$CALLS_FILE"

# W3 (c): FAIL-OPEN. Under the stub `git` the guard exits 1 (measured above), so
# this assert has teeth: without the `|| _rc=$?` shielding, seed would inherit
# that status under `set -euo pipefail` and the acquire would fail.
assert "W3: seed still exits 0 even though the guard exits 1 under the stub git" \
    test "$W1_RC" -eq 0

# W4 (d): STDOUT UNPOLLUTED. seed's stdout is a single-use machine-readable
# channel; the guard's diagnostics must never reach it. Same shape as C6/E3.
assert "W4: STDOUT is exactly <lane_dir>/target with the guard call in place" \
    bash -c '[ "$1" = "'"$W_LANE1/target"'" ]' _ "$W1_OUT"

# ── W5-W8: the REIFY_WARM_LANE_RERERE_ARM operator escape hatch ──────────────
#
# This call now runs on EVERY acquire across 254 linked worktrees that share ONE
# .git/config, so an off-switch that needs no code change and no merge is prudent
# for that blast radius. The control must be strictly opt-IN-to-skip: the failure
# direction has to be "still protected", never "silently off". Same
# truncate-then-run_helper shape and the same --is-inside-work-tree discriminator
# as W1/W2.

# W5 (e): the exact value 0 suppresses ONLY the rerere call — the seed still
# succeeds and its stdout contract is untouched.
W_LANE3="$(make_isolated_lane W-optout)"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_RERERE_ARM=0 \
    run_helper "$W_BASE" "$W_LANE3" --fresh-checkout

assert "W5: REIFY_WARM_LANE_RERERE_ARM=0 suppresses the guard call" \
    bash -c '! grep "^git" "$1" | grep -q -- "--is-inside-work-tree"' _ "$CALLS_FILE"
assert "W5: ...and the seed still exits 0" \
    test "$RC" -eq 0
assert "W5: ...and STDOUT is still exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$W_LANE3/target"'" ]' _ "$OUT"

# W6 (f): UNSET keeps the defence armed — the default must be protected.
W_LANE4="$(make_isolated_lane W-unset)"
reset_calls
unset REIFY_WARM_LANE_RERERE_ARM
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$W_BASE" "$W_LANE4" --fresh-checkout

assert "W6: REIFY_WARM_LANE_RERERE_ARM unset still invokes the guard (default armed)" \
    bash -c 'grep "^git" "$1" | grep -q -- "--is-inside-work-tree"' _ "$CALLS_FILE"

# W7 (g): an explicit 1 keeps it armed too, so no stray value can silently
# disable the fleet-wide defence — only a literal 0 does.
W_LANE5="$(make_isolated_lane W-explicit-1)"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_RERERE_ARM=1 \
    run_helper "$W_BASE" "$W_LANE5" --fresh-checkout

assert "W7: REIFY_WARM_LANE_RERERE_ARM=1 invokes the guard (only a literal 0 skips)" \
    bash -c 'grep "^git" "$1" | grep -q -- "--is-inside-work-tree"' _ "$CALLS_FILE"

# W8 (h): the control surface is DISCOVERABLE — --help still exits 0 and names
# the variable, alongside the existing REIFY_WARM_LANE_RESEED_TRASH_SYNC /
# REIFY_WARM_LANE_MOUNT entries. An existence check on the documented control
# surface, not a prose pin. Usage goes to stderr (see A1).
reset_calls
run_helper --help
assert "W8: --help still exits 0" test "$RC" -eq 0
assert "W8: --help documents REIFY_WARM_LANE_RERERE_ARM" \
    bash -c 'printf "%s\n" "$1" | grep -q "REIFY_WARM_LANE_RERERE_ARM"' _ "$ERR_OUT"

# Do not let the knob leak into any later block of this long-lived suite.
unset REIFY_WARM_LANE_RERERE_ARM

# ── W10: the SUBCOMMAND is `arm`, not `check` ────────────────────────────────
#
# The half of the coverage gap CALLS_FILE structurally CANNOT reach: the guard's
# only observable git calls (`--is-inside-work-tree`, `--git-common-dir`) both
# precede its subcommand dispatch (:951-953), so no CALLS_FILE assert can tell
# `arm` from `check`. That distinction is the whole point of the block — a
# `check` would REPORT the drift while leaving all ~253 lanes ARMED.
#
# MECHANISM — a guard SHIM beside a fixture copy of the script. Sound here for a
# reason established by measurement, not assumed: `_SCRIPT_DIR` is derived once
# at scripts/seed-warm-lane.sh:171 from BASH_SOURCE[0], and `grep -n _SCRIPT_DIR`
# returns exactly three lines — that derivation plus its only two uses, the `-x`
# existence gate (:1666) and the invocation (:1668). seed sources no sibling lib
# at all, so a copy into a temp dir is faithful in every respect EXCEPT which
# guard it finds — precisely the substitution wanted. run_helper is UNCHANGED: it
# invokes `bash "$SCRIPT" "$@"`, so BASH_SOURCE[0] — and therefore _SCRIPT_DIR —
# resolves to the temp dir.
#
# DELIBERATELY ADDITIVE, not a replacement for W1-W9. The real guard running
# against the stub `git` and exiting 1 is what gives W3's fail-open assert its
# teeth; a shim that exits 0 would make W3 vacuous, reintroducing exactly the
# defect under remediation. So W1-W9 stay on the real guard and W10 stands
# alongside. Because the shim exits 0, W10 also exercises the previously
# UNTESTED `_rerere_arm_rc -eq 0` success branch — no other assert covers it,
# since the real guard never returns 0 under the stub git.
W10_DIR="$(mktemp -d /tmp/test-seed-W10-XXXXXX)"
_TMPDIRS+=("$W10_DIR")
cp "$SCRIPT" "$W10_DIR/seed-warm-lane.sh"
W10_ARGV="$W10_DIR/guard-argv"

# Self-contained shim: it writes BESIDE ITSELF, so the heredoc needs no
# expansion and cannot pick up a stale path from the enclosing shell.
cat > "$W10_DIR/git-rerere-guard.sh" <<'W10_SHIM'
#!/usr/bin/env bash
echo "$*" >> "$(dirname "${BASH_SOURCE[0]}")/guard-argv"
exit 0
W10_SHIM
# EXECUTABLE matters: :1666 gates on `-x`, so a non-executable shim silently
# takes the warn branch and every assert below would pass vacuously against a
# never-created file. Asserted, not merely chmod-ed.
chmod +x "$W10_DIR/git-rerere-guard.sh"

assert "W10: fixture — the shim is executable (a non-exec shim takes the warn branch)" \
    test -x "$W10_DIR/git-rerere-guard.sh"

W_LANE6="$(make_isolated_lane W-shim)"
W10_SCRIPT_SAVED="$SCRIPT"
SCRIPT="$W10_DIR/seed-warm-lane.sh"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$W_BASE" "$W_LANE6" --fresh-checkout
W10_RC="$RC"
W10_OUT="$OUT"
# RESTORED IMMEDIATELY, in the same sub-block: this suite is long-lived and every
# later block reads $SCRIPT, so a leaked override would silently retarget the
# remainder of a 440-assert run at a stale copy.
SCRIPT="$W10_SCRIPT_SAVED"

assert "W10: the SCRIPT global was restored (later blocks must not run the copy)" \
    bash -c '[ "$1" = "$2" ]' _ "$SCRIPT" "$REPO_ROOT/scripts/seed-warm-lane.sh"

# Exactly one line, so a future call that ALSO ran `check` cannot satisfy the
# `arm` assert by accident.
assert "W10: the guard was invoked exactly once" \
    bash -c '[ "$(wc -l < "$1")" -eq 1 ]' _ "$W10_ARGV"

assert "W10: the argv is EXACTLY 'arm <lane_dir>'" \
    bash -c '[ "$(cat "$1")" = "arm $2" ]' _ "$W10_ARGV" "$W_LANE6"

assert "W10: ...argv[1] is 'arm'" \
    bash -c '[ "$(cut -d" " -f1 "$1")" = arm ]' _ "$W10_ARGV"

assert "W10: ...and is explicitly NOT 'check' (which would leave the fleet armed)" \
    bash -c '[ "$(cut -d" " -f1 "$1")" != check ]' _ "$W10_ARGV"

# The exit-0 branch, reachable only under the shim: seed must still honour its
# C5/C6/E3/H1c/I3 single-use-stdout contract on the SUCCESS path too.
assert "W10: seed still exits 0 on the guard-success branch" \
    test "$W10_RC" -eq 0

assert "W10: ...and STDOUT is still exactly <lane_dir>/target on that branch" \
    bash -c '[ "$1" = "$2" ]' _ "$W10_OUT" "$W_LANE6/target"

test_summary
