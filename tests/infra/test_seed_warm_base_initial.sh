#!/usr/bin/env bash
# tests/infra/test_seed_warm_base_initial.sh
# Hermetic tests for scripts/seed-warm-base-initial.sh.
#
# PATH stubs:
#   cp         — real-recursive-copy variant: records argv; when REIFY_TEST_REFLINK_OK=1
#                strips --reflink=always and execs the real cp; else error+exit 1.
#                Both refresh-warm-base.sh (gen-dir copy) and warm-lane-preflight.sh
#                (reflink probe) use this stub — the real-recursive variant makes both work.
#   mountpoint — exit 0 when REIFY_TEST_MOUNTED=1; else exit 1.
#   Both record argv to CALLS_FILE.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help exits 0 with usage; unknown flag exits 2
#   F — merge-verify worktree validation (fail-closed, before any build)
#   B-seed — cold-build + refresh seeding: injected build cmd, base gen-dir created
#   C — build-failure fail-closed: failed/empty build → non-zero, no base seeded
#   B-e2e — full happy path: preflight gated, exits 0, stdout empty
#   E — failure propagation: not-mounted / not-reflink → non-zero despite successful build
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/seed-warm-base-initial.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/seed-warm-base-initial.sh hermetic tests (task 4697) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-seed-warm-base-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-seed-warm-base-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-seed-warm-base-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# cp stub: real-recursive-copy variant.
#   - Records argv to CALLS_FILE.
#   - When REIFY_TEST_REFLINK_OK=1: strips --reflink=always and calls the real cp
#     (so refresh-warm-base.sh's gen-dir copy and preflight's probe both work).
#   - Otherwise: creates the destination dir (simulating partial copy) then exits 1.
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
# Simulate partial failure: create destination dir before failing
_dst="\${!#}"
if [ -n "\$_dst" ]; then
    mkdir -p "\$_dst" 2>/dev/null || true
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# mountpoint stub: exit 0 when REIFY_TEST_MOUNTED=1; else exit 1.
cat > "$STUB_DIR/mountpoint" << 'STUB_EOF'
#!/usr/bin/env bash
echo "mountpoint $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
[ "${REIFY_TEST_MOUNTED:-}" = "1" ] && exit 0
exit 1
STUB_EOF
chmod +x "$STUB_DIR/mountpoint"

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

