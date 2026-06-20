#!/usr/bin/env bash
# tests/infra/test_warm_base_coherence.sh
# Two-way base-coherence boundary test for scripts/refresh-warm-base.sh.
# Pins the D8/D10 base contract — reify side (R5, task #4698).
#
# Exercises three behaviors:
#   Block C — inv.9 `--landed-commit` provenance-guard contract (accept / reject cases)
#   Block A — torn-read coherence: a pinned reader never sees mixed-gen content
#   Block B-reap — GC-defer anti-tautology: the deferred gen IS reaped once the
#                  reader releases its flock -s lock
#
# PATH stubs:
#   cp   — records argv to CALLS_FILE; when REIFY_TEST_REFLINK_OK=1 performs a
#           real recursive copy via the absolute cp (stripping --reflink=always);
#           else prints an error + exits 1.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/refresh-warm-base.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/refresh-warm-base.sh base-coherence boundary tests (task #4698) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-warm-base-coherence-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-warm-base-coherence-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-warm-base-coherence-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# cp stub: record argv; if REIFY_TEST_REFLINK_OK=1 perform a real recursive
# copy (real cp with --reflink=always stripped); else print error + exit 1.
_REAL_CP="$(command -v cp)"
cat > "$STUB_DIR/cp" << STUB_EOF
#!/usr/bin/env bash
echo "cp \$*" >> "\${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "\${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    args=()
    for a in "\$@"; do
        [ "\$a" = "--reflink=always" ] && continue
        args+=("\$a")
    done
    exec "${_REAL_CP}" "\${args[@]}"
fi
_dst="\${!#}"
if [ -n "\$_dst" ]; then
    mkdir -p "\$_dst" 2>/dev/null || true
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes the script under the stub PATH.
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        PATH="$STUB_DIR:$PATH" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

reset_calls() {
    > "$CALLS_FILE"
}

# mk_git_advancing <parent_dir> [<subdir>]
# Creates a hermetic git worktree at <parent_dir>/lane with a committed tracked
# placeholder (.placeholder) so `git status --porcelain --untracked-files=no`
# is clean (empty).  Creates <parent_dir>/lane/<subdir> (default: advancing) as
# an UNtracked subdirectory (like Cargo target/).  Prints the lane dir to stdout.
#
# Usage:
#   LANE="$(mk_git_advancing "$MY_TMP")"
#   HEAD="$(git -C "$LANE" rev-parse HEAD)"
#   echo "..." > "$LANE/advancing/file.txt"
#   BASE="$MY_TMP/base"
#   run_helper "$LANE/advancing" "$BASE" --landed-commit "$HEAD"
mk_git_advancing() {
    local parent_dir="$1"
    local subdir="${2:-advancing}"
    local lane_dir="$parent_dir/lane"
    mkdir -p "$lane_dir"
    printf 'placeholder\n' > "$lane_dir/.placeholder"
    git -C "$lane_dir" init -q
    git -C "$lane_dir" add -- .placeholder
    git -C "$lane_dir" \
        -c user.email="warm-lane-test@localhost" \
        -c user.name="Warm Lane Test" \
        -c commit.gpgsign=false \
        commit -q --no-verify -m "fixture: hermetic advancing lane"
    mkdir -p "$lane_dir/$subdir"
    echo "$lane_dir"
}

# ──────────────────────────────────────────────────────────────────────────────
# Helper: _guard_case <parent_dir> [extra args for run_helper]
#
# Sets up a fresh clean advancing lane via mk_git_advancing under <parent_dir>.
# Stamps a single file in the advancing subdir so the copy has content.
# Runs run_helper with REIFY_TEST_REFLINK_OK=1 passing <parent_dir>/base as
# base_dir plus any extra args supplied by the caller.
# Leaves RC and ERR_OUT set for assertions.
_guard_case() {
    local parent_dir="$1"; shift
    local lane base advancing head base_dir
    lane="$(mk_git_advancing "$parent_dir")"
    advancing="$lane/advancing"
    head="$(git -C "$lane" rev-parse HEAD)"
    echo "fixture" > "$advancing/content.txt"
    base_dir="$parent_dir/base"
    reset_calls
    REIFY_TEST_REFLINK_OK=1 run_helper "$advancing" "$base_dir" "$@"
}

# Helper: _mk_dirty_lane <parent_dir>
#
# Builds a clean advancing lane via mk_git_advancing, then creates a TRACKED
# file change that is staged but not committed (making git status --porcelain
# --untracked-files=no non-empty). Leaves the lane at <parent_dir>/lane.
_mk_dirty_lane() {
    local parent_dir="$1"
    local lane
    lane="$(mk_git_advancing "$parent_dir")"
    # Add a tracked change: create a file, stage it (tracked) but don't commit.
    echo "dirty-wip" > "$lane/dirty.txt"
    git -C "$lane" add -- dirty.txt
}

# Helper: _guard_case_from_dirty_lane <parent_dir>
#
# Used after _mk_dirty_lane. Runs run_helper with a matching --landed-commit
# against the dirty lane. The guard must reject because the worktree is dirty.
_guard_case_from_dirty_lane() {
    local parent_dir="$1"
    local lane advancing head base_dir
    lane="$parent_dir/lane"
    advancing="$lane/advancing"
    head="$(git -C "$lane" rev-parse HEAD)"
    echo "fixture" > "$advancing/content.txt"
    base_dir="$parent_dir/base"
    reset_calls
    REIFY_TEST_REFLINK_OK=1 run_helper "$advancing" "$base_dir" --landed-commit "$head"
}

# ──────────────────────────────────────────────────────────────────────────────
# Block C — inv.9 `--landed-commit` provenance-guard contract
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: provenance-guard contract ---"

# c1: ACCEPT — clean advancing lane + matching --landed-commit → RC 0
# _guard_case with no extra args uses --landed-commit $(git rev-parse HEAD)
# internally via a matching wrapper: we call the helper that resolves HEAD
# inside the pre-built lane and passes it. Use _guard_case_accept which
# explicitly passes the correct HEAD sha.
C_TMP="$(mktemp -d /tmp/test-warm-base-coherence-c-XXXXXX)"
_TMPDIRS+=("$C_TMP")

# Build the lane first so we can resolve HEAD, then call run_helper directly.
_c1_lane="$(mk_git_advancing "$C_TMP")"
_c1_head="$(git -C "$_c1_lane" rev-parse HEAD)"
echo "fixture" > "$_c1_lane/advancing/content.txt"
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$_c1_lane/advancing" "$C_TMP/base" --landed-commit "$_c1_head"
assert "c1: clean lane + matching --landed-commit exits 0" test "$RC" -eq 0
assert "c1: stderr reports 'Provenance guard: OK'" \
    bash -c 'printf "%s\n" "$1" | grep -q "Provenance guard: OK"' _ "$ERR_OUT"

# c2: REJECT — missing --landed-commit flag → RC≠0
C2_TMP="$(mktemp -d /tmp/test-warm-base-coherence-c2-XXXXXX)"
_TMPDIRS+=("$C2_TMP")

_guard_case "$C2_TMP"
assert "c2: missing --landed-commit exits non-zero" test "$RC" -ne 0
assert "c2: stderr names missing provenance assertion" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "provenance assertion missing|landed-commit.*required"' _ "$ERR_OUT"

# c3: REJECT — HEAD-mismatch (bogus sha) → RC≠0
C3_TMP="$(mktemp -d /tmp/test-warm-base-coherence-c3-XXXXXX)"
_TMPDIRS+=("$C3_TMP")

_guard_case "$C3_TMP" --landed-commit "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
assert "c3: HEAD-mismatch exits non-zero" test "$RC" -ne 0
assert "c3: stderr names HEAD mismatch" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "HEAD.*match|HEAD mismatch"' _ "$ERR_OUT"

# c4: REJECT — dirty/WIP lane (uncommitted tracked change) → RC≠0
C4_TMP="$(mktemp -d /tmp/test-warm-base-coherence-c4-XXXXXX)"
_TMPDIRS+=("$C4_TMP")

_mk_dirty_lane "$C4_TMP"
_guard_case_from_dirty_lane "$C4_TMP"
assert "c4: dirty/WIP lane exits non-zero" test "$RC" -ne 0

# ──────────────────────────────────────────────────────────────────────────────
# Helper: _run_flip_with_pinned_reader <parent_dir>
#
# Sets up the two-gen flip scenario with a sentinel-sequenced background reader:
#
#   1. mk_git_advancing → lane; stamp advancing with distinct GEN1 content
#      (several files + nested subdir, each containing "GEN1").
#   2. First refresh (REIFY_TEST_REFLINK_OK=1, --landed-commit HEAD) →
#      <base>→<base>.gen.1.
#   3. Resolve _GEN1_DIR=$(readlink <base>); lock path ${_GEN1_DIR}.lock.
#   4. Spawn background reader subshell:
#        ( exec 9>"$LOCK"; flock -s 9; touch "$READY"
#          until [ -f "$GO" ]; do sleep 0.01; done
#          cp -a "$_GEN1_DIR" "$_READER_COPY_DIR"; echo $? > "$RC_FILE"
#          touch "$DONE"; ) &
#      (plain cp -a — no stub, no --reflink — works on any FS)
#   5. Foreground polls until READY exists (bounded; fails fast if reader hangs).
#   6. Re-stamp advancing dir with GEN2 content; second refresh → creates gen.2,
#      flips symlink → gen.2, GC tries retired gen.1 but reader holds flock -s →
#      flock -n -x fails → gen.1 deferred (rm skipped).
#   7. Capture _LIVE_AFTER_FLIP and check _GEN1_DIR still exists BEFORE releasing.
#   8. touch "$GO" to unblock the reader; wait for reader to complete.
#   9. Read _READER_RC from RC_FILE.
#
# Exports to caller scope (via global assignment):
#   _GEN1_DIR       — path of the gen.1 dir (pinned by the reader)
#   _READER_COPY_DIR — dir where the reader's cp -a landed
#   _LIVE_AFTER_FLIP — value of readlink <base> after the second refresh
#   _READER_RC      — exit code of the reader's cp -a
#
# Also sets _FLIP_LANE and _FLIP_BASE for use by _run_post_release_gc.
_FLIP_LANE=""
_FLIP_BASE=""
_GEN1_DIR=""
_READER_COPY_DIR=""
_LIVE_AFTER_FLIP=""
_READER_RC=0

_run_flip_with_pinned_reader() {
    local parent_dir="$1"
    local lane base advancing head
    local gen1_dir lock_path
    local ready_file go_file done_file rc_file
    local reader_copy_dir reader_pid
    local _poll_i

    # Step 1: hermetic lane + GEN1 content
    lane="$(mk_git_advancing "$parent_dir")"
    advancing="$lane/advancing"
    head="$(git -C "$lane" rev-parse HEAD)"

    # Stamp several files + nested subdir with GEN1 marker
    echo "GEN1-content-alpha" > "$advancing/alpha.txt"
    echo "GEN1-content-beta"  > "$advancing/beta.txt"
    mkdir -p "$advancing/sub"
    echo "GEN1-content-nested" > "$advancing/sub/nested.txt"

    # Step 2: first refresh → base→gen.1
    base="$parent_dir/base"
    REIFY_TEST_REFLINK_OK=1 \
        PATH="$STUB_DIR:$PATH" \
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        bash "$SCRIPT" "$advancing" "$base" --landed-commit "$head" >/dev/null 2>&1

    # Step 3: resolve gen.1 dir + lock path
    gen1_dir="$(readlink "$base")"
    lock_path="${gen1_dir}.lock"
    touch "$lock_path" 2>/dev/null || true

    # Step 4: sentinel files
    reader_copy_dir="$parent_dir/reader-copy"
    mkdir -p "$reader_copy_dir"
    ready_file="$parent_dir/.reader-ready"
    go_file="$parent_dir/.reader-go"
    done_file="$parent_dir/.reader-done"
    rc_file="$parent_dir/.reader-rc"

    # Spawn background reader: acquire flock -s, signal READY, wait for GO, then copy.
    # Plain cp -a (no stub, no --reflink) — works on any filesystem.
    _REAL_CP_ABS="$(command -v cp)"
    (
        exec 9>"$lock_path"
        flock -s 9
        touch "$ready_file"
        _poll_i=0
        until [ -f "$go_file" ]; do
            sleep 0.01
            _poll_i=$(( _poll_i + 1 ))
            [ "$_poll_i" -lt 500 ] || { echo "1" > "$rc_file"; touch "$done_file"; exit 1; }
        done
        _rc=0
        "$_REAL_CP_ABS" -a "$gen1_dir/." "$reader_copy_dir/" 2>/dev/null || _rc=$?
        echo "$_rc" > "$rc_file"
        touch "$done_file"
    ) &
    reader_pid=$!

    # Step 5: wait for READY (bounded poll — fail fast if reader never acquires lock)
    _poll_i=0
    until [ -f "$ready_file" ]; do
        sleep 0.01
        _poll_i=$(( _poll_i + 1 ))
        if [ "$_poll_i" -ge 500 ]; then
            echo "ERROR: reader never signaled READY after 5s" >&2
            kill "$reader_pid" 2>/dev/null || true
            return 1
        fi
    done

    # Step 6: re-stamp advancing with GEN2 content; second refresh
    # (reader holds flock -s on gen.1.lock → GC's flock -n -x fails → gen.1 deferred)
    echo "GEN2-content-alpha"  > "$advancing/alpha.txt"
    echo "GEN2-content-beta"   > "$advancing/beta.txt"
    echo "GEN2-content-nested" > "$advancing/sub/nested.txt"

    REIFY_TEST_REFLINK_OK=1 \
        PATH="$STUB_DIR:$PATH" \
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        bash "$SCRIPT" "$advancing" "$base" --landed-commit "$head" >/dev/null 2>&1

    # Step 7: capture live gen BEFORE releasing the reader
    _LIVE_AFTER_FLIP="$(readlink "$base")"
    _GEN1_DIR="$gen1_dir"

    # Step 8: release the reader (touch GO) and wait for it to finish
    touch "$go_file"
    wait "$reader_pid" 2>/dev/null || true

    # Step 9: read reader exit code
    _READER_RC=0
    if [ -f "$rc_file" ]; then
        _READER_RC="$(cat "$rc_file")"
    fi
    _READER_COPY_DIR="$reader_copy_dir"

    # Save fixture state for _run_post_release_gc
    _FLIP_LANE="$lane"
    _FLIP_BASE="$base"
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — torn-read coherence (a) + GC-defer-while-locked (b)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: torn-read coherence + GC-defer-while-locked ---"

A_TMP="$(mktemp -d /tmp/test-warm-base-coherence-a-XXXXXX)"
_TMPDIRS+=("$A_TMP")

_run_flip_with_pinned_reader "$A_TMP"

# Assertion (b): GC deferred — retired gen.1 dir still exists AFTER the flip
# (the reader held flock -s so flock -n -x in the GC failed → rm skipped).
assert "b: retired gen.1 dir still exists after flip (GC deferred by flock -s)" \
    bash -c '[ -d "$1" ]' _ "$_GEN1_DIR"

# Anti-tautology: the flip actually happened (base now resolves to gen.2)
assert "b: <base> now resolves to gen.2 (the flip ran)" \
    bash -c 'basename "$1" | grep -qE "[.]gen[.][0-9]+$" && [ "$1" != "$2" ]' \
        _ "$_LIVE_AFTER_FLIP" "$_GEN1_DIR"

# Assertion (a): coherence — reader copy is complete, 100% GEN1, no GEN2 leakage
assert "a: reader exited 0 (no ENOENT mid-walk)" \
    test "$_READER_RC" -eq 0
assert "a: reader copy dir exists" \
    test -d "$_READER_COPY_DIR"
assert "a: no GEN2 content in reader copy (coherent gen.1 read)" \
    bash -c '! grep -r "GEN2" "$1" 2>/dev/null | grep -q "GEN2"' _ "$_READER_COPY_DIR"
assert "a: all files in reader copy carry GEN1 marker" \
    bash -c 'count="$(grep -rl "GEN1" "$1" 2>/dev/null | wc -l)"; files="$(find "$1" -type f | wc -l)"; [ "$files" -gt 0 ] && [ "$count" -eq "$files" ]' _ "$_READER_COPY_DIR"
assert "a: reader copy has complete file set (no missing files from gen.1)" \
    bash -c 'orig="$(find "$1" -type f | wc -l)"; copy="$(find "$2" -type f | wc -l)"; [ "$orig" -eq "$copy" ]' _ "$_GEN1_DIR" "$_READER_COPY_DIR"

# ──────────────────────────────────────────────────────────────────────────────
# Helper: _run_post_release_gc <parent_dir>
#
# Runs a THIRD refresh after the Block-A reader has released its flock -s and
# been wait'd. Reuses the fixture state from _run_flip_with_pinned_reader
# (_FLIP_LANE and _FLIP_BASE). Re-stamps advancing with GEN3 content and runs
# the third refresh; the GC sweep now finds gen.1's lock file uncontested →
# flock -n -x succeeds → gen.1 dir is reaped; gen.2 (now retired) is also reaped.
# Leaves _FLIP_BASE pointing to the new gen.3 symlink.
_run_post_release_gc() {
    local advancing head
    advancing="$_FLIP_LANE/advancing"
    head="$(git -C "$_FLIP_LANE" rev-parse HEAD)"

    # Re-stamp advancing with GEN3 content
    echo "GEN3-content-alpha"  > "$advancing/alpha.txt"
    echo "GEN3-content-beta"   > "$advancing/beta.txt"
    echo "GEN3-content-nested" > "$advancing/sub/nested.txt"

    # Third refresh: GC now finds gen.1.lock uncontested → reaps gen.1; gen.2 also retired → reaped
    REIFY_TEST_REFLINK_OK=1 \
        PATH="$STUB_DIR:$PATH" \
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        bash "$SCRIPT" "$advancing" "$_FLIP_BASE" --landed-commit "$head" >/dev/null 2>&1
}

# ──────────────────────────────────────────────────────────────────────────────
# Block B-reap — anti-tautology control: deferred gen IS reaped once lock free
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B-reap: deferred gen reaped after lock released ---"

# Reuse the fixture state from _run_flip_with_pinned_reader (same A_TMP, same
# lane + base; the reader has already released its flock -s and been wait'd).
_run_post_release_gc "$A_TMP"

# The third refresh must have reaped the now-unlocked retired gen.1
assert "B-reap: gen.1 dir is GONE after third refresh (deferred GC now succeeded)" \
    bash -c '[ ! -d "$1" ]' _ "$_GEN1_DIR"

# And the base now resolves to gen.3
assert "B-reap: <base> resolves to gen.3 after third refresh" \
    bash -c 'basename "$(readlink "$1")" | grep -qE "[.]gen[.][0-9]+$"' _ "$_FLIP_BASE"
assert "B-reap: <base> gen index is 3" \
    bash -c '_gen="$(readlink "$1")"; _n="${_gen##*.gen.}"; [ "$_n" = "3" ]' _ "$_FLIP_BASE"

test_summary
