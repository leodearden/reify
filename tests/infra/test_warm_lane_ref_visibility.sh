#!/usr/bin/env bash
# tests/infra/test_warm_lane_ref_visibility.sh
# Hermetic tests for scripts/warm-lane-ref-check.sh.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# PATH stubs (Block B only):
#   git — counter-based stub: intercepts `rev-parse --verify refs/heads/task/*`
#         only; delegates ALL other git subcommands to the real git.
#         Fails (exit 1) while counter <= $REIFY_GIT_STUB_FAIL_UNTIL,
#         then delegates to real git.
#         Records every invocation to $REIFY_TEST_CALLS_FILE.
#
# Blocks:
#   A — reify-provisioning-innocence: real git worktree fixture + seed + clean,
#         ref resolves before AND after (proves reify primitives don't perturb refs)
#   B — deterministic TOCTOU-class reproduction (added in step-3): counter git
#         stub proves single-shot fails / bounded-retry succeeds
#   C — exit-code taxonomy + read-only invariant (added in step-5)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.
#
# T8 de-flake audit (task #4847):
#   ZERO absolute-wall-clock-upper-bound or scheduling-latency assertions.
#   All assertions are structural (exit codes, stdout SHA grep, stderr markers,
#   CALLS_FILE argv). The TOCTOU race in Block B is reproduced deterministically
#   via a counter-based git stub (no sleeps, no real concurrency).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-ref-check.sh"
SEED_SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"

# Resolve real git once so the Block B stub can delegate to it.
REAL_GIT="$(command -v git)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-ref-check.sh hermetic tests (task 4855) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp /tmp/test-warm-lane-ref-vis-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ────────────────────────────────────────────────────────────────
# Invokes the script under test with no PATH stub.
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ─────────────────────────────────────────────────────────────────────────────
# Block A — reify-provisioning-innocence guard
#
# Proves that the reify warm-lane provisioning primitives (seed-warm-lane.sh
# --fresh-checkout and reset_lane's git clean -xfd -e target) NEVER perturb
# ref-visibility in the linked worktree.
#
# Fixture: a hermetic main-checkout repo with two task branches (9999 and 9998),
# a linked worktree simulating a warm lane.  The ref-check must resolve both
# branches from the lane before and after provisioning.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: reify-provisioning-innocence ---"

A_TMP="$(mktemp -d /tmp/test-warm-lane-ref-vis-a-XXXXXX)"
_TMPDIRS+=("$A_TMP")

# ── A-fixture: build hermetic git repos ──────────────────────────────────────
A_MAIN="$A_TMP/main"
A_LANE="$A_TMP/lane"

git init -q -b main "$A_MAIN"
git -C "$A_MAIN" config user.email "test@test.local"
git -C "$A_MAIN" config user.name "Test"
touch "$A_MAIN/README.md"
git -C "$A_MAIN" add README.md
git -C "$A_MAIN" commit -q -m "initial"

# git worktree add creates the linked worktree AND the branch task/9999.
# The branch lives in the main checkout's .git/refs/heads/task/9999 (shared).
git -C "$A_MAIN" worktree add -b task/9999 "$A_LANE" main

# Create a second branch in main (also visible from the lane via shared refs).
git -C "$A_MAIN" branch task/9998 main

# ── A1: both refs resolve from the lane before any provisioning ───────────────
run_helper --lane "$A_LANE" --task 9999 --expect-common-dir "$A_MAIN/.git"
assert "A1: task/9999 resolves from lane (exits 0)" test "$RC" -eq 0
assert "A1: stdout is a 40-hex SHA" \
    bash -c 'printf "%s" "$1" | grep -qxE "[0-9a-f]{40}"' _ "$OUT"

run_helper --lane "$A_LANE" --task 9998 --expect-common-dir "$A_MAIN/.git"
assert "A2: task/9998 resolves from lane via shared refs (exits 0)" test "$RC" -eq 0
assert "A2: stdout is a 40-hex SHA" \
    bash -c 'printf "%s" "$1" | grep -qxE "[0-9a-f]{40}"' _ "$OUT"

# ── A3: exercise the reify provisioning path ──────────────────────────────────
# Set up a dummy base_target with sidecar so seed-warm-lane.sh guards pass.
A_BASE_PARENT="$A_TMP/base-parent"
A_BASE_TARGET="$A_BASE_PARENT/target"
mkdir -p "$A_BASE_TARGET"
echo "dummy-artifact" > "$A_BASE_TARGET/dummy.rlib"

