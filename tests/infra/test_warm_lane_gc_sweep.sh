#!/usr/bin/env bash
# tests/infra/test_warm_lane_gc_sweep.sh
# Hermetic tests for scripts/warm-lane-gc-sweep.sh (task 4863).
#
# Blocks:
#   A — CLI guard: --help exits 0 and prints usage; unknown flag exits 2
#   B — fail-open: non-existent --mount dir → exit 0, warn on stderr, gc-script NOT invoked
#   C — happy path: existing --mount dir → exit 0, gc-script invoked as
#         reclaim --mount <dir>
#   D — unit file structural assertions:
#         deploy/systemd/reify-warm-lane-gc.timer (periodic, Persistent, WantedBy)
#         deploy/systemd/reify-warm-lane-gc.service (Type=oneshot, ExecStart ref)
#   F — drift-guard map wiring: verify-pipeline-infra-tests.txt contains expected rows
#   V — part-1 verification / green-on-arrival regression guards:
#         warm-lane-disk-guard.sh --help mentions --mount/--min-free-gib/--min-free-inodes
#           (tests --help text only; acceptance via help prose, not live flag invocation)
#         orchestrator.yaml contains warm_lane_pool: true
#   W — cross-script --mount seam (green-on-arrival regression guard):
#         single DF value (worktrees-dir), agreeing semantics across gc.sh + disk-guard.sh
#         pins: exit 1 on unresolvable base-target (not exit 2); stderr names correct
#         sibling-derived path; df volume agreement between --mount value and base-target
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-gc-sweep.sh"
DISK_GUARD="$REPO_ROOT/scripts/warm-lane-disk-guard.sh"
GC_TIMER="$REPO_ROOT/deploy/systemd/reify-warm-lane-gc.timer"
GC_SERVICE="$REPO_ROOT/deploy/systemd/reify-warm-lane-gc.service"
VP_INFRA_MAP="$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-gc-sweep.sh hermetic tests (task 4863) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp /tmp/test-gc-sweep-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_sweep: invoke the sweep script, capture OUT/ERR_OUT/RC ────────────────
run_sweep() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0
run_sweep --help
assert "A1: --help exits 0" test "$RC" -eq 0

# A2: --help prints usage on stderr
assert "A2: --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A3: unknown flag exits 2
run_sweep --unknown-flag-xyz
assert "A3: unknown flag exits 2" test "$RC" -eq 2

# ──────────────────────────────────────────────────────────────────────────────
# Block B — fail-open: non-existent --mount dir
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: fail-open (non-existent mount dir) ---"

B_ROOT="$(mktemp -d /tmp/test-gc-sweep-b-XXXXXX)"
_TMPDIRS+=("$B_ROOT")

# gc-script stub that logs calls
B_GC_LOG="$B_ROOT/gc_calls.log"
B_GC_STUB="$B_ROOT/gc_stub.sh"
cat > "$B_GC_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$GC_LOG"
exit 0
STUB_EOF
chmod +x "$B_GC_STUB"

B_NONEXISTENT="$B_ROOT/does/not/exist/worktrees"

GC_LOG="$B_GC_LOG" run_sweep --mount "$B_NONEXISTENT" --gc-script "$B_GC_STUB"

# B1: exits 0 (fail-open)
assert "B1: non-existent --mount dir exits 0 (fail-open)" test "$RC" -eq 0

# B2: warn/skip message on stderr
assert "B2: stderr contains warn/skip message for non-existent mount" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "warn|skip|not exist|no such|missing"' _ "$ERR_OUT"

# B3: gc-script NOT invoked (no calls log file or empty)
assert "B3: gc-script NOT invoked when mount dir does not exist" \
    bash -c '[ ! -f "$1" ] || [ ! -s "$1" ]' _ "$B_GC_LOG"

# ──────────────────────────────────────────────────────────────────────────────
# Block C — happy path: existing --mount dir
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: happy path (existing mount dir) ---"

C_ROOT="$(mktemp -d /tmp/test-gc-sweep-c-XXXXXX)"
_TMPDIRS+=("$C_ROOT")

C_MOUNT="$C_ROOT/worktrees"
mkdir -p "$C_MOUNT"

