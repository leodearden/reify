#!/usr/bin/env bash
# scripts/warm-lane-audit.sh — Standalone, timer-friendly audit/telemetry
# report for the warm-lane CoW pool. Read-only observability: never mutates a
# lane and never gates dispatch/reclaim/merge (PRD §9.5 inv.12).
#
# Part of PRD docs/prds/warm-lane-pool-sizing-lifecycle.md §9.1, §10 B1/B2.
# Consumer: reify ζ (integration gate), dark-factory θ (soft-floor dispatch
# throttle), the operator, and a health timer (§13 open-Q 3).
#
# Usage:
#   scripts/warm-lane-audit.sh [--mount WORKTREES_DIR] [--format table|json] \
#       [--status-cmd CMD] [--stale-age-min N] [--main-ref REF] [--safety N]
#
# For each resident worktree under --mount (lanes _lane-*/_spec-* and orphan
# git-worktree dirs), emits: lane · role · ASSIGNED|FREE · branch ·
# backing-task-status (terminal|non-terminal|unknown) · recoverable
# (LANDED|PUSHED|ORPHAN) · dirty (clean|residue-only|wip) · divergent_gib ·
# age_min · classification (LIVE|RECLAIMABLE|LEAKED|PRESERVED-OK). A trailing
# HEADROOM line summarizes: resident/assigned/free/reclaimable/leaked counts
# + divergent_gib/free_gib/budget_gib.
#
# Options (env defaults shown):
#   --mount DIR           Warm-lane worktrees dir (env: REIFY_WARM_LANE_MOUNT;
#                         shared with warm-lane-preflight.sh / warm-lane-gc.sh).
#                         A nonexistent/empty mount reports resident=0 (not
#                         an error — this script is advisory-only).
#   --format table|json  Output format (default: table).
#   --status-cmd CMD     Backing-task status oracle: invoked as `<cmd> <id>`,
#                         expected to print a status (e.g. done/cancelled/
#                         pending) to stdout. Empty output or a non-zero exit
#                         is treated as unknown. Default: REIFY_LANE_LEAK_STATUS_CMD
#                         (the same oracle warm-lane-preflight.sh Check 6 /
#                         warm-lane-gc.sh consume — D6, no new status plumbing).
#   --stale-age-min N     Minutes; a LEAKED candidate is stale when
#                         age_min >= N (env: REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN;
#                         default: 60).
#   --main-ref REF        Git ref for "main" (env: REIFY_WARM_LANE_AUDIT_MAIN_REF;
#                         default: main).
#   --safety N            Dimensionless divisor for budget_gib = floor(free_gib
#                         / safety) (env: REIFY_WARM_LANE_AUDIT_SAFETY; default:
#                         1.5, mirroring the illustrative safety factor in PRD
#                         §9.2's worked example). Must be > 0.
#   -h, --help            Print this message and exit.
#
# Additional env knobs (no dedicated CLI flag):
#   REIFY_WARM_LANE_AUDIT_DF             df command override (default: df),
#                                         mirrors REIFY_WARM_LANE_DISK_GUARD_DF.
#   REIFY_WARM_LANE_AUDIT_RESIDUE_GLOB   Glob (or comma-separated globs)
#                                         matching "residue" dirty paths that
#                                         don't count as unrecoverable WIP
#                                         (default: data/queue/*.db*, the
#                                         §2/D1 write_queue.db residue).
#
# Exit codes:
#   0  — Always, on every valid invocation (advisory/observability — this
#        script must never gate anything; PRD §9.5 inv.12). A status-lookup
#        failure, a df/du measurement failure, or a nonexistent/empty mount
#        all degrade gracefully rather than aborting.
#   2  — Usage error: unknown flag, missing flag value, invalid --format/
#        --stale-age-min/--safety.
#
# Invariants:
#   A1 — read-only: never mutates a lane (no reset/rm/reclaim); the ASSIGNED/
#        FREE probe opens an EXISTING <dir>.lock read-only and never creates
#        a missing one.
#   A2 — the flock -n -x probe is non-blocking and released immediately.
#   A3 — a status-lookup failure degrades that lane to `unknown` (never
#        aborts, never reclassifies as reclaimable/leaked).
#   A4 — `stale` is always the relation age_min >= stale_age_min against the
#        declared knob — never an inline/undeclared literal.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=scripts/lib_portable.sh
source "$SCRIPT_DIR/lib_portable.sh"

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [--mount DIR] [--format table|json] [--status-cmd CMD]
                       [--stale-age-min N] [--main-ref REF] [--safety N]

  Standalone, timer-friendly audit/telemetry report for the warm-lane CoW
  pool. Read-only: never mutates a lane; never gates dispatch/reclaim/merge.

  Options:
    --mount DIR           Warm-lane worktrees dir (default: \$REIFY_WARM_LANE_MOUNT).
    --format table|json   Output format (default: table).
    --status-cmd CMD      Backing-task status oracle (default:
                          \$REIFY_LANE_LEAK_STATUS_CMD).
    --stale-age-min N     Minutes before a candidate is stale (default:
                          \$REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN or 60).
    --main-ref REF        Git ref for "main" (default: \$REIFY_WARM_LANE_AUDIT_MAIN_REF
                          or main).
    --safety N            Divisor for budget_gib = floor(free_gib / safety)
                          (default: \$REIFY_WARM_LANE_AUDIT_SAFETY or 1.5).
    -h, --help            Print this message and exit.

  Exit codes:
    0  — Always, on every valid invocation (advisory-only; never gates).
    2  — Usage error.
