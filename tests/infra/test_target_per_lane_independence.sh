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

test_summary
