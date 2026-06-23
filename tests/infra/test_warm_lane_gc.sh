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

test_summary
