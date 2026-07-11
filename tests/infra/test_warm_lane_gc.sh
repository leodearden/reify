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
#       reclaim missing --worktrees-dir or --base-target
#   B — reset a divergent FREE lane (seed-script invoked with resolved gen path)
#   C — remove an orphaned-landed clean worktree
#   D — preserve dirty WIP (dirty tracked changes)
#   E — preserve unlanded ahead-of-main commits
#   F — preserve a lane with a live-consumer lock
#   G — combined PRD δ signal: all five fixtures + protect-glob + summary line
#   I — Tier-3 terminal-task reclaim: attached task/NNNN branch, status-oracle
#       seam (--status-cmd / REIFY_LANE_LEAK_STATUS_CMD fallback) (task 5167)
#   J — Tier-3 backing-task resolution for a detached HEAD lane (task 5167)
#   K — disk-pressure fast-path: rm -rf target/ instead of an alpha reseed
#       clone (--disk-pressure / REIFY_WARM_LANE_GC_DISK_PRESSURE) (task 5167)
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
# Shared scaffolding: status-oracle stubs + task-lane factory (Blocks I/J/K)
# ──────────────────────────────────────────────────────────────────────────────
# Status-oracle contract mirrors tests/infra/test_warm_lane_preflight.sh Block D
# (leak-oracle.sh / leak-oracle-fail.sh) and warm-lane-preflight.sh Check 6 /
# warm-lane-degenerate-ref-check.sh _ref_status byte-for-byte: `<cmd> <task_id>`
# prints a status on stdout (or empty for unknown ids), exits 0. Threaded via
# ORACLE_MAP (one "<id> <status>" pair per line, exported before run_helper).

STUB_DIR="$(mktemp -d /tmp/test-warm-lane-gc-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

# gc-status-oracle.sh: given task-id $1, looks up status in ORACLE_MAP file
# (one "id status" pair per line). Exits 0 with empty output for unknown ids.
cat > "$STUB_DIR/gc-status-oracle.sh" << 'STUB_EOF'
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
chmod +x "$STUB_DIR/gc-status-oracle.sh"

# gc-status-oracle-fail.sh: always exits non-zero — drives the set -e/pipefail
# hardening test (oracle failure must NOT abort the sweep; unknown = non-terminal).
cat > "$STUB_DIR/gc-status-oracle-fail.sh" << 'STUB_EOF'
#!/usr/bin/env bash
exit 1
STUB_EOF
chmod +x "$STUB_DIR/gc-status-oracle-fail.sh"

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
# Block D — preserve dirty WIP
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: preserve dirty WIP ---"

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

# _lane-2: dirty tracked change (file modified but not committed)
git -C "$D_REPO" worktree add -q "$D_WORKTREES/_lane-2"
echo "dirty" >> "$D_WORKTREES/_lane-2/README.md"
mkdir -p "$D_WORKTREES/_lane-2/target"
touch "$D_WORKTREES/_lane-2/target/DIVERGENT_MARKER"

# task-8888: dirty orphan (modified tracked file)
git -C "$D_REPO" worktree add -q "$D_WORKTREES/task-8888"
echo "dirty" >> "$D_WORKTREES/task-8888/README.md"

D_SEED_LOG="$D_ROOT/seed_calls.log"
D_SEED_STUB="$D_ROOT/seed_stub.sh"
cat > "$D_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
exit 0
STUB_EOF
chmod +x "$D_SEED_STUB"
export SEED_LOG="$D_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$D_WORKTREES" \
    --base-target "$D_BASE/target" \
    --seed-script "$D_SEED_STUB"

assert "D1: exit 0" test "$RC" -eq 0
# Dirty lane NOT reset: seed-script must NOT be invoked for it
assert "D2: dirty lane seed-script NOT invoked" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_lane-2" "$1"' _ "$D_SEED_LOG"
# Dirty lane marker still present (not thinned)
assert "D3: dirty lane divergent marker intact" \
    test -f "$D_WORKTREES/_lane-2/target/DIVERGENT_MARKER"
# Dirty orphan NOT removed
assert "D4: dirty orphan task-8888 still present" \
    test -d "$D_WORKTREES/task-8888"
# Stderr should mention preserving dirty WIP
assert "D5: stderr mentions dirty WIP preservation" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "dirty|preserving|wip|tracked"' _ "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — preserve unlanded ahead-of-main commits
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: preserve unlanded ahead-of-main commits ---"

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

