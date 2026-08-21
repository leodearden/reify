#!/usr/bin/env bash
# tests/infra/govtest_slice_reaper_lib.sh — lifecycle for the private per-run
# systemd slices created by the infra suites that place real cgroup work:
#   * tests/infra/test_cpu_load_governance.sh   — `reify-govtest` (task 5930)
#   * tests/infra/test_cpu_governed_exec_hostexcl.sh — `reify-test` (task 6386)
# and by tests/infra/test_govtest_slice_reaper.sh, which is this library's test.
#
# Functions:
#   govtest_profile_set <prefix> <child_suffix>...
#                                  select the name grammar this library
#                                  operates on. VALIDATES both arguments; a
#                                  refused call leaves the previous profile
#                                  intact. The DEFAULT `reify-govtest agents
#                                  merge` is applied at source time, so a
#                                  consumer that never calls this is unaffected
#   govtest_slice_pid <unit>       echo the embedded pid, or nothing if <unit>
#                                  is outside the configured name grammar
#   govtest_slice_name <pid> [child_suffix]
#                                  echo ONE unit name — the bare parent, or the
#                                  named child
#   govtest_slice_units <pid>      echo this run's unit names, one per line, in
#                                  TEARDOWN order (children, parent)
#   govtest_stale_units <self_pid> <listing>
#                                  filter a `systemctl --user list-units`
#                                  listing down to one PARENT unit name per
#                                  dead predecessor run
#   govtest_reap_stale [<self_pid>] enumerate + stop every dead predecessor's
#                                  slice; no-op when systemctl is absent
#   govtest_legacy_stale <listing> <legacy_unit>...
#                                  filter a listing down to the PIDLESS
#                                  dash-nesting parents that are safe to stop
#                                  right now (present, and with no enumerated
#                                  dash-child left to cascade into)
#   govtest_reap_legacy <legacy_unit>...
#                                  enumerate + stop those, re-checking each
#                                  against a FRESH enumeration immediately
#                                  before its own stop; no-op when systemctl
#                                  is absent
#   govtest_slice_teardown <pid>   unconditionally stop this run's own slices,
#                                  children before parent
#
# Knobs:
#   REIFY_GOVTEST_TEST_MODE             set to 1 to ARM the test seams below.
#                                       Nothing else in the repo sets it, so
#                                       the seams are inert on every real run.
#   REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS  space-separated pid list that replaces
#                                       the `kill -0` liveness oracle (test
#                                       seam; mirrors the REIFY_CPU_GOV_TEST_*
#                                       idiom in test_cpu_load_governance.sh).
#                                       Honoured ONLY when
#                                       REIFY_GOVTEST_TEST_MODE=1 — see
#                                       _govtest_pid_alive for why a liveness
#                                       seam needs a second key.
#
# WHY THIS LIVES IN tests/infra/ AND NOT scripts/lib_cgroup.sh
# Both prefixes this library serves are test-private BY CONSTRUCTION: each
# consuming suite requires its slice names to differ from the production
# `reify-governed-{agents,merge}.slice` so its measurements stay isolated from
# concurrent real agent placement (ζ) and so it can never disturb the live
# orchestrator's governance hierarchy. Meanwhile scripts/lib_cgroup.sh is
# sourced by scripts/cpu-governed-exec.sh on EVERY governed exec, so teaching
# that production hot-path library about test-only slice prefixes would be a
# layering inversion — production code would carry knowledge of, and a stop
# path for, units only tests create.
# tests/infra/*_lib.sh is the established house pattern for logic shared
# between infra tests (cpu_load_fixture.sh, load_tolerance_lib.sh,
# nextest_absent_lib.sh, run-all-classification-lib.sh). It is also not a
# `test_*.sh`, so run_all.sh's auto-discovery skips it and it needs no
# run-all-classification.manifest row.
#
# WHY THE PREFIX IS A PROFILE AND NOT A CONSTANT (task 6386)
# test_cpu_governed_exec_hostexcl.sh leaked its own five per-run units in the
# same two ways task 5930 closed here for `reify-govtest`. Duplicating this
# file to serve it would be a lockstep copy of an already-reviewed SAFETY
# mechanism — the worst thing to fork, because the two copies drift silently
# and only one of them gets the next fix. So the two literals became a
# profile. What used to be guaranteed by the literal `reify-govtest` being
# spelled in the source is now guaranteed by govtest_profile_set's charset
# validation instead: see its header for why that validation is the load-
# bearing safety boundary rather than input hygiene.

