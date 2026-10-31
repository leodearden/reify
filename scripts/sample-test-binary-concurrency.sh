#!/usr/bin/env bash
# Sample the HOST-WIDE peak number of concurrently-running Rust test binaries.
# Host-wide holds for a caller with ptrace read reach over the host's processes;
# a sandboxed caller sees only its own process tree, and the summary says which.
#
# Task 6018's ACCEPTANCE instrument (residual of task 5984 carry-over 1,
# esc-5984-2): 6018 un-narrowed the global nextest pool ([profile.default]
# test-threads: a fixed 16 -> the host CPU count) and its acceptance clause asks
# for an OBSERVED peak concurrency, not a computed one.
#
# Contract guard: tests/infra/test_test_binary_concurrency_sampler.sh.
#
# ---------------------------------------------------------------------------
# USAGE
#   scripts/sample-test-binary-concurrency.sh [--duration S] [--interval S]
#                                             [--deps-glob GLOB]
#
#   --duration S    total sampling window in seconds (default 900).  0 = take
#                   exactly one sample and exit.
#   --interval S    seconds between samples (default 1).  0 = no sleep.
#   --deps-glob G   space-separated shell glob(s) an exe path must match to
#                   count (default `*/target/*/deps/*`).
#
# Emits ONE summary line on stdout; all diagnostics go to stderr.  Exit 0 on an
# empty host — absence of test binaries is DATA, not an error: it is launched
# beside a verify without being coupled to it, so a window that starts early or
# outlives the run must not fail whatever launched it.
#
# The summary also carries own_uid_links= (candidates running as the caller's
# effective uid) and resolved_links= (how many of THOSE the caller could read),
# each summed over the window.  A stderr WARNING fires when resolved_links is
# under half of own_uid_links: the caller then lacks ptrace read reach, and the
# reading covers only its visible process tree.
#
# ENVIRONMENT — two injection seams; the contract guard drives both.
#   REIFY_SAMPLER_PROC_ROOT  the /proc to read (default /proc).
#   REIFY_SAMPLER_PIDS_CMD   OVERRIDE for candidate discovery.  Unset or empty
#                            — the default — enumerates <PROC_ROOT>/<pid>/exe.
#
# ---------------------------------------------------------------------------
# WHY THE ARGV MATCH ALONE IS WRONG (defect (a), measured).
#
# The obvious instrument is `pgrep -fc "<lane>/target/debug/deps/"`.  It
# OVERCOUNTS, because pgrep -f matches the process's ARGV and argv is not
# identity: a compiler invoked with a -o path under target/*/deps/ matches just
# as well as the test binary that path will become.  Reproduced live in the
# task-6018 worktree — `pgrep -f "target/.*/deps/"` returned pids whose
# /proc/<pid>/exe resolves to ~/.cargo/bin/sccache and to rustup's rustc.  The
# earlier 48-count snapshot in the task description is that artefact, not 48
# concurrent tests.
#
# The fix is to CONFIRM every candidate by readlink /proc/<pid>/exe and glob-match
# the RESULT.  /proc/<pid>/exe is the kernel's own answer to "what image is this
# process running", unforgeable from userspace and correct even for a binary that
# has since been deleted or copied.  Same idiom, same reason, as the orphaned
# test-binary reaper (scripts/lib_proc_reaper.sh:197-205); keeping one definition
# of "is this process a test binary" across the repo is deliberate.
#
# ---------------------------------------------------------------------------
# WHY THE PREFILTER HAD TO GO (defect (d), measured).
#
# That confirmation was originally paid for with ONE readlink FORK PER CANDIDATE,
# behind a `pgrep -f` argv prefilter.  Both halves leak the same way, and fixing
# only one leaves the defect intact:
#
#   1. PER-CANDIDATE CONFIRMATION.  Every fork widens the gap between "this pid
#      was listed" and "this pid's exe was read".  Test binaries live 0.2-0.8 s,
#      so candidates that were genuinely running when the sample began were
#      recorded as vanished by the time their turn came.
#   2. THE PREFILTER ITSELF.  `pgrep -f` walks argv across all of /proc, so the
#      list it returned had already decayed before confirmation started.
#      Batching the confirmation of a stale list still confirms a stale list,
#      however fast the batch is.
#
# MEASURED: the pre-fix instrument UNDERCOUNTS a batched whole-/proc snapshot by
# 35-65%, and the old affordability objection to such a scan (that it degrades
# from the intended 1 Hz to about 0.05 Hz under load) was an objection to ONE
# FORK PER PID, which one fork TOTAL removes.  Both A/B series, the method, and
# the per-pass costs live in
# docs/notes/nextest-global-pool-concurrency-observation.md -> "Prefilter->confirm
# race", which is their single owner; they are cited here, not restated, because
# a measurement copied into three files has already been seen to diverge over
# which copy is authoritative.
#
# ONE consequence belongs here, because it governs how this instrument's own
# output must be read: that note's window 1 peak=14 is a FLOOR, not a bound that
# held.  Discovery and confirmation are now ONE pass over ONE snapshot, which is
# what a race-free sample requires.
#
# ---------------------------------------------------------------------------
# WHY nonzero_samples EXISTS (defect (b), measured).
#
# peak alone is not falsifiable.  An earlier 15-minute window reported a maximum
# of 7 — below the then-configured cap of 16 — and that reads exactly like "the
# bound held".  It was not: compile and link occupy much of a verify pass and
# `test-threads` bounds the EXECUTION phase only, so the window had simply landed
# between execution phases.
#
# nonzero_samples is the count of samples in which at least one confirmed test
# binary was running.  It makes the distinction mechanical rather than a matter
# of the reader's judgement:
#
#   nonzero_samples == 0  ->  the window never observed the execution phase.
#                             The result is INCONCLUSIVE.  It is NOT evidence
#                             about the pool bound, and must never be reported
#                             as "peak stayed under N".  Re-run over a longer
#                             window, or one aligned to a known test phase.
#   nonzero_samples > 0   ->  peak is a real observation of that window.
#
# Report both, always, together.
#
# ---------------------------------------------------------------------------
# WHY THE PATTERN LIST IS SPLIT WITH GLOBBING OFF (defect (c), measured).
#
# DEPS_GLOB is a SPACE-SEPARATED LIST of patterns, so it has to be word-split —
# but its default, `*/target/*/deps/*`, is itself a live glob.  An unquoted
# `for glob in $DEPS_GLOB` therefore got pathname expansion as well, and
# whenever the sampler's cwd happened to contain a matching tree the loop
# variable bound to REAL RELATIVE PATHS instead of the pattern.  A relative path
# can never match an absolute /proc/<pid>/exe target, so the sampler silently
# counted zero.  Measured on one identical fixture: peak=1 from a lane root,
# peak=0 from /home/leo/src/warm-lanes/worktrees.
#
# This is a HOST-WIDE instrument that may be launched from any directory, so its
# result MUST be cwd-independent — and a cwd-induced zero is textually
# indistinguishable from the genuine defect-(b) INCONCLUSIVE window above, i.e.
# it corrupts the one reading this script exists to make trustworthy.
#
# The split is therefore done ONCE, at parse time, under `set -f`: that
# suppresses pathname expansion while leaving word-splitting intact, which is
# exactly the half we need and exactly the half we must keep.  `set +f` restores
# globbing immediately afterwards — that restore is load-bearing, not cosmetic,
# and now doubly so: the DEFAULT candidate discovery is itself a glob over
# <PROC_ROOT>/*/exe, and REIFY_SAMPLER_PIDS_CMD is a caller-supplied command that
# may legitimately rely on globbing too.  Splitting once rather than per-pid also
# removes a per-sample cost blowup, which matters against the per-pass figures
# recorded under defect (d) above.
#
# A whitespace-only --deps-glob is rejected for the same reason: it passes a
# naive non-empty check but splits to ZERO patterns, after which every sample
# counts 0 forever — the same silent-wrong-measurement class.
#
# ---------------------------------------------------------------------------
# WHY own_uid_links / resolved_links EXIST (defect (e), measured).
#
# A caller without ptrace read reach (e.g. one inside a Landlock domain) can
# confirm only its own process tree, so over a host busy running tests it
# reports peak=0 nonzero_samples=0: a THIRD silent zero, textually the
# defect-(b) INCONCLUSIVE window.  The two fields and their WARNING (USAGE
# above) make it visible.  The mechanism, the measurements and the reason the
# denominator is own-uid live in
# docs/notes/nextest-global-pool-concurrency-observation.md -> "Ptrace blindness
# (detected, task #7500)", their single owner.
# ---------------------------------------------------------------------------
set -euo pipefail

