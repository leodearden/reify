#!/usr/bin/env bash
# scripts/warm-lane-ref-check.sh — Read-only warm-lane ref-visibility diagnostic.
#
# Cross-repo seam primitive: reify ships this, dark-factory ζ wires it as a
# pre-resolution preflight before the steward's single-shot branch-presence
# resolve.  See docs/design/warm-lane-ref-visibility-seam.md for root-cause
# and seam handoff documentation.
#
# Root cause addressed (task #4855):
#   The steward's merge_queue.py::_classify_branch_presence → git_ops.py::
#   resolve_branch_sha call chain uses a single-shot `git rev-parse --verify
#   refs/heads/task/<id>` with no retry.  Any transient warm-lane lifecycle
#   branch-ref churn (release/acquire re-attaching task/<id>) makes that
#   one attempt fail, and the steward escalates after 1 attempt even though
#   the branch exists before and after the window.  A bounded retry here
#   (--retries N) rides over the window without escalating.
#
# Usage:
#   scripts/warm-lane-ref-check.sh \
#       --lane <dir> --task <id> \
#       [--branch-prefix <pfx>] [--expect-common-dir <dir>] \
#       [--retries N] [--delay S]
#
# Options:
#   --lane <dir>            Lane worktree directory (required)
#   --task <id>             Task ID to resolve (required; resolves
#                           refs/heads/<prefix><id>)
#   --branch-prefix <pfx>  Branch-name prefix (default: "task/")
#   --expect-common-dir <d> If given, assert the lane's git-common-dir
#                           matches this path (canonicalized).  Mismatch
#                           exits 3 — reify provisioning regression.
#   --retries N             Max resolve attempts (default: 5).  1 = single
#                           shot (no retry), which reproduces the steward
#                           symptom when the stub is active.
#   --delay S               Seconds to sleep between retries (default: 0.5).
#                           Pass 0 for deterministic/no-sleep tests.
#   -h, --help              Print this message and exit 0.
#
# Stdout contract:
#   On success: the resolved 40-hex SHA, followed by a newline. ONLY this.
#   On any failure: nothing on stdout; actionable diagnostics on stderr.
#
# Exit codes:
#   0  — success (SHA on stdout)
#   1  — ref absent after all retries (DF-seam suspect: steward should
#          retry/back-off, not escalate-after-1)
#   2  — usage error
#   3  — commondir mismatch (reify provisioning regression)
#
# Never mutates refs, worktrees, or any git state.

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
hint()  { printf '\033[1;33m[hint]\033[0m  %s\n' "$*" >&2; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") --lane <dir> --task <id> [OPTIONS]

  Read-only warm-lane ref-visibility diagnostic.  Resolves
  refs/heads/<prefix><id> from within the linked worktree <dir>,
  optionally with a bounded retry loop to ride over transient lifecycle
  branch-ref churn.

  Options:
    --lane <dir>             Lane worktree directory (required)
    --task <id>              Task ID to resolve (required)
    --branch-prefix <pfx>   Branch-name prefix (default: "task/")
    --expect-common-dir <d>  Assert lane's git-common-dir matches this path
                             (exits 3 on mismatch — reify provisioning check)
    --retries N              Max resolve attempts (default: 5; 1 = single-shot)
    --delay S                Sleep seconds between retries (default: 0.5)
    -h, --help               Print this message and exit 0.

  Exit codes: 0=success  1=ref-absent-after-retries  2=usage  3=commondir-mismatch

  See: docs/design/warm-lane-ref-visibility-seam.md
EOF
}

# ── arg parsing ────────────────────────────────────────────────────────────────
LANE=""
TASK_ID=""
BRANCH_PREFIX="task/"
EXPECT_COMMON_DIR=""
RETRIES=5
DELAY="0.5"

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --lane)
            [ $# -ge 2 ] || { err "--lane requires a value"; exit 2; }
            LANE="$2"; shift 2 ;;
        --task)
            [ $# -ge 2 ] || { err "--task requires a value"; exit 2; }
            TASK_ID="$2"; shift 2 ;;
        --branch-prefix)
            [ $# -ge 2 ] || { err "--branch-prefix requires a value"; exit 2; }
            BRANCH_PREFIX="$2"; shift 2 ;;
        --expect-common-dir)
            [ $# -ge 2 ] || { err "--expect-common-dir requires a value"; exit 2; }
            EXPECT_COMMON_DIR="$2"; shift 2 ;;
        --retries)
            [ $# -ge 2 ] || { err "--retries requires a value"; exit 2; }
            RETRIES="$2"; shift 2 ;;
        --delay)
            [ $# -ge 2 ] || { err "--delay requires a value"; exit 2; }
            DELAY="$2"; shift 2 ;;
        *)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# ── required-arg validation ────────────────────────────────────────────────────
