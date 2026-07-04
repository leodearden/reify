#!/usr/bin/env bash
# scripts/ensure-warm-base.sh — Autonomous-first self-heal ladder for the
# warm-lane CoW pool base (<mount>/base/target), run as a boot oneshot.
#
# An absent/empty base leaves the warm pool dead until the next merge-landing
# (refresh-warm-base.sh is normally the only builder). This script self-heals
# at boot via a 4-rung ladder where escalation is the LAST rung, not the
# first — autonomous remediation is always attempted before any human is
# paged.
#
# The ladder:
#   Rung 1 — base present & non-empty (warm-lane-preflight.sh Check 3
#            predicate, symlink-following) -> exit 0, silent no-op.
#            Idempotent: safe to run repeatedly.
#   Rung 2 — base absent/dangling AND a warm source survives
#            (<merge-verify>/target non-empty, clean git worktree) -> CHEAP
#            reflink reseed via refresh-warm-base.sh --landed-commit
#            "$(git -C <mv> rev-parse HEAD)" <mv>/target <base> (seconds).
#            On success -> exit 0 silent, NO cold-build, NO escalation.
#            On reseed failure / base still absent -> escalate + exit non-zero
#            (exceptional FS/source fault; P2 forbids a silent full-copy
#            fallback).
#   Rung 3 — base absent AND no warm source (fresh/wiped partition) -> AUTO
#            cold-build via seed-warm-base-initial.sh, in the BACKGROUND by
#            default (REIFY_WARM_BASE_HEALTH_BUILD_ASYNC=1) so the unit/
#            orchestrator is never blocked for the multi-hour build (inv.6
#            fail-open). Logs loudly at operational level; exits 0 (a known
#            deterministic remedy is underway — NOT an escalation). If the
#            backgrounded build later fails, the background wrapper emits the
#            rung-4 escalation signal to the journal.
#   Rung 4 — remediation FAILS (cold-build errors, or base still absent after
#            a remediation attempt) -> emit exactly ONE escalation signal +
#            exit non-zero.
#
# Escalation-signal contract (reify ships the primitive; dark-factory wires
# the issue-filing): a boot-time systemd oneshot has no MCP/orchestrator
# context, so it cannot call escalate_blocker directly. Following the
# warm-lane-ref-check.sh stdout-contract precedent (stdout carries the single
# machine-consumable value; all diagnostics on stderr), escalate() emits
# EXACTLY ONE once-guarded stdout token line:
#     REIFY_WARM_BASE_HEALTH_ESCALATION <reason>
# plus a loud stderr diagnostic, and the script exits non-zero. The companion
# dark-factory BASE_ABSENT task greps the unit's journal for this token and
# files the one host-scoped issue. Rungs 1-3 keep stdout EMPTY (grep -c == 0);
# only rung 4 emits the one token (grep -c == 1).
#
# Usage:
#   scripts/ensure-warm-base.sh [OPTIONS]
#
# Options (env defaults shown):
#   --mount DIR         Warm-lane mount point (env: REIFY_WARM_LANE_MOUNT)
#   --base-dir DIR      Base directory to heal (env: REIFY_WARM_LANE_BASE;
#                       default: <mount>/base/target)
#   --merge-verify DIR  _merge-verify worktree, the rung-2 warm source
#                       (default: <mount>/worktrees/_merge-verify)
#   --sync              Run rung-3's cold-build inline instead of backgrounding
#                       it (equivalent to REIFY_WARM_BASE_HEALTH_BUILD_ASYNC=0)
#   -h, --help          Print this message and exit 0.
#
# Env:
#   REIFY_WARM_BASE_HEALTH_REFRESH_CMD   Override the refresh-warm-base.sh
#                                        invocation (default: $_SCRIPT_DIR/
#                                        refresh-warm-base.sh). Test seam.
#   REIFY_WARM_BASE_HEALTH_SEED_CMD      Override the seed-warm-base-initial.sh
#                                        invocation (default: $_SCRIPT_DIR/
#                                        seed-warm-base-initial.sh). Test seam.
#   REIFY_WARM_BASE_HEALTH_BUILD_ASYNC   1 (default) = rung 3 backgrounds the
#                                        cold build; 0 = run inline (--sync).
#
# Stdout: EMPTY on rungs 1-3. Rung 4 emits exactly one
#         "REIFY_WARM_BASE_HEALTH_ESCALATION <reason>" line.
# Stderr: all progress/diagnostic messages.
#
# Exit codes: 0 = healthy or autonomously remediated (or a rung-3 cold-build
#             was kicked off in the background); non-zero = rung 4 escalation,
#             or a usage error.

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
hint()  { printf '\033[1;33m[hint]\033[0m  %s\n' "$*" >&2; }

# ── locate script dir ──────────────────────────────────────────────────────────
_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$_SCRIPT_DIR/.." && pwd)"

