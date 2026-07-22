#!/usr/bin/env bash
# scripts/warm-lane-gc-sweep.sh — Periodic GC backstop for the warm-lane CoW pool.
#
# Invokes warm-lane-gc.sh reclaim --mount <worktree_base> unconditionally on each
# run (no disk-guard gate — the point is to reclaim FREE divergent lanes proactively
# to PREVENT monotonic accretion BETWEEN acquires, before the disk floor is reached).
#
# Driven by deploy/systemd/reify-warm-lane-gc.timer (periodic oneshot).
# The on-demand dark-factory ε path (disk-guard check → reclaim → requeue on low disk)
# is separate; this wrapper is the BACKGROUND sweep that keeps the pool lean.
#
# Fail-open: if the warm-lane mount (worktree_base / --mount) does not exist,
# warn on stderr and exit 0 — the timer must degrade quietly on a cold host or
# before the loopback mount is provisioned (inv.6 cold-fallback philosophy).
#
# Stale-trash reaper (task 5326): on every run this sweep also reaps stranded
# $MOUNT/.reseed-trash/<lane>.<pid> dirs — trash that seed-warm-lane.sh's
# rename-then-background-rm leaves behind when SIGKILL'd mid-reseed. An entry is
# removed only when its encoded PID is dead AND no live process still references
# it (cwd / open fd / mmap). This lives in the periodic δ sweep, not gc.sh's
# per-acquire ε reclaim, because the live-reference /proc scan is too costly for
# the hot path. See _reap_stale_trash below.
#
# Part of PRD docs/prds/warm-lane-pool-space-safety.md §12 (δ GC trigger backstop).
#
# Usage:
#   scripts/warm-lane-gc-sweep.sh [--mount DIR] [--gc-script PATH]
#       [--df CMD] [--critical-free-gib N]
#
# Options:
#   --mount DIR        Warm-lane WORKTREES base dir — the value dark-factory
#                      passes as str(self.worktree_base) to warm-lane scripts
#                      (git_ops.py _run_warm_lane_gc_reclaim line 1401); NOT
#                      the XFS mount point.  Forwarded verbatim to gc.sh --mount.
#                      Default: /home/leo/src/warm-lanes/worktrees or
#                      $REIFY_WARM_LANE_GC_SWEEP_MOUNT.
#   --gc-script PATH   Path to warm-lane-gc.sh (default: sibling warm-lane-gc.sh;
#                      overridable for hermetic tests via $REIFY_WARM_LANE_GC_SWEEP_GC_SCRIPT).
#   --df CMD           df command override (default: df; env:
#                      $REIFY_WARM_LANE_GC_SWEEP_DF). Hermetic-test seam only.
#   --critical-free-gib N
#                      Emergency low-water floor in GiB (default: 2; env:
#                      $REIFY_WARM_LANE_GC_SWEEP_CRITICAL_FREE_GIB). Measured
#                      once against --mount right before invoking gc.sh; at or
#                      below this floor, --disk-pressure is appended to the
#                      reclaim invocation, engaging the rm -rf <lane>/target
#                      fast-path reset (task 5167) instead of the α
#                      reflink-reseed clone, which stages a rename-to-trash
#                      (frees 0 bytes) then a reflink clone whose metadata
#                      write a near-full volume denies — the reset=N/removed=0
#                      deadlock of the 2026-07-10 outage. Deliberately well
#                      below the γ disk-guard's 50 GiB admission floor: that
#                      floor denies NEW work, this floor stops STAGING a
#                      space-consuming reseed once free is critically low.
#                      A df failure or unparseable measurement falls back to
#                      plain reclaim (fail-to-α: --disk-pressure engages ONLY
#                      on a positive low-water measurement).
#   -h, --help         Print this message and exit 0.
#
# Exit codes:
#   0  — Sweep attempted or skipped (fail-open).
#   1  — Runtime error propagated from gc.sh.
#   2  — Usage error: unknown flag or non-integer --critical-free-gib.
#
# Env knobs:
#   REIFY_WARM_LANE_GC_SWEEP_MOUNT               — default --mount
#   REIFY_WARM_LANE_GC_SWEEP_GC_SCRIPT           — default --gc-script
#   REIFY_WARM_LANE_GC_SWEEP_DF                  — default --df
#   REIFY_WARM_LANE_GC_SWEEP_CRITICAL_FREE_GIB   — default --critical-free-gib

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── log helpers (all write to stderr) ──────────────────────────────────────────
_warn() { printf '[warm-lane-gc-sweep] WARN:  %s\n' "$*" >&2; }
_info() { printf '[warm-lane-gc-sweep] INFO:  %s\n' "$*" >&2; }

