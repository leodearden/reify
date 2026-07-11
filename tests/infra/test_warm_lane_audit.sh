#!/usr/bin/env bash
# tests/infra/test_warm_lane_audit.sh
# Hermetic tests for scripts/warm-lane-audit.sh.
#
# PRD: docs/prds/warm-lane-pool-sizing-lifecycle.md §9.1 (task α); boundary
# tests B1/B2 (§10). Reuses the task-4749 status-oracle seam
# (REIFY_LANE_LEAK_STATUS_CMD) and the <lane_dir>.lock flock convention from
# scripts/warm-lane-gc.sh / tests/infra/test_warm_lane_gc.sh.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag
#   (additional blocks land in subsequent commits as the script grows: the
#    ASSIGNED/FREE probe + LIVE + headroom, backing-task-status +
#    RECLAIMABLE-via-terminal, recoverable LANDED/PUSHED/ORPHAN,
#    residue-only-dirty, LEAKED + stale, measured disk figures, --format
#    json, the combined B1 fixture + A3 degradation, and B2 read-only.)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-audit.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-audit.sh hermetic tests (task 5172) ==="

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

ERR_FILE="$(mktemp /tmp/test-warm-lane-audit-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes warm-lane-audit.sh, capturing OUT (stdout), ERR_OUT (stderr), RC.
run_helper() {
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

# A1: --help exits 0 and prints usage on stderr
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits 2
run_helper --nope
assert "A2: unknown flag exits 2" test "$RC" -eq 2

# ──────────────────────────────────────────────────────────────────────────────
# Shared fixture scaffolding: make_lane + _wait_for_reader_lock
# ──────────────────────────────────────────────────────────────────────────────

# make_lane DIR [BRANCH]
# Creates a minimal standalone git repo at DIR (one initial commit on main).
# BRANCH: task/NNNN -> checkout a new branch; DETACH -> detach HEAD;
# main/"" -> stay on main. Mirrors tests/infra/test_warm_lane_preflight.sh's
# identically-named helper (Block D) — each lane is its OWN independent repo
# (not a linked worktree of a shared primary), which is sufficient for
# warm-lane-audit.sh's per-lane git predicates (merge-base/status/symbolic-ref
# all operate within a single lane's own repo).
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
# subshell's lock acquisition under load. Mirrors tests/infra/test_warm_lane_gc.sh's
# identically-named helper.
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

# ──────────────────────────────────────────────────────────────────────────────
# Block B — ASSIGNED/FREE probe + LIVE + headroom counts
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: ASSIGNED/FREE probe + LIVE + headroom counts ---"

B_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-b-XXXXXX)"
_TMPDIRS+=("$B_MOUNT")

# _lane-live: a live consumer holds the lane's exclusive flock (ASSIGNED -> LIVE).
# Causal READY-marker handshake instead of a fixed sleep (de-flake convention,
# tests/infra/test_warm_lane_gc.sh Block I6 precedent).
make_lane "$B_MOUNT/_lane-live"
touch "$B_MOUNT/_lane-live.lock"
B_READY="$B_MOUNT/_lane-live.lock.ready-marker"
( flock -x 9 && touch "$B_READY" && sleep 300 ) 9>"$B_MOUNT/_lane-live.lock" &
B_LOCK_PID=$!
_BGPIDS+=("$B_LOCK_PID")
_wait_for_reader_lock "$B_READY" 30

# _lane-free: unheld, pre-created lock file (FREE).
make_lane "$B_MOUNT/_lane-free"
touch "$B_MOUNT/_lane-free.lock"

run_helper --mount "$B_MOUNT"

assert "B1: exit 0" test "$RC" -eq 0
assert "B2: _lane-live row shows ASSIGNED" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-live .*assigned=ASSIGNED"' _ "$OUT"
assert "B3: _lane-live classification LIVE" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-live .*classification=LIVE"' _ "$OUT"
assert "B4: _lane-free row shows FREE" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-free .*assigned=FREE"' _ "$OUT"
assert "B5: HEADROOM line shows resident=2" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "resident=2"' _ "$OUT"
assert "B6: HEADROOM line shows assigned=1" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "assigned=1"' _ "$OUT"
assert "B7: HEADROOM line shows free=1" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "free=1"' _ "$OUT"