DURATION=900
INTERVAL=1
DEPS_GLOB='*/target/*/deps/*'

_die() { echo "sample-test-binary-concurrency.sh: $*" >&2; exit 64; }

while [ "$#" -gt 0 ]; do
    case "$1" in
        (--duration)   [ "$#" -ge 2 ] || _die "--duration needs a value"; DURATION="$2"; shift 2 ;;
        (--duration=*) DURATION="${1#*=}"; shift ;;
        (--interval)   [ "$#" -ge 2 ] || _die "--interval needs a value"; INTERVAL="$2"; shift 2 ;;
        (--interval=*) INTERVAL="${1#*=}"; shift ;;
        (--deps-glob)  [ "$#" -ge 2 ] || _die "--deps-glob needs a value"; DEPS_GLOB="$2"; shift 2 ;;
        (--deps-glob=*) DEPS_GLOB="${1#*=}"; shift ;;
        (-h|--help)
            sed -n '2,38p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        (*) _die "unknown argument '$1'" ;;
    esac
done

case "$DURATION" in (''|*[!0-9]*) _die "--duration must be a non-negative integer, got '$DURATION'" ;; esac
case "$INTERVAL" in (''|*[!0-9]*) _die "--interval must be a non-negative integer, got '$INTERVAL'" ;; esac
[ -n "$DEPS_GLOB" ] || _die "--deps-glob must be non-empty"