# _lane-3: clean but has a committed change NOT in main (ahead-of-main)
git -C "$E_REPO" worktree add -q "$E_WORKTREES/_lane-3"
echo "ahead" >> "$E_WORKTREES/_lane-3/README.md"
git -C "$E_WORKTREES/_lane-3" add README.md
git -C "$E_WORKTREES/_lane-3" commit -q -m "ahead-of-main commit"
mkdir -p "$E_WORKTREES/_lane-3/target"
touch "$E_WORKTREES/_lane-3/target/DIVERGENT_MARKER"

# task-7777: clean but has a committed change NOT in main
git -C "$E_REPO" worktree add -q "$E_WORKTREES/task-7777"
echo "ahead" >> "$E_WORKTREES/task-7777/README.md"
git -C "$E_WORKTREES/task-7777" add README.md
git -C "$E_WORKTREES/task-7777" commit -q -m "ahead-of-main commit"

E_SEED_LOG="$E_ROOT/seed_calls.log"
E_SEED_STUB="$E_ROOT/seed_stub.sh"
cat > "$E_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
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
# Ahead lane NOT reset
assert "E2: ahead-of-main lane seed-script NOT invoked" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_lane-3" "$1"' _ "$E_SEED_LOG"
assert "E3: ahead-of-main lane divergent marker intact" \
    test -f "$E_WORKTREES/_lane-3/target/DIVERGENT_MARKER"
# Ahead orphan NOT removed
assert "E4: ahead-of-main orphan task-7777 still present" \
    test -d "$E_WORKTREES/task-7777"
# Stderr should mention unlanded
assert "E5: stderr mentions unlanded/ahead-of-main preservation" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "unlanded|ahead|preserving"' _ "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block F — preserve a lane with a live-consumer lock
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: preserve live-consumer-locked lane ---"

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

# _lane-4: clean, landed, but has a live consumer holding the exclusive lock
git -C "$F_REPO" worktree add -q "$F_WORKTREES/_lane-4"
mkdir -p "$F_WORKTREES/_lane-4/target"
touch "$F_WORKTREES/_lane-4/target/DIVERGENT_MARKER"

# Create the lock file and hold it with a background process
touch "$F_WORKTREES/_lane-4.lock"
# Use a background flock to hold the exclusive lock
( flock -x 9 && sleep 300 ) 9>"$F_WORKTREES/_lane-4.lock" &
F_LOCK_PID=$!
_BGPIDS+=("$F_LOCK_PID")
# Give the background process a moment to acquire the lock
sleep 0.1

F_SEED_LOG="$F_ROOT/seed_calls.log"
F_SEED_STUB="$F_ROOT/seed_stub.sh"
cat > "$F_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
exit 0
STUB_EOF
chmod +x "$F_SEED_STUB"
export SEED_LOG="$F_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$F_WORKTREES" \
    --base-target "$F_BASE/target" \
    --seed-script "$F_SEED_STUB"

assert "F1: exit 0" test "$RC" -eq 0
# Lane with live consumer must NOT be reset
assert "F2: live-consumer lane seed-script NOT invoked" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_lane-4" "$1"' _ "$F_SEED_LOG"
# Divergent marker still present (not thinned)
assert "F3: live-consumer lane divergent marker intact" \
    test -f "$F_WORKTREES/_lane-4/target/DIVERGENT_MARKER"
# Stderr should mention live consumer
assert "F4: stderr mentions live consumer preservation" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "live.consumer|locked|preserving|consumer"' _ "$ERR_OUT"

# Release the lock
kill "$F_LOCK_PID" 2>/dev/null || true
_BGPIDS=()  # clear so cleanup doesn't double-kill

# ──────────────────────────────────────────────────────────────────────────────
# Block G — combined PRD δ signal: all five fixtures + protect-glob + summary
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: combined PRD delta signal ---"

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

# Fixture 3: dirty-WIP lane (_lane-dirty)
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

# Reclaimable lane was reset
assert "G2: reclaimable lane _lane-free was reset (seed-script invoked)" \
    bash -c 'grep -q "_lane-free" "$1"' _ "$G_SEED_LOG"