# Record base provenance (empty RUSTFLAGS + INVOCATION) so guards pass.
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
    bash "$SEED_SCRIPT" --record-base "$A_BASE_TARGET" >/dev/null

# Probe for XFS reflink support (seed-warm-lane.sh uses cp --reflink=always).
# If the current filesystem supports reflinks, run a real seed;
# otherwise skip the reflink step but still exercise git clean (which is
# all that is needed to prove git-only ops don't perturb refs).
_A_probe_src="$A_TMP/probe.src"
_A_probe_dst="$A_TMP/probe.dst"
: > "$_A_probe_src"
if cp --reflink=always "$_A_probe_src" "$_A_probe_dst" 2>/dev/null; then
    rm -f "$_A_probe_src" "$_A_probe_dst"
    # Full seed: replaces lane/target with a CoW clone of base_target.
    # REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 makes trash-rm synchronous (test hygiene).
    RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
    REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 \
        bash "$SEED_SCRIPT" "$A_BASE_TARGET" "$A_LANE" --fresh-checkout >/dev/null 2>&1
    echo "A3: ran seed-warm-lane.sh --fresh-checkout (XFS reflink available)" >&2
else
    rm -f "$_A_probe_src" "$_A_probe_dst"
    echo "A3: no XFS reflink; skipping seed step — git clean still exercised" >&2
fi

# reset_lane's working-tree wipe: remove everything except target/ and .git.
# This is the git clean step that dark-factory runs after acquire/reset_lane.
# It MUST NOT disturb worktree refs (they live in the shared .git common dir).
git -C "$A_LANE" clean -xfd -e target >/dev/null 2>&1 || true

# ── A4: re-assert both refs still resolve after provisioning ─────────────────
run_helper --lane "$A_LANE" --task 9999 --expect-common-dir "$A_MAIN/.git"
assert "A4: task/9999 still resolves after seed+clean (exits 0)" test "$RC" -eq 0
assert "A4: stdout SHA unchanged after provisioning" \
    bash -c 'printf "%s" "$1" | grep -qxE "[0-9a-f]{40}"' _ "$OUT"

run_helper --lane "$A_LANE" --task 9998 --expect-common-dir "$A_MAIN/.git"
assert "A5: task/9998 still resolves after seed+clean (exits 0)" test "$RC" -eq 0

# ─────────────────────────────────────────────────────────────────────────────
# Block B — deterministic TOCTOU-class reproduction
#
# Proves that the fault lever is the single-shot/non-retry resolver (not the
# ref store) by using a counter-based git stub:
#   - The stub intercepts `rev-parse --verify refs/heads/task/*` only.
#   - It exits 1 (simulates "branch not found") while counter <= FAIL_UNTIL.
#   - After FAIL_UNTIL calls, it delegates to the real git (ref resolves fine).
#   - All other git subcommands always delegate to the real git.
#
# This deterministically models the TOCTOU window WITHOUT sleeps or real
# concurrency (T8 de-flake mandate #4847).
#
# B1: --retries 1  (single-shot) → stub fails on attempt 1 → exits non-zero
# B2: --retries 3 --delay 0      → stub fails attempt 1, succeeds attempt 2
#                                   → exits 0, prints SHA
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: deterministic TOCTOU-class reproduction ---"

# ── Block B shared state ───────────────────────────────────────────────────────
B_TMP="$(mktemp -d /tmp/test-warm-lane-ref-vis-b-XXXXXX)"
_TMPDIRS+=("$B_TMP")

# Build a hermetic git fixture for Block B (separate from Block A).
B_MAIN="$B_TMP/main"
B_LANE="$B_TMP/lane"

git init -q -b main "$B_MAIN"
git -C "$B_MAIN" config user.email "test@test.local"
git -C "$B_MAIN" config user.name "Test"
touch "$B_MAIN/README.md"
git -C "$B_MAIN" add README.md
git -C "$B_MAIN" commit -q -m "initial"
git -C "$B_MAIN" worktree add -b task/7777 "$B_LANE" main

# CALLS_FILE records every git invocation from the stub.
CALLS_FILE="$(mktemp /tmp/test-warm-lane-ref-vis-b-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

# COUNTER_FILE is reset before each sub-test that uses the stub.
B_COUNTER_FILE="$(mktemp /tmp/test-warm-lane-ref-vis-b-counter-XXXXXX)"
_TMPDIRS+=("$B_COUNTER_FILE")