# Split the pattern LIST once, here, with pathname expansion disabled — see the
# header block on defect (c).  `set -f` kills globbing but NOT word-splitting.
set -f
# shellcheck disable=SC2206
DEPS_GLOBS=( $DEPS_GLOB )
set +f
# Restored above so a glob-dependent REIFY_SAMPLER_PIDS_CMD keeps working; this
# script uses no other glob, so returning to the default is safe.
[ "${#DEPS_GLOBS[@]}" -gt 0 ] || \
    _die "--deps-glob must contain at least one non-whitespace pattern, got '$DEPS_GLOB'"

# Preflight the two external tools the batched confirmation depends on, for the
# same reason the --deps-glob guards above exist: every failure of that one
# invocation collapses into an empty result, then `confirmed=0`, then a summary
# line byte-for-byte indistinguishable from a genuine INCONCLUSIVE window (a
# missing `xargs` yields peak=0 samples=1 nonzero_samples=0 and exit 0 —
# measured with a stub that exits 127).  Batching made that blast radius the
# WHOLE sample, where a per-candidate readlink lost one pid; `xargs` is also a
# dependency this script did not previously have.  Die loudly here instead: the
# missing-tool case is fully in this script's control, unlike the "present but
# every operand denied" case (a caller without ptrace read reach, defect (e)),
# which no preflight can prevent and which is instead REPORTED: own_uid_links /
# resolved_links on the summary line, plus the blindness WARNING.
for _tool in readlink xargs; do
    command -v "$_tool" >/dev/null 2>&1 || \
        _die "$_tool not found on PATH — required for the batched exe confirmation"
done
unset _tool

# Injection seams (testability; the contract guard drives both).  PIDS_CMD is an
# OVERRIDE: unset or empty — the default, and what a real run uses — means
# enumerate PROC_ROOT itself, with no prefilter (defect (d) above).
PROC_ROOT="${REIFY_SAMPLER_PROC_ROOT:-/proc}"
PIDS_CMD="${REIFY_SAMPLER_PIDS_CMD:-}"

_host_nproc() {
    local n=""
    if command -v nproc >/dev/null 2>&1; then n="$(nproc 2>/dev/null || true)"; fi
    if [ -z "$n" ] && command -v getconf >/dev/null 2>&1; then
        n="$(getconf _NPROCESSORS_ONLN 2>/dev/null || true)"
    fi
    case "${n:-}" in (''|*[!0-9]*) n="unknown" ;; esac
    printf '%s' "$n"
}
HOST_NPROC="$(_host_nproc)"