assert "G3: reclaimable lane divergent marker removed" \
    bash -c '[ ! -f "$1" ]' _ "$G_WORKTREES/_lane-free/target/DIVERGENT_MARKER"

# Reclaimable orphan was removed
assert "G4: reclaimable orphan task-free was removed" \
    bash -c '[ ! -d "$1" ]' _ "$G_WORKTREES/task-free"

# Three protected fixtures preserved
assert "G5: dirty lane _lane-dirty marker intact" \
    test -f "$G_WORKTREES/_lane-dirty/target/DIVERGENT_MARKER"
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
assert "G10: summary shows reset=1" \
    bash -c 'printf "%s\n" "$1" | grep -qE "reset=1"' _ "$OUT"
assert "G11: summary shows removed=1" \
    bash -c 'printf "%s\n" "$1" | grep -qE "removed=1"' _ "$OUT"
assert "G12: summary shows preserved=4" \
    bash -c 'printf "%s\n" "$1" | grep -qE "preserved=4"' _ "$OUT"

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
# Block I — Tier-3 terminal-task reclaim (attached task/NNNN branch)
# A FREE lane whose backing task/NNNN is terminal (done|cancelled) is
# reclaimable REGARDLESS of ahead-of-main (rebase-orphan tip) — closing the
# 2026-07-10 ENOSPC leak (task 5167). Each sub-case gets its own repo/
# worktrees/base under a shared I_ROOT.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block I: Tier-3 terminal-task reclaim (attached branch) ---"

I_ROOT="$(mktemp -d /tmp/test-gc-i-XXXXXX)"
_TMPDIRS+=("$I_ROOT")

_seed_stub_body() {
    cat << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
rm -rf "$LANE_DIR/target/DIVERGENT_MARKER" 2>/dev/null || true
exit 0
STUB_EOF
}

# ── I1: done-task, ahead-of-main (rebase-orphan) lane IS reclaimed ─────────────
I1_REPO="$I_ROOT/i1-repo"
I1_WORKTREES="$I_ROOT/i1-worktrees"
I1_BASE="$I_ROOT/i1-base"
mkdir -p "$I1_WORKTREES" "$I1_BASE"
make_repo "$I1_REPO"
mkdir -p "$I1_BASE/target.gen.1"
touch "$I1_BASE/target.gen.1.lock"
ln -sfn "$I1_BASE/target.gen.1" "$I1_BASE/target"

make_task_lane "$I1_REPO" "$I1_WORKTREES" "_lane-1" "task/4827" "ahead"

I1_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$I1_MAP")
printf '4827 done\n' > "$I1_MAP"

I1_SEED_LOG="$I_ROOT/i1-seed-calls.log"
I1_SEED_STUB="$I_ROOT/i1-seed-stub.sh"
_seed_stub_body > "$I1_SEED_STUB"
chmod +x "$I1_SEED_STUB"
export SEED_LOG="$I1_SEED_LOG"
export ORACLE_MAP="$I1_MAP"

run_helper reclaim \
    --worktrees-dir "$I1_WORKTREES" \
    --base-target "$I1_BASE/target" \
    --seed-script "$I1_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle.sh"

assert "I1: exit 0" test "$RC" -eq 0
assert "I1: done-task ahead-of-main lane seed-script invoked (reclaimed)" \
    bash -c 'test -f "$1" && grep -q "_lane-1" "$1"' _ "$I1_SEED_LOG"
assert "I1: divergent target marker removed" \
    bash -c '[ ! -f "$1" ]' _ "$I1_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# ── I2: cancelled-task, ahead-of-main lane IS reclaimed ────────────────────────
I2_REPO="$I_ROOT/i2-repo"
I2_WORKTREES="$I_ROOT/i2-worktrees"
I2_BASE="$I_ROOT/i2-base"
mkdir -p "$I2_WORKTREES" "$I2_BASE"
make_repo "$I2_REPO"
mkdir -p "$I2_BASE/target.gen.1"
touch "$I2_BASE/target.gen.1.lock"
ln -sfn "$I2_BASE/target.gen.1" "$I2_BASE/target"

make_task_lane "$I2_REPO" "$I2_WORKTREES" "_lane-1" "task/5033" "ahead"

I2_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$I2_MAP")
printf '5033 cancelled\n' > "$I2_MAP"