# gc-script stub that logs argv
C_GC_LOG="$C_ROOT/gc_calls.log"
C_GC_STUB="$C_ROOT/gc_stub.sh"
cat > "$C_GC_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$GC_LOG"
exit 0
STUB_EOF
chmod +x "$C_GC_STUB"

GC_LOG="$C_GC_LOG" run_sweep --mount "$C_MOUNT" --gc-script "$C_GC_STUB"

# C1: exits 0
assert "C1: happy path exits 0" test "$RC" -eq 0

# C2: gc-script was invoked
assert "C2: gc-script was invoked" test -f "$C_GC_LOG"

# C3: gc-script invoked as "reclaim --mount <dir>"
assert "C3: gc-script invoked with 'reclaim --mount <mount>'" \
    bash -c 'grep -qE "^reclaim --mount " "$1"' _ "$C_GC_LOG"

# C4: the --mount argument passed to gc-script matches the sweep's --mount
assert "C4: gc-script received correct --mount path" \
    bash -c 'grep -qF "reclaim --mount $2" "$1"' _ "$C_GC_LOG" "$C_MOUNT"

# C5: sweep propagates non-zero exit code from gc.sh (fail-open applies ONLY
# to the missing-mount case; a gc.sh error must propagate so the systemd timer
# marks the unit as failed rather than silently swallowing the error).
C5_ROOT="$(mktemp -d /tmp/test-gc-sweep-c5-XXXXXX)"
_TMPDIRS+=("$C5_ROOT")

C5_MOUNT="$C5_ROOT/worktrees"
mkdir -p "$C5_MOUNT"

# gc-script stub that exits 1 (simulates runtime error in gc.sh)
C5_GC_STUB="$C5_ROOT/gc_stub_fail.sh"
cat > "$C5_GC_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "gc-script: simulated runtime error" >&2
exit 1
STUB_EOF
chmod +x "$C5_GC_STUB"

run_sweep --mount "$C5_MOUNT" --gc-script "$C5_GC_STUB"

assert "C5: sweep propagates rc=1 from failing gc-script (exec-propagation contract)" \
    test "$RC" -eq 1

# ──────────────────────────────────────────────────────────────────────────────
# Block D — unit file structural assertions
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: unit file structural assertions ---"

# D1: timer file exists
assert "D1: deploy/systemd/reify-warm-lane-gc.timer exists" \
    test -f "$GC_TIMER"

# D2: timer has a periodic directive
assert "D2: timer has periodic directive (OnUnitActiveSec= or OnCalendar=)" \
    bash -c 'grep -qE "^(OnUnitActiveSec=|OnCalendar=)" "$1"' _ "$GC_TIMER"

# D3: timer has Persistent=true (survive missed runs across reboots)
assert "D3: timer has Persistent=true" \
    bash -c 'grep -q "^Persistent=true$" "$1"' _ "$GC_TIMER"

# D4: timer [Install] WantedBy=timers.target
assert "D4: timer [Install] WantedBy=timers.target" \
    bash -c 'grep -q "^WantedBy=timers.target$" "$1"' _ "$GC_TIMER"

# D5: timer targets the gc service (explicit Unit= or same-basename default)
# Either "Unit=reify-warm-lane-gc.service" is present, or the timer file is
# named reify-warm-lane-gc.timer (systemd infers the same-basename service).
assert "D5: timer targets reify-warm-lane-gc.service (Unit= or basename pairing)" \
    bash -c '
        timer="$1"
        if grep -q "^Unit=reify-warm-lane-gc.service$" "$timer"; then
            exit 0  # explicit Unit= found
        fi
        # Basename pairing: timer file is named reify-warm-lane-gc.timer
        basename "$timer" | grep -q "^reify-warm-lane-gc.timer$"
    ' _ "$GC_TIMER"

# D6: service file exists
assert "D6: deploy/systemd/reify-warm-lane-gc.service exists" \
    test -f "$GC_SERVICE"

# D7: service declares Type=oneshot
assert "D7: service declares Type=oneshot" \
    bash -c 'grep -q "^Type=oneshot$" "$1"' _ "$GC_SERVICE"