# Release the lock
kill "$B_LOCK_PID" 2>/dev/null || true
_BGPIDS=()  # clear so cleanup doesn't double-kill

# ──────────────────────────────────────────────────────────────────────────────
# Shared scaffolding: status-oracle stub + ahead-of-main commit helper
# ──────────────────────────────────────────────────────────────────────────────

# _add_ahead_commit DIR — add a committed change NOT reachable from main.
_add_ahead_commit() {
    local dir="$1"
    echo "ahead" >> "$dir/README.md"
    git -C "$dir" add README.md
    git -C "$dir" commit -q -m "ahead-of-main commit"
}

# leak-oracle.sh: given task-id $1, looks up status in ORACLE_MAP file (one
# "id status" pair per line). Exits 0 with empty output for unknown ids.
# Mirrors tests/infra/test_warm_lane_preflight.sh Block D / test_warm_lane_gc.sh
# byte-for-byte (the shared task-4749 status-oracle contract).
ORACLE_STUB_DIR="$(mktemp -d /tmp/test-warm-lane-audit-oracle-stub-XXXXXX)"
_TMPDIRS+=("$ORACLE_STUB_DIR")
cat > "$ORACLE_STUB_DIR/leak-oracle.sh" << 'STUB_EOF'
#!/usr/bin/env bash
_qid="$1"
if [ -f "${ORACLE_MAP:-}" ]; then
    while IFS=' ' read -r _mid _mst; do
        if [ "$_mid" = "$_qid" ]; then
            printf '%s\n' "$_mst"
            exit 0
        fi
    done < "$ORACLE_MAP"
fi
exit 0
STUB_EOF
chmod +x "$ORACLE_STUB_DIR/leak-oracle.sh"

# leak-oracle-fail.sh: always exits non-zero -- drives the A3 hardening test
# (oracle failure must NOT abort; unknown = neither terminal nor non-terminal).
cat > "$ORACLE_STUB_DIR/leak-oracle-fail.sh" << 'STUB_EOF'
#!/usr/bin/env bash
exit 1
STUB_EOF
chmod +x "$ORACLE_STUB_DIR/leak-oracle-fail.sh"

# ──────────────────────────────────────────────────────────────────────────────
# Block C — backing-task-status via the 4749 seam + terminal -> RECLAIMABLE
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: backing-task-status + terminal -> RECLAIMABLE ---"

C_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-c-XXXXXX)"
_TMPDIRS+=("$C_MOUNT")

C_MAP="$(mktemp /tmp/test-warm-lane-audit-c-map-XXXXXX)"
_TMPDIRS+=("$C_MAP")
printf '100 done\n200 pending\n' > "$C_MAP"

# _lane-done: FREE, task/100, oracle=done, ahead-of-main.
make_lane "$C_MOUNT/_lane-done" "task/100"
_add_ahead_commit "$C_MOUNT/_lane-done"

# _lane-pending: FREE, task/200, oracle=pending, ahead-of-main.
make_lane "$C_MOUNT/_lane-pending" "task/200"
_add_ahead_commit "$C_MOUNT/_lane-pending"

ORACLE_MAP="$C_MAP" run_helper --mount "$C_MOUNT" --status-cmd "$ORACLE_STUB_DIR/leak-oracle.sh"

assert "C1: exit 0" test "$RC" -eq 0
assert "C2: _lane-done backing-task-status=terminal" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-done .*status=terminal"' _ "$OUT"
assert "C3: _lane-done classification RECLAIMABLE" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-done .*classification=RECLAIMABLE"' _ "$OUT"
assert "C4: HEADROOM line shows reclaimable=1" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "reclaimable=1"' _ "$OUT"
assert "C5: _lane-pending backing-task-status=non-terminal" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-pending .*status=non-terminal"' _ "$OUT"
assert "C6: _lane-pending classification PRESERVED-OK (not reclaimable-via-terminal)" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-pending .*classification=PRESERVED-OK"' _ "$OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block D — recoverable LANDED|PUSHED|ORPHAN + recoverable -> RECLAIMABLE
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: recoverable LANDED|PUSHED|ORPHAN ---"

