#!/usr/bin/env bash
# tests/infra/test_warm_lane_gc.sh
# Hermetic tests for scripts/warm-lane-gc.sh.
#
# Seed-script stub:
#   Wired via --seed-script flag (overrides the default sibling seed-warm-lane.sh).
#   Records argv to a log file and simulates thinning by removing the divergent
#   marker file from the lane's target/ directory.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag, bare invocation, unknown subcommand,
#       reclaim missing --worktrees-dir or --base-target; --status-cmd is now
#       an unknown flag (A8) — the Tier-3 machinery was removed (task 5326)
#   B — reset a divergent FREE lane (seed-script invoked with resolved gen path)
#   C — remove an orphaned-landed clean worktree
#   D — always-reclaim (task 5326): a DIRTY POOL LANE is now RECLAIMED (reset),
#       while a DIRTY ORPHAN stays preserved by Pass-2 _is_reclaimable
#   E — always-reclaim (task 5326): an AHEAD-OF-MAIN POOL LANE is now RECLAIMED
#       (no --status-cmd wired — terminality- AND ahead-independent), while an
#       AHEAD-OF-MAIN ORPHAN stays preserved by Pass-2 _is_reclaimable
#   F — preserve a lane with a live-consumer lock — the SOLE remaining Pass-1
#       preserve gate, holding even for a lane that is ALSO dirty+ahead (5326)
#   G — combined PRD δ signal: pool lanes (clean + dirty) reset, orphan removed,
#       ahead orphan + live-consumer lane + protect-glob preserved + summary line
#   K — disk-pressure fast-path: rm -rf target/ instead of an alpha reseed clone
#       (--disk-pressure / REIFY_WARM_LANE_GC_DISK_PRESSURE); lanes reclaim via
#       always-reclaim, no --status-cmd needed (task 5167, 5326)
#   M — ephemeral verify/sweep worktrees (_mainsweep-*/_mainprobe-*) protected
#       by the DEFAULT protect-glob, DF-faithful (--mount only, no
#       --protect-glob) (task 5221)
#   N — full managed-worktree protect set (_solo-*/_substrate-gate-*/
#       _offline-deep (exact name)/_iact-*) protected by default, plus an
#       explicit --protect-glob override still re-narrows the set (task 5221)
#
# The former Tier-3 blocks I/J/L (terminal-task reclaim + Pass-2 boundary,
# task 5167) were deleted when task 5326 collapsed the Pass-1 gate to the
# live-consumer flock alone (always-reclaim): their essential coverage —
# live-consumer preserves a reclaimable lane, ahead-of-main orphan preserved —
# migrated into the rewritten Blocks F and E.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-gc.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-gc.sh hermetic tests (task 4717) ==="

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

ERR_FILE="$(mktemp /tmp/test-warm-lane-gc-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes warm-lane-gc.sh, capturing OUT (stdout), ERR_OUT (stderr), RC.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ── git repo factory ───────────────────────────────────────────────────────────
# make_repo DIR  — create a bare-minimum git repo at DIR with one initial commit.
# Sets global REPO_DIR to the created path.
# Always creates the 'main' branch (requires git >= 2.28; -b flag).
make_repo() {
    local dir="$1"
    git init -q -b main "$dir"
    git -C "$dir" config user.email "test@test.local"
    git -C "$dir" config user.name "Test"
    touch "$dir/README.md"
    git -C "$dir" add README.md
    git -C "$dir" commit -q -m "initial"
}

# ──────────────────────────────────────────────────────────────────────────────
# Shared scaffolding: task-lane factory + causal live-consumer-lock handshake
# ──────────────────────────────────────────────────────────────────────────────
# The Tier-3 status-oracle stubs (gc-status-oracle{,-fail,-counting}.sh) were
# removed with the Tier-3 machinery in task 5326 (always-reclaim collapses the
# Pass-1 gate to the live-consumer flock alone). make_task_lane is retained: it
# still builds the ahead-of-main / dirty pool-lane fixtures that Blocks E/K now
# reclaim via always-reclaim, and the ahead-of-main ORPHAN fixtures Pass 2 still
# preserves.

# make_task_lane REPO WORKTREES NAME BRANCH [ahead]
# Creates a lane at WORKTREES/NAME as a git worktree checked out on BRANCH
# (task/NNNN), seeded with a target/DIVERGENT_MARKER. When ahead="ahead", adds
# a committed change NOT reachable from main (rebase-orphan simulation: HEAD
# stands in for a task that landed via merge-train REBASE, so its actual work
# is on main under new SHAs while this lane's branch tip is an orphan —
# `git merge-base --is-ancestor HEAD main` is false).
make_task_lane() {
    local repo="$1" worktrees="$2" name="$3" branch="$4" ahead="${5:-}"
    git -C "$repo" worktree add -q -b "$branch" "$worktrees/$name"
    if [ "$ahead" = "ahead" ]; then
        echo "ahead (rebase-orphan simulation)" >> "$worktrees/$name/README.md"
        git -C "$worktrees/$name" add README.md
        git -C "$worktrees/$name" commit -q -m "rebase-orphan simulation: ahead-of-main tip"
    fi
    mkdir -p "$worktrees/$name/target"
    touch "$worktrees/$name/target/DIVERGENT_MARKER"
}

# _seed_stub_body — printed to stdout; redirect into a seed-script stub file.
# Logs its argv to $SEED_LOG and removes the lane's divergent marker (simulates
# the α reflink-reseed's thinning effect). Shared by the make_task_lane-based
# fixtures (Block K).
_seed_stub_body() {
    cat << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
}

# _wait_for_reader_lock <ready-marker> <deadline-seconds>
# Causal ordering (technique R, docs/prds/infra-test-wallclock-deflake.md,
# task #4847): polls for the READY marker file in 0.05s ticks, returning 0
# as soon as it appears, or non-zero once the generous anti-hang deadline
# (technique T) elapses. The READY marker is touched by a backgrounded lock
# holder AFTER it acquires its flock, so returning 0 causally guarantees the
# flock is held at the caller's next statement — replacing a fixed `sleep`
# that races the background subshell's lock acquisition under load (the
# subshell may not have won the lock within a short fixed sleep, letting a
# competing acquisition — e.g. the GC sweep under test — win instead).
# Mirrors tests/infra/test_warm_lane_pool.sh's identically-named helper.
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
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits 2" test "$RC" -eq 2

# A3: bare invocation (no subcommand) exits 2
run_helper
assert "A3: bare invocation exits 2" test "$RC" -eq 2

# A4: unknown subcommand exits 2
run_helper frobulate
assert "A4: unknown subcommand exits 2" test "$RC" -eq 2

# A5: reclaim without --worktrees-dir exits 2
run_helper reclaim --base-target /tmp/some-base
assert "A5: reclaim without --worktrees-dir exits 2" test "$RC" -eq 2

# A6: reclaim without --base-target exits 2
run_helper reclaim --worktrees-dir /tmp/some-dir
assert "A6: reclaim without --base-target exits 2" test "$RC" -eq 2