# STUB_DIR — prepended to PATH for Block B runs.
B_STUB_DIR="$(mktemp -d /tmp/test-warm-lane-ref-vis-b-stub-XXXXXX)"
_TMPDIRS+=("$B_STUB_DIR")

# Write the counter-based git stub.
# Intercepts: git [flags] rev-parse [flags] --verify refs/heads/task/*
# Delegates:  everything else to $REAL_GIT.
# shellcheck disable=SC2016  # single-quoted here-doc; $REAL_GIT expanded at write time
cat > "$B_STUB_DIR/git" << STUB_EOF
#!/usr/bin/env bash
# Counter-based git stub — intercepts rev-parse --verify refs/heads/task/*
printf 'git %s\n' "\$*" >> "\${REIFY_TEST_CALLS_FILE:-/dev/null}"

# Scan args for a refs/heads/task/ target AND for rev-parse subcommand.
_has_revparse=0
_has_task_ref=0
for _a in "\$@"; do
    [ "\$_a" = "rev-parse" ] && _has_revparse=1
    case "\$_a" in refs/heads/task/*) _has_task_ref=1 ;; esac
done

if [ "\$_has_revparse" = "1" ] && [ "\$_has_task_ref" = "1" ]; then
    _cf="\${REIFY_GIT_STUB_COUNTER_FILE:-/dev/null}"
    _count=0
    [ -f "\$_cf" ] && _count=\$(cat "\$_cf" 2>/dev/null || echo 0)
    _count=\$((_count + 1))
    echo "\$_count" > "\$_cf"
    if [ "\$_count" -le "\${REIFY_GIT_STUB_FAIL_UNTIL:-0}" ]; then
        exit 1
    fi
fi

exec ${REAL_GIT} "\$@"
STUB_EOF
chmod +x "$B_STUB_DIR/git"

# ── run_b_helper: runs script with stub PATH + counter env vars ───────────────
reset_b_counter() {
    echo "0" > "$B_COUNTER_FILE"
    > "$CALLS_FILE"
}

run_b_helper() {
    local fail_until="$1"; shift
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        REIFY_GIT_STUB_COUNTER_FILE="$B_COUNTER_FILE" \
        REIFY_GIT_STUB_FAIL_UNTIL="$fail_until" \
        PATH="$B_STUB_DIR:$PATH" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ── B1: single-shot (--retries 1) with fail-once stub → exits non-zero ────────
# Models the steward symptom: resolve_branch_sha single-shot lands in the
# TOCTOU window → "branch not found" even though the branch exists.
reset_b_counter
run_b_helper 1 \
    --lane "$B_LANE" --task 7777 --retries 1 --delay 0
assert "B1: single-shot with fail-once stub exits non-zero" test "$RC" -ne 0
assert "B1: stderr mentions branch-not-found class diagnostic" \
    bash -c 'printf "%s\n" "$1" | grep -qi "not found\|branch not found\|absent"' _ "$ERR_OUT"
assert "B1: stdout is empty (no SHA on failure)" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# Verify the stub intercepted exactly 1 task-branch resolve attempt.
# (The stub records ALL git calls; we filter by refs/heads/task/ to count
# only the actual ref-resolve attempts, excluding --is-inside-work-tree etc.)
assert "B1: stub intercepted exactly 1 task-branch resolve attempt" \
    bash -c 'grep -c "refs/heads/task/" "$1" | grep -qx 1' _ "$CALLS_FILE"

# ── B2: bounded-retry (--retries 3 --delay 0) with fail-once stub → exits 0 ──
# Models the fix: bounded retry rides over the TOCTOU window.
# Stub fails on attempt 1, succeeds on attempt 2; retry catches it.
reset_b_counter
run_b_helper 1 \
    --lane "$B_LANE" --task 7777 --retries 3 --delay 0
assert "B2: bounded-retry (3) with fail-once stub exits 0" test "$RC" -eq 0
assert "B2: stdout is the resolved 40-hex SHA" \
    bash -c 'printf "%s" "$1" | grep -qxE "[0-9a-f]{40}"' _ "$OUT"
assert "B2: stderr is non-empty (progress diagnostics on retry)" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# Verify the stub was called twice for the task-branch ref resolve:
# attempt 1 (fails), attempt 2 (succeeds = delegates to real git, also recorded).
# Both are visible as refs/heads/task/ lines in CALLS_FILE.
assert "B2: stub intercepted 2 task-branch resolve attempts (1 fail + 1 succeed)" \
    bash -c 'grep -c "refs/heads/task/" "$1" | grep -qx 2' _ "$CALLS_FILE"

test_summary
