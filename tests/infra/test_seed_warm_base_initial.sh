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
#   E — failure propagation: not-mounted / not-reflink → non-zero at FS pre-check (before build)
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

# ──────────────────────────────────────────────────────────────────────────────
# Block C — build-failure fail-closed
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: build-failure fail-closed ---"

C_TMP="$(mktemp -d /tmp/test-seed-warm-base-c-XXXXXX)"
_TMPDIRS+=("$C_TMP")
C_MNT="$C_TMP/mount"
mkdir -p "$C_MNT"

# C1: failing build cmd (exit 1) → script exits non-zero + actionable stderr,
#     refresh NOT reached, no base seeded
C_LANE="$(mk_git_advancing "$C_TMP")"
C_HEAD="$(git -C "$C_LANE" rev-parse HEAD)"
C_BASE="$C_MNT/base/target"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    run_helper \
    --mount "$C_MNT" \
    --base-dir "$C_BASE" \
    --merge-verify "$C_LANE" \
    --build-cmd "exit 1" \
    --landed-commit "$C_HEAD" \
    --rustflags "" \
    --invocation "test"
assert "C1: failing build-cmd exits non-zero" test "$RC" -ne 0
assert "C1: stderr mentions build failure" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "build|failed|cold"' _ "$ERR_OUT"
assert "C1: no base seeded (no symlink, no gen dir)" \
    test ! -L "$C_BASE"
assert "C1: no <base>.gen.* dir created (refresh not reached)" \
    bash -c '_n=0; for _g in "${1}".gen.*; do [ -e "$_g" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$C_BASE"
assert "C1: no <base>.rustflags stamp (refresh not reached)" \
    test ! -f "${C_BASE}.rustflags"

# C2: build succeeds but leaves target/ empty → script exits non-zero,
#     no base seeded (empty-target guard before refresh)
C2_TMP="$(mktemp -d /tmp/test-seed-warm-base-c2-XXXXXX)"
_TMPDIRS+=("$C2_TMP")
C2_MNT="$C2_TMP/mount"
mkdir -p "$C2_MNT"
C2_LANE="$(mk_git_advancing "$C2_TMP")"
C2_HEAD="$(git -C "$C2_LANE" rev-parse HEAD)"
C2_BASE="$C2_MNT/base/target"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    run_helper \
    --mount "$C2_MNT" \
    --base-dir "$C2_BASE" \
    --merge-verify "$C2_LANE" \
    --build-cmd "mkdir -p target" \
    --landed-commit "$C2_HEAD" \
    --rustflags "" \
    --invocation "test"
assert "C2: empty-target build exits non-zero" test "$RC" -ne 0
assert "C2: stderr mentions empty target/" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "empty|target"' _ "$ERR_OUT"
assert "C2: no base seeded on empty target/ (no symlink)" \
    test ! -L "$C2_BASE"

# ──────────────────────────────────────────────────────────────────────────────
# Block B-e2e — full happy path: preflight gated, user-observable signal
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B-e2e: full happy path with preflight gate ---"

BE_TMP="$(mktemp -d /tmp/test-seed-warm-base-be-XXXXXX)"
_TMPDIRS+=("$BE_TMP")

BE_LANE="$(mk_git_advancing "$BE_TMP")"
BE_HEAD="$(git -C "$BE_LANE" rev-parse HEAD)"
BE_MNT="$BE_TMP/mount"
mkdir -p "$BE_MNT"
BE_BASE="$BE_MNT/base/target"

# B-e2e-1: full happy path exits 0 (build + refresh + preflight all pass)
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="" \
    run_helper \
    --mount "$BE_MNT" \
    --base-dir "$BE_BASE" \
    --merge-verify "$BE_LANE" \
    --build-cmd "mkdir -p target && printf 'x' > target/rustc" \
    --landed-commit "$BE_HEAD" \
    --rustflags "" \
    --invocation "sha256:be2e-test"
assert "B-e2e-1: full happy path exits 0" test "$RC" -eq 0

# B-e2e-2: preflight reported its own "all checks passed" line on stderr.
# Use the preflight-specific prefix so the assertion proves preflight ran, not just
# the seed script's own final summary (which also contains "all checks passed").
assert "B-e2e-2: 'warm-lane-preflight: all checks passed' on stderr (preflight ran)" \
    bash -c 'printf "%s\n" "$1" | grep -qi "warm-lane-preflight: all checks passed"' _ "$ERR_OUT"

# B-e2e-3: stdout is empty (all diagnostics on stderr)
assert "B-e2e-3: stdout is empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# B-e2e-4: base dir is a valid symlink with content
assert "B-e2e-4: base dir is a symlink after full seeding" \
    bash -c '[ -L "$1" ]' _ "$BE_BASE"
assert "B-e2e-4: base dir non-empty (has build content)" \
    bash -c '[ -n "$(ls -A "$1" 2>/dev/null)" ]' _ "$BE_BASE"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — failure propagation: preflight check failures → non-zero exit
# Signal not falsely green when preflight checks fail.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: failure propagation (preflight check failures) ---"

E_TMP="$(mktemp -d /tmp/test-seed-warm-base-e-XXXXXX)"
_TMPDIRS+=("$E_TMP")

# E1: not mounted (REIFY_TEST_MOUNTED unset) → script exits non-zero at the FS
#     pre-check (Step 0b) before the cold build even starts.
E_LANE="$(mk_git_advancing "$E_TMP")"
E_HEAD="$(git -C "$E_LANE" rev-parse HEAD)"
E_MNT="$E_TMP/mount"
mkdir -p "$E_MNT"
E_BASE="$E_MNT/base/target"