# Source guard — prevent double-sourcing (mirrors lib_portable.sh /
# lib_cgroup.sh / lib_test_semaphore.sh).
if [ "${_REIFY_GOVTEST_SLICE_REAPER_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_GOVTEST_SLICE_REAPER_LIB_SOURCED=1

# ---------------------------------------------------------------------------
# THE PROFILE — the prefix and child-suffix set every function below derives
# from. Set through govtest_profile_set; never assigned directly by a consumer.
#
#   _GOVTEST_PROFILE_PREFIX      e.g. `reify-test`
#   _GOVTEST_PROFILE_SUFFIXES    ordered, space-delimited, e.g. `agents merge`
#   _GOVTEST_PROFILE_SUFFIX_ALT  derived regex alternation, e.g. `-agents|-merge`
#                                (EMPTY when the profile declares no children)
# ---------------------------------------------------------------------------
_GOVTEST_PROFILE_PREFIX=""
_GOVTEST_PROFILE_SUFFIXES=""
_GOVTEST_PROFILE_SUFFIX_ALT=""

# ---------------------------------------------------------------------------
# govtest_profile_set <prefix> <child_suffix>...
#   Select the name grammar this library operates on. Returns 0 on success;
#   on refusal returns non-zero, writes a message to stderr, and leaves the
#   PREVIOUS profile completely intact.
#
#   THIS VALIDATION IS THE SAFETY BOUNDARY, NOT INPUT HYGIENE. <prefix> is
#   interpolated UNQUOTED into govtest_slice_pid's `[[ =~ ]]` pattern — it has
#   to be, since a quoted right-hand side is matched literally and the grammar
#   would never fire at all. That makes the prefix live regex source, and
#   govtest_slice_pid is the single chokepoint deciding whether a unit is
#   eligible to be STOPPED. A prefix of `reify-.*` would therefore make
#   `reify.slice` a match — and that is the shared implicit ROOT of the LIVE
#   production hierarchy (`reify-governed.slice` and
#   `reify-governed-agents.slice` nest under it and carry real orchestrator
#   agent placement), so stopping it cascades into the running fleet. The
#   conservative charset below — lowercase alphanumerics in dash-separated
#   segments, nothing else — is what makes every such widening unreachable,
#   for metacharacters nobody has thought of as much as for the ones above.
#
#   WHY EACH CHILD SUFFIX MUST BE DASH-FREE. systemd dash-nesting means
#   `a-b-c.slice` implies parents `a.slice` and `a-b.slice`, vivified
#   automatically and named by nobody. A suffix of `d7-task` would therefore
#   emit `<prefix><pid>-d7-task.slice` and silently create
#   `<prefix><pid>-d7.slice` — a unit no name list mentions, no teardown
#   stops, and no sweep can recognise. That is precisely the leak class task
#   6386 exists to close (it is how the pidless `reify-test-task.slice` /
#   `reify-test-merge.slice` parents accreted on the host in the first place),
#   so refusing the dash here is structural rather than stylistic.
#
#   The dot and the empty suffix are refused for the same round-trip reason:
#   every name govtest_slice_units emits must be re-recognised by
#   govtest_slice_pid, or teardown would stop units the startup sweep could
#   never identify as its own residue.
#
#   ALL-OR-NOTHING. Validation completes before anything is assigned, so a
#   half-applied profile — a new prefix paired with the old suffixes, a
#   grammar nobody reviewed — is not reachable.
# ---------------------------------------------------------------------------
govtest_profile_set() {
    local prefix="${1:-}"
    shift 2>/dev/null || true
    local suffix alt="" suffixes=""

    if [[ ! "$prefix" =~ ^[a-z][a-z0-9]*(-[a-z0-9]+)*$ ]]; then
        echo "govtest_profile_set: refusing prefix '$prefix' — must match ^[a-z][a-z0-9]*(-[a-z0-9]+)*\$ (it is interpolated unquoted into the unit-name regex)" >&2
        return 1
    fi

    for suffix in "$@"; do
        if [[ ! "$suffix" =~ ^[a-z0-9]+$ ]]; then
            echo "govtest_profile_set: refusing child suffix '$suffix' — must match ^[a-z0-9]+\$; a dash would vivify an unnamed implicit parent slice" >&2
            return 1
        fi
        case " $suffixes " in
            *" $suffix "*)
                echo "govtest_profile_set: refusing duplicate child suffix '$suffix'" >&2
                return 1
                ;;
        esac
        suffixes="${suffixes:+$suffixes }$suffix"
        alt="${alt:+$alt|}-$suffix"
    done

    _GOVTEST_PROFILE_PREFIX="$prefix"
    _GOVTEST_PROFILE_SUFFIXES="$suffixes"
    _GOVTEST_PROFILE_SUFFIX_ALT="$alt"
    return 0
}

# The DEFAULT profile, applied at source time and BELOW the source guard (so a
# double-source cannot reset a consumer's chosen profile). It reproduces the
# exact grammar this library shipped with as a pair of literals in task 5930,
# which is what keeps test_cpu_load_governance.sh — a consumer that never calls
# the setter — byte-for-byte unaffected by the parameterisation.
govtest_profile_set reify-govtest agents merge

