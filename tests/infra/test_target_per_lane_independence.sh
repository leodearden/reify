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
# Three assertion groups, each carrying a non-vacuity self-check where
# applicable. Execution order matters: BEHAVIORAL is substrate-gated and
# _skip()s (exit 0, with a prominent WARN) when no reflink FS is available,
# so it runs LAST -- both always-run groups (STATIC, REGISTRATION) execute
# unconditionally before that possible early exit.
#   STATIC       (always runs) -- greps scripts/seed-warm-lane.sh and
#                scripts/gc-worktree-targets.sh for the ABSENCE of a
#                shared-symlink materialization of a lane/worktree target
#                (the regression class this guard exists to catch).
#                Deliberately does NOT pin a positive materialization
#                mechanism (e.g. an exact `--reflink=always` invocation or a
#                `rm -rf "$target"` spelling) -- that would false-FAIL a
#                behavior-preserving refactor without adding real coverage;
#                the actual write-independence property is exercised
#                dynamically by BEHAVIORAL below, which is authoritative.
#   REGISTRATION (always runs) -- a single presence check that this test is
#                wired into scripts/verify-pipeline-infra-tests.txt (the
#                fail-fast pole for seed-warm-lane.sh edits). Does NOT
#                re-assert this file's tests/infra/run-all-classification.manifest
#                entry: that presence is already fully enforced by
#                test_run_all_classification.sh's Test 5 (declared-union
#                across all buckets must equal the live discovered test_*.sh
#                set), so re-checking it here would be pure duplication.
#   BEHAVIORAL   (substrate-gated; SKIPs with a prominent WARN when no
#                reflink FS is available; runs LAST) -- seeds two lanes from
#                a common base via the REAL seed-warm-lane.sh and asserts a
#                sentinel written into one lane's target/ never appears in
#                the sibling lane's or the base's target/, and that CoW
#                divergence holds on a shared-extent file overwrite.
#
# COVERAGE CAVEAT: on a host with no reflink-capable substrate (no usable
# REIFY_WARM_LANE_MOUNT and no reflink-capable ${TMPDIR:-/tmp}), BEHAVIORAL
# SKIPs -- loudly: both stdout and stderr carry a WARN block, not just a
# quiet stderr line -- and this guard's live coverage narrows to STATIC's
# shared-symlink source greps plus REGISTRATION's single wiring check -- a
# regression that avoided a literal `ln -s...target` line while still
# breaking real write-independence (e.g. a bind mount, or a hard-link-based
# clone) would not be caught on such a host. Set REIFY_WARM_LANE_MOUNT (or
# run where ${TMPDIR:-/tmp} is reflink-capable, e.g. btrfs or XFS with
# reflink=1) for this guard to have full teeth.
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
# Note: no MANIFEST path here -- the REGISTRATION group below deliberately
# does not re-check tests/infra/run-all-classification.manifest; see its
# comment for why.
SEED_SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"
GC_SCRIPT="$REPO_ROOT/scripts/gc-worktree-targets.sh"
VP_MAP="$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt"

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

# _skip(reason) -- emit a PROMINENT warning (to both stdout, so it lands in
# the normal run summary/log, and stderr) that the BEHAVIORAL group did not
# run, call test_summary (counts so far), then exit 0. A quiet stderr-only
# SKIP line is easy to miss in CI output, making a partial-coverage green
# run indistinguishable from full coverage -- see the file-header COVERAGE
# CAVEAT. Still exits 0 (not a failure): skipping on a non-reflink host is a
# legitimate, expected outcome -- this just makes the degradation impossible
# to miss rather than silent.
_skip() {
    local reason="$*"
    local warn_block
    warn_block="$(cat <<EOF

################################################################
# WARN: BEHAVIORAL group SKIPPED -- $reason
# Real per-lane target/ write-independence was NOT exercised on
# this run; only STATIC (source-level) and REGISTRATION (wiring)
# checks ran. Set REIFY_WARM_LANE_MOUNT to a reflink-capable
# mount, or run where TMPDIR is reflink-capable (e.g. btrfs, or
# XFS with reflink=1), for full behavioral coverage.
################################################################
EOF
)"
    printf '%s\n' "$warn_block"
    printf '%s\n' "$warn_block" >&2
    test_summary
    echo "WARN: coverage was STATIC+REGISTRATION only this run (BEHAVIORAL SKIPPED -- see warning above)"
    exit 0
}

# Negative assertion helper (assert() only checks for success rc). Mirrors
# tests/infra/test_plan_capture_lib.sh's refute().
refute() { ! "$@"; }

# _target_no_shared_symlink <script> -- STATIC predicate: returns 0 iff
# <script> contains NO `ln -s`/`ln -sfn`/`ln -sf` line whose destination
# operand is "$LANE_TARGET" or a literal .../target path -- i.e. it never
# materializes a lane/worktree target via a symlink into a shared/base
# location (the regression class this guard exists to catch). Matched
# anywhere on the line (no end-of-line anchor), so a trailing `|| true` or
# redirection after the target operand can't hide the regression.
#
# Deliberately negative-only: this does NOT require any particular positive
# materialization mechanism (e.g. an exact `cp --reflink=always` invocation
# or a `LANE_TARGET=` assignment spelling). Pinning that cosmetic detail
# would false-FAIL a behavior-preserving refactor (a rename, or switching
# clone strategy) without adding real regression coverage -- the actual
# write-independence property is exercised dynamically by the BEHAVIORAL
# group below, which is the authoritative check. Comment-only lines are
# stripped once up front so a doc comment mentioning these patterns can't
# produce a false FLAG.
_target_no_shared_symlink() {
    local script="$1"
    [ -f "$script" ] || return 1

    local code
    code="$(grep -v '^[[:space:]]*#' "$script")"

    if printf '%s\n' "$code" | grep -qE '\bln[[:space:]]+-s[a-zA-Z]*[[:space:]].*("\$LANE_TARGET"|/target"?)'; then
        return 1
    fi

    return 0
}