# A7: reclaim with both required flags exits 0 (empty worktrees-dir is valid)
A7_WORKTREES="$(mktemp -d /tmp/test-gc-a7-XXXXXX)"
_TMPDIRS+=("$A7_WORKTREES")
A7_BASE="$(mktemp -d /tmp/test-gc-a7-base-XXXXXX)"
_TMPDIRS+=("$A7_BASE")
# Create a gen dir so the base-target resolution works
mkdir -p "$A7_BASE/target.gen.1"
touch "$A7_BASE/target.gen.1.lock"
ln -sfn "$A7_BASE/target.gen.1" "$A7_BASE/target"
run_helper reclaim --worktrees-dir "$A7_WORKTREES" --base-target "$A7_BASE/target"
assert "A7: empty worktrees-dir exits 0" test "$RC" -eq 0

# A8: --status-cmd is now an UNKNOWN flag (Tier-3 machinery removed, task 5326).
# Locks the flag removal: reclaim --worktrees-dir … --base-target … --status-cmd X
# must exit 2 (usage error, unknown flag), not accept it. RED against the current
# gc.sh, which still parses --status-cmd; GREEN once step-2 deletes the flag.
run_helper reclaim \
    --worktrees-dir "$A7_WORKTREES" \
    --base-target "$A7_BASE/target" \
    --status-cmd /bin/true
assert "A8: --status-cmd rejected as unknown flag (exit 2)" test "$RC" -eq 2

# ──────────────────────────────────────────────────────────────────────────────
# Block B — reset a divergent FREE lane
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: reset a divergent FREE lane ---"

B_ROOT="$(mktemp -d /tmp/test-gc-b-XXXXXX)"
_TMPDIRS+=("$B_ROOT")

B_REPO="$B_ROOT/repo"
B_WORKTREES="$B_ROOT/worktrees"
B_BASE="$B_ROOT/base"
mkdir -p "$B_WORKTREES" "$B_BASE"

# Set up primary git repo
make_repo "$B_REPO"

# Create base gen directory (simulates the warm base)
mkdir -p "$B_BASE/target.gen.1"
touch "$B_BASE/target.gen.1.lock"
ln -sfn "$B_BASE/target.gen.1" "$B_BASE/target"

# Create _lane-1 as a git worktree (clean, HEAD == main, i.e. landed)
git -C "$B_REPO" worktree add -q "$B_WORKTREES/_lane-1"
# Add a divergent marker in target/ to prove it gets thinned
mkdir -p "$B_WORKTREES/_lane-1/target"
touch "$B_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# Seed-script stub: records argv to a log file and simulates thinning
B_SEED_LOG="$B_ROOT/seed_calls.log"
B_SEED_STUB="$B_ROOT/seed_stub.sh"
cat > "$B_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
# Seed-script stub: log all argv, simulate thinning
echo "$*" >> "$SEED_LOG"
# Simulate thinning: remove any DIVERGENT_MARKER in the lane's target/
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
chmod +x "$B_SEED_STUB"
export SEED_LOG="$B_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$B_WORKTREES" \
    --base-target "$B_BASE/target" \
    --seed-script "$B_SEED_STUB"

assert "B1: exit 0" test "$RC" -eq 0
assert "B2: seed-script was invoked for _lane-1" test -f "$B_SEED_LOG"
assert "B3: seed-script received resolved gen path (not symlink)" \
    bash -c 'grep -q "target.gen.1" "$1"' _ "$B_SEED_LOG"
assert "B4: seed-script received --fresh-checkout" \
    bash -c 'grep -q -- "--fresh-checkout" "$1"' _ "$B_SEED_LOG"
# gc holds the lane flock on FD 8 (line 417) AND the gen lock (flock -s) on
# FD 9 (line 461) itself across the seed call, so the seed must NOT re-acquire
# the lane lock on FD 9 — that would both self-refuse against gc's FD-8 lane
# lock and clobber gc's FD-9 gen lock. gc therefore passes --assume-lane-lock-held
# so the seed skips its own (now default-on) acquire (esc-5214/task 5354).
assert "B4b: seed-script received --assume-lane-lock-held (gc already holds the lane+gen locks; task 5354)" \
    bash -c 'grep -q -- "--assume-lane-lock-held" "$1"' _ "$B_SEED_LOG"
assert "B5: divergent target marker removed (thinned)" \
    bash -c '[ ! -f "$1" ]' _ "$B_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# ──────────────────────────────────────────────────────────────────────────────
# Block C — remove an orphaned-landed clean worktree
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: remove an orphaned-landed clean worktree ---"

C_ROOT="$(mktemp -d /tmp/test-gc-c-XXXXXX)"
_TMPDIRS+=("$C_ROOT")

C_REPO="$C_ROOT/repo"
C_WORKTREES="$C_ROOT/worktrees"
C_BASE="$C_ROOT/base"
mkdir -p "$C_WORKTREES" "$C_BASE"

make_repo "$C_REPO"

mkdir -p "$C_BASE/target.gen.1"
touch "$C_BASE/target.gen.1.lock"
ln -sfn "$C_BASE/target.gen.1" "$C_BASE/target"

# Create _lane-1 (pool lane, reclaimable)
git -C "$C_REPO" worktree add -q "$C_WORKTREES/_lane-1"
mkdir -p "$C_WORKTREES/_lane-1/target"
touch "$C_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# Create task-9999 (orphan cold worktree, clean, landed in main)
git -C "$C_REPO" worktree add -q "$C_WORKTREES/task-9999"

# Create _merge-verify (protected, must not be touched)
git -C "$C_REPO" worktree add -q "$C_WORKTREES/_merge-verify"
touch "$C_WORKTREES/_merge-verify/PROTECTED_MARKER"

C_SEED_LOG="$C_ROOT/seed_calls.log"
C_SEED_STUB="$C_ROOT/seed_stub.sh"
cat > "$C_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
chmod +x "$C_SEED_STUB"
export SEED_LOG="$C_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$C_WORKTREES" \
    --base-target "$C_BASE/target" \
    --seed-script "$C_SEED_STUB"

assert "C1: exit 0" test "$RC" -eq 0
# The orphan should be removed
assert "C2: task-9999 orphan removed (dir gone)" \
    bash -c '[ ! -d "$1" ]' _ "$C_WORKTREES/task-9999"
# task-9999 should no longer appear in worktree list
assert "C3: task-9999 absent from git worktree list" \
    bash -c '! git -C "$1" worktree list | grep -q "task-9999"' _ "$C_REPO"
# _lane-1 should still be reset
assert "C4: _lane-1 seed-script invoked" test -f "$C_SEED_LOG"
# _merge-verify must be untouched (protect-glob)
assert "C5: _merge-verify protected marker intact" \
    test -f "$C_WORKTREES/_merge-verify/PROTECTED_MARKER"