# ---------------------------------------------------------------------------
# govtest_slice_pid <unit>
#   Echo the pid embedded in a slice unit name, or NOTHING when the name is
#   outside the CONFIGURED grammar
#
#       ^<prefix>([0-9]+)(<-suffix|-suffix...>)?\.slice$
#
#   e.g. `^reify-govtest([0-9]+)(-agents|-merge)?\.slice$` under the default
#   profile. The pid is BASH_REMATCH[1] whether or not the optional child
#   alternation participates in the match, and stays there however many
#   suffixes the profile declares — that group is the only one after it.
#
#   EMPTINESS IS THE ONLY SIGNAL — this function always returns 0. Callers
#   run under `set -euo pipefail`, and a non-zero return from inside a
#   `pid="$(govtest_slice_pid "$u")"` capture is an abort hazard at every
#   call site for no benefit; testing the captured string is equivalent and
#   cannot misfire.
#
#   The grammar is deliberately EXACT rather than a prefix test. It is the
#   single chokepoint deciding whether a unit is eligible to be stopped, and
#   the per-user systemd session it operates in is shared host-wide with the
#   production `reify-governed-{agents,merge}.slice` units that carry real
#   agent placement. An exact anchored match is what makes those units
#   unreachable from here by construction. This is the same defensive
#   re-filter discipline dark-factory's leftover-scope reaper applies at
#   verify.py:3503 — it re-checks the full name with an anchored regex even
#   though it already enumerated by prefix glob, so a surprise in glob
#   semantics can never widen the blast radius.
# ---------------------------------------------------------------------------
govtest_slice_pid() {
    local unit="${1:-}" re

    # Built as a string and matched UNQUOTED: a quoted right-hand side is
    # matched literally by `[[ =~ ]]`, so quoting here would silently disable
    # the grammar entirely. govtest_profile_set's charset validation is what
    # makes interpolating into live regex source safe — see its header.
    if [ -n "$_GOVTEST_PROFILE_SUFFIX_ALT" ]; then
        re="^${_GOVTEST_PROFILE_PREFIX}([0-9]+)(${_GOVTEST_PROFILE_SUFFIX_ALT})?\\.slice$"
    else
        # A profile with no declared children. Spelled as its own branch
        # rather than letting the alternation collapse to `()?`, which is
        # undefined in POSIX ERE.
        re="^${_GOVTEST_PROFILE_PREFIX}([0-9]+)\\.slice$"
    fi

    if [[ "$unit" =~ $re ]]; then
        printf '%s\n' "${BASH_REMATCH[1]}"
    fi
    return 0
}

# ---------------------------------------------------------------------------
# govtest_slice_name <pid> [child_suffix]
#   Echo exactly ONE unit name: the bare parent `<prefix><pid>.slice`, or the
#   child `<prefix><pid>-<child_suffix>.slice`.
#
#   Consumers need the names INDIVIDUALLY as well as as a list —
#   test_cpu_governed_exec_hostexcl.sh feeds each of its five into a separate
#   REIFY_CPU_GOVERN_SLICE_* override — and cannot parse them back out of the
#   newline-separated teardown list govtest_slice_units emits. This is that
#   accessor, so the prefix is still spelled in exactly one place.
# ---------------------------------------------------------------------------
govtest_slice_name() {
    local pid="${1:-}" suffix="${2:-}"
    if [ -n "$suffix" ]; then
        printf '%s%s-%s.slice\n' "$_GOVTEST_PROFILE_PREFIX" "$pid" "$suffix"
    else
        printf '%s%s.slice\n' "$_GOVTEST_PROFILE_PREFIX" "$pid"
    fi
    return 0
}

# ---------------------------------------------------------------------------
# govtest_slice_units <pid>
#   Echo the unit names a single run owns, one per line: every child declared
#   by the profile IN DECLARED ORDER, then the bare parent.
#
#   THE ORDER IS TEARDOWN ORDER — children first, parent last. It preserves
#   the ordering rationale already carried by test_cpu_load_governance.sh's
#   _cleanup_all, which stops the confined-quota parent LAST, after its
#   children, to avoid leaving a quota'd empty parent unit behind.
#
#   DECLARED ORDER is part of the contract too, not an artefact of the loop: a
#   consumer naming its own slices through govtest_slice_name and its teardown
#   through this function must not be able to disagree with itself about which
#   suffix is which.
#
#   The names are fully determined by the pid, which is why teardown needs no
#   record of what was actually created (see govtest_slice_teardown).
# ---------------------------------------------------------------------------
govtest_slice_units() {
    local pid="${1:-}" suffix
    for suffix in ${_GOVTEST_PROFILE_SUFFIXES}; do
        govtest_slice_name "$pid" "$suffix"
    done
    govtest_slice_name "$pid"
    return 0
}

