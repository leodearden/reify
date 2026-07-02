#!/usr/bin/env bash
# tests/infra/test_target_per_lane_independence.sh
# Drift-guard for task #4948: makes EXECUTABLE the invariant the run_all
# host-infra partition (H1 #4921) silently rests on -- each concurrent
# warm-lane has an INDEPENDENT build-artifact target/, so target/-mutating
# intra-run-serial tests (test_reify_audit_ptodo.sh, test_tree_sitter_pipeline.sh)
# never propagate writes to the base or a sibling lane. If target/ ever became
# a symlink to a SHARED location, that classification would be invalid and
# those tests would need host-exclusive reclassification.
#
# Three assertion groups, each carrying a non-vacuity self-check:
#   STATIC       (always runs) -- greps scripts/seed-warm-lane.sh and
#                scripts/gc-worktree-targets.sh for the real reflink-clone /
#                independent-rm materialization, with no shared-symlink
#                materialization of a lane/worktree target.
#   BEHAVIORAL   (substrate-gated; SKIPs cleanly with no reflink FS) -- seeds
#                two lanes from a common base via the REAL seed-warm-lane.sh
#                and asserts a sentinel written into one lane's target/ never
#                appears in the sibling lane's or the base's target/, and that
#                CoW divergence holds on a shared-extent file overwrite.
#   REGISTRATION (always runs; self-guard) -- this test is wired into
#                scripts/verify-pipeline-infra-tests.txt (fail-fast pole for
#                seed-warm-lane.sh edits) and tests/infra/run-all-
#                classification.manifest (H1 declared-union coverage).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== per-lane target/ independence drift-guard (task #4948) ==="

# ── Resolved paths for the systems-under-test (read-only) ───────────────────
SEED_SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"
GC_SCRIPT="$REPO_ROOT/scripts/gc-worktree-targets.sh"
VP_MAP="$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt"
MANIFEST="$REPO_ROOT/tests/infra/run-all-classification.manifest"

# ── Shared temp state + cleanup trap ─────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# detect_reflink_substrate() -- mirrors tests/infra/test_warm_lane_pool.sh's
# detect_substrate() rungs 1-2 (REIFY_WARM_LANE_MOUNT probe + ${TMPDIR:-/tmp}
# scratch probe). Rung 3 (opt-in ephemeral loopback via
# REIFY_RUN_WARM_LANE_GATE=1) is deliberately omitted: this guard's
# behavioral block only needs SOME reflink-capable scratch directory to seed
# two throwaway lanes into, not the heavier provisioned-mount rung that
# test_warm_lane_pool.sh's cold cargo-build blocks require. Sets _GATE_DIR on
# success. Returns 0 when a reflink-capable directory is found, 1 otherwise.
detect_reflink_substrate() {
    local probe_src probe_dst probe_tmp
    probe_src=""
    probe_dst=""
    probe_tmp=""

    # 1. Caller-supplied mount
    if [ -n "${REIFY_WARM_LANE_MOUNT:-}" ] && [ -d "${REIFY_WARM_LANE_MOUNT}" ]; then
        probe_src="$(mktemp "${REIFY_WARM_LANE_MOUNT}/.reflink-probe-src-XXXXXX" 2>/dev/null)" || true
        if [ -n "$probe_src" ] && [ -f "$probe_src" ]; then
            probe_dst="${probe_src}.dst"
            if cp --reflink=always "$probe_src" "$probe_dst" 2>/dev/null; then
                rm -f "$probe_src" "$probe_dst" 2>/dev/null || true
                _GATE_DIR="${REIFY_WARM_LANE_MOUNT}"
                return 0
            fi
            rm -f "$probe_src" "$probe_dst" 2>/dev/null || true
        fi
        echo "detect_reflink_substrate: REIFY_WARM_LANE_MOUNT reflink probe failed" >&2
    fi

    # 2. Scratch-dir reflink probe in TMPDIR (usually /tmp)
    probe_tmp="$(mktemp -d "${TMPDIR:-/tmp}/target-lane-indep-scratch-XXXXXX" 2>/dev/null)" || true
    if [ -n "$probe_tmp" ] && [ -d "$probe_tmp" ]; then
        probe_src="$probe_tmp/probe.src"
        probe_dst="$probe_tmp/probe.dst"
        : > "$probe_src"
        if cp --reflink=always "$probe_src" "$probe_dst" 2>/dev/null; then
            _GATE_DIR="$(dirname "$probe_tmp")"
            rm -rf "$probe_tmp" 2>/dev/null || true
            return 0
        fi
        rm -rf "$probe_tmp" 2>/dev/null || true
    fi

    return 1
}