I2_SEED_LOG="$I_ROOT/i2-seed-calls.log"
I2_SEED_STUB="$I_ROOT/i2-seed-stub.sh"
_seed_stub_body > "$I2_SEED_STUB"
chmod +x "$I2_SEED_STUB"
export SEED_LOG="$I2_SEED_LOG"
export ORACLE_MAP="$I2_MAP"

run_helper reclaim \
    --worktrees-dir "$I2_WORKTREES" \
    --base-target "$I2_BASE/target" \
    --seed-script "$I2_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle.sh"

assert "I2: exit 0" test "$RC" -eq 0
assert "I2: cancelled-task ahead-of-main lane seed-script invoked (reclaimed)" \
    bash -c 'test -f "$1" && grep -q "_lane-1" "$1"' _ "$I2_SEED_LOG"
assert "I2: divergent target marker removed" \
    bash -c '[ ! -f "$1" ]' _ "$I2_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# ── I3: non-terminal (pending), ahead-of-main lane is PRESERVED ────────────────
I3_REPO="$I_ROOT/i3-repo"
I3_WORKTREES="$I_ROOT/i3-worktrees"
I3_BASE="$I_ROOT/i3-base"
mkdir -p "$I3_WORKTREES" "$I3_BASE"
make_repo "$I3_REPO"
mkdir -p "$I3_BASE/target.gen.1"
touch "$I3_BASE/target.gen.1.lock"
ln -sfn "$I3_BASE/target.gen.1" "$I3_BASE/target"

make_task_lane "$I3_REPO" "$I3_WORKTREES" "_lane-1" "task/9001" "ahead"

I3_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$I3_MAP")
printf '9001 pending\n' > "$I3_MAP"

I3_SEED_LOG="$I_ROOT/i3-seed-calls.log"
I3_SEED_STUB="$I_ROOT/i3-seed-stub.sh"
_seed_stub_body > "$I3_SEED_STUB"
chmod +x "$I3_SEED_STUB"
export SEED_LOG="$I3_SEED_LOG"
export ORACLE_MAP="$I3_MAP"

run_helper reclaim \
    --worktrees-dir "$I3_WORKTREES" \
    --base-target "$I3_BASE/target" \
    --seed-script "$I3_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle.sh"

assert "I3: exit 0" test "$RC" -eq 0
assert "I3: non-terminal ahead-of-main lane seed-script NOT invoked (preserved)" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_lane-1" "$1"' _ "$I3_SEED_LOG"
assert "I3: non-terminal lane divergent marker intact" \
    test -f "$I3_WORKTREES/_lane-1/target/DIVERGENT_MARKER"
assert "I3: stderr names ahead-of-main preservation" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "unlanded|ahead|preserving"' _ "$ERR_OUT"

# ── I4: oracle-failure lane is PRESERVED; script still exits 0 ─────────────────
I4_REPO="$I_ROOT/i4-repo"
I4_WORKTREES="$I_ROOT/i4-worktrees"
I4_BASE="$I_ROOT/i4-base"
mkdir -p "$I4_WORKTREES" "$I4_BASE"
make_repo "$I4_REPO"
mkdir -p "$I4_BASE/target.gen.1"
touch "$I4_BASE/target.gen.1.lock"
ln -sfn "$I4_BASE/target.gen.1" "$I4_BASE/target"

make_task_lane "$I4_REPO" "$I4_WORKTREES" "_lane-1" "task/6000" "ahead"

I4_SEED_LOG="$I_ROOT/i4-seed-calls.log"
I4_SEED_STUB="$I_ROOT/i4-seed-stub.sh"
_seed_stub_body > "$I4_SEED_STUB"
chmod +x "$I4_SEED_STUB"
export SEED_LOG="$I4_SEED_LOG"

run_helper reclaim \
    --worktrees-dir "$I4_WORKTREES" \
    --base-target "$I4_BASE/target" \
    --seed-script "$I4_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle-fail.sh"

assert "I4: exit 0 (fail-oracle does not abort the sweep)" test "$RC" -eq 0
assert "I4: fail-oracle lane seed-script NOT invoked (unknown=non-terminal, preserved)" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_lane-1" "$1"' _ "$I4_SEED_LOG"
assert "I4: fail-oracle lane divergent marker intact" \
    test -f "$I4_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# ── I5: env fallback — REIFY_LANE_LEAK_STATUS_CMD with NO --status-cmd flag ────