# ──────────────────────────────────────────────────────────────────────────────
# Block D — always-reclaim: dirty POOL LANE reclaimed, dirty ORPHAN preserved
# Under the task-5326 always-reclaim policy, a FREE pool lane whose live-consumer
# flock is free is reclaimed UNCONDITIONALLY — dirty tracked changes no longer
# preserve it (acquire_lane always re-seeds; committed work lives on the branch
# ref; reset touches only target/). A dirty ORPHAN cold worktree (Pass 2) is
# STILL preserved by the unchanged _is_reclaimable predicate.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: always-reclaim (dirty pool lane reset, dirty orphan preserved) ---"

D_ROOT="$(mktemp -d /tmp/test-gc-d-XXXXXX)"
_TMPDIRS+=("$D_ROOT")

D_REPO="$D_ROOT/repo"
D_WORKTREES="$D_ROOT/worktrees"
D_BASE="$D_ROOT/base"
mkdir -p "$D_WORKTREES" "$D_BASE"

make_repo "$D_REPO"

mkdir -p "$D_BASE/target.gen.1"
touch "$D_BASE/target.gen.1.lock"
ln -sfn "$D_BASE/target.gen.1" "$D_BASE/target"

# _lane-2: dirty POOL LANE (tracked file modified but not committed) — now RECLAIMED.
git -C "$D_REPO" worktree add -q "$D_WORKTREES/_lane-2"
echo "dirty" >> "$D_WORKTREES/_lane-2/README.md"
mkdir -p "$D_WORKTREES/_lane-2/target"
touch "$D_WORKTREES/_lane-2/target/DIVERGENT_MARKER"

# task-8888: dirty ORPHAN (modified tracked file) — STILL preserved (Pass 2).
git -C "$D_REPO" worktree add -q "$D_WORKTREES/task-8888"
echo "dirty" >> "$D_WORKTREES/task-8888/README.md"

# Thinning seed stub: logs argv AND removes the lane's DIVERGENT_MARKER, so a
# reset is observable both by the seed log and by the marker's disappearance.
D_SEED_LOG="$D_ROOT/seed_calls.log"
D_SEED_STUB="$D_ROOT/seed_stub.sh"
cat > "$D_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
chmod +x "$D_SEED_STUB"
export SEED_LOG="$D_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$D_WORKTREES" \
    --base-target "$D_BASE/target" \
    --seed-script "$D_SEED_STUB"

assert "D1: exit 0" test "$RC" -eq 0
# Dirty POOL LANE IS reset: seed-script invoked for it (always-reclaim).
assert "D2: dirty pool lane _lane-2 seed-script invoked (reclaimed)" \
    bash -c 'test -f "$1" && grep -q "_lane-2" "$1"' _ "$D_SEED_LOG"
# Dirty POOL LANE marker removed (thinned by the reset).
assert "D3: dirty pool lane divergent marker removed (reclaimed)" \
    bash -c '[ ! -f "$1" ]' _ "$D_WORKTREES/_lane-2/target/DIVERGENT_MARKER"
# Dirty ORPHAN NOT removed (Pass-2 _is_reclaimable unchanged).
assert "D4: dirty orphan task-8888 still present (preserved by Pass 2)" \
    test -d "$D_WORKTREES/task-8888"
# Stderr should still mention preserving dirty WIP (for the orphan).
assert "D5: stderr mentions dirty WIP preservation (orphan)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "dirty|preserving|wip|tracked"' _ "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — always-reclaim: ahead-of-main POOL LANE reclaimed, ahead ORPHAN preserved
# The ahead-of-main pool lane is reclaimed with NO --status-cmd wired, proving
# the always-reclaim policy is independent of BOTH the ahead-of-main tip AND any
# backing-task terminality (Tier-3 is gone). The ahead-of-main ORPHAN cold
# worktree (Pass 2) is STILL preserved by the unchanged _is_reclaimable predicate.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: always-reclaim (ahead pool lane reset, ahead orphan preserved) ---"

E_ROOT="$(mktemp -d /tmp/test-gc-e-XXXXXX)"
_TMPDIRS+=("$E_ROOT")

E_REPO="$E_ROOT/repo"
E_WORKTREES="$E_ROOT/worktrees"
E_BASE="$E_ROOT/base"
mkdir -p "$E_WORKTREES" "$E_BASE"

make_repo "$E_REPO"

mkdir -p "$E_BASE/target.gen.1"
touch "$E_BASE/target.gen.1.lock"
ln -sfn "$E_BASE/target.gen.1" "$E_BASE/target"

# _lane-3: clean POOL LANE with a committed change NOT in main (ahead-of-main) —
# now RECLAIMED via always-reclaim (no --status-cmd wired).
git -C "$E_REPO" worktree add -q "$E_WORKTREES/_lane-3"
echo "ahead" >> "$E_WORKTREES/_lane-3/README.md"
git -C "$E_WORKTREES/_lane-3" add README.md
git -C "$E_WORKTREES/_lane-3" commit -q -m "ahead-of-main commit"
mkdir -p "$E_WORKTREES/_lane-3/target"
touch "$E_WORKTREES/_lane-3/target/DIVERGENT_MARKER"

# task-7777: ahead-of-main ORPHAN — STILL preserved (Pass 2 _is_reclaimable).
git -C "$E_REPO" worktree add -q "$E_WORKTREES/task-7777"
echo "ahead" >> "$E_WORKTREES/task-7777/README.md"
git -C "$E_WORKTREES/task-7777" add README.md
git -C "$E_WORKTREES/task-7777" commit -q -m "ahead-of-main commit"

# Thinning seed stub: logs argv AND removes the lane's DIVERGENT_MARKER.
E_SEED_LOG="$E_ROOT/seed_calls.log"
E_SEED_STUB="$E_ROOT/seed_stub.sh"
cat > "$E_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
chmod +x "$E_SEED_STUB"
export SEED_LOG="$E_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$E_WORKTREES" \
    --base-target "$E_BASE/target" \
    --seed-script "$E_SEED_STUB" \
    --main-ref "main"

assert "E1: exit 0" test "$RC" -eq 0
# Ahead-of-main POOL LANE IS reset: seed-script invoked (always-reclaim).
assert "E2: ahead pool lane _lane-3 seed-script invoked (reclaimed)" \
    bash -c 'test -f "$1" && grep -q "_lane-3" "$1"' _ "$E_SEED_LOG"
assert "E3: ahead pool lane divergent marker removed (reclaimed)" \
    bash -c '[ ! -f "$1" ]' _ "$E_WORKTREES/_lane-3/target/DIVERGENT_MARKER"
# Ahead-of-main ORPHAN NOT removed (Pass-2 _is_reclaimable unchanged).
assert "E4: ahead-of-main orphan task-7777 still present (preserved by Pass 2)" \
    test -d "$E_WORKTREES/task-7777"