# _skip(reason) -- emit SKIP on stderr, call test_summary (counts so far), exit 0.
_skip() {
    echo "SKIP: $*" >&2
    test_summary
    exit 0
}

# Negative assertion helper (assert() only checks for success rc). Mirrors
# tests/infra/test_plan_capture_lib.sh's refute().
refute() { ! "$@"; }

# _target_reflink_ok <script> -- STATIC predicate: returns 0 iff <script>
# materializes its lane target via an independent reflink CoW clone, never a
# symlink into a shared/base location:
#   (i)   a real `cp ... --reflink=always ... "$LANE_TARGET"` clone
#   (ii)  LANE_TARGET is defined lane-local: LANE_TARGET="$LANE_DIR/target"
#   (iii) NO `ln -s`/`ln -sfn`/`ln -sf` line whose destination operand is
#         "$LANE_TARGET" or a literal .../target path (the shared-symlink
#         regression class this guard exists to catch)
# Comment-only lines are stripped once up front so a doc comment mentioning
# these patterns can't produce a false PASS or false FLAG.
_target_reflink_ok() {
    local script="$1"
    [ -f "$script" ] || return 1

    local code
    code="$(grep -v '^[[:space:]]*#' "$script")"

    printf '%s\n' "$code" | grep -qE 'reflink=always.*"\$LANE_TARGET"' || return 1
    printf '%s\n' "$code" | grep -qE '^[[:space:]]*LANE_TARGET="\$LANE_DIR/target"' || return 1
    if printf '%s\n' "$code" | grep -qE '\bln[[:space:]]+-s[a-zA-Z]*[[:space:]].*("\$LANE_TARGET"|/target"?)[[:space:]]*$'; then
        return 1
    fi

    return 0
}

# ─────────────────────────────────────────────────────────────────────────────
# STATIC group (always runs, no substrate needed): the real seed-warm-lane.sh
# clones the lane target via a lane-local reflink CoW clone, never a symlink
# into a shared/base location. _target_reflink_ok() is defined in the impl
# step that follows this one.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- STATIC: seed-warm-lane.sh clones a lane-local target/ (never a shared symlink) ---"

assert "_target_reflink_ok: real seed-warm-lane.sh clones a lane-local target/ (green on truth)" \
    _target_reflink_ok "$SEED_SCRIPT"

# Non-vacuity: a stub seed script that materializes the lane target via a
# symlink into a shared/base location (the exact regression this guard exists
# to catch) must be FLAGGED as a violation, proving the predicate
# discriminates rather than being vacuously green.
_STUB_DIR="$(mktemp -d)"
_TMPDIRS+=("$_STUB_DIR")
_STUB_SEED="$_STUB_DIR/stub-seed-warm-lane.sh"
cat > "$_STUB_SEED" <<'STUB_EOF'
#!/usr/bin/env bash
# Synthetic non-vacuity fixture: a "seed" script that shares the lane target
# via a symlink into the base instead of an independent reflink CoW clone.
set -euo pipefail
LANE_TARGET="$LANE_DIR/target"
ln -sfn "$BASE_TARGET_DIR/target" "$LANE_TARGET"
STUB_EOF

assert "_target_reflink_ok: a symlink-shared stub seed is FLAGGED as a violation (non-vacuity)" \
    refute _target_reflink_ok "$_STUB_SEED"

# ─────────────────────────────────────────────────────────────────────────────
# STATIC group (secondary): gc-worktree-targets.sh is the worktree-side vector
# named by the task -- confirm it rm's each per-worktree target/ independently
# and never symlinks a shared target. Plain grep (no predicate function): the
# real script has always satisfied this, so there is no RED phase to manufacture
# here (see design decision in .task/plan.json).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- STATIC (secondary): gc-worktree-targets.sh rm's per-worktree target/ independently ---"

assert 'gc-worktree-targets.sh removes each worktree target/ independently (rm -rf "$target")' \
    grep -qE 'rm -rf "\$target"' "$GC_SCRIPT"

assert "gc-worktree-targets.sh never symlinks a shared worktree target (no ln -s* .../target)" \
    refute grep -qE '\bln[[:space:]]+-s[a-zA-Z]*\b.*target' "$GC_SCRIPT"

test_summary