# ─────────────────────────────────────────────────────────────────────────────
# STATIC group (always runs, no substrate needed): the real seed-warm-lane.sh
# never materializes a lane target via a symlink into a shared/base
# location. _target_no_shared_symlink() is defined above.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- STATIC: seed-warm-lane.sh never symlinks a shared/base target into a lane ---"

assert "_target_no_shared_symlink: real seed-warm-lane.sh does not symlink a shared/base target (green on truth)" \
    _target_no_shared_symlink "$SEED_SCRIPT"

# Non-vacuity (negative case): a stub seed script that performs a real
# reflink clone but ALSO shares the lane target via a symlink into the base
# (the exact regression this guard exists to catch) must still be FLAGGED.
_STUB_DIR="$(mktemp -d)"
_TMPDIRS+=("$_STUB_DIR")
_STUB_SEED="$_STUB_DIR/stub-seed-warm-lane.sh"
cat > "$_STUB_SEED" <<'STUB_EOF'
#!/usr/bin/env bash
# Synthetic non-vacuity fixture: a "seed" script that performs a real
# reflink clone but ALSO shares the lane target via a symlink into the base
# (the regression this guard exists to catch).
set -euo pipefail
LANE_TARGET="$LANE_DIR/target"
cp -a --reflink=always "$BASE_TARGET_DIR/target" "$LANE_TARGET"
ln -sfn "$BASE_TARGET_DIR/target" "$LANE_TARGET"
STUB_EOF

assert "_target_no_shared_symlink: a stub that symlinks its lane target into the base is FLAGGED" \
    refute _target_no_shared_symlink "$_STUB_SEED"

# Non-vacuity (positive case): a stub with the SAME reflink clone but NO
# shared symlink must PASS -- proving the check doesn't spuriously flag a
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

assert "_target_no_shared_symlink: a compliant stub (no shared symlink) PASSES" \
    _target_no_shared_symlink "$_STUB_SEED_OK"

# ─────────────────────────────────────────────────────────────────────────────
# STATIC group (secondary): gc-worktree-targets.sh is the worktree-side vector
# named by the task -- confirm it never symlinks a shared target across
# worktrees. Comment-stripped before grepping (see _gc_code below), mirroring
# _target_no_shared_symlink's `code` var, so a doc comment mentioning these
# patterns can't produce a false FLAG. Deliberately NOT pinning the positive
# removal mechanism (e.g. the exact `rm -rf "$target"` spelling) -- same
# rationale as _target_no_shared_symlink above: a behavior-preserving `rm -r`
# or renamed variable would false-FAIL a positive pin without adding real
# coverage.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- STATIC (secondary): gc-worktree-targets.sh never symlinks a shared worktree target ---"

_gc_code="$(grep -v '^[[:space:]]*#' "$GC_SCRIPT")"

_gc_has_shared_target_symlink() {
    printf '%s\n' "$_gc_code" | grep -qE '\bln[[:space:]]+-s[a-zA-Z]*\b.*target'
}

assert "gc-worktree-targets.sh never symlinks a shared worktree target (no ln -s* .../target)" \
    refute _gc_has_shared_target_symlink

# ─────────────────────────────────────────────────────────────────────────────
# REGISTRATION group (always runs): a single presence check that this test is
# wired into the verify-pipeline artifact map (fail-fast pole for
# seed-warm-lane.sh edits). Deliberately does NOT also re-assert this file's
# tests/infra/run-all-classification.manifest bucket entry here: that
# presence is ALREADY fully enforced by test_run_all_classification.sh's
# Test 5 (the declared union across all buckets must equal the live
# discovered test_*.sh set) -- this file existing without a manifest entry
# would independently fail that Test 5, so re-checking it here would be pure
# duplication with no additional coverage. The verify-pipeline-infra-tests.txt
# mapping has no equivalent centralized completeness check, so it keeps a
# local guard. Placed BEFORE the substrate-gated BEHAVIORAL block (below) so
# it always executes even when _skip() exits early for lack of a reflink FS.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- REGISTRATION: this guard is wired into the verify-pipeline artifact map ---"

assert "verify-pipeline-infra-tests.txt maps scripts/seed-warm-lane.sh -> this test" \
    grep -qE '^scripts/seed-warm-lane\.sh[[:space:]]+tests/infra/test_target_per_lane_independence\.sh[[:space:]]*$' "$VP_MAP"

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

detect_reflink_substrate || _skip "no reflink-capable substrate available (no usable REIFY_WARM_LANE_MOUNT, and TMPDIR does not support --reflink=always)"

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
# base even though each lane's directory entry is independent. STATIC above
# does NOT grep for a `--reflink=always` string at all (it is negative-only
# -- see _target_no_shared_symlink's comment), so this dynamic check is the
# ONLY thing in this guard that would catch a hard-link-based regression;
# this block exercises the REAL invocation. Each --reflink=always clone is
# an independent inode sharing extents only until written; overwriting one
# clone's content allocates new extents for THAT clone alone, whereas a
# hard-linked file would mutate the shared inode (and hence every lane
# sharing it) in place.
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