# ---------------------------------------------------------------------------
# _govtest_pid_alive <pid>
#   Internal. Return 0 if <pid> is a live process, non-zero otherwise.
#
#   When REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS is set and non-empty AND
#   REIFY_GOVTEST_TEST_MODE=1, it REPLACES the real oracle with a
#   word-membership test against that list. The seam exists solely so the
#   reaper's liveness logic is testable without real host pids: a hermetic
#   test cannot make a chosen pid dead on demand, and picking a "surely dead"
#   fixture pid is exactly the kind of host-dependence that turns a pool
#   member into a flake. This is the same environment-fixture idiom
#   test_cpu_load_governance.sh already uses about ten times over
#   (REIFY_CPU_GOV_TEST_PROC_PATH, REIFY_CPU_GOV_TEST_CONFINE_CPUS,
#   REIFY_CPU_ADMIT_MEM_PROC_PATH, ...) and lib_cgroup.sh uses for
#   REIFY_CPU_GOVERN_CONTROLLERS_PATH.
#
#   WHY THIS SEAM NEEDS A SECOND KEY, UNLIKE THOSE. Every seam listed above
#   redirects a READ to a fixture; the worst a stray one can do is make a
#   test measure the wrong file. This one replaces the oracle that decides
#   what gets STOPPED, and it does so on the PRODUCTION path —
#   govtest_reap_stale runs at the top of every real test_cpu_load_governance
#   .sh run. A stray non-empty pid list would make every govtest pid outside
#   it read DEAD, which does not merely weaken the fail-safe direction
#   documented on govtest_stale_units, it INVERTS it: the sweep would stop a
#   live concurrent lane's parent slice mid-measurement. Requiring
#   REIFY_GOVTEST_TEST_MODE=1 as well means the fake oracle can only engage
#   under a marker no injection source sets and no production path passes,
#   so on a real run `kill -0` is the only oracle reachable — whatever else
#   happens to be exported.
# ---------------------------------------------------------------------------
_govtest_pid_alive() {
    local pid="${1:-}"
    [ -n "$pid" ] || return 1
    if [ "${REIFY_GOVTEST_TEST_MODE:-0}" = "1" ] \
        && [ -n "${REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS:-}" ]; then
        local fake
        for fake in ${REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS}; do
            [ "$fake" = "$pid" ] && return 0
        done
        return 1
    fi
    kill -0 "$pid" 2>/dev/null
}

# ---------------------------------------------------------------------------
# govtest_stale_units <self_pid> <listing>
#   Echo one PARENT unit name — `<prefix><pid>.slice` — per dead
#   predecessor run found in <listing>, which is raw output of
#   `systemctl --user list-units --all --plain --no-legend` (unit name in
#   field 1). Emission is in first-seen order; always returns 0.
#
#   ONE LINE PER RUN, NOT PER UNIT. Measured directly rather than inferred
#   from systemd docs: a throwaway child slice was created under a parent,
#   the PARENT ALONE was stopped, and BOTH units then vanished from
#   `systemctl --user list-units --all`. Stopping the parent cascades, so a
#   leaked run's three unit names must collapse to one action — which is why
#   deduplication by pid is part of this function's contract rather than an
#   optimisation, and why there is no ordering hazard from stopping a parent
#   whose children are still listed.
#
#   FAIL-SAFE IN EXACTLY ONE DIRECTION. A pid is skipped when it is alive,
#   when it is the caller's own, or when the unit name does not parse — and
#   the name check runs through govtest_slice_pid, so the production
#   `reify-governed-*` slices and any foreign unit the enumeration glob might
#   surprise us with are dropped here too. The only error this design can
#   make is a false NEGATIVE (pid reuse: an unrelated process now holds a
#   dead run's pid), which merely leaves one empty slice behind for the next
#   sweep to retry. It can never reap a live concurrent run — which matters
#   because run_all.sh schedules many lanes at once against ONE shared
#   per-user systemd session.
#
#   Dedup uses a plain space-delimited seen-list rather than an associative
#   array: the candidate count is bounded by the number of leaked runs (a
#   handful), and this keeps the library free of a bash-4 dependency.
# ---------------------------------------------------------------------------
govtest_stale_units() {
    local self_pid="${1:-}" listing="${2:-}"
    local line unit pid seen=" " emitted

    while IFS= read -r line; do
        # Field 1 is the unit name; `read` also drops blank/whitespace-only
        # rows here, since unit ends up empty for them.
        read -r unit _ <<EOF2
$line
EOF2
        [ -n "$unit" ] || continue

        pid="$(govtest_slice_pid "$unit")"
        [ -n "$pid" ] || continue                 # not a govtest unit
        [ "$pid" = "$self_pid" ] && continue      # never our own run
        _govtest_pid_alive "$pid" && continue     # never a live run

        case "$seen" in
            *" $pid "*) continue ;;               # already emitted this run
        esac
        seen="$seen$pid "

        emitted="$(govtest_slice_name "$pid")"
        printf '%s\n' "$emitted"
    done <<EOF
$listing
EOF

    return 0
}