I5_REPO="$I_ROOT/i5-repo"
I5_WORKTREES="$I_ROOT/i5-worktrees"
I5_BASE="$I_ROOT/i5-base"
mkdir -p "$I5_WORKTREES" "$I5_BASE"
make_repo "$I5_REPO"
mkdir -p "$I5_BASE/target.gen.1"
touch "$I5_BASE/target.gen.1.lock"
ln -sfn "$I5_BASE/target.gen.1" "$I5_BASE/target"

make_task_lane "$I5_REPO" "$I5_WORKTREES" "_lane-1" "task/4827" "ahead"

I5_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$I5_MAP")
printf '4827 done\n' > "$I5_MAP"

I5_SEED_LOG="$I_ROOT/i5-seed-calls.log"
I5_SEED_STUB="$I_ROOT/i5-seed-stub.sh"
_seed_stub_body > "$I5_SEED_STUB"
chmod +x "$I5_SEED_STUB"
export SEED_LOG="$I5_SEED_LOG"

ORACLE_MAP="$I5_MAP" REIFY_LANE_LEAK_STATUS_CMD="$STUB_DIR/gc-status-oracle.sh" \
    run_helper reclaim \
        --worktrees-dir "$I5_WORKTREES" \
        --base-target "$I5_BASE/target" \
        --seed-script "$I5_SEED_STUB"

assert "I5: exit 0" test "$RC" -eq 0
assert "I5: env-fallback done-task lane seed-script invoked (reclaimed)" \
    bash -c 'test -f "$1" && grep -q "_lane-1" "$1"' _ "$I5_SEED_LOG"
assert "I5: env-fallback divergent target marker removed" \
    bash -c '[ ! -f "$1" ]' _ "$I5_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# ── I6: live-consumer flock on a done-task lane is STILL preserved ─────────────
I6_REPO="$I_ROOT/i6-repo"
I6_WORKTREES="$I_ROOT/i6-worktrees"
I6_BASE="$I_ROOT/i6-base"
mkdir -p "$I6_WORKTREES" "$I6_BASE"
make_repo "$I6_REPO"
mkdir -p "$I6_BASE/target.gen.1"
touch "$I6_BASE/target.gen.1.lock"
ln -sfn "$I6_BASE/target.gen.1" "$I6_BASE/target"

make_task_lane "$I6_REPO" "$I6_WORKTREES" "_lane-1" "task/4827" "ahead"

I6_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$I6_MAP")
printf '4827 done\n' > "$I6_MAP"

# Hold the lane's exclusive flock in the background (live consumer).
# Causal handshake (technique R, #4847) instead of a fixed sleep: the
# subshell touches a READY marker AFTER acquiring flock -x, and we poll for
# it, so the assertions below only run once the flock is provably held —
# a fixed `sleep 0.1` races the backgrounded acquisition under CPU/IO load,
# in which case the GC sweep could win the lock first and reclaim the lane,
# spuriously failing I6.
touch "$I6_WORKTREES/_lane-1.lock"
I6_READY="$I6_WORKTREES/_lane-1.lock.ready-marker"
( flock -x 9 && touch "$I6_READY" && sleep 300 ) 9>"$I6_WORKTREES/_lane-1.lock" &
I6_LOCK_PID=$!
_BGPIDS+=("$I6_LOCK_PID")
_wait_for_reader_lock "$I6_READY" 30

I6_SEED_LOG="$I_ROOT/i6-seed-calls.log"
I6_SEED_STUB="$I_ROOT/i6-seed-stub.sh"
_seed_stub_body > "$I6_SEED_STUB"
chmod +x "$I6_SEED_STUB"
export SEED_LOG="$I6_SEED_LOG"
export ORACLE_MAP="$I6_MAP"

run_helper reclaim \
    --worktrees-dir "$I6_WORKTREES" \
    --base-target "$I6_BASE/target" \
    --seed-script "$I6_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle.sh"

assert "I6: exit 0" test "$RC" -eq 0
assert "I6: live-consumer done-task lane seed-script NOT invoked (flock wins)" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_lane-1" "$1"' _ "$I6_SEED_LOG"
assert "I6: live-consumer lane divergent marker intact" \
    test -f "$I6_WORKTREES/_lane-1/target/DIVERGENT_MARKER"
assert "I6: stderr mentions live consumer preservation" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "live.consumer|locked|preserving|consumer"' _ "$ERR_OUT"