# _candidate_exe_links — the <PROC_ROOT>/<pid>/exe links to confirm, one per
# line.  Always succeeds: a candidate source that matches nothing is data, not
# an error.
#
# IT YIELDS LINK PATHS, NOT PIDS, because that is what the confirmation consumes.
# An earlier shape had the default branch strip each globbed path back to a bare
# pid only for the sampler to re-assemble it — a lossy round-trip that
# existed solely to make the production path speak the pid-shaped vocabulary of
# the test-only override seam, and that split the "one pass over one snapshot"
# property across two functions.
#
# DEFAULT: read PROC_ROOT directly.  A bash glob is readdir only — no fork, no
# argv walk — so discovery and the confirmation that follows it are one pass over
# one snapshot, and the glob already holds exactly the paths needed.  It takes
# numeric entries only: the others (self, thread-self, ...) alias this script's
# own helper processes, which are never test binaries and would inflate both
# reach-audit counts.
#
# OVERRIDE: REIFY_SAMPLER_PIDS_CMD still speaks PIDS — the natural thing for a
# caller to produce — so its lines are mapped to links here, and the digits-only
# filter lives here too, where the untrusted input actually is.  Both branches
# converge on the SAME batched confirmation, which is what keeps the contract
# guard pinning the code a real run takes rather than a test-only branch.
_candidate_exe_links() {
    if [ -n "$PIDS_CMD" ]; then
        local pids pid
        pids="$(eval "$PIDS_CMD" 2>/dev/null || true)"
        [ -n "$pids" ] || return 0
        while IFS= read -r pid; do
            case "${pid:-}" in (''|*[!0-9]*) continue ;; esac
            printf '%s\n' "$PROC_ROOT/$pid/exe"
        done <<EOF
$pids
EOF
        return 0
    fi
    local -a exes=()
    shopt -s nullglob
    exes=( "$PROC_ROOT"/[0-9]*/exe )
    shopt -u nullglob
    [ "${#exes[@]}" -gt 0 ] || return 0
    printf '%s\n' "${exes[@]}"
}

# _resolve_links <link>... — ONE batched readlink over every operand (see
# _sample): one line per resolvable operand, the unresolvable ones silently
# omitted.  Always succeeds.
_resolve_links() {
    printf '%s\0' "$@" | xargs -0 readlink 2>/dev/null || true
}

# _reach_audit <link>... — prints `<own_uid> <resolved>`: how many of the given
# <root>/<pid>/exe links belong to a process running as this caller's effective
# uid, and how many of THOSE the caller can resolve.  Both counts range over the
# same own-uid operands, so a caller that can also read other uids' links (root,
# CAP_SYS_PTRACE) cannot mask blindness among its own.
#
# Ownership tests the <root>/<pid> DIRECTORY, never the exe link: `[ -O` follows
# symlinks, and following /proc/<pid>/exe is ptrace-gated exactly like reading
# it, so it would go blind together with the confirmation it exists to audit.  A
# stat of the directory is not ptrace-gated.  Builtin test only, no fork per link.
#
# It runs AFTER the confirmation, with a readlink of its own, because the
# ownership stats cost about as much as the confirmation itself: run first, they
# would widen the discovery->confirmation gap that peak depends on (defect (d)).
_reach_audit() {
    local link exe r=0
    local -a own=()
    for link in "$@"; do
        if [ -O "${link%/exe}" ]; then own+=( "$link" ); fi
    done
    if [ "${#own[@]}" -gt 0 ]; then
        while IFS= read -r exe; do
            [ -n "$exe" ] || continue
            r=$((r + 1))
        done <<EOF
$(_resolve_links "${own[@]}")
EOF
    fi
    printf '%s %s' "${#own[@]}" "$r"
}

# _lacks_ptrace_reach <own_uid_links> <resolved_links> — true iff the caller
# resolved fewer than half of the exe links of processes running as its own uid.
# The one definition of "blind"; the summary WARNING is its only consumer.
_lacks_ptrace_reach() {
    local own="$1" resolved="$2"
    [ "$own" -gt 0 ] && [ $((resolved * 2)) -lt "$own" ]
}