# D(a): HEAD is an ancestor of main (no ahead commit) -> LANDED -> RECLAIMABLE.
DA_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-da-XXXXXX)"
_TMPDIRS+=("$DA_MOUNT")
DA_MAP="$(mktemp /tmp/test-warm-lane-audit-da-map-XXXXXX)"
_TMPDIRS+=("$DA_MAP")
printf '300 pending\n' > "$DA_MAP"
make_lane "$DA_MOUNT/_lane-landed" "task/300"

ORACLE_MAP="$DA_MAP" run_helper --mount "$DA_MOUNT" --status-cmd "$ORACLE_STUB_DIR/leak-oracle.sh"
assert "D1: exit 0" test "$RC" -eq 0
assert "D2: _lane-landed recoverable=LANDED" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-landed .*recoverable=LANDED"' _ "$OUT"
assert "D3: _lane-landed classification RECLAIMABLE" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-landed .*classification=RECLAIMABLE"' _ "$OUT"

# D(b): pushed to a bare origin remote (origin/<branch> contains HEAD), ahead
# of local main -> PUSHED -> RECLAIMABLE.
DB_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-db-XXXXXX)"
_TMPDIRS+=("$DB_MOUNT")
DB_MAP="$(mktemp /tmp/test-warm-lane-audit-db-map-XXXXXX)"
_TMPDIRS+=("$DB_MAP")
printf '400 pending\n' > "$DB_MAP"
git init -q --bare "$DB_MOUNT/origin.git"
make_lane "$DB_MOUNT/_lane-pushed" "task/400"
_add_ahead_commit "$DB_MOUNT/_lane-pushed"
git -C "$DB_MOUNT/_lane-pushed" remote add origin "$DB_MOUNT/origin.git"
git -C "$DB_MOUNT/_lane-pushed" push -q origin task/400

ORACLE_MAP="$DB_MAP" run_helper --mount "$DB_MOUNT" --status-cmd "$ORACLE_STUB_DIR/leak-oracle.sh"
assert "D4: exit 0" test "$RC" -eq 0
assert "D5: _lane-pushed recoverable=PUSHED" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-pushed .*recoverable=PUSHED"' _ "$OUT"
assert "D6: _lane-pushed classification RECLAIMABLE" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-pushed .*classification=RECLAIMABLE"' _ "$OUT"

# D(c): ahead of main, no origin -> ORPHAN; not stale -> PRESERVED-OK.
DC_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-dc-XXXXXX)"
_TMPDIRS+=("$DC_MOUNT")
DC_MAP="$(mktemp /tmp/test-warm-lane-audit-dc-map-XXXXXX)"
_TMPDIRS+=("$DC_MAP")
printf '500 pending\n' > "$DC_MAP"
make_lane "$DC_MOUNT/_lane-orphan" "task/500"
_add_ahead_commit "$DC_MOUNT/_lane-orphan"

ORACLE_MAP="$DC_MAP" run_helper --mount "$DC_MOUNT" --status-cmd "$ORACLE_STUB_DIR/leak-oracle.sh"
assert "D7: exit 0" test "$RC" -eq 0
assert "D8: _lane-orphan recoverable=ORPHAN" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-orphan .*recoverable=ORPHAN"' _ "$OUT"
assert "D9: _lane-orphan classification PRESERVED-OK (not stale)" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-orphan .*classification=PRESERVED-OK"' _ "$OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — residue-only-dirty -> RECLAIMABLE vs genuine WIP
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: residue-only-dirty vs genuine WIP ---"

