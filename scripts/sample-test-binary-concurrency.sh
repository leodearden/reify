#!/usr/bin/env bash
# Sample the HOST-WIDE peak number of concurrently-running Rust test binaries.
#
# Task 6018's ACCEPTANCE instrument (the residual deliverable of task 5984
# carry-over 1, esc-5984-2).  Task 6018 un-narrowed the global nextest pool —
# `[profile.default] test-threads` went from a fixed 16 to the host CPU count —
# and the acceptance clause asks for an OBSERVED peak concurrency, not a computed
# one.  This is that observation's instrument.
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
# empty host — absence of test binaries is DATA, not an error (this is meant to
# be launched beside a verify without being coupled to it, so a window that
# starts early or outlives the run must not fail whatever launched it).
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
# WHY THE PREFILTER IS STILL THERE.  Confirmation is exact but readlink-per-pid
# over all of /proc is what made the earlier attempt unaffordable: at loadavg 78
# a whole-/proc scan degraded from the intended 1 Hz to about 0.05 Hz, so the
# sampler was blind for 20 s at a time — precisely the phase-aliasing that
# produced defect (b).  The argv prefilter cuts the confirmation set to a handful
# of pids: measured 0.28 s per pass in this worktree at its then-current load,
# which makes 1 Hz affordable.  Prefilter for SPEED, confirm for TRUTH.
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

# Injection seams (testability; the contract guard drives both).  Defaults are
# the real host: /proc, and the argv prefilter described above.
PROC_ROOT="${REIFY_SAMPLER_PROC_ROOT:-/proc}"
PIDS_CMD="${REIFY_SAMPLER_PIDS_CMD:-pgrep -f 'target/.*/deps/'}"

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

# ---------------------------------------------------------------------------
# _confirmed_count — one sample.  Prefilter for speed, confirm for truth.
#
# The prefilter is allowed to fail (pgrep exits 1 when nothing matches; a
# candidate may also exit between the two steps, leaving no <root>/<pid>) — both
# are normal, not errors, so every step is guarded and the function always
# succeeds with a count on stdout.
# ---------------------------------------------------------------------------
_confirmed_count() {
    local pids pid exe glob n=0 matched
    pids="$(eval "$PIDS_CMD" 2>/dev/null || true)"
    [ -n "$pids" ] || { printf '%s' 0; return 0; }
    while IFS= read -r pid; do
        case "${pid:-}" in (''|*[!0-9]*) continue ;; esac
        # Race: the process may have exited since the prefilter listed it, in
        # which case its whole directory is gone and readlink yields nothing.
        # Permission denied (a pid we cannot inspect) lands here identically.
        exe="$(readlink "$PROC_ROOT/$pid/exe" 2>/dev/null || echo "")"
        [ -n "$exe" ] || continue
        matched=0
        for glob in $DEPS_GLOB; do
            # shellcheck disable=SC2254
            case "$exe" in ($glob) matched=1; break ;; esac
        done
        [ "$matched" -eq 1 ] && n=$((n + 1))
    done <<EOF
$pids
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