# Stderr should still mention unlanded/ahead-of-main preservation (for the orphan).
assert "E5: stderr mentions unlanded/ahead-of-main preservation (orphan)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "unlanded|ahead|preserving"' _ "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block F — live-consumer flock is the SOLE remaining Pass-1 preserve gate
# _lane-4: clean+landed, but a live consumer holds its flock — preserved.
# _lane-5: dirty AND ahead-of-main AND a live consumer holds its flock — STILL
# preserved. Under always-reclaim (task 5326) a dirty+ahead lane WOULD be
# reclaimed if its flock were free (Blocks D/E); the flock alone keeps _lane-5,
# isolating the live-consumer flock as the only Pass-1 preserve gate that survives.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: live-consumer flock is the sole Pass-1 preserve gate ---"

F_ROOT="$(mktemp -d /tmp/test-gc-f-XXXXXX)"
_TMPDIRS+=("$F_ROOT")

F_REPO="$F_ROOT/repo"
F_WORKTREES="$F_ROOT/worktrees"
F_BASE="$F_ROOT/base"
mkdir -p "$F_WORKTREES" "$F_BASE"

make_repo "$F_REPO"

mkdir -p "$F_BASE/target.gen.1"
touch "$F_BASE/target.gen.1.lock"
ln -sfn "$F_BASE/target.gen.1" "$F_BASE/target"

# _lane-4: clean, landed, with a live consumer holding the exclusive lock.
git -C "$F_REPO" worktree add -q "$F_WORKTREES/_lane-4"
mkdir -p "$F_WORKTREES/_lane-4/target"
touch "$F_WORKTREES/_lane-4/target/DIVERGENT_MARKER"
touch "$F_WORKTREES/_lane-4.lock"
F4_READY="$F_WORKTREES/_lane-4.lock.ready-marker"
( flock -x 9 && touch "$F4_READY" && sleep 300 ) 9>"$F_WORKTREES/_lane-4.lock" &
F4_LOCK_PID=$!
_BGPIDS+=("$F4_LOCK_PID")

# _lane-5: dirty (uncommitted tracked change) AND ahead-of-main, with a live
# consumer holding the exclusive lock. Both dirty and ahead are now
# non-preserving on their own (Blocks D/E), so only the flock keeps this lane.
git -C "$F_REPO" worktree add -q "$F_WORKTREES/_lane-5"
echo "ahead" >> "$F_WORKTREES/_lane-5/README.md"
git -C "$F_WORKTREES/_lane-5" add README.md
git -C "$F_WORKTREES/_lane-5" commit -q -m "ahead-of-main commit"
echo "dirty uncommitted change" >> "$F_WORKTREES/_lane-5/README.md"
mkdir -p "$F_WORKTREES/_lane-5/target"
touch "$F_WORKTREES/_lane-5/target/DIVERGENT_MARKER"
touch "$F_WORKTREES/_lane-5.lock"
F5_READY="$F_WORKTREES/_lane-5.lock.ready-marker"
( flock -x 9 && touch "$F5_READY" && sleep 300 ) 9>"$F_WORKTREES/_lane-5.lock" &
F5_LOCK_PID=$!
_BGPIDS+=("$F5_LOCK_PID")

# Causal handshake (technique R, #4847): proceed only once BOTH flocks are
# provably held, so the GC sweep under test cannot race in and win either lock.
_wait_for_reader_lock "$F4_READY" 30
_wait_for_reader_lock "$F5_READY" 30

F_SEED_LOG="$F_ROOT/seed_calls.log"
F_SEED_STUB="$F_ROOT/seed_stub.sh"
cat > "$F_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
chmod +x "$F_SEED_STUB"
export SEED_LOG="$F_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$F_WORKTREES" \
    --base-target "$F_BASE/target" \
    --seed-script "$F_SEED_STUB" \
    --main-ref "main"

assert "F1: exit 0" test "$RC" -eq 0
# _lane-4 (clean, locked) NOT reset.
assert "F2: live-consumer lane _lane-4 seed-script NOT invoked" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_lane-4" "$1"' _ "$F_SEED_LOG"
assert "F3: live-consumer lane _lane-4 divergent marker intact" \
    test -f "$F_WORKTREES/_lane-4/target/DIVERGENT_MARKER"
assert "F4: stderr mentions live consumer preservation" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "live.consumer|locked|preserving|consumer"' _ "$ERR_OUT"
# _lane-5 (dirty+ahead, locked) STILL NOT reset — the flock is the sole gate.
assert "F5: dirty+ahead live-consumer lane _lane-5 seed-script NOT invoked (flock wins)" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_lane-5" "$1"' _ "$F_SEED_LOG"
assert "F6: dirty+ahead live-consumer lane _lane-5 divergent marker intact" \
    test -f "$F_WORKTREES/_lane-5/target/DIVERGENT_MARKER"

# Release the locks
kill "$F4_LOCK_PID" "$F5_LOCK_PID" 2>/dev/null || true
_BGPIDS=()  # clear so cleanup doesn't double-kill

# ──────────────────────────────────────────────────────────────────────────────
# Block G — combined PRD δ signal under always-reclaim (task 5326)
# Pool lanes (clean _lane-free + dirty _lane-dirty) are RESET; the clean orphan
# task-free is REMOVED; the ahead-of-main orphan task-ahead, the live-consumer
# lane _lane-locked, and the protected _merge-verify are PRESERVED. Summary:
# reset=2 removed=1 preserved=3.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: combined PRD delta signal (always-reclaim) ---"

G_ROOT="$(mktemp -d /tmp/test-gc-g-XXXXXX)"
_TMPDIRS+=("$G_ROOT")

G_REPO="$G_ROOT/repo"
G_WORKTREES="$G_ROOT/worktrees"
G_BASE="$G_ROOT/base"
mkdir -p "$G_WORKTREES" "$G_BASE"

make_repo "$G_REPO"

mkdir -p "$G_BASE/target.gen.1"
touch "$G_BASE/target.gen.1.lock"
ln -sfn "$G_BASE/target.gen.1" "$G_BASE/target"

# Fixture 1: reclaimable FREE lane (_lane-free)
git -C "$G_REPO" worktree add -q "$G_WORKTREES/_lane-free"
mkdir -p "$G_WORKTREES/_lane-free/target"
touch "$G_WORKTREES/_lane-free/target/DIVERGENT_MARKER"

# Fixture 2: reclaimable orphan worktree (task-free)
git -C "$G_REPO" worktree add -q "$G_WORKTREES/task-free"

# Fixture 3: dirty-WIP POOL LANE (_lane-dirty) — now RECLAIMED (always-reclaim)
git -C "$G_REPO" worktree add -q "$G_WORKTREES/_lane-dirty"
echo "dirty" >> "$G_WORKTREES/_lane-dirty/README.md"
mkdir -p "$G_WORKTREES/_lane-dirty/target"
touch "$G_WORKTREES/_lane-dirty/target/DIVERGENT_MARKER"

# Fixture 4: unlanded-ahead orphan (task-ahead)
git -C "$G_REPO" worktree add -q "$G_WORKTREES/task-ahead"
echo "ahead" >> "$G_WORKTREES/task-ahead/README.md"
git -C "$G_WORKTREES/task-ahead" add README.md
git -C "$G_WORKTREES/task-ahead" commit -q -m "ahead of main"

