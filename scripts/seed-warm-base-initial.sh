#!/usr/bin/env bash
# scripts/seed-warm-base-initial.sh — One-shot operator script that stands up the
# rolling warm base at <mount>/base/target and proves it valid with the preflight
# checker.  This is the R4 activation step: run once per host as part of the
# #4665 deploy maintenance window, AFTER provision-warm-lane-fs.sh (R2) and
# relocate-worktrees-to-warm-lane.sh (R3) have completed.
#
# Usage:
#   scripts/seed-warm-base-initial.sh [OPTIONS]
#
# 3-step choreography (runbook):
#   1. Cold-build the _merge-verify worktree's target/ (the advancing source):
#        cd <merge-verify> && cargo build --release
#      This is the only full cold build — lane clones are reflinked from it.
#   2. Refresh the gen-dir base (initialize <mount>/base/target):
#        scripts/refresh-warm-base.sh --landed-commit <HEAD> \
#          --rustflags "$RUSTFLAGS" --invocation "$INVOCATION" \
#          <merge-verify>/target <mount>/base/target
#   3. Run the preflight checker to validate all 5 checks pass:
#        scripts/warm-lane-preflight.sh --mount <mount> \
#          --base-dir <mount>/base/target --invocation "$INVOCATION"
#
# Fail-closed ordering: validate _merge-verify worktree → cold-build →
# assert target/ non-empty → refresh-warm-base → warm-lane-preflight.
# The script's exit status is the preflight result.  A base is NEVER seeded
# from a failed or empty build.
#
# Options:
#   --mount DIR         Warm-lane mount point
#                       (env: REIFY_WARM_LANE_MOUNT; default: <_default_mount>)
#   --base-dir DIR      Base directory to seed
#                       (default: <mount>/base/target)
#   --merge-verify DIR  _merge-verify worktree to cold-build
#                       (default: <mount>/worktrees/_merge-verify)
#   --build-cmd CMD     Cold-build command run inside <merge-verify>
#                       (env: REIFY_SEED_BUILD_CMD; default: cargo build --release)
#   --landed-commit SHA Assert advancing HEAD == SHA (passed to refresh-warm-base.sh
#                       inv.9 provenance guard; default: git rev-parse HEAD)
#   --rustflags VALUE   RUSTFLAGS stamp for the base (default: ${RUSTFLAGS:-})
#   --invocation FP     Invocation fingerprint stamp (default: '')
#   -h, --help          Print this message and exit.
#
# Stdout:  empty — all diagnostics on stderr (matches refresh-warm-base.sh +
#          warm-lane-preflight.sh convention; exit code is the signal).
# Stderr:  all progress messages and errors.
#
# Prerequisites:
#   - provision-warm-lane-fs.sh (R2): XFS mount at <mount> with reflink support.
#   - relocate-worktrees-to-warm-lane.sh (R3): worktrees on XFS.
#   - _merge-verify worktree must be a clean git worktree (no uncommitted tracked
#     changes) at the intended landed HEAD.

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

# ── locate script dir + repo root ─────────────────────────────────────────────
_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$_SCRIPT_DIR/.." && pwd)"

# ── default mount dir: ascend past worktrees/ if present ──────────────────────
# Mirrors provision-warm-lane-fs.sh + relocate-worktrees-to-warm-lane.sh so all
# warm-lane scripts agree on the default mount path.
_default_mount() {
    local repo="${1:-$REPO_ROOT}"
    local parent
    parent="$(dirname "$repo")"
    # If the repo root is inside a worktrees/ directory, surface one level higher
    # so the warm-lanes dir lives beside the worktrees tree, not inside a worktree.
    if [ "$(basename "$parent")" = "worktrees" ]; then
        parent="$(dirname "$parent")"
    fi
    echo "$parent/warm-lanes"
}

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [OPTIONS]

  One-shot operator script: seed the warm-lane base at <mount>/base/target
  and validate it with the preflight checker (R4 activation step).

  Run once per host after provision-warm-lane-fs.sh (R2) and
  relocate-worktrees-to-warm-lane.sh (R3) in the #4665 deploy window.

  3-step choreography:
    1. Cold-build <merge-verify>/target/ (cargo build --release or --build-cmd).
    2. refresh-warm-base.sh --landed-commit <HEAD> ... to initialize the gen-dir base.
    3. warm-lane-preflight.sh --mount <mount> ... to validate all 5 checks pass.
  The script's exit status is the preflight result.

  Options:
    --mount DIR         Warm-lane mount point
                        (env: REIFY_WARM_LANE_MOUNT; default: $(_default_mount))
    --base-dir DIR      Base dir to seed (default: <mount>/base/target)
    --merge-verify DIR  _merge-verify worktree (default: <mount>/worktrees/_merge-verify)
    --build-cmd CMD     Build command run inside <merge-verify>
                        (env: REIFY_SEED_BUILD_CMD; default: cargo build --release)
    --landed-commit SHA Assert advancing HEAD == SHA (default: git rev-parse HEAD)
    --rustflags VALUE   RUSTFLAGS stamp (default: \${RUSTFLAGS:-})
    --invocation FP     Invocation fingerprint stamp (default: '')
    -h, --help          Print this message and exit.

  Stdout:  empty (exit code is the signal).
  Stderr:  all diagnostics.

  Prerequisites:
    - XFS mount at <mount> with reflink support (provision-warm-lane-fs.sh)
    - _merge-verify worktree on XFS (relocate-worktrees-to-warm-lane.sh)
    - _merge-verify must be a clean git worktree at the intended landed HEAD