# E(a): FREE, pending, ahead-of-main, ORPHAN lane whose ONLY dirty tracked path
# is the residue file data/queue/write_queue.db (committed first, then
# modified in place) -> dirty=residue-only -> classification RECLAIMABLE,
# despite being non-terminal and ORPHAN.
EA_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-ea-XXXXXX)"
_TMPDIRS+=("$EA_MOUNT")
EA_MAP="$(mktemp /tmp/test-warm-lane-audit-ea-map-XXXXXX)"
_TMPDIRS+=("$EA_MAP")
printf '600 pending\n' > "$EA_MAP"
make_lane "$EA_MOUNT/_lane-residue" "task/600"
_add_ahead_commit "$EA_MOUNT/_lane-residue"
mkdir -p "$EA_MOUNT/_lane-residue/data/queue"
printf 'db-v1\n' > "$EA_MOUNT/_lane-residue/data/queue/write_queue.db"
git -C "$EA_MOUNT/_lane-residue" add data/queue/write_queue.db
git -C "$EA_MOUNT/_lane-residue" commit -q -m "add residue db"
printf 'db-v2\n' > "$EA_MOUNT/_lane-residue/data/queue/write_queue.db"

ORACLE_MAP="$EA_MAP" run_helper --mount "$EA_MOUNT" --status-cmd "$ORACLE_STUB_DIR/leak-oracle.sh"
assert "E1: exit 0" test "$RC" -eq 0
assert "E2: _lane-residue dirty=residue-only" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-residue .*dirty=residue-only"' _ "$OUT"
assert "E3: _lane-residue classification RECLAIMABLE" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-residue .*classification=RECLAIMABLE"' _ "$OUT"

# E(b): FREE, pending, ahead-of-main, ORPHAN lane with a dirty tracked SOURCE
# file (README.md) -> dirty=wip -> classification PRESERVED-OK (genuine
# unrecoverable WIP; not yet stale — Block F introduces the LEAKED carve-out).
EB_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-eb-XXXXXX)"
_TMPDIRS+=("$EB_MOUNT")
EB_MAP="$(mktemp /tmp/test-warm-lane-audit-eb-map-XXXXXX)"
_TMPDIRS+=("$EB_MAP")
printf '700 pending\n' > "$EB_MAP"
make_lane "$EB_MOUNT/_lane-wip" "task/700"
_add_ahead_commit "$EB_MOUNT/_lane-wip"
printf 'uncommitted wip change\n' >> "$EB_MOUNT/_lane-wip/README.md"

ORACLE_MAP="$EB_MAP" run_helper --mount "$EB_MOUNT" --status-cmd "$ORACLE_STUB_DIR/leak-oracle.sh"
assert "E4: exit 0" test "$RC" -eq 0
assert "E5: _lane-wip dirty=wip" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-wip .*dirty=wip"' _ "$OUT"
assert "E6: _lane-wip classification PRESERVED-OK (genuine WIP, not stale)" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-wip .*classification=PRESERVED-OK"' _ "$OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block F — LEAKED + stale A4 relation
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: LEAKED + stale A4 relation ---"

F_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-f-XXXXXX)"
_TMPDIRS+=("$F_MOUNT")
F_MAP="$(mktemp /tmp/test-warm-lane-audit-f-map-XXXXXX)"
_TMPDIRS+=("$F_MAP")
printf '800 pending\n' > "$F_MAP"

# FREE, non-terminal (task/800, oracle=pending), ORPHAN (ahead-of-main commit,
# no origin) -- built FIRST; the lane dir's mtime is aged past the default
# 60-minute knob LAST (touch -d), so the staleness measurement reflects only
# the deliberate backdate, not repo setup (git commit only touches paths
# inside .git/ and README.md's own mtime, never the lane dir's own entries).
make_lane "$F_MOUNT/_lane-leaked" "task/800"
_add_ahead_commit "$F_MOUNT/_lane-leaked"
touch -d '90 minutes ago' "$F_MOUNT/_lane-leaked"

ORACLE_MAP="$F_MAP" run_helper --mount "$F_MOUNT" --status-cmd "$ORACLE_STUB_DIR/leak-oracle.sh"
assert "F1: exit 0" test "$RC" -eq 0
assert "F2: _lane-leaked classification LEAKED (default stale-age-min=60, age~90min)" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-leaked .*classification=LEAKED"' _ "$OUT"
assert "F3: HEADROOM line shows leaked=1" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "leaked=1"' _ "$OUT"