reset_calls
REIFY_TEST_MOUNTED="" REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="" \
    run_helper \
    --mount "$E_MNT" \
    --base-dir "$E_BASE" \
    --merge-verify "$E_LANE" \
    --build-cmd "mkdir -p target && printf 'x' > target/rustc" \
    --landed-commit "$E_HEAD" \
    --rustflags "" \
    --invocation "sha256:e1-test"
assert "E1: not-mounted → script exits non-zero (FS pre-check fires before build)" \
    test "$RC" -ne 0
assert "E1: build-cmd did NOT run (pre-check aborts before Step 1)" \
    test ! -f "$E_LANE/target/rustc"

# E2: not reflink-capable (REIFY_TEST_REFLINK_OK=0) → script exits non-zero at
#     the FS pre-check reflink probe (Step 0b), before the cold build starts.
E2_TMP="$(mktemp -d /tmp/test-seed-warm-base-e2-XXXXXX)"
_TMPDIRS+=("$E2_TMP")
E2_LANE="$(mk_git_advancing "$E2_TMP")"
E2_HEAD="$(git -C "$E2_LANE" rev-parse HEAD)"
E2_MNT="$E2_TMP/mount"
mkdir -p "$E2_MNT"
E2_BASE="$E2_MNT/base/target"

# With REIFY_TEST_REFLINK_OK=0 the cp stub fails on the pre-check reflink probe,
# so the script exits non-zero before the cold build starts.
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=0 \
    RUSTFLAGS="" \
    run_helper \
    --mount "$E2_MNT" \
    --base-dir "$E2_BASE" \
    --merge-verify "$E2_LANE" \
    --build-cmd "mkdir -p target && printf 'x' > target/rustc" \
    --landed-commit "$E2_HEAD" \
    --rustflags "" \
    --invocation "sha256:e2-test"
assert "E2: not-reflink-capable → script exits non-zero (FS pre-check fires before build)" \
    test "$RC" -ne 0
assert "E2: build-cmd did NOT run (pre-check aborts before Step 1)" \
    test ! -f "$E2_LANE/target/rustc"

# ──────────────────────────────────────────────────────────────────────────────
# Block G — RUSTFLAGS divergence: provenance + preflight coherence
# Exercises the case where --rustflags VALUE differs from ambient RUSTFLAGS env.
# Verifies that the script exports the resolved value so all three consumers
# (cold build, refresh stamp, preflight Check 5) observe one effective RUSTFLAGS.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: RUSTFLAGS divergence provenance coherence ---"

G_TMP="$(mktemp -d /tmp/test-seed-warm-base-g-XXXXXX)"
_TMPDIRS+=("$G_TMP")

G_LANE="$(mk_git_advancing "$G_TMP")"
G_HEAD="$(git -C "$G_LANE" rev-parse HEAD)"
G_MNT="$G_TMP/mount"
mkdir -p "$G_MNT"
G_BASE="$G_MNT/base/target"

# Build-cmd: records the effective RUSTFLAGS into target/rustflags-seen, then
# creates a target/rustc marker.  This lets G2/G4 verify the build compiled
# with the REQUESTED value, not the ambient sentinel.
G_BUILD_CMD='mkdir -p target && printf "%s" "${RUSTFLAGS:-}" > target/rustflags-seen && printf x > target/rustc'

# Run with ambient RUSTFLAGS set to a wrong sentinel, while --rustflags passes
# a different value.  After the fix the script exports the resolved value so all
# three consumers (build, refresh stamp, preflight) agree on "-Cdebuginfo=2".
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-Cambient-SHOULD-NOT-BE-USED" \
    run_helper \
    --mount "$G_MNT" \
    --base-dir "$G_BASE" \
    --merge-verify "$G_LANE" \
    --build-cmd "$G_BUILD_CMD" \
    --landed-commit "$G_HEAD" \
    --rustflags "-Cdebuginfo=2" \
    --invocation "sha256:g-test"

# G1: run exits 0 (all three consumers agreed)
assert "G1: RUSTFLAGS-divergence run exits 0" test "$RC" -eq 0

# G2: the cold build saw the REQUESTED flags, not the ambient sentinel
assert "G2: build compiled with --rustflags value (not ambient sentinel)" \
    bash -c '[ "$(cat "$1/target/rustflags-seen")" = "-Cdebuginfo=2" ]' _ "$G_LANE"

# G3: the base stamp contains the requested flags
assert "G3: <base-dir>.rustflags stamp contains requested flags" \
    bash -c '[ "$(cat "${1}.rustflags")" = "-Cdebuginfo=2" ]' _ "$G_BASE"

# G4: provenance coherence — stamp matches what build actually compiled with
assert "G4: rustflags stamp == rustflags-seen (provenance coherence inv.9/D4)" \
    bash -c '[ "$(cat "${1}.rustflags")" = "$(cat "${2}/target/rustflags-seen")" ]' \
    _ "$G_BASE" "$G_LANE"

# G5: preflight Check 5 matched despite ambient divergence → all checks passed
assert "G5: 'warm-lane-preflight: all checks passed' on stderr (Check 5 matched)" \
    bash -c 'printf "%s\n" "$1" | grep -qi "warm-lane-preflight: all checks passed"' _ "$ERR_OUT"

test_summary