# ---------------------------------------------------------------------------
# govtest_legacy_stale <listing> <legacy_unit>...
#   Echo the subset of <legacy_unit>... that is safe to stop right now, in
#   ARGUMENT order, deduplicated. <listing> is raw output of
#   `systemctl --user list-units --all --plain --no-legend` (unit name in
#   field 1). Always returns 0 — emptiness is the only signal, matching
#   govtest_slice_pid's contract, because callers run under `set -euo pipefail`
#   and a non-zero return from inside a capture is an abort hazard.
#
#   WHAT THESE ARE. systemd dash-nesting means `a-b-c.slice` implies parents
#   `a.slice` and `a-b.slice`, vivified automatically and named by nothing.
#   test_cpu_governed_exec_hostexcl.sh's pre-rename `reify-test-task-<pid>
#   .slice` therefore accreted `reify-test.slice` and `reify-test-task.slice`
#   on the host — units no teardown stopped and no pid-keyed sweep could
#   recognise. They carry NO PID, so govtest_stale_units' liveness oracle has
#   nothing to consult and they sit correctly outside the pid grammar. This is
#   the separate, explicitly-listed path they need, and its safety argument has
#   to be rebuilt from scratch: "the pid is dead" is unavailable, and what
#   replaces it is EMPTINESS read off a fresh enumeration.
#
#   THREE FILTERS, IN ORDER.
#     1. The arg must match `^<prefix>(-[a-z0-9]+)*\.slice$`. This is what
#        makes `reify.slice` — the shared implicit ROOT of the LIVE production
#        hierarchy, under which reify-governed.slice and
#        reify-governed-agents.slice carry real orchestrator agent placement —
#        structurally unreachable, along with the production slices themselves,
#        the other profile's namespace, and any path-traversal string. It also
#        excludes everything inside the PID grammar, and provably so rather
#        than incidentally: after the prefix that grammar requires a DIGIT
#        while this one requires `-` or `.`, so the two languages are disjoint.
#        Pid units belong to govtest_reap_stale, which consults the liveness
#        oracle first; admitting one here would stop it on emptiness alone.
#     2. The arg must appear as field 1 of some row in the FRESH listing. A
#        name nobody enumerated is not residue.
#     3. No other enumerated row may begin with the arg's basename plus a dash.
#
#   WHY (3) IS "BLOCKED BY ANY DASH-CHILD" AND NOT DEEPEST-FIRST-IN-ONE-PASS.
#   Stopping a slice CASCADES to its children (measured in task 5930), so a
#   parent with a live descendant must not be stopped. The alternative — sort
#   candidates deepest-first and stop them in that order within a single pass —
#   would have to justify each parent stop by a child stop that ALREADY
#   HAPPENED, and every stop on this path is fail-soft (`|| true`), so a
#   silently-failed child stop would leave the parent looking clear and cascade
#   into it anyway. Reasoning from a fresh enumeration instead means each stop
#   is justified by what systemd currently reports, with no such window.
#
#   WHY TWO-PASS CONVERGENCE IS ACCEPTABLE. The consequence of (3) is that the
#   first post-rename run stops the two childless leaves and skips their root,
#   and the NEXT run — whose enumeration then holds only the root — stops that.
#   Two passes to zero. The consuming suite is host-exclusive on the hot path
#   of every run_all.sh, so that is two verify runs; and after the rename
#   nothing in the repo produces these names again, so the sweep goes silent
#   and stays silent. This is the same fail-safe DIRECTION govtest_stale_units
#   already documents for pid reuse: the only error reachable is a false
#   NEGATIVE, which costs one more sweep. A false positive would stop
#   something live.
#
#   THE MEASURED PARENTAGE FACT THAT MAKES (3) CONVERGE AT ALL (host,
#   2026-08-21). `reify-test<pid>.slice` parents to `reify.slice`, NOT to
#   `reify-test.slice` — `reify-test1234` is ONE dash segment, not
#   `reify-test` + `1234` — so a concurrent lane's own units are not
#   dash-children of the legacy root and cannot block it. Were that not so,
#   any concurrent run would block the sweep forever and it would never
#   converge. Conversely a PRE-rename lane's `reify-test-task-<pid>.slice`
#   genuinely IS a dash-child of `reify-test-task.slice` and correctly does
#   block it, which is exactly the cascade this rule exists to prevent.
#
#   CALLER CONTRACT: pass PIDLESS parent names only. Filter (1) admits any
#   dash-segmented name under the prefix, including a pid-bearing pre-rename
#   name like `reify-test-task-1234.slice`, and emptiness does not prove that
#   unit's owning run is dead. The one caller hardcodes its own three pidless
#   names, which is the right place for that knowledge — the naming history
#   belongs to the suite that made it, not to this library.
#
#   Dedup uses the same plain space-delimited seen-list idiom as
#   govtest_stale_units, and field 1 is parsed with the same `read -r unit _`
#   heredoc, so blank and whitespace-only rows drop out identically.
# ---------------------------------------------------------------------------
govtest_legacy_stale() {
    local listing="${1:-}"
    shift 2>/dev/null || true
    local line unit units=" " arg base other seen=" " re blocked

    re="^${_GOVTEST_PROFILE_PREFIX}(-[a-z0-9]+)*\\.slice$"

    # Normalise the listing to a deduplicated, space-delimited set of unit
    # names ONCE. Unit names contain no spaces, so the membership and
    # dash-child tests below are plain `case` patterns and this function stays
    # builtin-only like the rest of the library.
    while IFS= read -r line; do
        read -r unit _ <<EOF2
$line
EOF2
        [ -n "$unit" ] || continue
        case "$units" in
            *" $unit "*) continue ;;
        esac
        units="$units$unit "
    done <<EOF