# Release the lock
kill "$I6_LOCK_PID" 2>/dev/null || true
_BGPIDS=()  # clear so cleanup doesn't double-kill

# ── I7: done-task lane with DIRTY tracked changes (uncommitted) IS reclaimed ───
# The PRD claim is "regardless of ahead-of-main AND dirty tracked changes"
# (warm-lane-gc.sh Tier-3 docstring); I1/I2/I5/I6 only ever exercise the
# ahead-of-main dimension. This proves Tier-3 also overrides
# _is_reclaimable's dirty-WIP check, not only its ahead-of-main check — a
# lane that's on its task/NNNN branch tip (not ahead) but has an uncommitted
# tracked-file edit.
I7_REPO="$I_ROOT/i7-repo"
I7_WORKTREES="$I_ROOT/i7-worktrees"
I7_BASE="$I_ROOT/i7-base"
mkdir -p "$I7_WORKTREES" "$I7_BASE"
make_repo "$I7_REPO"
mkdir -p "$I7_BASE/target.gen.1"
touch "$I7_BASE/target.gen.1.lock"
ln -sfn "$I7_BASE/target.gen.1" "$I7_BASE/target"

make_task_lane "$I7_REPO" "$I7_WORKTREES" "_lane-1" "task/4827"
# Dirty tracked change: modify README.md WITHOUT committing, so
# `git status --porcelain --untracked-files=no` is non-empty.
echo "dirty uncommitted change" >> "$I7_WORKTREES/_lane-1/README.md"

I7_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$I7_MAP")
printf '4827 done\n' > "$I7_MAP"

I7_SEED_LOG="$I_ROOT/i7-seed-calls.log"
I7_SEED_STUB="$I_ROOT/i7-seed-stub.sh"
_seed_stub_body > "$I7_SEED_STUB"
chmod +x "$I7_SEED_STUB"
export SEED_LOG="$I7_SEED_LOG"
export ORACLE_MAP="$I7_MAP"

run_helper reclaim \
    --worktrees-dir "$I7_WORKTREES" \
    --base-target "$I7_BASE/target" \
    --seed-script "$I7_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle.sh"

assert "I7: exit 0" test "$RC" -eq 0
assert "I7: dirty done-task lane seed-script invoked (reclaimed despite dirty tracked changes)" \
    bash -c 'test -f "$1" && grep -q "_lane-1" "$1"' _ "$I7_SEED_LOG"
assert "I7: divergent target marker removed" \
    bash -c '[ ! -f "$1" ]' _ "$I7_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# ──────────────────────────────────────────────────────────────────────────────
# Block J — Tier-3 backing-task resolution for a detached HEAD lane
# Proves the resolver both (J1) resolves a detached HEAD to its containing
# task/NNNN branch when exactly one such branch exists, and (J2) does NOT
# over-match when no task/* branch is reachable from HEAD at all — that
# fixture must fall through to the existing (unchanged) tiers.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block J: Tier-3 backing-task resolution (detached HEAD) ---"

J_ROOT="$(mktemp -d /tmp/test-gc-j-XXXXXX)"
_TMPDIRS+=("$J_ROOT")

# ── J1: detached HEAD at the tip of task/7777 (ahead-of-main), oracle done ──────
# Proves detached HEAD resolves to its (sole) containing task branch.
J1_REPO="$J_ROOT/j1-repo"
J1_WORKTREES="$J_ROOT/j1-worktrees"
J1_BASE="$J_ROOT/j1-base"
mkdir -p "$J1_WORKTREES" "$J1_BASE"
make_repo "$J1_REPO"
mkdir -p "$J1_BASE/target.gen.1"
touch "$J1_BASE/target.gen.1.lock"
ln -sfn "$J1_BASE/target.gen.1" "$J1_BASE/target"

make_task_lane "$J1_REPO" "$J1_WORKTREES" "_lane-1" "task/7777" "ahead"
# Detach HEAD at the (still-ahead-of-main) tip of task/7777; the branch ref
# itself is left in place, only the worktree's HEAD becomes detached.
git -C "$J1_WORKTREES/_lane-1" checkout -q --detach

J1_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$J1_MAP")
printf '7777 done\n' > "$J1_MAP"

