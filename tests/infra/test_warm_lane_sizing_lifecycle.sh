#!/usr/bin/env bash
# tests/infra/test_warm_lane_sizing_lifecycle.sh
# END-TO-END INTEGRATION GATE for the warm-lane pool sizing lifecycle (task
# 5176, PRD docs/prds/warm-lane-pool-sizing-lifecycle.md §9/§10 — task ζ).
#
# Proves the four landed pillars COMPOSE, not just that each works alone:
#   Block A — integration-surface precondition: the actively-driven
#             primitives exist and are executable.
#   Block B (B8)     — release 2 of 3 divergent lanes; thin-on-release (δ)
#             frees their targets while the still-ASSIGNED lane is refused;
#             the audit (α) HEADROOM line reflects the composition (assigned
#             3->1, free 0->2, reclaimable 0->2). Records the MEASURED
#             free-recovered-on-release delta.
#   Block C          — β's §9.2 ADVISORY budget relation: audit HEADROOM
#             divergent_gib <= budget_gib, budget derived from a stubbed df
#             avail + --safety (not the online-grow op, which is
#             host-exclusive/root-only and covered by
#             test_provision_warm_lane_fs.sh).
#   Block D (B6/B6b) — the REAL warm-lane-disk-guard.sh driven across a
#             descending-avail (stubbed df) sequence: soft pressure (exit 3 +
#             sentinel) fires at a strictly higher avail than the hard floor
#             (exit 75); a df fail/garbage fail-closes to 75 with no
#             sentinel, even under --soft.
#
# Bucket: pool (fully hermetic -- mktemp fixtures, stubbed df, real rm/flock
# scoped to fixtures; never touches the real host warm-lane mount, cargo, or
# a reflink FS).
#
# Consumed primitives (all landed; this task does not modify them):
#   scripts/thin-warm-lane.sh, scripts/warm-lane-audit.sh,
#   scripts/warm-lane-disk-guard.sh -- asserted present/executable in Block A.
#   scripts/provision-warm-lane-fs.sh (β) is consumed indirectly, via the
#   audit's budget FORMULA in Block C -- its own --grow op is host-exclusive/
#   root-only and is covered by test_provision_warm_lane_fs.sh, not here.
#
# Per-primitive run helpers capture STDOUT, STDERR, and RC separately:
#   THIN_OUT/THIN_ERR_OUT/THIN_RC     — scripts/thin-warm-lane.sh
#   AUDIT_OUT/AUDIT_ERR_OUT/AUDIT_RC  — scripts/warm-lane-audit.sh
#   GUARD_OUT/GUARD_ERR_OUT/GUARD_RC  — scripts/warm-lane-disk-guard.sh
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
THIN_SCRIPT="$REPO_ROOT/scripts/thin-warm-lane.sh"
AUDIT_SCRIPT="$REPO_ROOT/scripts/warm-lane-audit.sh"
GUARD_SCRIPT="$REPO_ROOT/scripts/warm-lane-disk-guard.sh"
PROVISION_SCRIPT="$REPO_ROOT/scripts/provision-warm-lane-fs.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== warm-lane sizing lifecycle END-TO-END INTEGRATION GATE (task 5176) ==="
echo "(β/provision-warm-lane-fs.sh consumed via the Block C budget formula, not invoked directly: $PROVISION_SCRIPT)"

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
_BGPIDS=()
cleanup() {
    for pid in "${_BGPIDS[@]+${_BGPIDS[@]}}"; do
        kill "$pid" 2>/dev/null || true
    done
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# ──────────────────────────────────────────────────────────────────────────────
# Shared fixture scaffolding
# ──────────────────────────────────────────────────────────────────────────────

# make_lane DIR [BRANCH]
# Creates a minimal standalone git repo at DIR (one initial commit on main).
# BRANCH: task/NNNN -> checkout a new branch; DETACH -> detach HEAD;
# main/"" -> stay on main. Each lane is its OWN independent repo (not a
# linked worktree of a shared primary) -- sufficient for warm-lane-audit.sh's
# per-lane git predicates (merge-base/status/symbolic-ref all operate within
# a single lane's own repo). Verbatim copy of
# tests/infra/test_warm_lane_audit.sh's identically-named helper.
make_lane() {
    local dir="$1" branch="${2:-}"
    git init -q -b main "$dir"
    git -C "$dir" config user.email "test@test.local"
    git -C "$dir" config user.name "Test"
    touch "$dir/README.md"
    git -C "$dir" add README.md
    git -C "$dir" commit -q -m "initial"
    case "$branch" in
        task/*)
            git -C "$dir" checkout -q -b "$branch" ;;
        DETACH)
            git -C "$dir" checkout -q --detach ;;
        main|"")
            : ;;
    esac
}

# _wait_for_reader_lock <ready-marker> <deadline-seconds>
# Causal ordering (technique R, docs/prds/infra-test-wallclock-deflake.md,
# task #4847): polls for the READY marker file in 0.05s ticks, returning 0 as
# soon as it appears, or non-zero once the anti-hang deadline elapses. The
# READY marker is touched by a backgrounded lock holder AFTER it acquires its
# flock, so returning 0 causally guarantees the flock is held at the caller's
# next statement -- replacing a fixed `sleep` that races the background
# subshell's lock acquisition under load. This bounded tick ceiling is an
# anti-hang safeguard, not an assert -- it introduces no wallclock upper
# bound (DD5). Verbatim copy of tests/infra/test_warm_lane_audit.sh's
# identically-named helper (itself mirroring test_warm_lane_gc.sh).
_wait_for_reader_lock() {
    local ready_marker="$1"
    local deadline_s="$2"
    local max_ticks=$(( deadline_s * 20 ))
    local tick=0
    while [ "$tick" -lt "$max_ticks" ]; do
        [ -f "$ready_marker" ] && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
    return 1
}

# _release_holder_body <ready_marker> <release_marker>
# Body of a RELEASE-marker flock holder. Callers background it as
# `( _release_holder_body READY RELEASE ) 9>lock &` (fd 9 supplied by the
# CALLER's redirect, so the flock is acquired directly by that backgrounded
# subshell -- a DIRECT child of this script, so `wait $!` on it later works).
# Once the exclusive flock is acquired, touches <ready_marker> (the causal
# handshake _wait_for_reader_lock polls for), then polls for <release_marker>
# in bounded 0.05s ticks and returns as soon as it appears -- closing fd 9
# (and thus dropping the flock) the instant the subshell exits.
#
# NOT `sleep N` + `kill`: an orphaned `sleep` child inherits fd 9, so killing
# only the parent subshell can leave fd 9 (and the flock) held after the
# "release", flaking a subsequent thin-warm-lane.sh call to a spurious T3
# (exit 75) refusal. `touch RELEASE; wait $pid` instead causally guarantees
# the flock is dropped before the caller's next statement runs. The bounded
# tick ceiling is an anti-hang safeguard, not an assert -- no wallclock upper
# bound (DD5).
_release_holder_body() {
    local ready_marker="$1"
    local release_marker="$2"
    flock -x 9
    touch "$ready_marker"
    local tick=0
    while [ "$tick" -lt 600 ]; do
        [ -f "$release_marker" ] && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
}

# _pool_target_bytes <mount>
# Sums `du -sB1` over every EXISTING <mount>/_lane-*/target dir (0 bytes for
# a lane whose target/ has already been thinned/never existed). Used to
# measure the pool-wide divergent footprint independent of the audit's own
# (GiB-floored) divergent_gib figure -- the B8 freed-bytes delta is measured
# directly, never inferred from a floored/frozen constant (G6/D8).
_pool_target_bytes() {
    local mount="$1"
    local total=0
    local d bytes du_out
    for d in "$mount"/_lane-*/target; do
        [ -d "$d" ] || continue
        du_out="$(du -sB1 "$d" 2>/dev/null)" || continue
        bytes="$(printf '%s\n' "$du_out" | cut -f1)"
        case "$bytes" in
            ''|*[!0-9]*) continue ;;
        esac
        total=$(( total + bytes ))
    done
    printf '%d' "$total"
}

# _headroom_field <audit_stdout> <field_name>
# Extracts <field_name>=<int> from the "^HEADROOM ..." line of a table-format
# warm-lane-audit.sh run. Every field is preceded by whitespace (including
# the first, which follows the literal "HEADROOM" token), so a leading
# [[:space:]] anchor avoids any accidental substring collision (e.g.
# "free=" never matching inside "free_gib=").
_headroom_field() {
    local out="$1" field="$2"
    printf '%s\n' "$out" | grep '^HEADROOM' | grep -oE "[[:space:]]${field}=[0-9]+" | head -1 | cut -d= -f2
}

# _write_audit_df_stub <path> <avail_bytes>
# Writes a 1-col df stub (mimics `df -B1 --output=avail`) at <path> that
# unconditionally emits the given avail byte count. Mirrors
# tests/infra/test_warm_lane_audit.sh Block G's stub shape byte-for-byte
# (baked value, not env-driven -- Block C only ever needs one avail figure
# per stub instance). Wired via REIFY_WARM_LANE_AUDIT_DF.
_write_audit_df_stub() {
    local path="$1" avail_bytes="$2"
    cat > "$path" << EOF
#!/usr/bin/env bash
printf '     Avail\n'
printf '%s\n' "$avail_bytes"
EOF
    chmod +x "$path"
}

# _write_guard_df_stub <path>
# Writes a 2-col df stub (mimics `df -B1 --output=avail,iavail`) at <path>,
# controlled entirely by env vars at INVOCATION time
# (REIFY_TEST_AVAIL_BYTES/REIFY_TEST_AVAIL_INODES/REIFY_TEST_DF_FAIL/
# REIFY_TEST_DF_GARBAGE) -- one stub script serves every regime driven in
# Block D. Byte-for-byte mirrors tests/infra/test_warm_lane_disk_guard.sh's
# stub. Wired via REIFY_WARM_LANE_DISK_GUARD_DF.
_write_guard_df_stub() {
    local path="$1"
    cat > "$path" << 'STUB_EOF'
#!/usr/bin/env bash
if [ "${REIFY_TEST_DF_FAIL:-}" = "1" ]; then
    echo "df: error: permission denied" >&2
    exit 1
fi
if [ "${REIFY_TEST_DF_GARBAGE:-}" = "1" ]; then
    printf '      Avail      IFree\n'
    printf 'not-an-integer not-an-integer\n'
    exit 0
fi
printf '      Avail      IFree\n'
printf ' %s %s\n' \
    "${REIFY_TEST_AVAIL_BYTES:-107374182400}" \
    "${REIFY_TEST_AVAIL_INODES:-1000000}"
STUB_EOF
    chmod +x "$path"
}

# ── per-primitive run helpers: capture OUT/ERR_OUT/RC separately ─────────────
THIN_ERR_FILE="$(mktemp /tmp/test-warm-lane-sizing-thin-err-XXXXXX)"
_TMPDIRS+=("$THIN_ERR_FILE")
AUDIT_ERR_FILE="$(mktemp /tmp/test-warm-lane-sizing-audit-err-XXXXXX)"
_TMPDIRS+=("$AUDIT_ERR_FILE")
GUARD_ERR_FILE="$(mktemp /tmp/test-warm-lane-sizing-guard-err-XXXXXX)"
_TMPDIRS+=("$GUARD_ERR_FILE")

# run_thin <mount> <lane_dir> [thin-warm-lane.sh flags...]
# Invokes thin-warm-lane.sh with REIFY_WARM_LANE_MOUNT=<mount> (so its
# under-mount guard is exercised for real, not skipped), capturing
# THIN_OUT/THIN_ERR_OUT/THIN_RC.
run_thin() {
    local mount="$1"; shift
    local rc=0
    > "$THIN_ERR_FILE"
    THIN_OUT="$(REIFY_WARM_LANE_MOUNT="$mount" bash "$THIN_SCRIPT" "$@" 2>"$THIN_ERR_FILE")" || rc=$?
    THIN_ERR_OUT="$(cat "$THIN_ERR_FILE")"
    THIN_RC=$rc
}

# run_audit <warm-lane-audit.sh args...>
# Captures AUDIT_OUT/AUDIT_ERR_OUT/AUDIT_RC. Callers may prefix inline env
# vars (e.g. REIFY_WARM_LANE_AUDIT_DF=...) before the call.
run_audit() {
    local rc=0
    > "$AUDIT_ERR_FILE"
    AUDIT_OUT="$(bash "$AUDIT_SCRIPT" "$@" 2>"$AUDIT_ERR_FILE")" || rc=$?
    AUDIT_ERR_OUT="$(cat "$AUDIT_ERR_FILE")"
    AUDIT_RC=$rc
}

# run_guard <warm-lane-disk-guard.sh args...>
# Captures GUARD_OUT/GUARD_ERR_OUT/GUARD_RC. Callers prefix inline env vars
# (REIFY_WARM_LANE_DISK_GUARD_DF=... plus its REIFY_TEST_* controls).
run_guard() {
    local rc=0
    > "$GUARD_ERR_FILE"
    GUARD_OUT="$(bash "$GUARD_SCRIPT" "$@" 2>"$GUARD_ERR_FILE")" || rc=$?
    GUARD_ERR_OUT="$(cat "$GUARD_ERR_FILE")"
    GUARD_RC=$rc
}

# Always-unknown status oracle stub: a hermetic, neutral --status-cmd for
# audit runs whose classification does not depend on backing-task status
# (e.g. Block B's lanes are recoverable=LANDED via no-ahead-commit, which
# short-circuits the RECLAIMABLE classification regardless of status).
# Prevents an ambient REIFY_LANE_LEAK_STATUS_CMD from perturbing this
# hermetic gate's output.
NULL_STATUS_DIR="$(mktemp -d /tmp/test-warm-lane-sizing-nullstatus-XXXXXX)"
_TMPDIRS+=("$NULL_STATUS_DIR")
NULL_STATUS_CMD="$NULL_STATUS_DIR/null-status.sh"
cat > "$NULL_STATUS_CMD" << 'STUB_EOF'
#!/usr/bin/env bash
exit 0
STUB_EOF
chmod +x "$NULL_STATUS_CMD"

# ──────────────────────────────────────────────────────────────────────────────
# Block A — integration-surface precondition
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: integration-surface precondition ---"

# The three ACTIVELY-DRIVEN primitives (thin/audit/guard are invoked for real
# in Blocks B-D below) must exist and be executable, or this integration gate
# cannot compose what is absent. β/provision-warm-lane-fs.sh is integrated via
# the audit's budget relation in Block C, not a grow op here, so it is kept
# out of this minimal precondition set (see header comment).
assert "A1: scripts/thin-warm-lane.sh exists" test -f "$THIN_SCRIPT"
assert "A2: scripts/thin-warm-lane.sh is executable" test -x "$THIN_SCRIPT"
assert "A3: scripts/warm-lane-audit.sh exists" test -f "$AUDIT_SCRIPT"
assert "A4: scripts/warm-lane-audit.sh is executable" test -x "$AUDIT_SCRIPT"
assert "A5: scripts/warm-lane-disk-guard.sh exists" test -f "$GUARD_SCRIPT"
assert "A6: scripts/warm-lane-disk-guard.sh is executable" test -x "$GUARD_SCRIPT"

test_summary