$listing
EOF

    for arg in "$@"; do
        # (1) inside this profile's legacy namespace — see header
        [[ "$arg" =~ $re ]] || continue
        # (2) present in the fresh enumeration
        case "$units" in
            *" $arg "*) ;;
            *) continue ;;
        esac
        case "$seen" in
            *" $arg "*) continue ;;
        esac

        # (3) no enumerated dash-child, or stopping this would cascade into it
        base="${arg%.slice}"
        blocked=0
        for other in $units; do
            case "$other" in
                "$base-"*) blocked=1; break ;;
            esac
        done
        [ "$blocked" -eq 0 ] || continue

        seen="$seen$arg "
        printf '%s\n' "$arg"
    done

    return 0
}

# ---------------------------------------------------------------------------
# _govtest_enumerate
#   Internal. Echo a fresh `systemctl --user list-units` listing of this
#   profile's slices, or NOTHING when systemctl is absent or fails. Always
#   returns 0 — emptiness is the only signal, the same contract every other
#   function here follows, because callers run under `set -euo pipefail` and a
#   non-zero return from inside a capture is an abort hazard.
#
#   ONE ENUMERATION, THREE CALLERS. Both actuators and govtest_reap_legacy's
#   pre-stop re-check need exactly this, and the plumbing is subtle enough in
#   three separate ways that having it written once is worth a function:
#
#     * THE GLOB IS THE OUTER BLAST-RADIUS BOUND. The systemd user session is
#       shared host-wide, so listing every unit and filtering afterwards would
#       put the production `reify-governed-*` slices — and every other
#       project's units — inside the candidate set in the first place. The
#       anchored grammar re-filters each caller applies afterwards are the
#       INNER bound, kept even though the glob already ran: exactly the
#       belt-and-braces of dark-factory verify.py:3492 (glob) + :3503
#       (anchored regex re-check).
#     * `|| true` INSIDE the capture AND `|| listing=""` OUTSIDE it. The
#       former covers a non-zero systemctl exit; the latter covers the
#       assignment itself failing under `set -e`.
#     * THE `command -v` GUARD. A host without systemctl has nothing to
#       enumerate, and returning empty here gives every caller its no-op for
#       free rather than each repeating the guard.
#
#   The STOP RULES stay apart in their own actuators deliberately — see
#   govtest_reap_legacy's header for why folding two differently-justified
#   stop rules into one body would be the wrong deduplication. This is the
#   plumbing they share, not the policy they don't: a future fix here (a
#   `--no-pager`, a changed glob, a systemctl exit-code quirk) now lands in
#   one place instead of needing to be applied twice and silently being
#   applied once.
# ---------------------------------------------------------------------------
_govtest_enumerate() {
    local listing=""

    command -v systemctl >/dev/null 2>&1 || return 0

    listing="$(systemctl --user list-units --all --plain --no-legend \
        "${_GOVTEST_PROFILE_PREFIX}*.slice" 2>/dev/null || true)" || listing=""

    [ -n "$listing" ] || return 0
    printf '%s\n' "$listing"
    return 0
}