if [ -z "$LANE" ]; then
    err "--lane is required"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi
if [ -z "$TASK_ID" ]; then
    err "--task is required"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

BRANCH_REF="refs/heads/${BRANCH_PREFIX}${TASK_ID}"

info "warm-lane-ref-check: lane=$LANE  branch=$BRANCH_REF  retries=$RETRIES"

# ── check 1: lane is inside a git work tree ────────────────────────────────────
if ! git -C "$LANE" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    err "Lane is not inside a git work tree: $LANE"
    hint "Check that the lane was provisioned with 'git worktree add'."
    exit 1
fi

# ── check 2: commondir coherence (optional) ────────────────────────────────────
if [ -n "$EXPECT_COMMON_DIR" ]; then
    ACTUAL_COMMON_DIR="$(git -C "$LANE" rev-parse --git-common-dir 2>/dev/null || echo "")"
    # Canonicalize both paths for comparison (resolve symlinks, trailing slashes).
    # Note: git -C <lane> rev-parse --git-common-dir returns an ABSOLUTE path;
    # do NOT prepend $LANE to it.
    ACTUAL_CANON="$(realpath -m "$ACTUAL_COMMON_DIR" 2>/dev/null || echo "$ACTUAL_COMMON_DIR")"
    EXPECT_CANON="$(realpath -m "$EXPECT_COMMON_DIR" 2>/dev/null || echo "$EXPECT_COMMON_DIR")"
    if [ "$ACTUAL_CANON" != "$EXPECT_CANON" ]; then
        err "commondir mismatch for lane: $LANE"
        err "  expected common dir: $EXPECT_CANON"
        err "  actual common dir:   $ACTUAL_CANON"
        hint "This indicates a reify provisioning regression."
        hint "Re-run scripts/seed-warm-lane.sh or scripts/provision-warm-lane-fs.sh"
        hint "and verify the worktree is linked to the correct main checkout."
        exit 3
    fi
    ok "commondir OK: $ACTUAL_CANON"
fi

# ── check 3: bounded-retry ref resolve ────────────────────────────────────────
_attempt=0
_sha=""
while [ "$_attempt" -lt "$RETRIES" ]; do
    _attempt=$((_attempt + 1))
    _sha="$(git -C "$LANE" rev-parse --verify "$BRANCH_REF" 2>/dev/null || true)"
    if [ -n "$_sha" ]; then
        ok "Resolved $BRANCH_REF → $_sha (attempt $_attempt/$RETRIES)"
        # Stdout contract: SHA only on success
        printf '%s\n' "$_sha"
        exit 0
    fi
    if [ "$_attempt" -lt "$RETRIES" ]; then
        info "Attempt $_attempt/$RETRIES: $BRANCH_REF not found; retrying after ${DELAY}s ..."
        # Sleep between retries; skip if DELAY is 0 or 0.0 (test/no-op path).
        case "$DELAY" in
            0|0.0|0.00) ;;
            *) sleep "$DELAY" ;;
        esac
    fi
done

# Ref absent after all retries — report forensic state for diagnosis.
# git -C <lane> rev-parse --git-common-dir returns an absolute path.
_common_dir="$(git -C "$LANE" rev-parse --git-common-dir 2>/dev/null || echo "")"
_loose_path="${_common_dir}/refs/heads/${BRANCH_PREFIX}${TASK_ID}"
_loose_abs="$(realpath -m "$_loose_path" 2>/dev/null || echo "$_loose_path")"
_loose_state="absent"
[ -f "$_loose_abs" ] && _loose_state="present"

_packed_state="absent"
_packed_refs="${_common_dir}/packed-refs"
_packed_abs="$(realpath -m "$_packed_refs" 2>/dev/null || echo "$_packed_refs")"
if [ -f "$_packed_abs" ] && grep -qF "refs/heads/${BRANCH_PREFIX}${TASK_ID}" "$_packed_abs" 2>/dev/null; then
    _packed_state="present"
fi

err "Branch not found after $_attempt attempt(s): $BRANCH_REF"
err "  lane:         $LANE"
err "  loose ref:    $_loose_state"
err "  packed-refs:  $_packed_state"
hint "DF-seam suspect: the steward's resolve_branch_sha is single-shot with no retry."
hint "Wire warm-lane-ref-check.sh as a pre-resolution preflight (see seam doc) so"
hint "the steward retries/backs-off rather than escalating after 1 attempt."
hint "See: docs/design/warm-lane-ref-visibility-seam.md"
exit 1