# Fixture 5: live-consumer-locked lane (_lane-locked)
git -C "$G_REPO" worktree add -q "$G_WORKTREES/_lane-locked"
mkdir -p "$G_WORKTREES/_lane-locked/target"
touch "$G_WORKTREES/_lane-locked/target/DIVERGENT_MARKER"
touch "$G_WORKTREES/_lane-locked.lock"
( flock -x 9 && sleep 300 ) 9>"$G_WORKTREES/_lane-locked.lock" &
G_LOCK_PID=$!
_BGPIDS+=("$G_LOCK_PID")
sleep 0.1

# Fixture 6: protected _merge-verify (protect-glob)
git -C "$G_REPO" worktree add -q "$G_WORKTREES/_merge-verify"
touch "$G_WORKTREES/_merge-verify/PROTECTED_MARKER"

G_SEED_LOG="$G_ROOT/seed_calls.log"
G_SEED_STUB="$G_ROOT/seed_stub.sh"
cat > "$G_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
chmod +x "$G_SEED_STUB"
export SEED_LOG="$G_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$G_WORKTREES" \
    --base-target "$G_BASE/target" \
    --seed-script "$G_SEED_STUB" \
    --main-ref "main"

assert "G1: exit 0" test "$RC" -eq 0

# Reclaimable clean pool lane was reset
assert "G2: reclaimable lane _lane-free was reset (seed-script invoked)" \
    bash -c 'grep -q "_lane-free" "$1"' _ "$G_SEED_LOG"
assert "G3: reclaimable lane divergent marker removed" \
    bash -c '[ ! -f "$1" ]' _ "$G_WORKTREES/_lane-free/target/DIVERGENT_MARKER"

# Reclaimable orphan was removed
assert "G4: reclaimable orphan task-free was removed" \
    bash -c '[ ! -d "$1" ]' _ "$G_WORKTREES/task-free"

# Dirty POOL LANE now reclaimed too (always-reclaim, task 5326)
assert "G5: dirty pool lane _lane-dirty divergent marker removed (reclaimed)" \
    bash -c '[ ! -f "$1" ]' _ "$G_WORKTREES/_lane-dirty/target/DIVERGENT_MARKER"

# Preserved fixtures: ahead ORPHAN (Pass 2), live-consumer lane, protected glob
assert "G6: ahead orphan task-ahead still present" \
    test -d "$G_WORKTREES/task-ahead"
assert "G7: locked lane _lane-locked marker intact" \
    test -f "$G_WORKTREES/_lane-locked/target/DIVERGENT_MARKER"

# Protected glob untouched
assert "G8: _merge-verify protected marker intact" \
    test -f "$G_WORKTREES/_merge-verify/PROTECTED_MARKER"

# Summary line on stdout with reset/removed/preserved counts
assert "G9: stdout contains machine-readable summary" \
    bash -c 'printf "%s\n" "$1" | grep -qE "reclaim:.*reset=.*removed=.*preserved="' _ "$OUT"
assert "G10: summary shows reset=2 (_lane-free + _lane-dirty)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "reset=2"' _ "$OUT"
assert "G11: summary shows removed=1" \
    bash -c 'printf "%s\n" "$1" | grep -qE "removed=1"' _ "$OUT"
assert "G12: summary shows preserved=3 (task-ahead + _lane-locked + _merge-verify)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "preserved=3"' _ "$OUT"

# Release lock
kill "$G_LOCK_PID" 2>/dev/null || true
_BGPIDS=()

# ──────────────────────────────────────────────────────────────────────────────
# Block H — --mount derivation: DF consumer contract
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: --mount derivation (DF consumer contract) ---"

H_ROOT="$(mktemp -d /tmp/test-gc-h-XXXXXX)"
_TMPDIRS+=("$H_ROOT")

H_REPO="$H_ROOT/repo"
H_WORKTREES="$H_ROOT/worktrees"
H_BASE="$H_ROOT/base"
mkdir -p "$H_WORKTREES" "$H_BASE"

# Set up primary git repo
make_repo "$H_REPO"

# Create base gen directory (simulates the warm base)
# Layout: <root>/worktrees/ (mount) + <root>/base/target.gen.1 (sibling)
mkdir -p "$H_BASE/target.gen.1"
touch "$H_BASE/target.gen.1.lock"
ln -sfn "$H_BASE/target.gen.1" "$H_BASE/target"

# Create _lane-1 as a git worktree (clean, HEAD == main, i.e. landed)
git -C "$H_REPO" worktree add -q "$H_WORKTREES/_lane-1"
# Add a divergent marker in target/ to prove it gets thinned
mkdir -p "$H_WORKTREES/_lane-1/target"
touch "$H_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# Seed-script stub: records argv to a log file and simulates thinning
H_SEED_LOG="$H_ROOT/seed_calls.log"
H_SEED_STUB="$H_ROOT/seed_stub.sh"
cat > "$H_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
# Seed-script stub: log all argv, simulate thinning
echo "$*" >> "$SEED_LOG"
# Simulate thinning: remove any DIVERGENT_MARKER in the lane's target/
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
chmod +x "$H_SEED_STUB"
export SEED_LOG="$H_SEED_LOG"

# H1–H5: reclaim --mount <root>/worktrees (NO --worktrees-dir/--base-target)
run_helper reclaim \
    --mount "$H_WORKTREES" \
    --seed-script "$H_SEED_STUB"

assert "H1: --mount exits 0" test "$RC" -eq 0
assert "H2: seed-stub invoked for _lane-1 (WORKTREES_DIR derived = \$MOUNT)" \
    bash -c 'test -f "$1" && grep -q "_lane-1" "$1"' _ "$H_SEED_LOG"
assert "H3: seed-stub received resolved base/target.gen.1 path (BASE_TARGET derived = \$(dirname \$MOUNT)/base/target, symlink-resolved)" \
    bash -c 'grep -q "target.gen.1" "$1"' _ "$H_SEED_LOG"
assert "H4: --fresh-checkout passed through to seed-stub" \
    bash -c 'grep -q -- "--fresh-checkout" "$1"' _ "$H_SEED_LOG"
assert "H5: divergent target marker removed (thinned)" \
    bash -c '[ ! -f "$1" ]' _ "$H_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# H6: explicit --base-target together with --mount overrides the derived base
H6_ROOT="$(mktemp -d /tmp/test-gc-h6-XXXXXX)"
_TMPDIRS+=("$H6_ROOT")
H6_REPO="$H6_ROOT/repo"
H6_WORKTREES="$H6_ROOT/worktrees"
H6_ALT_BASE="$H6_ROOT/alt_base"
mkdir -p "$H6_WORKTREES" "$H6_ALT_BASE"
make_repo "$H6_REPO"
mkdir -p "$H6_ALT_BASE/target.gen.99"
touch "$H6_ALT_BASE/target.gen.99.lock"
ln -sfn "$H6_ALT_BASE/target.gen.99" "$H6_ALT_BASE/target"
# Lane: landed and clean
git -C "$H6_REPO" worktree add -q "$H6_WORKTREES/_lane-1"
mkdir -p "$H6_WORKTREES/_lane-1/target"
touch "$H6_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