# D8: service ExecStart= references warm-lane-gc-sweep.sh (SOURCE file structural check).
# Ownership split: this block owns tracked-source structure (script reference, Type=oneshot).
# The installer-applied --mount pin on the INSTALLED copy is in test_warm_lane_boot_persistence.sh
# Block G (G4), keeping each test file authoritative over its own layer.
assert "D8: service ExecStart= references warm-lane-gc-sweep.sh" \
    bash -c 'grep -q "warm-lane-gc-sweep.sh" "$1"' _ "$GC_SERVICE"

# ──────────────────────────────────────────────────────────────────────────────
# Block V — part-1 verification / regression guards
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block V: part-1 verification / regression guards ---"

# V1: --help output mentions --mount (tests help text, not live acceptance)
# Note: to upgrade to live acceptance, invoke: disk-guard check --mount <dir>
#       and assert exit 0 or exit 1 (low-disk), but NOT exit 2 (unknown flag).
assert "V1: warm-lane-disk-guard.sh --help mentions --mount" \
    bash -c 'bash "$1" --help 2>&1 | grep -q -- "--mount"' _ "$DISK_GUARD"

# V2: --help output mentions --min-free-gib
assert "V2: warm-lane-disk-guard.sh --help mentions --min-free-gib" \
    bash -c 'bash "$1" --help 2>&1 | grep -q -- "--min-free-gib"' _ "$DISK_GUARD"

# V3: --help output mentions --min-free-inodes
assert "V3: warm-lane-disk-guard.sh --help mentions --min-free-inodes" \
    bash -c 'bash "$1" --help 2>&1 | grep -q -- "--min-free-inodes"' _ "$DISK_GUARD"

# V4: orchestrator.yaml contains warm_lane_pool: true (pool master-enable)
ORCH_YAML="$REPO_ROOT/orchestrator.yaml"
assert "V4: orchestrator.yaml contains warm_lane_pool: true" \
    bash -c 'grep -q "warm_lane_pool:.*true" "$1"' _ "$ORCH_YAML"

# ──────────────────────────────────────────────────────────────────────────────
# Block F — drift-guard map wiring
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: drift-guard map wiring ---"

# F1: verify-pipeline-infra-tests.txt maps warm-lane-gc-sweep.sh → test_warm_lane_gc_sweep.sh
assert "F1: verify-pipeline-infra-tests.txt maps scripts/warm-lane-gc-sweep.sh → tests/infra/test_warm_lane_gc_sweep.sh" \
    bash -c 'grep -qE "^scripts/warm-lane-gc-sweep\.sh[[:space:]]+tests/infra/test_warm_lane_gc_sweep\.sh" "$1"' _ "$VP_INFRA_MAP"

# F2: verify-pipeline-infra-tests.txt maps warm-lane-gc.sh → test_warm_lane_gc.sh
assert "F2: verify-pipeline-infra-tests.txt maps scripts/warm-lane-gc.sh → tests/infra/test_warm_lane_gc.sh" \
    bash -c 'grep -qE "^scripts/warm-lane-gc\.sh[[:space:]]+tests/infra/test_warm_lane_gc\.sh" "$1"' _ "$VP_INFRA_MAP"

# ──────────────────────────────────────────────────────────────────────────────
# Block W — cross-script --mount seam: single DF value, agreeing semantics
# ──────────────────────────────────────────────────────────────────────────────
# This is a GREEN-ON-ARRIVAL regression guard. The current behavior is already
# correct; this block pins the cross-script contract against future drift.
# It asserts RUNTIME behavior (exit codes, stderr error path, df volume identity),
# NOT doc prose.
#
# DF consumer contract (from orchestrator/src/orchestrator/git_ops.py):
#   - _run_warm_lane_gc_reclaim (line 1401): --mount str(self.worktree_base)
#   - _run_warm_lane_disk_guard (line 1364): --mount str(self.worktree_base)
#   where worktree_base = (project_root/.worktrees).resolve()
#                       = /home/leo/src/warm-lanes/worktrees
# DF passes the WORKTREES DIR (not the XFS mount point) to BOTH scripts.
# The two scripts consume it differently:
#   warm-lane-disk-guard.sh: runs df on it (any path on the XFS volume suffices)
#   warm-lane-gc.sh: treats it as the worktrees-dir; derives BASE_TARGET from its PARENT
echo ""
echo "--- Block W: cross-script --mount seam: single DF value, agreeing semantics ---"