# A4 relation (not a frozen age): the SAME lane, re-run with --stale-age-min
# raised far above its ~90-minute age -> PRESERVED-OK, leaked=0.
ORACLE_MAP="$F_MAP" run_helper --mount "$F_MOUNT" --status-cmd "$ORACLE_STUB_DIR/leak-oracle.sh" --stale-age-min 100000
assert "F4: exit 0" test "$RC" -eq 0
assert "F5: _lane-leaked classification PRESERVED-OK under --stale-age-min 100000" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-leaked .*classification=PRESERVED-OK"' _ "$OUT"
assert "F6: HEADROOM line shows leaked=0 under --stale-age-min 100000" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "leaked=0"' _ "$OUT"

# Same relation via the env knob (no --stale-age-min flag) -- confirms the
# knob is read from REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN, not just the flag.
REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN=100000 ORACLE_MAP="$F_MAP" run_helper --mount "$F_MOUNT" --status-cmd "$ORACLE_STUB_DIR/leak-oracle.sh"
assert "F7: exit 0" test "$RC" -eq 0
assert "F8: _lane-leaked classification PRESERVED-OK under env REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN=100000" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-leaked .*classification=PRESERVED-OK"' _ "$OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block G — measured divergent_gib / free_gib / budget_gib (DD3, no frozen constant)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: measured disk figures (DD3, no frozen GB constant) ---"

G_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-g-XXXXXX)"
_TMPDIRS+=("$G_MOUNT")

# A lane with no target/ -> divergent_gib must report 0.
make_lane "$G_MOUNT/_lane-g"

# Stub df: ignores all args, emits a KNOWN avail byte count in the same shape
# as `df -B1 --output=avail -- <mount>` (a header line + one data line). The
# byte count is deliberately non-round (+123456789) so the floor-to-GiB
# truncation is exercised for real, not a coincidental passthrough.
G_DF_STUB_DIR="$(mktemp -d /tmp/test-warm-lane-audit-g-dfstub-XXXXXX)"
_TMPDIRS+=("$G_DF_STUB_DIR")
G_AVAIL_BYTES=$(( 200 * 1024 * 1024 * 1024 + 123456789 ))
cat > "$G_DF_STUB_DIR/df-fake.sh" << EOF
#!/usr/bin/env bash
printf '     Avail\n'
printf '%s\n' "$G_AVAIL_BYTES"
EOF
chmod +x "$G_DF_STUB_DIR/df-fake.sh"

# The test COMPUTES the expected free_gib/budget_gib from the SAME stub input
# + the SAME --safety knob passed to the script -- a derived relation, not a
# hardcoded literal (DD3/G6/D8 no-frozen-constant rule).
G_SAFETY=4
G_EXPECTED_FREE_GIB=$(( G_AVAIL_BYTES / 1073741824 ))
G_EXPECTED_BUDGET_GIB=$(( G_EXPECTED_FREE_GIB / G_SAFETY ))

REIFY_WARM_LANE_AUDIT_DF="$G_DF_STUB_DIR/df-fake.sh" run_helper --mount "$G_MOUNT" --safety "$G_SAFETY"

assert "G1: exit 0" test "$RC" -eq 0
assert "G2: _lane-g row has a non-negative integer divergent_gib" \
    bash -c 'printf "%s\n" "$1" | grep -qE "lane=_lane-g .*divergent_gib=[0-9]+"' _ "$OUT"
assert "G3: _lane-g divergent_gib=0 (no target/)" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-g .*divergent_gib=0"' _ "$OUT"
assert "G4: HEADROOM divergent_gib is a non-negative integer" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -qE "divergent_gib=[0-9]+"' _ "$OUT"
assert "G5: HEADROOM free_gib == floor(stub_avail_bytes / 2^30)" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "free_gib=$2 budget_gib="' _ "$OUT" "$G_EXPECTED_FREE_GIB"
assert "G6: HEADROOM budget_gib == floor(free_gib / safety)" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -qE "budget_gib=$2\$"' _ "$OUT" "$G_EXPECTED_BUDGET_GIB"

test_summary