H6_SEED_LOG="$H6_ROOT/seed_calls.log"
H6_SEED_STUB="$H6_ROOT/seed_stub.sh"
cat > "$H6_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
chmod +x "$H6_SEED_STUB"
export SEED_LOG="$H6_SEED_LOG"

run_helper reclaim \
    --mount "$H6_WORKTREES" \
    --base-target "$H6_ALT_BASE/target" \
    --seed-script "$H6_SEED_STUB"

assert "H6: explicit --base-target overrides --mount derived base (uses alt gen.99)" \
    bash -c 'grep -q "target.gen.99" "$1"' _ "$H6_SEED_LOG"

# ──────────────────────────────────────────────────────────────────────────────
# Block K — disk-pressure fast-path
# Under --disk-pressure, a reclaimable lane's target/ is deleted outright
# (rm -rf) instead of going through the alpha reflink-reseed clone — valid
# because acquire_lane always re-seeds from base. Mirrors the manual
# 2026-07-10 remediation for the 768G/404G leaked lanes.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block K: disk-pressure fast-path ---"

K_ROOT="$(mktemp -d /tmp/test-gc-k-XXXXXX)"
_TMPDIRS+=("$K_ROOT")

# ── K1: --disk-pressure deletes target/ outright, no seed-script invocation ────
K1_REPO="$K_ROOT/k1-repo"
K1_WORKTREES="$K_ROOT/k1-worktrees"
K1_BASE="$K_ROOT/k1-base"
mkdir -p "$K1_WORKTREES" "$K1_BASE"
make_repo "$K1_REPO"
mkdir -p "$K1_BASE/target.gen.1"
touch "$K1_BASE/target.gen.1.lock"
ln -sfn "$K1_BASE/target.gen.1" "$K1_BASE/target"

make_task_lane "$K1_REPO" "$K1_WORKTREES" "_lane-1" "task/4827" "ahead"

K1_SEED_LOG="$K_ROOT/k1-seed-calls.log"
K1_SEED_STUB="$K_ROOT/k1-seed-stub.sh"
_seed_stub_body > "$K1_SEED_STUB"
chmod +x "$K1_SEED_STUB"
export SEED_LOG="$K1_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$K1_WORKTREES" \
    --base-target "$K1_BASE/target" \
    --seed-script "$K1_SEED_STUB" \
    --disk-pressure

assert "K1: exit 0" test "$RC" -eq 0
assert "K1: target/ directory deleted outright (disk-pressure fast-path)" \
    bash -c '[ ! -d "$1" ]' _ "$K1_WORKTREES/_lane-1/target"
assert "K1: seed-script NOT invoked (no reflink clone under disk-pressure)" \
    bash -c '[ ! -f "$1" ]' _ "$K1_SEED_LOG"
assert "K1: summary line parses as reclaim: reset=.. removed=.. preserved=.." \
    bash -c 'printf "%s\n" "$1" | grep -qE "reclaim:.*reset=.*removed=.*preserved="' _ "$OUT"
assert "K1: summary shows reset=1 (disk-pressure delete still counts as reset)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "reset=1"' _ "$OUT"

# ── K2: control — same fixture WITHOUT --disk-pressure resets via alpha ────────
K2_REPO="$K_ROOT/k2-repo"
K2_WORKTREES="$K_ROOT/k2-worktrees"
K2_BASE="$K_ROOT/k2-base"
mkdir -p "$K2_WORKTREES" "$K2_BASE"
make_repo "$K2_REPO"
mkdir -p "$K2_BASE/target.gen.1"
touch "$K2_BASE/target.gen.1.lock"
ln -sfn "$K2_BASE/target.gen.1" "$K2_BASE/target"

make_task_lane "$K2_REPO" "$K2_WORKTREES" "_lane-1" "task/4827" "ahead"

K2_SEED_LOG="$K_ROOT/k2-seed-calls.log"
K2_SEED_STUB="$K_ROOT/k2-seed-stub.sh"
_seed_stub_body > "$K2_SEED_STUB"
chmod +x "$K2_SEED_STUB"
export SEED_LOG="$K2_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$K2_WORKTREES" \
    --base-target "$K2_BASE/target" \
    --seed-script "$K2_SEED_STUB"

assert "K2: exit 0" test "$RC" -eq 0
assert "K2: seed-script invoked (normal alpha reset path, no disk-pressure)" \
    bash -c 'test -f "$1" && grep -q "_lane-1" "$1"' _ "$K2_SEED_LOG"
assert "K2: target/ directory still present (thinned by alpha, not deleted)" \
    test -d "$K2_WORKTREES/_lane-1/target"
assert "K2: summary shows reset=1" \
    bash -c 'printf "%s\n" "$1" | grep -qE "reset=1"' _ "$OUT"

# ── K3: landed-clean NON-task-branch lane under --disk-pressure ────────────────
# --disk-pressure is documented (usage()/header) as applying to EVERY
# reclaimable Pass-1 lane. This locks that pool-wide scope: an ordinary
# landed/clean lane (no task/NNNN branch) also gets its target/ deleted
# outright under --disk-pressure rather than alpha-reseeded.
K3_REPO="$K_ROOT/k3-repo"
K3_WORKTREES="$K_ROOT/k3-worktrees"
K3_BASE="$K_ROOT/k3-base"
mkdir -p "$K3_WORKTREES" "$K3_BASE"
make_repo "$K3_REPO"
mkdir -p "$K3_BASE/target.gen.1"
touch "$K3_BASE/target.gen.1.lock"
ln -sfn "$K3_BASE/target.gen.1" "$K3_BASE/target"

# Landed, clean, non-task-branch lane (mirrors Block B's fixture).
git -C "$K3_REPO" worktree add -q "$K3_WORKTREES/_lane-1"
mkdir -p "$K3_WORKTREES/_lane-1/target"
touch "$K3_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

K3_SEED_LOG="$K_ROOT/k3-seed-calls.log"
K3_SEED_STUB="$K_ROOT/k3-seed-stub.sh"
_seed_stub_body > "$K3_SEED_STUB"
chmod +x "$K3_SEED_STUB"
export SEED_LOG="$K3_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$K3_WORKTREES" \
    --base-target "$K3_BASE/target" \
    --seed-script "$K3_SEED_STUB" \
    --disk-pressure

assert "K3: exit 0" test "$RC" -eq 0
assert "K3: landed-clean non-task-branch lane target/ deleted outright (pool-wide disk-pressure scope)" \
    bash -c '[ ! -d "$1" ]' _ "$K3_WORKTREES/_lane-1/target"
assert "K3: seed-script NOT invoked (no reflink clone under disk-pressure)" \
    bash -c '[ ! -f "$1" ]' _ "$K3_SEED_LOG"
