#!/usr/bin/env bash
# Sample the HOST-WIDE peak number of concurrently-running Rust test binaries.
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
# ENVIRONMENT — two injection seams; the contract guard drives both.
#   REIFY_SAMPLER_PROC_ROOT  the /proc to read (default /proc).
#   REIFY_SAMPLER_PIDS_CMD   OVERRIDE for candidate discovery.  Unset or empty
#                            — the default — enumerates <PROC_ROOT>/*/exe.
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
#   2. THE PREFILTER ITSELF.  `pgrep -f` walks argv across all of /proc and cost
#      0.15-0.24 s here (2.6 s per pass on the loaded host that produced window
#      1), so the list it returned had already decayed before confirmation
#      started.  Batching the confirmation of a stale list still confirms a
#      stale list, however fast the batch is.
#
# MEASURED.  The pre-fix sampler against a batched whole-/proc snapshot on this
# host (1163 processes, loadavg 94), five rounds alternating within the same
# second:
#
#       batched snapshot    24  30  27  30  26
#       pre-fix sampler     14  18  16  17   9
#
# A 35-65% UNDERCOUNT.  Window 1's peak=14 in
# docs/notes/nextest-global-pool-concurrency-observation.md must therefore be
# read as a FLOOR, not as a bound that held.
#
# THE OLD AFFORDABILITY OBJECTION IS SUPERSEDED, also measured.  It held that a
# whole-/proc scan degraded from the intended 1 Hz to about 0.05 Hz at loadavg
# 78 — but that was an objection to ONE FORK PER PID, and one fork TOTAL removes
# it.  On this host at loadavg 94 with 1163 processes a batched whole-/proc pass
# costs 0.04/0.04/0.22 s, against 0.15-0.24 s for the pgrep prefilter ALONE with
# a fork per candidate still to come on top.  Discovery and confirmation are now
# ONE pass over ONE snapshot, which is what a race-free sample requires.
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
            sed -n '2,30p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
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

# _candidate_pids — the pids to confirm, one per line.  Always succeeds: a
# candidate source that matches nothing is data, not an error.
#
# DEFAULT: read PROC_ROOT directly.  A bash glob is readdir only — no fork, no
# argv walk — so discovery and the confirmation that follows it are one pass over
# one snapshot.  Non-numeric entries (self, thread-self, net, ...) come through
# harmlessly: the digits-only filter in _confirmed_count drops them, and their
# exe targets could not match a */target/*/deps/* pattern anyway.
#
# OVERRIDE: REIFY_SAMPLER_PIDS_CMD, evaluated exactly as before.  Both branches
# feed the SAME batched confirmation, which is what keeps the contract guard
# pinning the code a real run takes rather than a test-only branch.
_candidate_pids() {
    if [ -n "$PIDS_CMD" ]; then
        eval "$PIDS_CMD" 2>/dev/null || true
        return 0
    fi
    local link
    local -a exes=()
    shopt -s nullglob
    exes=( "$PROC_ROOT"/*/exe )
    shopt -u nullglob
    for link in "${exes[@]}"; do
        link="${link%/exe}"
        printf '%s\n' "${link##*/}"
    done
}

# ---------------------------------------------------------------------------
# _confirmed_count — one sample.  ONE batched confirmation, never one per pid.
#
# Candidate discovery may legitimately yield nothing (a host with no test
# binaries), and any candidate may exit before it is confirmed, leaving no
# <root>/<pid> — both are normal, not errors, so every step is guarded and the
# function always succeeds with a count on stdout.
#
# The confirmation is a SINGLE multi-operand `readlink`: it prints one line per
# resolvable operand, silently omits the unresolvable ones, and exits 1 if any
# failed.  That omission IS the "a vanished pid is skipped, not fatal" contract
# (A5) — now satisfied structurally rather than by a per-pid guard — and the
# single invocation is what closes the prefilter->confirm race (defect (d), B1).
# `xargs -0` keeps it ARG_MAX-safe on a pathological host; on a realistic one
# (~1200 processes) the whole set is a single batch.
# ---------------------------------------------------------------------------
_confirmed_count() {
    local pids pid exe glob resolved n=0 matched
    local -a links=()
    pids="$(_candidate_pids)"
    [ -n "$pids" ] || { printf '%s' 0; return 0; }
    while IFS= read -r pid; do
        case "${pid:-}" in (''|*[!0-9]*) continue ;; esac
        links+=( "$PROC_ROOT/$pid/exe" )
    done <<EOF
$pids
EOF
    [ "${#links[@]}" -gt 0 ] || { printf '%s' 0; return 0; }
    resolved="$(printf '%s\0' "${links[@]}" | xargs -0 readlink 2>/dev/null || true)"
    [ -n "$resolved" ] || { printf '%s' 0; return 0; }
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
    printf '%s' "$n"
}

_started="$SECONDS"
peak=0
samples=0
nonzero=0

# Sample FIRST, then test the deadline: --duration 0 must still yield exactly one
# observation rather than an empty window.
while :; do
    c="$(_confirmed_count)"
    samples=$((samples + 1))
    [ "$c" -gt "$peak" ] && peak="$c"
    [ "$c" -gt 0 ] && nonzero=$((nonzero + 1))
    echo "sample-test-binary-concurrency.sh: t=$((SECONDS - _started))s confirmed=$c peak=$peak" >&2
    [ $((SECONDS - _started)) -ge "$DURATION" ] && break
    [ "$INTERVAL" -gt 0 ] && sleep "$INTERVAL"
done

# ONE grep-able summary line, mirroring the `INFO: ... peak=<P> ...` grammar
# tests/infra/run_all.sh:1722 already uses for its worker-shell peak.
echo "INFO: nextest test-binary concurrency: peak=${peak} samples=${samples} nonzero_samples=${nonzero} interval=${INTERVAL}s duration=${DURATION}s deps_glob=${DEPS_GLOB} host_nproc=${HOST_NPROC}"

if [ "$nonzero" -eq 0 ]; then
    echo "sample-test-binary-concurrency.sh: WARNING — nonzero_samples=0: this window never observed a test-execution phase. The result is INCONCLUSIVE and is NOT evidence about the pool bound. Re-run over a longer window." >&2
fi
exit 0