EOF
}

# ── defaults ───────────────────────────────────────────────────────────────────
MOUNT="${REIFY_WARM_LANE_MOUNT:-}"
FORMAT="table"
STATUS_CMD="${REIFY_LANE_LEAK_STATUS_CMD:-}"
STALE_AGE_MIN="${REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN:-60}"
MAIN_REF="${REIFY_WARM_LANE_AUDIT_MAIN_REF:-main}"
SAFETY="${REIFY_WARM_LANE_AUDIT_SAFETY:-1.5}"
DF="${REIFY_WARM_LANE_AUDIT_DF:-df}"
RESIDUE_GLOB="${REIFY_WARM_LANE_AUDIT_RESIDUE_GLOB:-data/queue/*.db*}"

# ── arg parsing ────────────────────────────────────────────────────────────────
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --mount)
            [ $# -ge 2 ] || { err "--mount requires a value"; exit 2; }
            MOUNT="$2"; shift 2 ;;
        --format)
            [ $# -ge 2 ] || { err "--format requires a value"; exit 2; }
            FORMAT="$2"; shift 2 ;;
        --status-cmd)
            [ $# -ge 2 ] || { err "--status-cmd requires a value"; exit 2; }
            STATUS_CMD="$2"; shift 2 ;;
        --stale-age-min)
            [ $# -ge 2 ] || { err "--stale-age-min requires a value"; exit 2; }
            STALE_AGE_MIN="$2"; shift 2 ;;
        --main-ref)
            [ $# -ge 2 ] || { err "--main-ref requires a value"; exit 2; }
            MAIN_REF="$2"; shift 2 ;;
        --safety)
            [ $# -ge 2 ] || { err "--safety requires a value"; exit 2; }
            SAFETY="$2"; shift 2 ;;
        -*)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
        *)
            err "Unexpected argument: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# ── validate ───────────────────────────────────────────────────────────────────
case "$FORMAT" in
    table|json) : ;;
    *)
        err "Unknown --format: '$FORMAT' (expected table or json)."
        err "Run '$(basename "$0") --help' for usage."
        exit 2 ;;
esac

if ! printf '%s\n' "$STALE_AGE_MIN" | grep -qE '^[0-9]+$'; then
    err "--stale-age-min (or REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN) is not a valid non-negative integer: '$STALE_AGE_MIN'."
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