# ---------------------------------------------------------------------------
# _sample — one sample.  ONE batched confirmation, never one per pid.  Prints
# ONE line of three integers: `<confirmed> <own_uid> <resolved>` — test binaries
# confirmed, then _reach_audit's two counts.
#
# Candidate discovery may legitimately yield nothing (a host with no test
# binaries), and any candidate may exit before it is confirmed, leaving no
# <root>/<pid> — both are normal, not errors, so every step is guarded and the
# function always succeeds with its three counts on stdout.  An EMPTY
# confirmation is not a reason to stop early: total blindness is exactly the
# case the reach audit must still report.
#
# The confirmation is a SINGLE multi-operand `readlink`: it prints one line per
# resolvable operand, silently omits the unresolvable ones, and exits 1 if any
# failed.  That omission IS the "a vanished pid is skipped, not fatal" contract
# (A5) — now satisfied structurally rather than by a per-pid guard — and the
# single invocation is what closes the prefilter->confirm race (defect (d), B1).
# `xargs -0` keeps it ARG_MAX-safe on a pathological host; on a realistic one
# (~1200 processes) the whole set is a single batch.  Both tools are preflighted
# at startup, so an empty result here means the operands were unresolvable — not
# that the confirmation never ran.
# ---------------------------------------------------------------------------
_sample() {
    local candidates link exe glob resolved n=0 matched
    local -a links=()
    candidates="$(_candidate_exe_links)"
    [ -n "$candidates" ] || { printf '0 0 0\n'; return 0; }
    while IFS= read -r link; do
        [ -n "$link" ] || continue
        links+=( "$link" )
    done <<EOF
$candidates
EOF
    [ "${#links[@]}" -gt 0 ] || { printf '0 0 0\n'; return 0; }
    resolved="$(_resolve_links "${links[@]}")"
    while IFS= read -r exe; do
        [ -n "$exe" ] || continue
        matched=0
        # Iterate the pre-split ARRAY, never the raw string (defect (c)).  The
        # `case` pattern stays UNQUOTED on purpose: case patterns undergo neither
        # pathname expansion nor field splitting, so $glob was never the bug
        # there — quoting it would turn pattern matching into literal comparison.
        for glob in "${DEPS_GLOBS[@]}"; do
            # shellcheck disable=SC2254
            case "$exe" in ($glob) matched=1; break ;; esac
        done
        [ "$matched" -eq 1 ] && n=$((n + 1))
    done <<EOF
$resolved
EOF
    printf '%s %s\n' "$n" "$(_reach_audit "${links[@]}")"
}

_started="$SECONDS"
peak=0
samples=0
nonzero=0
own_uid_total=0
resolved_total=0

# Sample FIRST, then test the deadline: --duration 0 must still yield exactly one
# observation rather than an empty window.
while :; do
    read -r c o r <<<"$(_sample)"
    samples=$((samples + 1))
    own_uid_total=$((own_uid_total + o))
    resolved_total=$((resolved_total + r))
    [ "$c" -gt "$peak" ] && peak="$c"
    [ "$c" -gt 0 ] && nonzero=$((nonzero + 1))
    echo "sample-test-binary-concurrency.sh: t=$((SECONDS - _started))s confirmed=$c peak=$peak" >&2
    [ $((SECONDS - _started)) -ge "$DURATION" ] && break
    [ "$INTERVAL" -gt 0 ] && sleep "$INTERVAL"
done

# ONE grep-able summary line, mirroring the `INFO: ... peak=<P> ...` grammar
# tests/infra/run_all.sh:1722 already uses for its worker-shell peak.
echo "INFO: nextest test-binary concurrency: peak=${peak} samples=${samples} nonzero_samples=${nonzero} interval=${INTERVAL}s duration=${DURATION}s deps_glob=${DEPS_GLOB} host_nproc=${HOST_NPROC} own_uid_links=${own_uid_total} resolved_links=${resolved_total}"

if [ "$nonzero" -eq 0 ]; then
    echo "sample-test-binary-concurrency.sh: WARNING — nonzero_samples=0: this window never observed a test-execution phase. The result is INCONCLUSIVE and is NOT evidence about the pool bound. Re-run over a longer window." >&2
fi
if _lacks_ptrace_reach "$own_uid_total" "$resolved_total"; then
    echo "sample-test-binary-concurrency.sh: WARNING — resolved_links=${resolved_total} of own_uid_links=${own_uid_total}: this caller could not read /proc/<pid>/exe for most processes running as its own uid, i.e. it lacks ptrace read reach (a sandboxed caller, e.g. under Landlock, sees only its own process tree). peak and nonzero_samples cover only the visible processes: a zero is NOT evidence of an empty host, and a non-zero peak is a FLOOR. Re-run from an unsandboxed shell, or launch the workload as a descendant of this sampler's caller." >&2
fi
exit 0