EOF
}

# ── arg parsing ────────────────────────────────────────────────────────────────
MOUNT="${REIFY_WARM_LANE_MOUNT:-}"
BASE_DIR=""
MERGE_VERIFY=""
BUILD_CMD="${REIFY_SEED_BUILD_CMD:-}"
LANDED_COMMIT=""
RUSTFLAGS_VAL="${RUSTFLAGS:-}"
INVOCATION_VAL=""

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
        --build-cmd)
            [ $# -ge 2 ] || { err "--build-cmd requires a value"; exit 2; }
            BUILD_CMD="$2"; shift 2 ;;
        --landed-commit)
            [ $# -ge 2 ] || { err "--landed-commit requires a value"; exit 2; }
            LANDED_COMMIT="$2"; shift 2 ;;
        --rustflags)
            [ $# -ge 2 ] || { err "--rustflags requires a value"; exit 2; }
            RUSTFLAGS_VAL="$2"; shift 2 ;;
        --invocation)
            [ $# -ge 2 ] || { err "--invocation requires a value"; exit 2; }
            INVOCATION_VAL="$2"; shift 2 ;;
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
if [ -z "$BUILD_CMD" ]; then
    BUILD_CMD="cargo build --release"
fi

info "seed-warm-base-initial.sh: mount=$MOUNT  base=$BASE_DIR  merge-verify=$MERGE_VERIFY"

# ── Step 0: validate _merge-verify worktree (fail-closed, before building) ───
if [ ! -d "$MERGE_VERIFY" ]; then
    err "The _merge-verify worktree directory does not exist: $MERGE_VERIFY"
    err "Expected path: $MERGE_VERIFY"
    err "Run provision-warm-lane-fs.sh (R2) and relocate-worktrees-to-warm-lane.sh (R3)"
    err "first, then ensure the orchestrator has provisioned the _merge-verify worktree."
    exit 1
fi
if ! git -C "$MERGE_VERIFY" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    err "The _merge-verify directory exists but is NOT inside a git worktree: $MERGE_VERIFY"
    err "Expected path: $MERGE_VERIFY"
    err "Ensure the _merge-verify worktree is provisioned and initialized."
    exit 1
fi
info "Merge-verify worktree validated: $MERGE_VERIFY"

# ── Step 1: cold-build the _merge-verify target/ ──────────────────────────────
info "Step 1: cold-building _merge-verify target/ (cmd: $BUILD_CMD) ..."
if ! (cd "$MERGE_VERIFY" && bash -c "$BUILD_CMD"); then
    err "Cold build of _merge-verify failed (cmd: $BUILD_CMD); base NOT seeded."
    err "Fix the build failure and re-run this script."
    exit 1
fi
ok "Cold build complete."

# Assert target/ is non-empty before proceeding to refresh — fail-closed:
# a failed or no-op build must never seed the base.
if [ ! -d "$MERGE_VERIFY/target" ] || [ -z "$(ls -A "$MERGE_VERIFY/target" 2>/dev/null)" ]; then
    err "Cold build completed but <merge-verify>/target is missing or empty: $MERGE_VERIFY/target"
    err "A no-op build must not seed the base (fail-closed)."
    err "Check the build command: $BUILD_CMD"
    exit 1
fi
ok "target/ non-empty — advancing source ready."

# ── Step 2: initialize the gen-dir base via refresh-warm-base.sh ──────────────
# Ensure <mount>/base (parent of base-dir) exists so refresh can resolve it.
mkdir -p "$(dirname "$BASE_DIR")"

# Resolve landed commit: --landed-commit flag takes precedence; else use worktree HEAD.
if [ -z "$LANDED_COMMIT" ]; then
    LANDED_COMMIT="$(git -C "$MERGE_VERIFY" rev-parse HEAD)"
fi

info "Step 2: initializing gen-dir base via refresh-warm-base.sh ..."
info "  advancing source: $MERGE_VERIFY/target"
info "  base dir:         $BASE_DIR"
info "  landed-commit:    $LANDED_COMMIT"
"$_SCRIPT_DIR/refresh-warm-base.sh" \
    --landed-commit "$LANDED_COMMIT" \
    --rustflags "$RUSTFLAGS_VAL" \
    --invocation "$INVOCATION_VAL" \
    "$MERGE_VERIFY/target" \
    "$BASE_DIR"