# ── default mount dir: ascend past worktrees/ if present ──────────────────────
# Mirrors seed-warm-base-initial.sh so all warm-lane scripts agree on the
# default mount path.
_default_mount() {
    local repo="${1:-$REPO_ROOT}"
    local parent
    parent="$(dirname "$repo")"
    if [ "$(basename "$parent")" = "worktrees" ]; then
        parent="$(dirname "$parent")"
    fi
    echo "$parent/warm-lanes"
}

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [OPTIONS]

  Autonomous-first self-heal ladder for the warm-lane CoW pool base
  (<mount>/base/target), run as a boot oneshot. Escalation is the LAST rung.

  Options:
    --mount DIR         Warm-lane mount point
                        (env: REIFY_WARM_LANE_MOUNT; default: $(_default_mount))
    --base-dir DIR      Base directory to heal
                        (env: REIFY_WARM_LANE_BASE; default: <mount>/base/target)
    --merge-verify DIR  _merge-verify worktree, the rung-2 warm source
                        (default: <mount>/worktrees/_merge-verify)
    -h, --help          Print this message and exit 0.

  Stdout: empty on rungs 1-3; rung 4 emits exactly one
          "REIFY_WARM_BASE_HEALTH_ESCALATION <reason>" line.
  Stderr: all diagnostics.
EOF
}

# ── arg parsing ────────────────────────────────────────────────────────────────
MOUNT="${REIFY_WARM_LANE_MOUNT:-}"
BASE_DIR="${REIFY_WARM_LANE_BASE:-}"
MERGE_VERIFY=""
BUILD_ASYNC="${REIFY_WARM_BASE_HEALTH_BUILD_ASYNC:-1}"

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --mount)
            [ $# -ge 2 ] || { err "--mount requires a value"; exit 2; }
            MOUNT="$2"; shift 2 ;;
        --base-dir)
            [ $# -ge 2 ] || { err "--base-dir requires a value"; exit 2; }
            BASE_DIR="$2"; shift 2 ;;
        --merge-verify)
            [ $# -ge 2 ] || { err "--merge-verify requires a value"; exit 2; }
            MERGE_VERIFY="$2"; shift 2 ;;
        *)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# ── resolve defaults ───────────────────────────────────────────────────────────
if [ -z "$MOUNT" ]; then
    MOUNT="$(_default_mount)"
fi
if [ -z "$BASE_DIR" ]; then
    BASE_DIR="$MOUNT/base/target"
fi
if [ -z "$MERGE_VERIFY" ]; then
    MERGE_VERIFY="$MOUNT/worktrees/_merge-verify"
fi

# ── command hooks (test seams; default to the real sibling scripts) ──────────
REFRESH_CMD="${REIFY_WARM_BASE_HEALTH_REFRESH_CMD:-$_SCRIPT_DIR/refresh-warm-base.sh}"
SEED_CMD="${REIFY_WARM_BASE_HEALTH_SEED_CMD:-$_SCRIPT_DIR/seed-warm-base-initial.sh}"

info "ensure-warm-base.sh: mount=$MOUNT  base=$BASE_DIR  merge-verify=$MERGE_VERIFY  async=$BUILD_ASYNC"

# ── base_healthy: warm-lane-preflight.sh Check 3 predicate ────────────────────
# Reused verbatim (symlink-following: both -d and ls resolve through a base ->
# gen.N symlink; a dangling symlink fails -d -> treated as unhealthy). Not a
# full preflight invocation — rung 1's gate is solely presence+non-emptiness;
# a full 5-check preflight would over-couple boot self-heal to mount/reflink/
# RUSTFLAGS-stamp state that this ladder does not govern.
base_healthy() {
    [ -d "$BASE_DIR" ] && [ -n "$(ls -A "$BASE_DIR" 2>/dev/null)" ]
}

# ── Rung 1: base present & non-empty -> silent no-op ──────────────────────────
if base_healthy; then
    ok "Rung 1: base healthy at $BASE_DIR — no-op."
    exit 0
fi

# ── escalate: rung-4 signal — exactly ONE stdout token + loud stderr ─────────
# Once-guarded (module var) so the "exactly ONE" contract holds regardless of
# which caller reaches it first. Stdout carries ONLY the machine-parseable
# token (warm-lane-ref-check.sh stdout-contract precedent); all diagnostics
# go to stderr.
_ESCALATED=0
escalate() {
    local reason="$1"
    [ "$_ESCALATED" = "1" ] && return 0
    _ESCALATED=1
    err "ESCALATION: $reason"
    hint "Boot self-heal could not remediate the warm-lane base autonomously."
    hint "A companion task greps the unit journal for the token below and files the issue."
    printf 'REIFY_WARM_BASE_HEALTH_ESCALATION %s\n' "$reason"
}

# ── Rung 2: base absent/dangling + a warm source survives -> reflink reseed ──
if [ -d "$MERGE_VERIFY/target" ] && [ -n "$(ls -A "$MERGE_VERIFY/target" 2>/dev/null)" ]; then
    info "Rung 2: warm source found at $MERGE_VERIFY/target — attempting reflink reseed."
    _r2_head="$(git -C "$MERGE_VERIFY" rev-parse HEAD 2>/dev/null || true)"
    _r2_rc=0
    "$REFRESH_CMD" --landed-commit "$_r2_head" "$MERGE_VERIFY/target" "$BASE_DIR" || _r2_rc=$?
    if [ "$_r2_rc" -eq 0 ] && base_healthy; then
        ok "Rung 2: reflink reseed succeeded — base healthy at $BASE_DIR."
        exit 0
    fi
    escalate "rung2: reflink reseed failed or base still unhealthy after reseed attempt (refresh rc=$_r2_rc)"
    exit 1
fi

# ── ladder: rungs 3-4 filled in by later TDD steps ────────────────────────────
err "ensure-warm-base.sh: rungs 3-4 not yet implemented."
exit 1