# ── usage ───────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [--mount DIR] [--gc-script PATH] [--df CMD] [--critical-free-gib N]

  Periodic GC backstop for the warm-lane CoW pool.
  Invokes warm-lane-gc.sh reclaim --mount <DIR>, appending --disk-pressure
  when available space on --mount is at or below the emergency floor.
  Fail-open: if --mount DIR does not exist, warns and exits 0.

  Options:
    --mount DIR        Warm-lane WORKTREES base dir — NOT the XFS mount point
                       (default: /home/leo/src/warm-lanes/worktrees).
    --gc-script PATH   Path to warm-lane-gc.sh (default: sibling warm-lane-gc.sh).
    --df CMD           df command override (default: df). Hermetic-test seam.
    --critical-free-gib N
                       Emergency low-water floor in GiB (default: 2). At or
                       below this floor, --disk-pressure is appended to the
                       reclaim invocation (rm-outright reset instead of the
                       α reflink-reseed clone). A df failure or unparseable
                       measurement falls back to plain reclaim (fail-to-α).
    -h, --help         Print this message and exit 0.

  Exit codes:
    0  — Sweep attempted or skipped (fail-open on missing mount).
    1  — Runtime error from gc.sh.
    2  — Usage error: unknown flag or non-integer --critical-free-gib.
EOF
}

# ── stale .reseed-trash reaper (task 5326) ────────────────────────────────────
# seed-warm-lane.sh --fresh-checkout renames a non-empty target/ to
# $MOUNT/.reseed-trash/<basename LANE_DIR>.<pid> then background-rm's it. A
# SIGKILL between the rename and the rm strands that trash; the seed's own
# orphan-sweep only recovers it on the NEXT acquire of THAT lane, so a lane
# never re-acquired strands it forever (observed 224 GB on 2026-07-21). This
# reaper generalizes the seed's single-lane, next-acquire-only sweep into a
# pool-wide dead-PID sweep over the whole .reseed-trash/ dir.
#
# It lives in this periodic δ backstop (systemd timer + hand-run), NOT in
# warm-lane-gc.sh's per-acquire ε reclaim hot path, because the live-reference
# guard is an O(processes × fds) /proc scan too costly for the hot path.
#
# An entry is reaped ONLY when its encoded PID is dead AND no live process still
# references the dir via cwd / open fd / mmap — defending against PID reuse and
# against removing trash an in-flight rm or seed still touches. No per-lane
# flock: trash is PID-keyed crash-orphan state, the guard covers live
# references, and rm -rf is idempotent versus seed-warm-lane.sh's own per-lane
# orphan-sweep. Synchronous rm; fail-open (a failed rm warns and never aborts
# the sweep).