# ---------------------------------------------------------------------------
# govtest_reap_stale [<self_pid>]
#   CRASH RECOVERY. Enumerate this profile's slices, and stop the parent of
#   every run whose pid is dead. Defaults <self_pid> to `$$`.
#   Always returns 0.
#
#   WHY A STARTUP SWEEP IS NEEDED AT ALL. A consuming suite tears its own
#   slices down from an EXIT trap, and that trap is more robust than
#   it looks — measured on this host, a bash script blocked in a foreground
#   command runs its EXIT trap on SIGTERM, SIGINT and SIGHUP alike. Only
#   SIGKILL skips it. So widening the trap to `EXIT INT TERM HUP` would
#   change nothing; the residue this sweep exists to clean is precisely the
#   uncatchable one — a verify timeout, a harness reap, or an OOM kill.
#   Recovering at the next start is the same shape dark-factory's
#   leftover-verify-scope reaper uses (harness.py `_run_leftover_verify_scope
#   _reaper_pass` -> verify.py `reap_leftover_verify_scopes`).
#
#   TWO BOUNDS ON THE BLAST RADIUS, BOTH DELIBERATE. _govtest_enumerate's
#   `<prefix>*.slice` glob is the OUTER bound (see its header for why the
#   shared host-wide session makes that essential); the anchored grammar
#   re-filter inside govtest_slice_pid is the INNER bound, applied even
#   though the glob already ran. Keeping both is exactly the belt-and-braces
#   of dark-factory verify.py:3492 (glob) + :3503 (anchored regex re-check),
#   and it means a surprise in systemctl's glob semantics cannot widen what
#   gets stopped.
#
#   FAIL-SOFT THROUGHOUT. A missing systemctl and a failing one both arrive
#   as an empty listing from _govtest_enumerate and return immediately; a
#   failing stop is swallowed. This function is called at the top of a suite
#   running under `set -euo pipefail`, so any escaping non-zero status would
#   abort the whole governance run before a single row executed and surface
#   as a governance regression that isn't one.
#
#   Each reap is announced on stdout rather than done silently, so a sweep is
#   visible in the governance suite's transcript — the same log-what-you-
#   reaped discipline the dark-factory sweep follows.
# ---------------------------------------------------------------------------
govtest_reap_stale() {
    local self_pid="${1:-$$}"
    local listing unit

    # Absent systemctl, a failing one and an empty session all arrive here as
    # an empty listing, which is this function's no-op — see
    # _govtest_enumerate for the glob bound and the two-layer `|| true`.
    listing="$(_govtest_enumerate)" || listing=""

    [ -n "$listing" ] || return 0

    # `</dev/null` on the stop is STRUCTURAL, not cosmetic: this loop's stdin
    # IS the heredoc carrying the remaining units, so a callee that read from
    # stdin would swallow them and silently reap only the first of several
    # leaked runs. systemctl does not read stdin today; detaching it means a
    # future one — or a shim/wrapper placed on PATH — cannot turn this into a
    # truncated sweep. Same reason on govtest_slice_teardown's loop.
    while IFS= read -r unit; do
        [ -n "$unit" ] || continue
        systemctl --user stop "$unit" </dev/null 2>/dev/null || true
        echo "  reaped stale govtest slice: $unit"
    done <<EOF
$(govtest_stale_units "$self_pid" "$listing")
EOF

    return 0
}

# ---------------------------------------------------------------------------
# govtest_reap_legacy <legacy_unit>...
#   LEGACY-RESIDUE RECOVERY. Enumerate this profile's slices and stop whichever
#   of the caller-named PIDLESS dash-nesting parents govtest_legacy_stale says
#   is safe right now. Always returns 0.
#
#   WHY A SEPARATE ACTUATOR RATHER THAN A BRANCH INSIDE govtest_reap_stale.
#   That function's contract — enumerate, consult the liveness oracle, stop one
#   parent per DEAD run — is pinned by seven Block D assertions and is the more
#   dangerous of the two paths. Folding a second, differently-justified stop
#   rule into it would put both blast-radius arguments in one body where a
#   reviewer has to disentangle them, and would make every existing assertion
#   about "what reap_stale stops" conditional on a new argument. The only cost
#   of keeping them apart is one extra `systemctl list-units` per run, which is
#   negligible against a suite that places real cgroup scopes.
#
#   Structurally a direct mirror of govtest_reap_stale, deliberately: the same
#   _govtest_enumerate call for the listing, the same early return when it
#   comes back empty, the same fail-soft stop, the same announce-what-you-
#   reaped line, the same unconditional `return 0`. The two STOP RULES are
#   what stay apart; the enumeration plumbing they share is factored into
#   _govtest_enumerate so a future fix to it cannot land in only one of them.
#
#   THE PRE-STOP RE-CHECK, AND THE RACE IT CLOSES. govtest_legacy_stale decides
#   "no dash-child left to cascade into" from a SNAPSHOT. Between that
#   enumeration and the stop, a concurrent lane still running the PRE-rename
#   script can create `<prefix>-task-<pid>.slice` — and stopping
#   `<prefix>-task.slice` then cascades straight into that lane's live
#   measurement, which is the single hazard filter (3) exists to prevent. This
#   is not hypothetical: when the rename landed, dozens of lanes still had the
#   old file checked out. So each unit is re-validated against a FRESH
#   enumeration immediately before its own stop, shrinking the window from the
#   whole emit loop to systemd's own stop path. It cannot be closed entirely
#   from here — only systemd could stop-if-empty atomically — but the residual
#   window is orders of magnitude smaller and the failure mode of the re-check
#   itself is the safe one: a transiently empty or failing enumeration makes
#   the re-check emit nothing, so the unit is SKIPPED and left for the next
#   sweep. The skip is announced, not silent, for the same reason the reap is.
#
#   WHY RE-RUN THE WHOLE FILTER RATHER THAN JUST THE CHILD TEST. Passing the
#   single unit back through govtest_legacy_stale re-applies all three filters
#   — prefix, presence and childlessness — against the fresh listing, so a unit
#   that VANISHED between the two enumerations is skipped too (there is nothing
#   left to stop), and the prefix re-check that keeps `reify.slice` unreachable
#   is applied on the fresh data as well as the stale. One rule, evaluated
#   twice, rather than a second hand-written approximation of it that could
#   drift from the first.
#
#   THE `</dev/null` IS STRUCTURAL, not cosmetic — same reason as on the two
#   loops above. This loop's stdin IS the heredoc carrying the remaining units,
#   so a callee that read from stdin would swallow them and silently truncate
#   the sweep to its first unit. systemctl does not read stdin today; detaching
#   it means a future one, or a shim placed on PATH, cannot change that.
#
#   THE CALLER SUPPLIES THE NAMES, not this library. A legacy name is a fact
#   about one suite's own naming HISTORY — which slices its previous shape
#   accreted — and belongs in the file that made them, not in generic
#   machinery every profile shares. It also keeps this function's blast radius
#   a function of its arguments, re-filtered against the configured prefix by
#   govtest_legacy_stale before anything is stopped.
# ---------------------------------------------------------------------------
govtest_reap_legacy() {
    local listing unit fresh

    # No legacy names to look for means nothing to enumerate FOR — skip the
    # systemctl round-trip rather than listing and then filtering to empty.
    [ "$#" -gt 0 ] || return 0

    # Absent systemctl, a failing one and an empty session all arrive here as
    # an empty listing, which is this function's no-op.
    listing="$(_govtest_enumerate)" || listing=""

    [ -n "$listing" ] || return 0

    while IFS= read -r unit; do
        [ -n "$unit" ] || continue

        # RE-CHECK against a fresh enumeration, immediately before this unit's
        # own stop — see the header. Skipping is the safe direction, so an
        # enumeration that comes back empty for any reason skips too.
        fresh="$(_govtest_enumerate)" || fresh=""
        if [ -z "$(govtest_legacy_stale "$fresh" "$unit")" ]; then
            echo "  skipped legacy slice (no longer safe to stop): $unit"
            continue
        fi

        systemctl --user stop "$unit" </dev/null 2>/dev/null || true
        echo "  reaped legacy slice: $unit"
    done <<EOF
$(govtest_legacy_stale "$listing" "$@")
EOF

    return 0
}