assert "K3: summary shows reset=1" \
    bash -c 'printf "%s\n" "$1" | grep -qE "reset=1"' _ "$OUT"

# ── K4: rm failure under --disk-pressure is preserved, not silently reset ──────
# The rm-failure branch (warn + preserve, scripts/warm-lane-gc.sh) is the
# failure-mode handling for the exact scenario --disk-pressure exists to
# remediate (ENOSPC / disk pressure) — an untested regression here would
# silently mis-count reclaim results. Stubs `rm` via PATH to fail
# deterministically: unlike a chmod-0-parent-dir permission trick, this is
# root-safe (permission bits are bypassed when the test process runs as
# root, which a filesystem-based trick cannot guarantee against).
K4_REPO="$K_ROOT/k4-repo"
K4_WORKTREES="$K_ROOT/k4-worktrees"
K4_BASE="$K_ROOT/k4-base"
mkdir -p "$K4_WORKTREES" "$K4_BASE"
make_repo "$K4_REPO"
mkdir -p "$K4_BASE/target.gen.1"
touch "$K4_BASE/target.gen.1.lock"
ln -sfn "$K4_BASE/target.gen.1" "$K4_BASE/target"

make_task_lane "$K4_REPO" "$K4_WORKTREES" "_lane-1" "task/4827" "ahead"

K4_SEED_LOG="$K_ROOT/k4-seed-calls.log"
K4_SEED_STUB="$K_ROOT/k4-seed-stub.sh"
_seed_stub_body > "$K4_SEED_STUB"
chmod +x "$K4_SEED_STUB"
export SEED_LOG="$K4_SEED_LOG"

# rm stub: always fails — simulates rm -rf hitting EACCES / a busy mount /
# an immutable file under real disk pressure. Prepended to PATH only for
# the run_helper call below (bash's temporary-environment semantics for a
# prefix assignment on a function call: visible for that call's duration,
# including subprocesses it spawns, and reverted immediately after).
K4_RMSTUB_DIR="$(mktemp -d /tmp/test-gc-k4-rmstub-XXXXXX)"
_TMPDIRS+=("$K4_RMSTUB_DIR")
cat > "$K4_RMSTUB_DIR/rm" << 'STUB_EOF'
#!/usr/bin/env bash
echo "rm: cannot remove target: simulated failure (test stub)" >&2
exit 1
STUB_EOF
chmod +x "$K4_RMSTUB_DIR/rm"

PATH="$K4_RMSTUB_DIR:$PATH" run_helper reclaim \
    --worktrees-dir "$K4_WORKTREES" \
    --base-target "$K4_BASE/target" \
    --seed-script "$K4_SEED_STUB" \
    --disk-pressure

assert "K4: exit 0 (rm failure does not abort the sweep)" test "$RC" -eq 0
assert "K4: seed-script NOT invoked (disk-pressure path has no alpha fallback)" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_lane-1" "$1"' _ "$K4_SEED_LOG"
assert "K4: target/ NOT removed (rm failed, lane left untouched)" \
    test -d "$K4_WORKTREES/_lane-1/target"
assert "K4: divergent marker intact (nothing was reset)" \
    test -f "$K4_WORKTREES/_lane-1/target/DIVERGENT_MARKER"
assert "K4: summary counts the failed reset as preserved, not reset" \
    bash -c 'printf "%s\n" "$1" | grep -qE "reset=0 removed=0 preserved=1"' _ "$OUT"
assert "K4: stderr captures the rm failure detail (not swallowed)" \
    bash -c 'printf "%s\n" "$1" | grep -qi "simulated failure"' _ "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block M — ephemeral verify/sweep worktrees protected by default
# _mainsweep-*/_mainprobe-* are dark-factory's ephemeral background-verify and
# merge/main-probe worktrees, minted directly under the warm-lane mount (the
# same directory gc.sh scans). DF's gc reclaim call site
# (git_ops.py _run_warm_lane_gc_reclaim) passes ONLY --mount — no
# --protect-glob — so gc's DEFAULT protect-glob is the only thing standing
# between a live sweep/probe worktree and Pass-2 `git worktree remove --force`.
# Their sole other guard, a per-worktree advisory flock, demonstrably failed
# twice in production (task 5221 analysis), removing a live worktree out from
# under a running background integrity sweep. This block is DF-faithful:
# --mount only, no --protect-glob, exercising the default exactly as invoked
# in production.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block M: ephemeral verify/sweep worktrees protected by default ---"

M_ROOT="$(mktemp -d /tmp/test-gc-m-XXXXXX)"
_TMPDIRS+=("$M_ROOT")

M_REPO="$M_ROOT/repo"
M_WORKTREES="$M_ROOT/worktrees"
M_BASE="$M_ROOT/base"
mkdir -p "$M_WORKTREES" "$M_BASE"

make_repo "$M_REPO"

mkdir -p "$M_BASE/target.gen.1"
touch "$M_BASE/target.gen.1.lock"
ln -sfn "$M_BASE/target.gen.1" "$M_BASE/target"

# Fixture 1: _mainsweep-abcd1234 — DF's background main-tip integrity-sweep
# worktree (clean, landed — as it is immediately after DF mints it at main_sha).
git -C "$M_REPO" worktree add -q "$M_WORKTREES/_mainsweep-abcd1234"
touch "$M_WORKTREES/_mainsweep-abcd1234/PROTECTED_MARKER"
mkdir -p "$M_WORKTREES/_mainsweep-abcd1234/target"
touch "$M_WORKTREES/_mainsweep-abcd1234/target/DIVERGENT_MARKER"

# Fixture 2: _mainprobe-abcd1234 — DF's merge/main-probe worktree, same shape.
git -C "$M_REPO" worktree add -q "$M_WORKTREES/_mainprobe-abcd1234"
touch "$M_WORKTREES/_mainprobe-abcd1234/PROTECTED_MARKER"
mkdir -p "$M_WORKTREES/_mainprobe-abcd1234/target"
touch "$M_WORKTREES/_mainprobe-abcd1234/target/DIVERGENT_MARKER"

# Control: task-9999 — genuine cold orphan (non-underscore, clean, landed),
# proving reclaim capability stays intact and the fix does not over-protect.
git -C "$M_REPO" worktree add -q "$M_WORKTREES/task-9999"

M_SEED_LOG="$M_ROOT/seed_calls.log"
M_SEED_STUB="$M_ROOT/seed_stub.sh"
cat > "$M_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
chmod +x "$M_SEED_STUB"
export SEED_LOG="$M_SEED_LOG"

# DF-faithful invocation: --mount only, NO --protect-glob (exercises the DEFAULT).
run_helper reclaim \
    --mount "$M_WORKTREES" \
    --seed-script "$M_SEED_STUB"

assert "M1: exit 0" test "$RC" -eq 0
assert "M2: _mainsweep-abcd1234 dir preserved" \
    test -d "$M_WORKTREES/_mainsweep-abcd1234"
assert "M3: _mainsweep-abcd1234 PROTECTED_MARKER intact" \
    test -f "$M_WORKTREES/_mainsweep-abcd1234/PROTECTED_MARKER"