if ! printf '%s\n' "$SAFETY" | grep -qE '^[0-9]+(\.[0-9]+)?$' || ! awk -v s="$SAFETY" 'BEGIN{exit !(s>0)}'; then
    err "--safety (or REIFY_WARM_LANE_AUDIT_SAFETY) is not a valid positive number: '$SAFETY'."
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

info "warm-lane-audit.sh: mount=$MOUNT format=$FORMAT stale_age_min=$STALE_AGE_MIN main_ref=$MAIN_REF safety=$SAFETY"

# ── helper: is this dir a git worktree? ───────────────────────────────────────
_is_git_worktree() {
    local dir="$1"
    [ -d "$dir" ] || return 1
    git -C "$dir" rev-parse --git-dir >/dev/null 2>&1
}

# ── helper: role from the resident dir's basename ─────────────────────────────
# _lane-* -> lane; _spec-* -> spec; anything else (that is still a git
# worktree) -> orphan. Mirrors warm-lane-gc.sh's lane-glob/orphan bucketing.
_lane_role() {
    local name="$1"
    case "$name" in
        _lane-*) printf 'lane' ;;
        _spec-*) printf 'spec' ;;
        *) printf 'orphan' ;;
    esac
}

# ── helper: non-mutating ASSIGNED/FREE probe (A1/A2/DD1) ──────────────────────
# Opens an EXISTING <dir>.lock read-only and attempts a non-blocking exclusive
# flock on that read-only fd: success (lock acquired) => FREE, released
# immediately; failure (already held) => ASSIGNED. A missing lock file is
# FREE and is NEVER created (no `>`-open/truncation, no `flock <file> <cmd>`
# convenience form -- both would mutate the pool).
_probe_assigned() {
    local lock="$1"
    local result='FREE'
    if [ -e "$lock" ]; then
        if exec 7<"$lock" 2>/dev/null; then
            if flock -n -x 7 2>/dev/null; then
                flock -u 7 2>/dev/null || true
            else
                result='ASSIGNED'
            fi
            exec 7<&- 2>/dev/null || true
        fi
    fi
    printf '%s' "$result"
    return 0
}

# ── helper: resolve the lane's raw branch (empty string when detached) ────────
# Callers needing a display value substitute "(detached)" for an empty result
# (e.g. the table row); recoverable's PUSHED check needs the raw (possibly
# empty) value so a detached lane never spuriously matches refs/remotes/origin/.
_lane_branch_raw() {
    local dir="$1"
    git -C "$dir" symbolic-ref --short HEAD 2>/dev/null || true
}

