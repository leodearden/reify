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
# Three assertion groups, each carrying a non-vacuity self-check. Execution
# order matters: BEHAVIORAL is substrate-gated and _skip()s (exit 0) when no
# reflink FS is available, so it runs LAST -- both always-run groups (STATIC,
# REGISTRATION) execute unconditionally before that possible early exit.
#   STATIC       (always runs) -- greps scripts/seed-warm-lane.sh and
#                scripts/gc-worktree-targets.sh for the real reflink-clone /
#                independent-rm materialization, with no shared-symlink
#                materialization of a lane/worktree target.
#   REGISTRATION (always runs; self-guard) -- this test is wired into
#                scripts/verify-pipeline-infra-tests.txt (fail-fast pole for
#                seed-warm-lane.sh edits) and tests/infra/run-all-
#                classification.manifest (H1 declared-union coverage).
#   BEHAVIORAL   (substrate-gated; SKIPs cleanly with no reflink FS; runs
#                LAST) -- seeds two lanes from a common base via the REAL
#                seed-warm-lane.sh and asserts a sentinel written into one
#                lane's target/ never appears in the sibling lane's or the
#                base's target/, and that CoW divergence holds on a
#                shared-extent file overwrite.
#
# COVERAGE CAVEAT: on a host with no reflink-capable substrate (no usable
# REIFY_WARM_LANE_MOUNT and no reflink-capable ${TMPDIR:-/tmp}), BEHAVIORAL
# SKIPs and this guard's live coverage narrows to STATIC's source-level greps
# plus REGISTRATION's wiring checks -- a regression that preserved those
# grepped patterns while breaking real write-independence would not be
# caught on such a host. Set REIFY_WARM_LANE_MOUNT (or run where
# ${TMPDIR:-/tmp} is reflink-capable, e.g. btrfs or XFS with reflink=1) for
# this guard to have real teeth.
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
_GATE_DIR=""    # set by detect_reflink_substrate to the reflink-capable dir
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
#   (ii)  LANE_TARGET is derived from a lane-scoped variable (its name
#         contains "LANE", e.g. LANE_DIR or LANE_ROOT), not hardcoded to a
#         base/shared path -- loosely matched (optional ${..} braces, any
#         *LANE*-named variable) so a behavior-preserving rename/refactor of
#         the upstream variable doesn't false-FAIL this guard; the exact
#         destination suffix "/target" is still required.
#   (iii) NO `ln -s`/`ln -sfn`/`ln -sf` line whose destination operand is
#         "$LANE_TARGET" or a literal .../target path (the shared-symlink
#         regression class this guard exists to catch) -- matched anywhere
#         on the line (no end-of-line anchor), so a trailing `|| true` or
#         redirection after the target operand can't hide the regression.
# Comment-only lines are stripped once up front so a doc comment mentioning
# these patterns can't produce a false PASS or false FLAG.
_target_reflink_ok() {
    local script="$1"
    [ -f "$script" ] || return 1

    local code
    code="$(grep -v '^[[:space:]]*#' "$script")"

    printf '%s\n' "$code" | grep -qE 'reflink=always.*"\$LANE_TARGET"' || return 1
    printf '%s\n' "$code" | grep -qE '^[[:space:]]*LANE_TARGET="\$\{?[A-Za-z_]*LANE[A-Za-z_]*\}?/target"' || return 1
    if printf '%s\n' "$code" | grep -qE '\bln[[:space:]]+-s[a-zA-Z]*[[:space:]].*("\$LANE_TARGET"|/target"?)'; then
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

# Non-vacuity: a stub seed script that performs a real reflink clone (so
# checks (i)/(ii) below both PASS in isolation) but ALSO shares the lane
# target via a symlink into the base (the exact regression this guard exists
# to catch) must still be FLAGGED as a violation. This isolates check (iii)
# -- the shared-symlink detector -- as the sole source of the FLAG: a stub
# missing the reflink line entirely would already fail at check (i), leaving
# check (iii)'s regex completely unexercised.
_STUB_DIR="$(mktemp -d)"
_TMPDIRS+=("$_STUB_DIR")
_STUB_SEED="$_STUB_DIR/stub-seed-warm-lane.sh"
cat > "$_STUB_SEED" <<'STUB_EOF'
#!/usr/bin/env bash
# Synthetic non-vacuity fixture: a "seed" script that performs a real
# reflink clone (satisfies checks (i)/(ii)) but ALSO shares the lane target
# via a symlink into the base (the regression check (iii) exists to catch).
set -euo pipefail
LANE_TARGET="$LANE_DIR/target"
cp -a --reflink=always "$BASE_TARGET_DIR/target" "$LANE_TARGET"
ln -sfn "$BASE_TARGET_DIR/target" "$LANE_TARGET"
STUB_EOF

assert "_target_reflink_ok: a stub with a real reflink clone PLUS a shared symlink is FLAGGED (isolates check iii)" \
    refute _target_reflink_ok "$_STUB_SEED"

# Positive isolation: a stub with the SAME reflink clone but NO shared
# symlink must PASS -- proving check (iii) doesn't spuriously flag a
# compliant script merely for containing a reflink-clone line.
_STUB_DIR_OK="$(mktemp -d)"
_TMPDIRS+=("$_STUB_DIR_OK")
_STUB_SEED_OK="$_STUB_DIR_OK/stub-seed-warm-lane-ok.sh"
cat > "$_STUB_SEED_OK" <<'STUB_EOF'
#!/usr/bin/env bash
# Synthetic non-vacuity fixture: a compliant "seed" script -- real reflink
# clone into a lane-local target, no shared symlink.
set -euo pipefail
LANE_TARGET="$LANE_DIR/target"
cp -a --reflink=always "$BASE_TARGET_DIR/target" "$LANE_TARGET"
STUB_EOF

assert "_target_reflink_ok: a compliant stub (reflink clone, no shared symlink) PASSES (isolates check iii's negative case)" \
    _target_reflink_ok "$_STUB_SEED_OK"

# ─────────────────────────────────────────────────────────────────────────────
# STATIC group (secondary): gc-worktree-targets.sh is the worktree-side vector
# named by the task -- confirm it rm's each per-worktree target/ independently
# and never symlinks a shared target. Comment-stripped before grepping (see
# _gc_code below), mirroring _target_reflink_ok's `code` var, so a doc comment
# mentioning these patterns can't produce a false PASS or false FLAG. The real
# script has always satisfied this, so there is no RED phase to manufacture
# here (see design decision in .task/plan.json).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- STATIC (secondary): gc-worktree-targets.sh rm's per-worktree target/ independently ---"

_gc_code="$(grep -v '^[[:space:]]*#' "$GC_SCRIPT")"

_gc_removes_target_independently() {
    printf '%s\n' "$_gc_code" | grep -qE 'rm -rf "\$target"'
}

_gc_has_shared_target_symlink() {
    printf '%s\n' "$_gc_code" | grep -qE '\bln[[:space:]]+-s[a-zA-Z]*\b.*target'
}

assert 'gc-worktree-targets.sh removes each worktree target/ independently (rm -rf "$target")' \
    _gc_removes_target_independently

assert "gc-worktree-targets.sh never symlinks a shared worktree target (no ln -s* .../target)" \
    refute _gc_has_shared_target_symlink

# ─────────────────────────────────────────────────────────────────────────────
# REGISTRATION group (always runs; self-guard): this test must be wired into
# the verify-pipeline artifact map (fail-fast pole for seed-warm-lane.sh
# edits) and the H1 run_all.sh classification manifest (declared-union
# coverage) -- otherwise this file existing at all drifts
# test_run_all_classification.sh's Test 5 (declared union vs. discovered set).
# Placed BEFORE the substrate-gated BEHAVIORAL block (below) so it always
# executes even when _skip() exits early for lack of a reflink FS.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- REGISTRATION: this guard is wired into the verify-pipeline artifact map + H1 manifest ---"

assert "verify-pipeline-infra-tests.txt maps scripts/seed-warm-lane.sh -> this test" \
    grep -qE '^scripts/seed-warm-lane\.sh[[:space:]]+tests/infra/test_target_per_lane_independence\.sh[[:space:]]*$' "$VP_MAP"

assert "run-all-classification.manifest declares this test in a valid bucket (pool|intra-run-serial|host-exclusive)" \
    grep -qE '^test_target_per_lane_independence\.sh[[:space:]]+(pool|intra-run-serial|host-exclusive)[[:space:]]*$' "$MANIFEST"

# _sentinel_propagates <src_lane_target> <other_dir> -- writes a uniquely
# named sentinel file into <src_lane_target>, then checks whether a file of
# the same basename is visible under <other_dir>. Returns 0 (propagates --
# the two dirs are actually the same underlying storage, e.g. via a symlink)
# if found, 1 (independent) otherwise. Cleans up the sentinel from <src>
# afterward (a single unlink removes it from both views when they alias).
_sentinel_propagates() {
    local src="$1" other="$2"
    local name="sentinel-$$-$RANDOM"
    : > "$src/$name"
    local rc=1
    [ -e "$other/$name" ] && rc=0
    rm -f "$src/$name" 2>/dev/null || true
    return "$rc"
}

# ─────────────────────────────────────────────────────────────────────────────
# BEHAVIORAL group (substrate-gated: SKIPs cleanly with no reflink FS): seed
# two lanes from a common base via the REAL seed-warm-lane.sh and assert a
# sentinel written into one lane's target/ never appears in the sibling
# lane's or the base's target/.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- BEHAVIORAL: write-independence across seeded lanes (substrate-gated) ---"

detect_reflink_substrate || _skip "no reflink substrate available (set REIFY_WARM_LANE_MOUNT, or use a reflink-capable TMPDIR) -- guard coverage narrows to STATIC+REGISTRATION only on this host"

_BEH_ROOT="$(mktemp -d "${_GATE_DIR}/target-lane-indep-beh-XXXXXX")"
_TMPDIRS+=("$_BEH_ROOT")

_BASE_WS="$_BEH_ROOT/base_ws"
_LANE_A="$_BEH_ROOT/lane_a"
_LANE_B="$_BEH_ROOT/lane_b"
_BASE_TARGET="$_BASE_WS/target"

mkdir -p "$_BASE_TARGET" "$_LANE_A" "$_LANE_B"
printf 'original-base-content\n' > "$_BASE_TARGET/base_file"

# Stamp the base sidecar with the CURRENT env so the seed calls' fail-closed
# RUSTFLAGS/INVOCATION guards pass regardless of ambient RUSTFLAGS (design
# decision: record-base + reset-in-place, see .task/plan.json).
#
# All three invocations run with REIFY_WARM_LANE_MOUNT unset. When
# detect_reflink_substrate falls through to the rung-2 TMPDIR scratch probe
# (e.g. a caller-supplied REIFY_WARM_LANE_MOUNT is set but its own reflink
# probe failed), _GATE_DIR resolves under TMPDIR while the ambient
# REIFY_WARM_LANE_MOUNT would stay exported -- and seed-warm-lane.sh's
# --fresh-checkout path refuses a LANE_TARGET outside that mount. This
# fixture only ever calls --reset-in-place, which does not enforce that
# guard today, but unsetting the var decouples the fixture from that
# mode-specific exemption holding forever (belt-and-suspenders) rather than
# risk a spurious hard failure under `set -e` if it's ever tightened.
env -u REIFY_WARM_LANE_MOUNT bash "$SEED_SCRIPT" --record-base "$_BASE_TARGET" >/dev/null

# Seed both lanes from the same base via the REAL script into cold empty
# mktemp lanes (--reset-in-place exercises ONLY the reflink clone, no
# bulk-stamp/git machinery).
env -u REIFY_WARM_LANE_MOUNT bash "$SEED_SCRIPT" "$_BASE_TARGET" "$_LANE_A" --reset-in-place >/dev/null
env -u REIFY_WARM_LANE_MOUNT bash "$SEED_SCRIPT" "$_BASE_TARGET" "$_LANE_B" --reset-in-place >/dev/null

_LANE_A_TARGET="$_LANE_A/target"
_LANE_B_TARGET="$_LANE_B/target"

# A sentinel written into lane A's target/ must never appear in lane B's
# target/ or the base's target/ -- via the not-yet-defined predicate
# _sentinel_propagates <src_lane_target> <other_dir>.
assert "a sentinel written into lane A's target/ does NOT propagate to lane B's target/ (independence)" \
    refute _sentinel_propagates "$_LANE_A_TARGET" "$_LANE_B_TARGET"

assert "a sentinel written into lane A's target/ does NOT propagate to the base's target/ (independence)" \
    refute _sentinel_propagates "$_LANE_A_TARGET" "$_BASE_TARGET"

# Divergence control: mutating a shared-extent file in lane A must NOT be
# observed in lane B or the base. This targets a DIFFERENT regression class
# than the sentinel-propagates asserts above: sentinel-propagates only
# proves a NEW file created in one lane is invisible elsewhere (catches
# directory-level aliasing, e.g. a symlinked or bind-mounted target/). It
# would NOT catch a clone step that hard-links (rather than reflinks)
# pre-existing files -- a plausible future "optimize --reset-in-place"
# regression that would leave lane target/ files sharing inodes with the
# base even though each lane's directory entry is independent. The STATIC
# grep above only confirms the `--reflink=always` string exists SOMEWHERE in
# the script, not that this exact --reset-in-place runtime path used it --
# this block instead exercises the REAL invocation dynamically. Each
# --reflink=always clone is an independent inode sharing extents only until
# written; overwriting one clone's content allocates new extents for THAT
# clone alone, whereas a hard-linked file would mutate the shared inode (and
# hence every lane sharing it) in place.
printf 'mutated-by-lane-a\n' > "$_LANE_A_TARGET/base_file"

assert "mutating a shared-extent file in lane A leaves lane B's copy unchanged (CoW divergence; catches hard-link-instead-of-reflink regressions sentinel-propagates cannot)" \
    bash -c "[ \"\$(cat '$_LANE_B_TARGET/base_file')\" = 'original-base-content' ]"

assert "mutating a shared-extent file in lane A leaves the base's copy unchanged (CoW divergence; catches hard-link-instead-of-reflink regressions sentinel-propagates cannot)" \
    bash -c "[ \"\$(cat '$_BASE_TARGET/base_file')\" = 'original-base-content' ]"

# Non-vacuity control: a symlink-shared lane target DOES observe the
# sentinel, proving _sentinel_propagates discriminates rather than being
# vacuously true.
_SHARED_DIR="$_BEH_ROOT/shared"
mkdir -p "$_SHARED_DIR"
ln -s "$(realpath "$_LANE_A_TARGET")" "$_SHARED_DIR/target"

assert "a symlink-shared target DOES observe the sentinel (non-vacuity control)" \
    _sentinel_propagates "$_LANE_A_TARGET" "$_SHARED_DIR/target"

test_summary