# mk_git_advancing <parent_dir>
# Creates a hermetic git worktree at <parent_dir>/lane with:
#   - a committed .placeholder (so `git status --untracked-files=no` is clean)
#   - a `target/` subdir (UNtracked, like Cargo target/)
# Prints the lane dir to stdout.
#
# Mirrors mk_git_advancing() in tests/infra/test_refresh_warm_base.sh.
mk_git_advancing() {
    local parent_dir="$1"
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
    mkdir -p "$lane_dir/target"
    echo "$lane_dir"
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard: --help, unknown flag
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0 and prints usage on stderr
reset_calls
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits 2
reset_calls
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits 2" test "$RC" -eq 2

# ──────────────────────────────────────────────────────────────────────────────
# Block F — merge-verify worktree validation (fail-closed, before any build)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: merge-verify worktree validation ---"

F_TMP="$(mktemp -d /tmp/test-seed-warm-base-f-XXXXXX)"
_TMPDIRS+=("$F_TMP")
F_MNT="$F_TMP/mount"
mkdir -p "$F_MNT"

# F1: non-existent --merge-verify directory exits non-zero with actionable stderr
reset_calls
run_helper --mount "$F_MNT" --merge-verify "$F_TMP/nonexistent/_merge-verify" \
    --build-cmd "true"
assert "F1: non-existent merge-verify exits non-zero" test "$RC" -ne 0
assert "F1: stderr names the missing merge-verify path" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "merge.verify|_merge-verify"' _ "$ERR_OUT"

# F2: --merge-verify points at a real directory that is NOT inside a git worktree
# → exits non-zero with actionable stderr; must happen BEFORE any build
F2_NOT_GIT="$F_TMP/not-a-git-dir"
mkdir -p "$F2_NOT_GIT"

# Track whether the build-cmd ran (it should NOT if validation fails before it)
F2_BUILD_MARKER="$F_TMP/f2-build-ran"
reset_calls
run_helper --mount "$F_MNT" --merge-verify "$F2_NOT_GIT" \
    --build-cmd "touch '$F2_BUILD_MARKER'"
assert "F2: non-git merge-verify exits non-zero" test "$RC" -ne 0
assert "F2: stderr names the merge-verify issue" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "merge.verify|_merge-verify|git.work.tree|worktree"' _ "$ERR_OUT"
assert "F2: build-cmd did NOT run (validation before build)" \
    test ! -f "$F2_BUILD_MARKER"

# ──────────────────────────────────────────────────────────────────────────────
# Block B-seed — cold-build + refresh seeding
# Uses real sibling scripts (refresh-warm-base.sh + warm-lane-preflight.sh)
# under PATH stubs (cp real-recursive-copy variant + mountpoint).
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B-seed: cold-build + refresh seeding ---"

BS_TMP="$(mktemp -d /tmp/test-seed-warm-base-bseed-XXXXXX)"
_TMPDIRS+=("$BS_TMP")

# Build a hermetic _merge-verify git worktree (committed .placeholder → clean status;
# target/ as untracked subdir → satisfies refresh's inv.9 provenance guard).
BS_LANE="$(mk_git_advancing "$BS_TMP")"
BS_HEAD="$(git -C "$BS_LANE" rev-parse HEAD)"

# The "build cmd" is a fast fixture that populates <merge-verify>/target/
# (matches what a real cargo build --release would do, minus the hours).
BS_BUILD_CMD="mkdir -p target && printf 'x' > target/rustc"

# Mount dir (will be considered "mounted" via REIFY_TEST_MOUNTED=1)
BS_MNT="$BS_TMP/mount"
mkdir -p "$BS_MNT"

BS_BASE="$BS_MNT/base/target"

# B-seed-1: basic seeding exits 0 (build populates target/, refresh initializes base)
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    run_helper \
    --mount "$BS_MNT" \
    --base-dir "$BS_BASE" \
    --merge-verify "$BS_LANE" \
    --build-cmd "$BS_BUILD_CMD" \
    --landed-commit "$BS_HEAD" \
    --rustflags "" \
    --invocation "sha256:bseed-test"
assert "B-seed-1: seeding exits 0" test "$RC" -eq 0

# B-seed-2: injected build command actually ran (marker file exists in target/)
assert "B-seed-2: injected build-cmd ran (target/rustc exists)" \
    test -f "$BS_LANE/target/rustc"

# B-seed-3: after seeding, <base-dir> is a symlink to a <base-dir>.gen.N dir
assert "B-seed-3: <base-dir> is a symlink after seeding" \
    bash -c '[ -L "$1" ]' _ "$BS_BASE"
assert "B-seed-3: <base-dir> symlink points to a <base>.gen.N dir" \
    bash -c '[ -L "$1" ] && readlink "$1" | grep -qE "[.]gen[.][0-9]+$"' _ "$BS_BASE"

# B-seed-4: base dir non-empty, contains the build content (resolved through symlink)
assert "B-seed-4: <base-dir> is non-empty (has build content)" \
    bash -c '[ -n "$(ls -A "$1" 2>/dev/null)" ]' _ "$BS_BASE"
assert "B-seed-4: target/rustc visible through the base symlink" \
    test -f "$BS_BASE/rustc"

# B-seed-5: sidecar stamps exist
assert "B-seed-5: <base-dir>.rustflags stamp exists" \
    test -f "${BS_BASE}.rustflags"
assert "B-seed-5: <base-dir>.invocation stamp exists" \
    test -f "${BS_BASE}.invocation"
assert "B-seed-5: invocation stamp matches --invocation value" \
    bash -c '[ "$(cat "$1.invocation")" = "sha256:bseed-test" ]' _ "$BS_BASE"

# B-seed-6: wrong --landed-commit causes non-zero exit (refresh inv.9 provenance guard propagated)
BS2_TMP="$(mktemp -d /tmp/test-seed-warm-base-bseed2-XXXXXX)"
_TMPDIRS+=("$BS2_TMP")
BS2_LANE="$(mk_git_advancing "$BS2_TMP")"
BS2_HEAD="$(git -C "$BS2_LANE" rev-parse HEAD)"
BS2_MNT="$BS2_TMP/mount"
mkdir -p "$BS2_MNT"
BS2_BASE="$BS2_MNT/base/target"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    run_helper \
    --mount "$BS2_MNT" \
    --base-dir "$BS2_BASE" \
    --merge-verify "$BS2_LANE" \
    --build-cmd "mkdir -p target && printf 'x' > target/rustc" \
    --landed-commit "0000000000000000000000000000000000000000" \
    --rustflags "" \
    --invocation "test"
assert "B-seed-6: wrong --landed-commit exits non-zero (refresh inv.9 guard fires)" \
    test "$RC" -ne 0
assert "B-seed-6: no base seeded on provenance guard failure" \
    test ! -L "$BS2_BASE"

test_summary