# ---------------------------------------------------------------------------
# govtest_slice_teardown <pid>
#   Stop this run's own slices — children first, parent last —
#   UNCONDITIONALLY. Always returns 0. Safe to call from an EXIT trap.
#
#   WHY UNCONDITIONAL RATHER THAN FLAG-GUARDED. The mechanism this replaces
#   recorded what it had created in `_ROW4_SLICE_TASK_CREATED` /
#   `_ROW4_SLICE_MERGE_CREATED` / `_ROW4_CONFINE_PARENT_CREATED` and stopped
#   only what those named. That is a drift-prone design: it re-derives at 8
#   scattered assignment sites something `$$` already determines before any
#   test cycle runs. And it had drifted — `_row4_confine_apply_quota` vivifies
#   the parent slice from FOUR call sites but only TWO set the parent flag,
#   and those two sit behind branches that additionally require `taskset` and
#   a readable Cpus_allowed_list. On a host with cgroup governance but no
#   `taskset`, the two unguarded sites still created the parent and nothing
#   ever stopped it — a leak on a fully GREEN exit, distinct from the SIGKILL
#   leak govtest_reap_stale handles.
#
#   Unconditional teardown closes that whole class rather than the one
#   instance: the names are fully determined by the pid, and
#   `systemctl --user stop` on a never-started slice is a harmless no-op
#   already swallowed by the `|| true` this function keeps. So there is
#   nothing left to remember, and no future call site can forget to.
#
#   Stopping the PARENT alone would in fact suffice — cascade was measured
#   (see govtest_stale_units). The children are stopped first purely as
#   belt-and-braces for a host where cascade semantics differ, and the
#   children-then-parent order preserves the rationale the previous
#   _cleanup_all already carried: never leave a quota'd empty parent behind.
#
#   Returns 0 even when systemctl is missing or failing. This runs inside an
#   EXIT trap, where a non-zero return would overwrite the governance suite's
#   real exit status and report a passing run as failed.
# ---------------------------------------------------------------------------
govtest_slice_teardown() {
    local pid="${1:-$$}"
    local unit

    command -v systemctl >/dev/null 2>&1 || return 0

    # `</dev/null` — see govtest_reap_stale: the loop's stdin is the heredoc
    # listing the units still to stop, so a stdin-consuming callee would strand
    # the parent slice, which is the one that actually matters.
    while IFS= read -r unit; do
        [ -n "$unit" ] || continue
        systemctl --user stop "$unit" </dev/null 2>/dev/null || true
    done <<EOF
$(govtest_slice_units "$pid")
EOF

    return 0
}