# ── backing-task resolution (D6 reuse, lifted from warm-lane-gc.sh) ──────────
# _backing_task_id <dir>
# Prints the numeric task id backing <dir>'s HEAD, or nothing if none can be
# resolved. Never fails under set -e (all git calls guarded with `|| true`).
# attached  — HEAD is on a branch named task/NNNN (purely numeric NNNN).
# detached  — enumerate refs/heads/task/* branches that CONTAIN HEAD; use the
#             id ONLY when exactly one DISTINCT task/NNNN branch matches.
_backing_task_id() {
    local dir="$1"
    local br id
    br="$(git -C "$dir" symbolic-ref --short HEAD 2>/dev/null || true)"
    if [ -n "$br" ]; then
        case "$br" in
            task/*)
                id="${br#task/}"
                case "$id" in
                    ''|*[!0-9]*) return 0 ;;  # non-numeric id — no match
                esac
                printf '%s' "$id"
                ;;
        esac
        return 0  # attached to a non-task (or non-numeric-task) branch — no id
    fi

    # Detached HEAD: resolve via containing task/* branches.
    local -a ids=()
    local ref rid
    while IFS= read -r ref; do
        [ -n "$ref" ] || continue
        rid="${ref#task/}"
        case "$rid" in
            ''|*[!0-9]*) continue ;;  # non-numeric id — skip
        esac
        ids+=("$rid")
    done < <(git -C "$dir" for-each-ref --format='%(refname:short)' --contains HEAD refs/heads/task/* 2>/dev/null || true)

    if [ "${#ids[@]}" -eq 1 ]; then
        printf '%s' "${ids[0]}"
    fi
    return 0
}

# _backing_task_status <dir>
# Prints terminal|non-terminal|unknown (A3: a status-lookup failure/unresolved
# id/unset STATUS_CMD always degrades to unknown, never aborts). Oracle
# contract mirrors warm-lane-preflight.sh Check 6 / warm-lane-gc.sh
# _backing_task_terminal byte-for-byte: non-zero exit / empty output = unknown.
_backing_task_status() {
    local dir="$1"
    local id st
    [ -n "$STATUS_CMD" ] || { printf 'unknown'; return 0; }
    id="$(_backing_task_id "$dir")"
    [ -n "$id" ] || { printf 'unknown'; return 0; }
    st="$("$STATUS_CMD" "$id" 2>/dev/null | tr -d '[:space:]' || true)"
    case "$st" in
        done|cancelled) printf 'terminal' ;;
        '') printf 'unknown' ;;
        *) printf 'non-terminal' ;;
    esac
    return 0
}

# ── recoverable: LANDED | PUSHED | ORPHAN ─────────────────────────────────────
# _recoverable <dir> <branch>
# LANDED <=> HEAD is an ancestor of --main-ref. Else PUSHED <=> HEAD is an
# ancestor of refs/remotes/origin/<branch> (attached branches only -- a
# detached lane, branch=="", is never PUSHED; the merge-base call against a
# nonexistent remote ref fails harmlessly and falls through). Else ORPHAN.
_recoverable() {
    local dir="$1" branch="$2"
    if git -C "$dir" merge-base --is-ancestor HEAD "$MAIN_REF" 2>/dev/null; then
        printf 'LANDED'; return 0
    fi
    if [ -n "$branch" ] && git -C "$dir" merge-base --is-ancestor HEAD "refs/remotes/origin/$branch" 2>/dev/null; then
        printf 'PUSHED'; return 0
    fi
    printf 'ORPHAN'
    return 0
}

# ── helper: name/path matches a glob pattern ──────────────────────────────────
# _matches_glob <value> <comma-separated-globs>
# Lifted verbatim from warm-lane-gc.sh's identically-named helper.
_matches_glob() {
    local value="$1"
    local globs="$2"
    local g
    # Split comma-separated globs
    local IFS=","
    for g in $globs; do
        # shellcheck disable=SC2254
        case "$value" in
            $g) return 0 ;;
        esac
    done
    return 1
}

# ── dirty-state: clean | residue-only | wip ───────────────────────────────────
# _dirty_state <dir>
# git status --porcelain --untracked-files=no (reuse gc.sh's _is_reclaimable
# dirty predicate verbatim: --untracked-files=no excludes untracked artifacts
# like target/, so only uncommitted changes to TRACKED files are considered).
# Empty -> clean. Non-empty AND every changed path matches RESIDUE_GLOB
# (default data/queue/*.db*, the §2/D1 write_queue.db residue) -> residue-only.
# Any non-residue path -> wip (genuine unrecoverable WIP). A `git status`
# failure degrades fail-closed to wip (never mistakenly reclaimable).
_dirty_state() {
    local dir="$1"
    local status_out
    status_out="$(git -C "$dir" status --porcelain --untracked-files=no 2>/dev/null)" || { printf 'wip'; return 0; }
    [ -n "$status_out" ] || { printf 'clean'; return 0; }

    local line path
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        # Porcelain v1 short format: "XY path" (renames/copies: "XY orig -> new"
        # -- use the new path). Strip the 2-char status code + separating space.
        path="${line:3}"
        case "$path" in
            *' -> '*) path="${path#*' -> '}" ;;
        esac
        if ! _matches_glob "$path" "$RESIDUE_GLOB"; then
            printf 'wip'
            return 0
        fi
    done <<< "$status_out"

    printf 'residue-only'
    return 0
}

# ── age_min: whole minutes since <dir>'s mtime ────────────────────────────────
# _age_min <dir>
# floor((now - mtime(dir)) / 60), via lib_portable.sh's portable_mtime. A
# missing/unreadable dir degrades to 0 (never stale) rather than aborting.
_age_min() {
    local dir="$1"
    local mtime now
    mtime="$(portable_mtime "$dir" 2>/dev/null)" || { printf '0'; return 0; }
    now="$(date +%s)"
    printf '%d' $(( (now - mtime) / 60 ))
}

# ── classification (monotonically refined across the walk's build-out) ───────
# _classify assigned status recoverable dirty age_min
_classify() {
    local assigned="$1" status="$2" recoverable="$3" dirty="$4" age_min="$5"
    if [ "$assigned" = "ASSIGNED" ]; then
        printf 'LIVE'; return 0
    fi
    # FREE lane.
    if [ "$status" = "terminal" ] || [ "$recoverable" = "LANDED" ] || [ "$recoverable" = "PUSHED" ] || [ "$dirty" = "residue-only" ]; then
        printf 'RECLAIMABLE'; return 0
    fi
    # LEAKED (A4: stale is always the age_min >= STALE_AGE_MIN relation vs the
    # declared knob, never an inline literal). recoverable==ORPHAN is, by
    # _recoverable's construction (HEAD is not an ancestor of MAIN_REF),
    # always "ahead-of-main" in the PRD's sense, so the "(genuine-WIP ∨
    # ahead-of-main)" disjunct is satisfied automatically here.
    if [ "$status" = "non-terminal" ] && [ "$recoverable" = "ORPHAN" ] && [ "$age_min" -ge "$STALE_AGE_MIN" ]; then
        printf 'LEAKED'; return 0
    fi
    printf 'PRESERVED-OK'
}

# ── resident walk ──────────────────────────────────────────────────────────────
RESIDENT=0
ASSIGNED_COUNT=0
FREE_COUNT=0
RECLAIMABLE_COUNT=0
LEAKED_COUNT=0
TABLE_OUT=""

# A nonexistent/empty --mount is NOT an error (advisory-only script): the walk
# below simply visits nothing and resident stays 0.
if [ -n "$MOUNT" ] && [ -d "$MOUNT" ]; then
    for entry in "$MOUNT"/*/; do
        entry="${entry%/}"
        [ -d "$entry" ] || continue
        name="$(basename "$entry")"
        _is_git_worktree "$entry" || continue

        RESIDENT=$((RESIDENT + 1))
        role="$(_lane_role "$name")"

        assigned="$(_probe_assigned "$MOUNT/$name.lock")"
        if [ "$assigned" = "ASSIGNED" ]; then
            ASSIGNED_COUNT=$((ASSIGNED_COUNT + 1))
        else
            FREE_COUNT=$((FREE_COUNT + 1))
        fi

        raw_branch="$(_lane_branch_raw "$entry")"
        branch="${raw_branch:-(detached)}"
        status="$(_backing_task_status "$entry")"
        recoverable="$(_recoverable "$entry" "$raw_branch")"
        dirty="$(_dirty_state "$entry")"
        age_min="$(_age_min "$entry")"

        classification="$(_classify "$assigned" "$status" "$recoverable" "$dirty" "$age_min")"
        case "$classification" in
            RECLAIMABLE) RECLAIMABLE_COUNT=$((RECLAIMABLE_COUNT + 1)) ;;
            LEAKED) LEAKED_COUNT=$((LEAKED_COUNT + 1)) ;;
        esac

        TABLE_OUT="${TABLE_OUT}lane=${name} role=${role} assigned=${assigned} branch=${branch} status=${status} recoverable=${recoverable} dirty=${dirty} age_min=${age_min} classification=${classification}
"
    done
fi

printf '%s' "$TABLE_OUT"
printf 'HEADROOM resident=%d assigned=%d free=%d reclaimable=%d leaked=%d\n' \
    "$RESIDENT" "$ASSIGNED_COUNT" "$FREE_COUNT" "$RECLAIMABLE_COUNT" "$LEAKED_COUNT"

exit 0