J1_SEED_LOG="$J_ROOT/j1-seed-calls.log"
J1_SEED_STUB="$J_ROOT/j1-seed-stub.sh"
_seed_stub_body > "$J1_SEED_STUB"
chmod +x "$J1_SEED_STUB"
export SEED_LOG="$J1_SEED_LOG"
export ORACLE_MAP="$J1_MAP"

run_helper reclaim \
    --worktrees-dir "$J1_WORKTREES" \
    --base-target "$J1_BASE/target" \
    --seed-script "$J1_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle.sh"

assert "J1: exit 0" test "$RC" -eq 0
assert "J1: detached-HEAD done-task lane seed-script invoked (reclaimed)" \
    bash -c 'test -f "$1" && grep -q "_lane-1" "$1"' _ "$J1_SEED_LOG"
assert "J1: divergent target marker removed" \
    bash -c '[ ! -f "$1" ]' _ "$J1_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# ── J2: detached HEAD reachable from NO task/* branch — no over-match ──────────
# Separate repo with zero task/* branches ever created: HEAD == main tip
# (landed, clean), so the resolver must find no containing task branch and
# fall through to the existing tiers, which already reclaim a landed/clean lane.
J2_REPO="$J_ROOT/j2-repo"
J2_WORKTREES="$J_ROOT/j2-worktrees"
J2_BASE="$J_ROOT/j2-base"
mkdir -p "$J2_WORKTREES" "$J2_BASE"
make_repo "$J2_REPO"
mkdir -p "$J2_BASE/target.gen.1"
touch "$J2_BASE/target.gen.1.lock"
ln -sfn "$J2_BASE/target.gen.1" "$J2_BASE/target"

git -C "$J2_REPO" worktree add -q --detach "$J2_WORKTREES/_lane-1"
mkdir -p "$J2_WORKTREES/_lane-1/target"
touch "$J2_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

J2_SEED_LOG="$J_ROOT/j2-seed-calls.log"
J2_SEED_STUB="$J_ROOT/j2-seed-stub.sh"
_seed_stub_body > "$J2_SEED_STUB"
chmod +x "$J2_SEED_STUB"
export SEED_LOG="$J2_SEED_LOG"
# No ORACLE_MAP entry can match (no task id resolves) — the fixture's outcome
# must be driven purely by the pre-existing tiers, not the status oracle.

run_helper reclaim \
    --worktrees-dir "$J2_WORKTREES" \
    --base-target "$J2_BASE/target" \
    --seed-script "$J2_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle.sh"

assert "J2: exit 0" test "$RC" -eq 0
assert "J2: no-task-branch detached lane seed-script invoked (reclaimed via existing landed/clean tier)" \
    bash -c 'test -f "$1" && grep -q "_lane-1" "$1"' _ "$J2_SEED_LOG"
assert "J2: divergent target marker removed" \
    bash -c '[ ! -f "$1" ]' _ "$J2_WORKTREES/_lane-1/target/DIVERGENT_MARKER"

# ── J3: detached HEAD reachable from TWO task/* branches — ambiguous, no over-match ─
# Proves the exactly-one-match guard in _backing_task_id (ids array length
# check): when two task/* branches both contain HEAD, the resolver must
# yield NO id — not silently pick one — even though one of the two
# candidate ids maps to a terminal status in the oracle. The lane must fall
# through to the existing ahead-of-main tier and be PRESERVED, proving the
# ambiguous match never drives a Tier-3 reclaim.
J3_REPO="$J_ROOT/j3-repo"
J3_WORKTREES="$J_ROOT/j3-worktrees"
J3_BASE="$J_ROOT/j3-base"
mkdir -p "$J3_WORKTREES" "$J3_BASE"
make_repo "$J3_REPO"
mkdir -p "$J3_BASE/target.gen.1"
touch "$J3_BASE/target.gen.1.lock"
ln -sfn "$J3_BASE/target.gen.1" "$J3_BASE/target"

make_task_lane "$J3_REPO" "$J3_WORKTREES" "_lane-1" "task/8881" "ahead"
# Second branch pointing at the SAME (ahead-of-main) commit as task/8881's
# tip, so it ALSO "contains" the detached HEAD below — the ambiguous case.
git -C "$J3_WORKTREES/_lane-1" branch -q task/8882 HEAD
git -C "$J3_WORKTREES/_lane-1" checkout -q --detach