# _trash_referenced_by_live_proc PATH — return 0 if any live process references
# PATH (or a descendant) via its cwd symlink, an open fd symlink, or a mmap'd
# region in /proc/<pid>/maps; return 1 otherwise.
#
# Single-pass scans (one find, one grep) — NOT a readlink fork per fd, which
# cost ~84s on a ~1300-process host. The verdict is read from captured OUTPUT,
# never a pipeline exit status: find/grep over /proc return non-zero on the
# unavoidable permission-denied entries of other users' processes (and SIGPIPE
# on an early awk exit), so under `set -o pipefail` the exit status is an
# unreliable signal. A vanishing /proc entry only produces suppressed stderr;
# every substitution is `|| true`-guarded so it never aborts under set -e.
_trash_referenced_by_live_proc() {
    local path="$1"
    local rp hit
    rp="$(readlink -f "$path" 2>/dev/null)" || rp=""
    [ -n "$rp" ] || return 1

    # cwd + open-fd references: one find pass prints every process's cwd and
    # open-fd symlink TARGETS (%l = the kernel-canonical target — no per-fd
    # fork); awk matches rp exactly or as a "$rp/" prefix (string compare, so
    # arbitrary path characters are safe). /proc/<pid>/task is pruned to skip
    # the per-thread symlink duplicates.
    hit="$(find /proc -maxdepth 3 -path '*/task' -prune -o -type l \
            \( -name cwd -o -path '*/fd/*' \) -printf '%l\n' 2>/dev/null \
        | awk -v rp="$rp" '$0 == rp || index($0, rp "/") == 1 { print "HIT"; exit }')" || true
    if [ -n "$hit" ]; then return 0; fi

    # mmap'd regions: a single grep over every process's maps for a file mapped
    # from inside the trash dir.
    hit="$(grep -lF "$rp" /proc/[0-9]*/maps 2>/dev/null | head -n1)" || true
    if [ -n "$hit" ]; then return 0; fi

    return 1
}

# _reap_stale_trash MOUNT — reap stranded MOUNT/.reseed-trash/<lane>.<pid> dirs.
_reap_stale_trash() {
    local mount="$1"
    local trash_dir="$mount/.reseed-trash"
    [ -d "$trash_dir" ] || return 0
    local entry name pid
    for entry in "$trash_dir"/*; do
        [ -e "$entry" ] || continue          # glob-nomatch guard (empty dir)
        name="${entry##*/}"
        pid="${name##*.}"
        # Ambiguous / malformed suffix (no numeric PID) → never reap.
        case "$pid" in
            ''|*[!0-9]*) continue ;;
        esac
        # Encoded PID still alive → a seed/rm may still be in flight; preserve.
        if kill -0 "$pid" 2>/dev/null; then continue; fi
        # A live process still references the dir (cwd/fd/mmap) → preserve
        # (PID-reuse or in-flight rm/seed guard).
        if _trash_referenced_by_live_proc "$entry"; then continue; fi
        # Dead PID, no live reference → reap synchronously; fail-open on error.
        if rm -rf "$entry"; then
            _info "reaped stale reseed-trash entry: $entry (encoded PID $pid dead, no live reference)"
        else
            _warn "failed to reap stale reseed-trash entry: $entry (rm -rf failed); leaving intact"
        fi
    done
    return 0
}

# ── defaults ───────────────────────────────────────────────────────────────────
MOUNT="${REIFY_WARM_LANE_GC_SWEEP_MOUNT:-/home/leo/src/warm-lanes/worktrees}"
GC_SCRIPT="${REIFY_WARM_LANE_GC_SWEEP_GC_SCRIPT:-$SCRIPT_DIR/warm-lane-gc.sh}"
DF="${REIFY_WARM_LANE_GC_SWEEP_DF:-df}"
CRITICAL_FREE_GIB="${REIFY_WARM_LANE_GC_SWEEP_CRITICAL_FREE_GIB:-2}"