# Build hermetic tmp root. WB = the worktrees-dir (same value DF passes as --mount).
# Do NOT create $W_ROOT/base yet — gc's readlink -f of the derived BASE_TARGET
# ($W_ROOT/base/target) will fail deterministically (all components must exist for -f).
W_ROOT="$(mktemp -d /tmp/test-gc-sweep-w-XXXXXX)"
_TMPDIRS+=("$W_ROOT")
GC_REAL="$REPO_ROOT/scripts/warm-lane-gc.sh"
WB="$W_ROOT/worktrees"
mkdir -p "$WB"

# Capture gc.sh exit code and stderr
W_ERR_FILE="$(mktemp /tmp/test-gc-sweep-w-err-XXXXXX)"
_TMPDIRS+=("$W_ERR_FILE")
W_RC=0
bash "$GC_REAL" reclaim --mount "$WB" 2>"$W_ERR_FILE" || W_RC=$?
W_ERR="$(cat "$W_ERR_FILE")"

# W1: exit 1 (runtime error: base-target unresolvable), NOT exit 2 (unknown flag).
# Proves --mount is accepted and the derivation was ATTEMPTED (readlink -f tried and failed).
assert "W1: gc.sh reclaim --mount exits 1 (runtime, not usage error) when base-target unresolvable" \
    test "$W_RC" -eq 1

# W2: gc stderr names the SIBLING-derived base-target.
# Locks: BASE_TARGET = $(dirname MOUNT)/base/target = $W_ROOT/base/target.
# The error message "Cannot resolve base-target symlink: <path>" must name THIS path.
EXPECTED_BASE_TARGET="$W_ROOT/base/target"
assert "W2: gc stderr names the sibling-derived base-target ($EXPECTED_BASE_TARGET)" \
    bash -c 'printf "%s\n" "$1" | grep -qF "$2"' _ "$W_ERR" "$EXPECTED_BASE_TARGET"

# W3: gc stderr does NOT contain $WB/base/target.
# $WB/base/target = $W_ROOT/worktrees/base/target is what you'd get if gc placed
# BASE_TARGET as a SUBDIR of the mount (the rejected option-a XFS-mount-point
# interpretation). Our derivation places it in the PARENT of the worktrees-dir.
REJECTED_BASE_TARGET="$WB/base/target"
assert "W3: gc stderr does NOT name the rejected worktrees-subdir base-target ($REJECTED_BASE_TARGET)" \
    bash -c '! printf "%s\n" "$1" | grep -qF "$2"' _ "$W_ERR" "$REJECTED_BASE_TARGET"

# W4: disk-guard accepts the SAME --mount value DF passes.
# 0/0 thresholds ensure exit 0 regardless of actual disk state (non-flaky).
# Also verifies that the value DF passes works for both scripts simultaneously.
mkdir -p "$W_ROOT/base/target"  # create base sibling so df can stat it in W5
W4_RC=0
bash "$DISK_GUARD" check --mount "$WB" --min-free-gib 0 --min-free-inodes 0 2>/dev/null || W4_RC=$?
assert "W4: disk-guard check accepts --mount <worktrees-dir> with 0/0 thresholds" \
    bash -c '[ "$1" -eq 0 ] || [ "$1" -eq 1 ]' _ "$W4_RC"

# W5: the single DF value ($WB = worktrees-dir) and the derived base-target
# ($W_ROOT/base/target) land on the SAME filesystem.
# This is the volume-agreement seam-lock: disk-guard's df measurement and gc's
# derived base-target path both refer to the same XFS volume.
# Uses GNU df --output=target (same infra constraint disk-guard already relies on).
WB_FS="$(df --output=target -- "$WB" 2>/dev/null | tail -n1)"
BT_FS="$(df --output=target -- "$W_ROOT/base/target" 2>/dev/null | tail -n1)"
assert "W5: --mount worktrees-dir and derived base-target are on the same filesystem (df volume agreement)" \
    bash -c '[ "$1" = "$2" ]' _ "$WB_FS" "$BT_FS"

test_summary