assert "M4: _mainprobe-abcd1234 dir preserved" \
    test -d "$M_WORKTREES/_mainprobe-abcd1234"
assert "M5: _mainprobe-abcd1234 PROTECTED_MARKER intact" \
    test -f "$M_WORKTREES/_mainprobe-abcd1234/PROTECTED_MARKER"
assert "M6: _mainsweep-abcd1234 seed-script NOT invoked (skipped, not reset)" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_mainsweep-abcd1234" "$1"' _ "$M_SEED_LOG"
assert "M7: _mainprobe-abcd1234 seed-script NOT invoked (skipped, not reset)" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_mainprobe-abcd1234" "$1"' _ "$M_SEED_LOG"
assert "M8: control orphan task-9999 removed (dir gone; reclaim capability intact)" \
    bash -c '[ ! -d "$1" ]' _ "$M_WORKTREES/task-9999"
assert "M9: control orphan task-9999 absent from git worktree list" \
    bash -c '! git -C "$1" worktree list | grep -q "task-9999"' _ "$M_REPO"
assert "M10: summary shows preserved=2 (the two managed worktrees)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "preserved=2"' _ "$OUT"
assert "M11: summary shows removed=1 (control orphan)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "removed=1"' _ "$OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block N — full managed-worktree protect set + explicit override intact
# Extends Block M's coverage to the remaining orchestrator-managed non-pool
# worktree kinds (mirroring dark-factory's PROTECTED_PREFIXES inventory):
# _solo-*, _substrate-gate-*, _offline-deep (persistent, exact name), _iact-*.
# Sub-case N-default proves the DEFAULT protect-glob covers the full set;
# sub-case N-override proves an explicit --protect-glob still re-narrows the
# protect set exactly as before — the fix changes only the DEFAULT, never the
# override mechanism.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block N: full managed-worktree protect set + explicit override intact ---"

# ── N-default: the full managed set survives the DEFAULT protect-glob ─────────
N_ROOT="$(mktemp -d /tmp/test-gc-n-XXXXXX)"
_TMPDIRS+=("$N_ROOT")

N_REPO="$N_ROOT/repo"
N_WORKTREES="$N_ROOT/worktrees"
N_BASE="$N_ROOT/base"
mkdir -p "$N_WORKTREES" "$N_BASE"

make_repo "$N_REPO"

mkdir -p "$N_BASE/target.gen.1"
touch "$N_BASE/target.gen.1.lock"
ln -sfn "$N_BASE/target.gen.1" "$N_BASE/target"

# Four remaining orchestrator-managed non-pool worktree kinds, each clean/landed.
for _n_name in _solo-1234 _substrate-gate-1234 _offline-deep _iact-demo; do
    git -C "$N_REPO" worktree add -q "$N_WORKTREES/$_n_name"
    touch "$N_WORKTREES/$_n_name/PROTECTED_MARKER"
done

# Control: task-8888 — genuine cold orphan, must still be removed.
git -C "$N_REPO" worktree add -q "$N_WORKTREES/task-8888"

N_SEED_LOG="$N_ROOT/seed_calls.log"
N_SEED_STUB="$N_ROOT/seed_stub.sh"
cat > "$N_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
chmod +x "$N_SEED_STUB"
export SEED_LOG="$N_SEED_LOG"

run_helper reclaim \
    --mount "$N_WORKTREES" \
    --seed-script "$N_SEED_STUB"

assert "N-default: exit 0" test "$RC" -eq 0
for _n_name in _solo-1234 _substrate-gate-1234 _offline-deep _iact-demo; do
    assert "N-default: $_n_name dir preserved" \
        test -d "$N_WORKTREES/$_n_name"
    assert "N-default: $_n_name PROTECTED_MARKER intact" \
        test -f "$N_WORKTREES/$_n_name/PROTECTED_MARKER"
    assert "N-default: $_n_name seed-script NOT invoked (skipped, not reset)" \
        bash -c '[ ! -f "$1" ] || ! grep -q "$2" "$1"' _ "$N_SEED_LOG" "$_n_name"
done
assert "N-default: control orphan task-8888 removed (dir gone; reclaim capability intact)" \
    bash -c '[ ! -d "$1" ]' _ "$N_WORKTREES/task-8888"
assert "N-default: control orphan task-8888 absent from git worktree list" \
    bash -c '! git -C "$1" worktree list | grep -q "task-8888"' _ "$N_REPO"
assert "N-default: summary shows preserved=4 (the four managed worktrees)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "preserved=4"' _ "$OUT"
assert "N-default: summary shows removed=1 (control orphan)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "removed=1"' _ "$OUT"

# ── N-override: explicit --protect-glob re-narrows the protect set ────────────
# On a separate mount, an explicit --protect-glob "_merge-*" re-narrows the
# set exactly as before the fix, so _mainsweep-abcd1234 (no longer covered by
# the narrowed glob) IS removed by Pass 2 — proving the fix changes only the
# DEFAULT, never the override mechanism.
NOV_ROOT="$(mktemp -d /tmp/test-gc-nov-XXXXXX)"
_TMPDIRS+=("$NOV_ROOT")

NOV_REPO="$NOV_ROOT/repo"
NOV_WORKTREES="$NOV_ROOT/worktrees"
NOV_BASE="$NOV_ROOT/base"
mkdir -p "$NOV_WORKTREES" "$NOV_BASE"

make_repo "$NOV_REPO"

mkdir -p "$NOV_BASE/target.gen.1"
touch "$NOV_BASE/target.gen.1.lock"
ln -sfn "$NOV_BASE/target.gen.1" "$NOV_BASE/target"

git -C "$NOV_REPO" worktree add -q "$NOV_WORKTREES/_mainsweep-abcd1234"
touch "$NOV_WORKTREES/_mainsweep-abcd1234/PROTECTED_MARKER"

NOV_SEED_LOG="$NOV_ROOT/seed_calls.log"
NOV_SEED_STUB="$NOV_ROOT/seed_stub.sh"
cat > "$NOV_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
exit 0
STUB_EOF
chmod +x "$NOV_SEED_STUB"
export SEED_LOG="$NOV_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$NOV_WORKTREES" \
    --base-target "$NOV_BASE/target" \
    --protect-glob "_merge-*" \
    --seed-script "$NOV_SEED_STUB"

assert "N-override: exit 0" test "$RC" -eq 0
assert "N-override: explicit --protect-glob _merge-* re-narrows — _mainsweep-abcd1234 IS removed" \
    bash -c '[ ! -d "$1" ]' _ "$NOV_WORKTREES/_mainsweep-abcd1234"
assert "N-override: _mainsweep-abcd1234 absent from git worktree list" \
    bash -c '! git -C "$1" worktree list | grep -q "_mainsweep-abcd1234"' _ "$NOV_REPO"
assert "N-override: summary shows removed=1" \
    bash -c 'printf "%s\n" "$1" | grep -qE "removed=1"' _ "$OUT"

test_summary