# ── arg parsing ────────────────────────────────────────────────────────────────
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --mount)
            [ $# -ge 2 ] || { _warn "--mount requires a value"; exit 2; }
            MOUNT="$2"; shift 2 ;;
        --gc-script)
            [ $# -ge 2 ] || { _warn "--gc-script requires a value"; exit 2; }
            GC_SCRIPT="$2"; shift 2 ;;
        --df)
            [ $# -ge 2 ] || { _warn "--df requires a value"; exit 2; }
            DF="$2"; shift 2 ;;
        --critical-free-gib)
            [ $# -ge 2 ] || { _warn "--critical-free-gib requires a value"; exit 2; }
            CRITICAL_FREE_GIB="$2"; shift 2 ;;
        -*)
            _warn "Unknown flag: $1"
            _warn "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
        *)
            _warn "Unexpected argument: $1"
            _warn "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# Validate --critical-free-gib is a non-negative integer (misconfiguration is
# a wiring bug, not transient pressure — exit 2 so it's loud rather than
# silently comparing against a bogus floor). Mirrors warm-lane-disk-guard.sh's
# MIN_FREE_GIB validation (scripts/warm-lane-disk-guard.sh:130-138).
if ! printf '%s\n' "$CRITICAL_FREE_GIB" | grep -qE '^[0-9]+$'; then
    _warn "REIFY_WARM_LANE_GC_SWEEP_CRITICAL_FREE_GIB (or --critical-free-gib) is not a valid integer: '$CRITICAL_FREE_GIB'."
    _warn "Run '$(basename "$0") --help' for usage."
    exit 2
fi

# ── fail-open: skip if mount dir does not exist ────────────────────────────────
if [ ! -d "$MOUNT" ]; then
    _warn "warm-lane mount dir does not exist: $MOUNT — skipping GC sweep (fail-open)"
    exit 0
fi

# ── reap stranded .reseed-trash/<lane>.<pid> dirs (task 5326) ─────────────────
# Runs on every sweep (independent of --disk-pressure): a periodic backstop for
# trash stranded by a SIGKILL between seed-warm-lane.sh's rename-to-trash and
# its background rm. Fail-open — never aborts the sweep.
_reap_stale_trash "$MOUNT"

# ── build reclaim args; measure disk pressure once, right before exec ─────────
# γ disk-guard df idiom (scripts/warm-lane-disk-guard.sh): a single snapshot,
# captured without tripping set -e. Fail-to-α: a df failure or unparseable
# measurement leaves args as plain reclaim — --disk-pressure engages ONLY on
# a positive low-water measurement, so a benign df misconfiguration in
# steady state never silently cold-ifies every reclaimable lane on every
# periodic sweep.
args=(reclaim --mount "$MOUNT")

df_out="$("$DF" -B1 --output=avail -- "$MOUNT")" || {
    _warn "df measurement failed (DF=$DF MOUNT=$MOUNT); proceeding without disk-pressure"
    df_out=""
}

if [ -n "$df_out" ]; then
    avail_bytes="$(printf '%s\n' "$df_out" | awk 'NR==2{print $1}')"
    if printf '%s\n' "$avail_bytes" | grep -qE '^[0-9]+$'; then
        critical_bytes=$(( CRITICAL_FREE_GIB * 1024 * 1024 * 1024 ))
        if [ "$avail_bytes" -le "$critical_bytes" ]; then
            _info "EMERGENCY low-water: avail=${avail_bytes} bytes <= floor=${critical_bytes} bytes (${CRITICAL_FREE_GIB} GiB) on $MOUNT — engaging --disk-pressure: reclaimable-lane resets now use rm -rf <lane>/target outright (free-before-reseed at true ENOSPC; acquire_lane re-seeds lazily per D10 §9.5; see esc-5058-5/esc-5114-1/esc-5115-2)"
            args+=(--disk-pressure)
        fi
    else
        _warn "df reported unparseable avail bytes (DF=$DF MOUNT=$MOUNT avail='${avail_bytes}'); proceeding without disk-pressure"
    fi
fi

# ── invoke gc.sh reclaim --mount <MOUNT> [--disk-pressure] ────────────────────
_info "running: $GC_SCRIPT ${args[*]}"
exec "$GC_SCRIPT" "${args[@]}"