J3_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$J3_MAP")
printf '8881 done\n' > "$J3_MAP"

J3_SEED_LOG="$J_ROOT/j3-seed-calls.log"
J3_SEED_STUB="$J_ROOT/j3-seed-stub.sh"
_seed_stub_body > "$J3_SEED_STUB"
chmod +x "$J3_SEED_STUB"
export SEED_LOG="$J3_SEED_LOG"
export ORACLE_MAP="$J3_MAP"

run_helper reclaim \
    --worktrees-dir "$J3_WORKTREES" \
    --base-target "$J3_BASE/target" \
    --seed-script "$J3_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle.sh"

assert "J3: exit 0" test "$RC" -eq 0
assert "J3: ambiguous-branch lane seed-script NOT invoked (no Tier-3 over-match)" \
    bash -c '[ ! -f "$1" ] || ! grep -q "_lane-1" "$1"' _ "$J3_SEED_LOG"
assert "J3: ambiguous-branch lane divergent marker intact (preserved via ahead-of-main tier)" \
    test -f "$J3_WORKTREES/_lane-1/target/DIVERGENT_MARKER"
assert "J3: stderr names ahead-of-main preservation (fell through Tier-3)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "unlanded|ahead|preserving"' _ "$ERR_OUT"

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

K1_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$K1_MAP")
printf '4827 done\n' > "$K1_MAP"

K1_SEED_LOG="$K_ROOT/k1-seed-calls.log"
K1_SEED_STUB="$K_ROOT/k1-seed-stub.sh"
_seed_stub_body > "$K1_SEED_STUB"
chmod +x "$K1_SEED_STUB"
export SEED_LOG="$K1_SEED_LOG"
export ORACLE_MAP="$K1_MAP"

run_helper reclaim \
    --worktrees-dir "$K1_WORKTREES" \
    --base-target "$K1_BASE/target" \
    --seed-script "$K1_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle.sh" \
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

K2_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$K2_MAP")
printf '4827 done\n' > "$K2_MAP"

K2_SEED_LOG="$K_ROOT/k2-seed-calls.log"
K2_SEED_STUB="$K_ROOT/k2-seed-stub.sh"
_seed_stub_body > "$K2_SEED_STUB"
chmod +x "$K2_SEED_STUB"
export SEED_LOG="$K2_SEED_LOG"
export ORACLE_MAP="$K2_MAP"

run_helper reclaim \
    --worktrees-dir "$K2_WORKTREES" \
    --base-target "$K2_BASE/target" \
    --seed-script "$K2_SEED_STUB" \
    --status-cmd "$STUB_DIR/gc-status-oracle.sh"

assert "K2: exit 0" test "$RC" -eq 0
assert "K2: seed-script invoked (normal alpha reset path, no disk-pressure)" \
    bash -c 'test -f "$1" && grep -q "_lane-1" "$1"' _ "$K2_SEED_LOG"
assert "K2: target/ directory still present (thinned by alpha, not deleted)" \
    test -d "$K2_WORKTREES/_lane-1/target"
assert "K2: summary shows reset=1" \
    bash -c 'printf "%s\n" "$1" | grep -qE "reset=1"' _ "$OUT"

# ── K3: landed-clean NON-task-branch lane under --disk-pressure ────────────────
# --disk-pressure is documented (usage()/header) as applying to EVERY
# reclaimable Pass-1 lane, not only Tier-3-reclaimed leaked lanes. This locks
# that pool-wide scope: an ordinary landed/clean lane (no task/NNNN branch,
# no --status-cmd wired, reclaimed purely via _is_reclaimable) also gets its
# target/ deleted outright under --disk-pressure rather than alpha-reseeded.
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

K4_MAP="$(mktemp /tmp/test-gc-oracle-map-XXXXXX)"
_TMPDIRS+=("$K4_MAP")
printf '4827 done\n' > "$K4_MAP"

K4_SEED_LOG="$K_ROOT/k4-seed-calls.log"
K4_SEED_STUB="$K_ROOT/k4-seed-stub.sh"
_seed_stub_body > "$K4_SEED_STUB"
chmod +x "$K4_SEED_STUB"
export SEED_LOG="$K4_SEED_LOG"
export ORACLE_MAP="$K4_MAP"

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
    --status-cmd "$STUB_DIR/gc-status-oracle.sh" \
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

test_summary
