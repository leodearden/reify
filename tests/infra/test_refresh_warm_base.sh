#!/usr/bin/env bash
# tests/infra/test_refresh_warm_base.sh
# Hermetic tests for scripts/refresh-warm-base.sh.
#
# PATH stubs:
#   cp       — records argv to CALLS_FILE; when REIFY_TEST_REFLINK_OK=1 performs
#              a real recursive copy via the absolute cp (stripping --reflink=always);
#              else prints an error + exits 1.
#   mv       — NOT stubbed; real mv so filesystem postconditions are observable.
#   xfs_bmap — records argv + emits REIFY_TEST_FRAG_EXTENTS extent rows per file.
#   rm       — records argv; when REIFY_TEST_PRUNE_RM_FAIL=1, fails a non-recursive
#              call (the prune stage's own `rm -f`) while a recursive call (the
#              EXIT trap's `rm -rf` cleanup) always execs the real rm — isolates a
#              simulated prune-unlink failure from the trap's own cleanup.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag, missing positional args
#   B — basic refresh happy path: cp --reflink=always, atomic rename, content OK
#   C — fail-closed reflink: probe failure -> non-zero, no partial base, pre-existing untouched
#   D — in-flight clone independence: clone dir untouched after refresh (B6)
#   E — base self-description stamps: .rustflags and .invocation written after swap
#   F — --check-frag defrag signal: verdict token + extent count, read-only
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.
#
# T8 de-flake audit (task #4847):
#   ZERO absolute-wall-clock-upper-bound or scheduling-latency assertions.
#   stat -c %Y / find -printf %T@ are used only for mtime/snapshot EQUALITY
#   invariants (D3 clone-untouched, F3 read-only) which are load-independent.
#   Escalations (5 esc / 3 task) are OUT-OF-CLASS for S/R/T techniques: most
#   plausibly real-cp disk pressure (cp stub does a real recursive copy) or
#   coarse merge-gate attribution.  See hand-off in:
#   docs/prds/infra-test-wallclock-deflake.warm-lane-audit-findings.md

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

echo "=== scripts/refresh-warm-base.sh hermetic tests (task 4661) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# Arm the shared-trash litter guard (task 5612). Sited immediately after
# `trap cleanup EXIT` because the helper registers its per-run root into
# _TMPDIRS and so must follow this file's own `_TMPDIRS=()`.
# Rationale, ordering contract, stem rules and honest scope: see the
# CANONICAL WIRING CONTRACT comment in tests/infra/test_helpers.sh.
init_isolated_lane_root test-refresh-warm-base

STUB_DIR="$(mktemp -d /tmp/test-refresh-warm-base-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-refresh-warm-base-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-refresh-warm-base-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# cp stub: record argv; if REIFY_TEST_REFLINK_OK=1 perform a real recursive copy
# (absolute cp with --reflink=always stripped); else simulate a partial copy
# (create the destination directory as a real cp would) then error + exit 1.
# This simulates the real-world failure mode where cp creates a partial
# <base>.gen.N.partial staging dir before encountering a non-reflink filesystem,
# so the EXIT trap test (Block C) can assert the partial is cleaned up.
# The real cp path is embedded at stub-creation time.
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
# Simulate partial failure: create destination dir (as real cp would) before failing
# The destination is always the last argument; ${!#} gives the last positional.
_dst="\${!#}"
if [ -n "\$_dst" ]; then
    mkdir -p "\$_dst" 2>/dev/null || true
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# xfs_bmap stub: record argv; emit REIFY_TEST_FRAG_EXTENTS extent rows.
# REIFY_TEST_XFSBMAP_OK=0 simulates xfs_bmap being unavailable/failing (exits 1).
cat > "$STUB_DIR/xfs_bmap" << 'STUB_EOF'
#!/usr/bin/env bash
echo "xfs_bmap $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_XFSBMAP_OK:-1}" = "0" ]; then
    echo "xfs_bmap: failed to get extents" >&2
    exit 1
fi
count="${REIFY_TEST_FRAG_EXTENTS:-1}"
for i in $(seq 1 "$count"); do
    printf "    %d: [0..511]: 1234..%d 512\n" "$((i-1))" "$((1234 + i*512))"
done
exit 0
STUB_EOF
chmod +x "$STUB_DIR/xfs_bmap"

# rm stub: record argv; when REIFY_TEST_PRUNE_RM_FAIL=1, fail ONLY a
# non-recursive invocation (the prune stage's own `xargs -0 rm -f -- <files>`,
# first arg "-f") while a recursive invocation (the EXIT trap's cleanup
# `rm -rf "$_p"`, first arg "-rf") always execs the real rm. This isolates
# "the prune's own unlink failed" from "the trap's cleanup afterward also
# failed for an unrelated reason" — a filesystem-permission fault (e.g. a
# write-protected debug/deps) cannot make that distinction: verified
# empirically that it defeats `rm -rf` identically, since both are the same
# unlink operation under the same directory permission, which would fail the
# "no residue" assertion for the wrong reason. Real rm path embedded at
# stub-creation time (mirrors the cp stub).
_REAL_RM="$(command -v rm)"
cat > "$STUB_DIR/rm" << STUB_EOF
#!/usr/bin/env bash
echo "rm \$*" >> "\${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "\${REIFY_TEST_PRUNE_RM_FAIL:-}" = "1" ]; then
    case "\$1" in
        -*r*) exec "${_REAL_RM}" "\$@" ;;
    esac
    echo "rm: SIMULATED failure (REIFY_TEST_PRUNE_RM_FAIL=1)" >&2
    exit 1
fi
exec "${_REAL_RM}" "\$@"
STUB_EOF
chmod +x "$STUB_DIR/rm"

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
# placeholder (.placeholder) so `git status --porcelain --untracked-files=no` is
# clean (empty).  Creates <parent_dir>/lane/<subdir> (default: advancing) as an
# UNtracked subdirectory (like Cargo target/).  Prints the lane dir to stdout.
#
# Usage:
#   LANE="$(mk_git_advancing "$MY_TMP")"
#   HEAD="$(git -C "$LANE" rev-parse HEAD)"
#   echo "..." > "$LANE/advancing/file.txt"   # add content to advancing dir
#   BASE="$MY_TMP/base"                        # base OUTSIDE the lane repo
#   run_helper "$LANE/advancing" "$BASE" --landed-commit "$HEAD"
#
# Mirrors _mk_clean_advancing_lane() in tests/infra/test_warm_lane_pool.sh — the
# authoritative pattern for satisfying the inv.9 provenance guard hermetically.
# The advancing subdir is left UNtracked (--untracked-files=no ignores it) so
# that adding content to it does NOT dirty the worktree status.
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
# Block A — CLI guard: --help, unknown flag, missing positional args
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0
reset_calls
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits 2
reset_calls
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits 2" test "$RC" -eq 2

# A3: missing all positional args exits non-zero
reset_calls
run_helper
assert "A3: missing all positional args exits non-zero" test "$RC" -ne 0

# A4: only one positional arg (missing base_dir) exits non-zero
reset_calls
run_helper /some/nonexistent/dir
assert "A4: missing second positional arg exits non-zero" test "$RC" -ne 0

# ──────────────────────────────────────────────────────────────────────────────
# Block B — basic refresh happy path
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: basic refresh happy path ---"

B_TMP="$(mktemp -d /tmp/test-refresh-warm-base-b-XXXXXX)"
_TMPDIRS+=("$B_TMP")

# Build a hermetic git-worktree advancing lane (satisfies inv.9 provenance guard).
# B_ADV = $B_TMP/lane/advancing (UNtracked subdir; content added after fixture setup).
# B_BASE = $B_TMP/base (sibling of the lane, OUTSIDE the git repo — cleanest).
B_LANE="$(mk_git_advancing "$B_TMP")"
B_ADV="$B_LANE/advancing"
B_HEAD="$(git -C "$B_LANE" rev-parse HEAD)"
echo "file1 content" > "$B_ADV/file1.txt"
echo "file2 content" > "$B_ADV/file2.txt"
mkdir -p "$B_ADV/subdir"
echo "nested" > "$B_ADV/subdir/nested.txt"

B_BASE="$B_TMP/base"

# B1: basic refresh (no pre-existing base) exits 0
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$B_ADV" "$B_BASE" --landed-commit "$B_HEAD"
assert "B1: basic refresh exits 0" test "$RC" -eq 0

# B2: cp was invoked with --reflink=always
assert "B2: cp invoked with --reflink=always" \
    bash -c 'grep "^cp " "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# B3: cp targeted the <base>.gen.<N>.partial staging path (symlink-gen design)
assert "B3: cp targeted <base>.gen.<N>.partial staging path" \
    bash -c 'grep "^cp " "$1" | grep -qE "[.]gen[.][0-9]+[.]partial$"' _ "$CALLS_FILE"

# B4: <base_dir> exists and contains the advancing content (resolved via symlink)
assert "B4: <base_dir> exists after refresh" test -d "$B_BASE"
assert "B4: file1.txt has advancing content" \
    bash -c '[ "$(cat "$1/file1.txt")" = "file1 content" ]' _ "$B_BASE"
assert "B4: file2.txt has advancing content" \
    bash -c '[ "$(cat "$1/file2.txt")" = "file2 content" ]' _ "$B_BASE"
assert "B4: subdir/nested.txt exists" test -f "$B_BASE/subdir/nested.txt"

# B5: no <base>.gen.*.partial remains (staging→final mv complete) AND
#     <base> is a symlink pointing to the final .gen.N dir (symlink-gen swap).
assert "B5: no <base>.gen.*.partial remains after successful refresh" \
    bash -c '_n=0; for _p in "${1}".gen.*.partial; do [ -d "$_p" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$B_BASE"
assert "B5: <base> is a symlink to a <base>.gen.N dir after refresh" \
    bash -c '[ -L "$1" ] && readlink "$1" | grep -qE "[.]gen[.][0-9]+$"' _ "$B_BASE"

# B6: diagnostics on stderr (ERR_OUT non-empty)
assert "B6: diagnostics on stderr (non-empty)" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# B7: stdout is empty (no stdout output from the script — diagnostics only on stderr)
assert "B7: stdout is empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# B8: refresh when base already exists (bootstrap rename + symlink-gen swap)
B2_TMP="$(mktemp -d /tmp/test-refresh-warm-base-b2-XXXXXX)"
_TMPDIRS+=("$B2_TMP")
B2_LANE="$(mk_git_advancing "$B2_TMP")"
B2_ADV="$B2_LANE/advancing"
B2_HEAD="$(git -C "$B2_LANE" rev-parse HEAD)"
echo "new content" > "$B2_ADV/newfile.txt"
B2_BASE="$B2_TMP/base"
mkdir -p "$B2_BASE"
echo "old content" > "$B2_BASE/oldfile.txt"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$B2_ADV" "$B2_BASE" --landed-commit "$B2_HEAD"
assert "B8: refresh with existing base exits 0" test "$RC" -eq 0
assert "B8: new base has advancing content" \
    bash -c '[ "$(cat "$1/newfile.txt")" = "new content" ]' _ "$B2_BASE"
assert "B8: old content gone after swap" \
    test ! -f "$B2_BASE/oldfile.txt"
assert "B8: no <base>.gen.*.partial remains after refresh" \
    bash -c '_n=0; for _p in "${1}".gen.*.partial; do [ -d "$_p" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$B2_BASE"
assert "B8: <base> is a symlink to a <base>.gen.N dir after refresh" \
    bash -c '[ -L "$1" ] && readlink "$1" | grep -qE "[.]gen[.][0-9]+$"' _ "$B2_BASE"

# B9: stale <base>.gen.*.partial from a prior interrupted run (SIGKILL/power-loss).
# The script must pre-clean stale .gen.*.partial dirs before the new staging copy so
# that cp does not nest the source inside the pre-existing partial directory.
B_STALE_TMP="$(mktemp -d /tmp/test-refresh-warm-base-bstale-XXXXXX)"
_TMPDIRS+=("$B_STALE_TMP")
B_STALE_LANE="$(mk_git_advancing "$B_STALE_TMP")"
B_STALE_ADV="$B_STALE_LANE/advancing"
B_STALE_HEAD="$(git -C "$B_STALE_LANE" rev-parse HEAD)"
echo "fresh content" > "$B_STALE_ADV/fresh.txt"
B_STALE_BASE="$B_STALE_TMP/base"
# Pre-create a stale .gen.1.partial (non-empty, simulating a prior partial cp)
mkdir -p "$B_STALE_BASE.gen.1.partial"
echo "stale content" > "$B_STALE_BASE.gen.1.partial/stale.txt"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$B_STALE_ADV" "$B_STALE_BASE" --landed-commit "$B_STALE_HEAD"
assert "B9: refresh with stale .gen.*.partial exits 0" test "$RC" -eq 0
assert "B9: base has fresh content (not stale partial content)" \
    bash -c '[ "$(cat "$1/fresh.txt")" = "fresh content" ]' _ "$B_STALE_BASE"
assert "B9: base does NOT contain stale partial content (no nested cp)" \
    bash -c '! test -f "$1/stale.txt"' _ "$B_STALE_BASE"
assert "B9: no <base>.gen.*.partial remains (stale partial pre-cleaned)" \
    bash -c '_n=0; for _p in "${1}".gen.*.partial; do [ -d "$_p" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$B_STALE_BASE"

# ──────────────────────────────────────────────────────────────────────────────
# Block C — fail-closed reflink: probe failure → non-zero, no partial, pre-existing untouched
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: fail-closed reflink ---"

C_TMP="$(mktemp -d /tmp/test-refresh-warm-base-c-XXXXXX)"
_TMPDIRS+=("$C_TMP")

# Hermetic git-worktree advancing lane so the inv.9 guard PASSES — exercises the
# reflink-failure path (not the guard short-circuit). Base dir OUTSIDE the lane.
C_LANE="$(mk_git_advancing "$C_TMP")"
C_ADV="$C_LANE/advancing"
C_HEAD="$(git -C "$C_LANE" rev-parse HEAD)"
echo "adv content" > "$C_ADV/file.txt"

C_BASE="$C_TMP/base"

# C1: reflink failure exits non-zero (no pre-existing base)
reset_calls
REIFY_TEST_REFLINK_OK=0 run_helper "$C_ADV" "$C_BASE" --landed-commit "$C_HEAD"
assert "C1: reflink failure exits non-zero" test "$RC" -ne 0

# C2: stderr names the reflink failure (guard passes → reflink path now reachable)
assert "C2: stderr names reflink failure" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "reflink|Operation not supported"' _ "$ERR_OUT"

# C3: no <base>.gen.*.partial remains after failure (EXIT trap removed the partial).
# Meaningfully exercises the trap's partial-cleanup (script lines 282-286).
assert "C3: no <base>.gen.*.partial remains after reflink failure" \
    bash -c '_n=0; for _p in "${1}".gen.*.partial; do [ -d "$_p" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$C_BASE"

# C4: <base> not created after reflink failure (no swap, no symlink)
assert "C4: <base> not created after reflink failure" \
    test ! -e "$C_BASE"

# C5: a pre-existing <base_dir> is left unchanged after a failed refresh.
# The EXIT trap's bootstrap-recovery (script lines 271-276) moves the renamed
# gen dir back to <base>, and the cleanup loop removes the partial.
C2_TMP="$(mktemp -d /tmp/test-refresh-warm-base-c2-XXXXXX)"
_TMPDIRS+=("$C2_TMP")
C2_LANE="$(mk_git_advancing "$C2_TMP")"
C2_ADV="$C2_LANE/advancing"
C2_HEAD="$(git -C "$C2_LANE" rev-parse HEAD)"
echo "new adv" > "$C2_ADV/new.txt"
C2_BASE="$C2_TMP/base"
mkdir -p "$C2_BASE"
echo "original" > "$C2_BASE/orig.txt"

reset_calls
REIFY_TEST_REFLINK_OK=0 run_helper "$C2_ADV" "$C2_BASE" --landed-commit "$C2_HEAD"
assert "C5: reflink failure with existing base exits non-zero" test "$RC" -ne 0
assert "C5: pre-existing base still exists" test -d "$C2_BASE"
assert "C5: pre-existing base content unchanged (orig.txt present)" \
    test -f "$C2_BASE/orig.txt"
assert "C5: no <base>.gen.*.partial remains (EXIT trap cleanup)" \
    bash -c '_n=0; for _p in "${1}".gen.*.partial; do [ -d "$_p" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$C2_BASE"
assert "C5: no leftover <base>.gen.* dir (bootstrap backup restored to <base>)" \
    bash -c '_n=0; for _g in "${1}".gen.[0-9]*; do [ -d "$_g" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$C2_BASE"

# ──────────────────────────────────────────────────────────────────────────────
# Block D — in-flight clone independence (B6): clone dir untouched after refresh
# The cp stub performs a real recursive copy of the advancing dir.
# A pre-existing sibling clone dir (simulating an in-flight lane) must remain
# byte-identical and must never appear in the CALLS_FILE (no drain protocol).
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: in-flight clone independence (B6) ---"

D_TMP="$(mktemp -d /tmp/test-refresh-warm-base-d-XXXXXX)"
_TMPDIRS+=("$D_TMP")

# Hermetic git-worktree advancing lane (inv.9 guard). D_BASE and D_CLONE stay
# OUTSIDE the lane repo — they simulate the pool-base and an in-flight clone.
D_LANE="$(mk_git_advancing "$D_TMP")"
D_ADV="$D_LANE/advancing"
D_HEAD="$(git -C "$D_LANE" rev-parse HEAD)"
echo "new adv content" > "$D_ADV/newfile.txt"
D_BASE="$D_TMP/base"
mkdir -p "$D_BASE"
echo "old base content" > "$D_BASE/oldfile.txt"

# Create a sibling in-flight clone (simulating a lane that grabbed the OLD base)
D_CLONE="$D_TMP/clone-lane-42"
mkdir -p "$D_CLONE"
echo "old base content" > "$D_CLONE/oldfile.txt"
_CLONE_MTIME="$(stat -c '%Y' "$D_CLONE/oldfile.txt")"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$D_ADV" "$D_BASE" --landed-commit "$D_HEAD"
assert "D1: refresh with in-flight clone exits 0" test "$RC" -eq 0

# D2: the clone dir still has its original content
assert "D2: clone dir still has original file" test -f "$D_CLONE/oldfile.txt"
assert "D2: clone dir original content unchanged" \
    bash -c '[ "$(cat "$1/oldfile.txt")" = "old base content" ]' _ "$D_CLONE"

# D3: clone mtime is unchanged (no touch/write to clone)
assert "D3: clone file mtime unchanged after refresh" \
    bash -c '[ "$(stat -c "%Y" "$1/oldfile.txt")" = "$2" ]' _ "$D_CLONE" "$_CLONE_MTIME"

# D4: CALLS_FILE never references the clone path (no drain: script never touches clone)
assert "D4: CALLS_FILE has no reference to clone path (no drain protocol)" \
    bash -c '! grep -qF "'"$D_CLONE"'" "$1"' _ "$CALLS_FILE"

# D5: the new advancing content is in the base (correct refresh happened)
assert "D5: base has advancing content after refresh" \
    bash -c '[ "$(cat "$1/newfile.txt")" = "new adv content" ]' _ "$D_BASE"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — base self-description stamps: .rustflags and .invocation
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: self-description stamps ---"

E_TMP="$(mktemp -d /tmp/test-refresh-warm-base-e-XXXXXX)"
_TMPDIRS+=("$E_TMP")
# Hermetic git-worktree advancing lane (inv.9 guard). E_BASE stays OUTSIDE lane.
E_LANE="$(mk_git_advancing "$E_TMP")"
E_ADV="$E_LANE/advancing"
E_HEAD="$(git -C "$E_LANE" rev-parse HEAD)"
echo "content" > "$E_ADV/f.txt"
E_BASE="$E_TMP/base"

# E1: .rustflags stamp written with RUSTFLAGS env value
reset_calls
RUSTFLAGS="-C foo" REIFY_TEST_REFLINK_OK=1 run_helper "$E_ADV" "$E_BASE" --landed-commit "$E_HEAD"
assert "E1: refresh with RUSTFLAGS exits 0" test "$RC" -eq 0
assert "E1: <base_dir>.rustflags exists after refresh" test -f "$E_BASE.rustflags"
assert "E1: <base_dir>.rustflags contains RUSTFLAGS value" \
    bash -c '[ "$(cat "$1.rustflags")" = "-C foo" ]' _ "$E_BASE"

# E2: .invocation stamp written with --invocation value
assert "E2: <base_dir>.invocation exists after refresh" test -f "$E_BASE.invocation"
assert "E2: <base_dir>.invocation is empty when --invocation not passed" \
    bash -c '[ -z "$(cat "$1.invocation")" ]' _ "$E_BASE"

# E3: stamps present after the symlink-gen swap (siblings of <base>, not inside)
assert "E3: stamps are siblings of <base_dir> (not inside it)" \
    bash -c 'test -f "$1.rustflags" && ! test -f "$1/base.rustflags"' _ "$E_BASE"

# E4: --rustflags flag overrides the RUSTFLAGS env
E2_TMP="$(mktemp -d /tmp/test-refresh-warm-base-e2-XXXXXX)"
_TMPDIRS+=("$E2_TMP")
E2_LANE="$(mk_git_advancing "$E2_TMP")"
E2_ADV="$E2_LANE/advancing"
E2_HEAD="$(git -C "$E2_LANE" rev-parse HEAD)"
echo "c" > "$E2_ADV/f.txt"
E2_BASE="$E2_TMP/base"

reset_calls
RUSTFLAGS="-C env-value" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$E2_ADV" "$E2_BASE" --landed-commit "$E2_HEAD" --rustflags "-C override"
assert "E4: --rustflags override exits 0" test "$RC" -eq 0
assert "E4: .rustflags contains --rustflags value (not RUSTFLAGS env)" \
    bash -c '[ "$(cat "$1.rustflags")" = "-C override" ]' _ "$E2_BASE"

# E5: RUSTFLAGS unset -> .rustflags file exists but is empty
E3_TMP="$(mktemp -d /tmp/test-refresh-warm-base-e3-XXXXXX)"
_TMPDIRS+=("$E3_TMP")
E3_LANE="$(mk_git_advancing "$E3_TMP")"
E3_ADV="$E3_LANE/advancing"
E3_HEAD="$(git -C "$E3_LANE" rev-parse HEAD)"
echo "c" > "$E3_ADV/f.txt"
E3_BASE="$E3_TMP/base"

reset_calls
unset RUSTFLAGS 2>/dev/null || true
REIFY_TEST_REFLINK_OK=1 run_helper "$E3_ADV" "$E3_BASE" --landed-commit "$E3_HEAD"
assert "E5: unset RUSTFLAGS refresh exits 0" test "$RC" -eq 0
assert "E5: .rustflags exists even when RUSTFLAGS unset" test -f "$E3_BASE.rustflags"
assert "E5: .rustflags is empty when RUSTFLAGS unset" \
    bash -c '[ -z "$(cat "$1.rustflags")" ]' _ "$E3_BASE"

# E6: --invocation value written to .invocation stamp
E4_TMP="$(mktemp -d /tmp/test-refresh-warm-base-e4-XXXXXX)"
_TMPDIRS+=("$E4_TMP")
E4_LANE="$(mk_git_advancing "$E4_TMP")"
E4_ADV="$E4_LANE/advancing"
E4_HEAD="$(git -C "$E4_LANE" rev-parse HEAD)"
echo "c" > "$E4_ADV/f.txt"
E4_BASE="$E4_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$E4_ADV" "$E4_BASE" --landed-commit "$E4_HEAD" \
    --invocation "sha256:abc123"
assert "E6: --invocation refresh exits 0" test "$RC" -eq 0
assert "E6: .invocation contains --invocation value" \
    bash -c '[ "$(cat "$1.invocation")" = "sha256:abc123" ]' _ "$E4_BASE"

# ──────────────────────────────────────────────────────────────────────────────
# Block F — --check-frag defrag signal: verdict token + extent count, read-only
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: --check-frag defrag signal ---"

F_TMP="$(mktemp -d /tmp/test-refresh-warm-base-f-XXXXXX)"
_TMPDIRS+=("$F_TMP")
F_BASE="$F_TMP/base"
mkdir -p "$F_BASE"
echo "binary" > "$F_BASE/rustc"
echo "other" > "$F_BASE/libstd.rlib"

# F1: extents below threshold -> stdout "ok N", exits 0
reset_calls
REIFY_TEST_FRAG_EXTENTS=2 run_helper --check-frag "$F_BASE" --frag-threshold 64
assert "F1: --check-frag below threshold exits 0" test "$RC" -eq 0
assert "F1: stdout starts with 'ok'" \
    bash -c 'printf "%s\n" "$1" | grep -q "^ok "' _ "$OUT"
assert "F1: stdout contains extent count" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^ok [0-9]+"' _ "$OUT"

# F2: extents at/above threshold -> stdout "reseed-due N", exits 0
reset_calls
REIFY_TEST_FRAG_EXTENTS=64 run_helper --check-frag "$F_BASE" --frag-threshold 64
assert "F2: --check-frag at threshold exits 0" test "$RC" -eq 0
assert "F2: stdout starts with 'reseed-due'" \
    bash -c 'printf "%s\n" "$1" | grep -q "^reseed-due "' _ "$OUT"
assert "F2: stdout contains extent count" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^reseed-due [0-9]+"' _ "$OUT"

# F3: --check-frag performs NO refresh (read-only).
# mv is NOT stubbed (real mv, no CALLS_FILE entry), so a CALLS_FILE check for
# "^mv" would be vacuously true. Instead: snapshot base content/mtime before
# the check and assert they are byte-identical afterward; also assert no
# .gen.* artifact is created (which a normal refresh would produce).
reset_calls
_F3_SNAPSHOT="$(find "$F_BASE" -type f -printf '%P:%s:%T@\n' 2>/dev/null | sort)"
REIFY_TEST_FRAG_EXTENTS=1 run_helper --check-frag "$F_BASE"
assert "F3: --check-frag: no cp --reflink recorded (read-only)" \
    bash -c '! grep -q "^cp.*--reflink=always" "$1"' _ "$CALLS_FILE"
assert "F3: --check-frag: base content+mtime unchanged (read-only)" \
    bash -c '_after="$(find "$1" -type f -printf '"'"'%P:%s:%T@\n'"'"' 2>/dev/null | sort)"; [ "$_after" = "$2" ]' _ "$F_BASE" "$_F3_SNAPSHOT"
assert "F3: --check-frag: no <base>.gen.* artifact created (read-only)" \
    bash -c '_n=0; for _g in "${1}".gen.*; do [ -e "$_g" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$F_BASE"

# F4: xfs_bmap was invoked per file under base
reset_calls
REIFY_TEST_FRAG_EXTENTS=1 run_helper --check-frag "$F_BASE"
assert "F4: xfs_bmap invoked at least once (per-file extent scan)" \
    bash -c 'grep -q "^xfs_bmap " "$1"' _ "$CALLS_FILE"

# F5: xfs_bmap unavailable/failing -> non-zero exit + actionable stderr
# REIFY_TEST_XFSBMAP_OK=0 makes the stub exit 1 (simulates xfs_bmap failure).
# The script must propagate this failure rather than swallowing it with || true.
reset_calls
REIFY_TEST_XFSBMAP_OK=0 run_helper --check-frag "$F_BASE"
assert "F5: xfs_bmap failure exits non-zero" test "$RC" -ne 0
assert "F5: actionable stderr when xfs_bmap fails" \
    bash -c 'printf "%s\n" "$1" | grep -qi "xfs_bmap"' _ "$ERR_OUT"

# F6: base_dir missing -> non-zero exit + actionable stderr
reset_calls
run_helper --check-frag "$F_TMP/nonexistent"
assert "F6: missing base_dir exits non-zero" test "$RC" -ne 0
assert "F6: actionable stderr when base_dir missing" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# F7: --check-frag with higher-extent file triggers reseed-due correctly
F2_TMP="$(mktemp -d /tmp/test-refresh-warm-base-f2-XXXXXX)"
_TMPDIRS+=("$F2_TMP")
F2_BASE="$F2_TMP/base"
mkdir -p "$F2_BASE"
echo "bin" > "$F2_BASE/binary"

reset_calls
REIFY_TEST_FRAG_EXTENTS=65 run_helper --check-frag "$F2_BASE" --frag-threshold 64
assert "F7: extents 65 >= threshold 64 -> reseed-due" \
    bash -c 'printf "%s\n" "$1" | grep -q "^reseed-due "' _ "$OUT"

reset_calls
REIFY_TEST_FRAG_EXTENTS=63 run_helper --check-frag "$F2_BASE" --frag-threshold 64
assert "F7: extents 63 < threshold 64 -> ok" \
    bash -c 'printf "%s\n" "$1" | grep -q "^ok "' _ "$OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block G — basecommit provenance: per-gen stamp written at promote time
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: basecommit provenance ---"

G_TMP="$(mktemp -d /tmp/test-refresh-warm-base-g-XXXXXX)"
_TMPDIRS+=("$G_TMP")
G_LANE="$(mk_git_advancing "$G_TMP")"
G_ADV="$G_LANE/advancing"
G_HEAD="$(git -C "$G_LANE" rev-parse HEAD)"
echo "content" > "$G_ADV/file.txt"
G_BASE="$G_TMP/base"

# G1: after a successful refresh, the per-gen stamp exists as a sibling of the gen dir
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$G_ADV" "$G_BASE" --landed-commit "$G_HEAD"
assert "G1: refresh exits 0" test "$RC" -eq 0
G1_GEN="$(readlink "$G_BASE")"
assert "G1: per-gen .basecommit exists after refresh" test -f "${G1_GEN}.basecommit"

# G2: stamp content == HEAD (the verified landed commit, not ahead)
assert "G2: .basecommit content == HEAD" \
    bash -c '[ "$(cat "${1}.basecommit")" = "$2" ]' _ "$G1_GEN" "$G_HEAD"

# G3: second refresh with same HEAD — new gen stamp is still HEAD
echo "content2" > "$G_ADV/file2.txt"
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$G_ADV" "$G_BASE" --landed-commit "$G_HEAD"
assert "G3: second refresh exits 0" test "$RC" -eq 0
G3_GEN="$(readlink "$G_BASE")"
assert "G3: second refresh .basecommit exists" test -f "${G3_GEN}.basecommit"
assert "G3: second refresh .basecommit content == HEAD" \
    bash -c '[ "$(cat "${1}.basecommit")" = "$2" ]' _ "$G3_GEN" "$G_HEAD"

# G4: new commit → refresh with new --landed-commit → new gen .basecommit == new HEAD
# (drift-proof: stamp tracks the promoted commit, never ahead)
G4_TMP="$(mktemp -d /tmp/test-refresh-warm-base-g4-XXXXXX)"
_TMPDIRS+=("$G4_TMP")
G4_LANE="$(mk_git_advancing "$G4_TMP")"
G4_ADV="$G4_LANE/advancing"
G4_HEAD1="$(git -C "$G4_LANE" rev-parse HEAD)"
echo "v1" > "$G4_ADV/file.txt"
G4_BASE="$G4_TMP/base"

# First refresh with HEAD1
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$G4_ADV" "$G4_BASE" --landed-commit "$G4_HEAD1"
assert "G4: first refresh exits 0" test "$RC" -eq 0

# Make a new commit to advance HEAD
printf 'placeholder2\n' > "$G4_LANE/placeholder2"
git -C "$G4_LANE" add -- placeholder2
git -C "$G4_LANE" \
    -c user.email="warm-lane-test@localhost" \
    -c user.name="Warm Lane Test" \
    -c commit.gpgsign=false \
    commit -q --no-verify -m "fixture: advance HEAD"
G4_HEAD2="$(git -C "$G4_LANE" rev-parse HEAD)"
echo "v2" > "$G4_ADV/file.txt"

# Second refresh with HEAD2
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$G4_ADV" "$G4_BASE" --landed-commit "$G4_HEAD2"
assert "G4: second refresh (new HEAD) exits 0" test "$RC" -eq 0
G4_NEW_GEN="$(readlink "$G4_BASE")"
assert "G4: new gen .basecommit == HEAD2 (new commit, not stale HEAD1)" \
    bash -c '[ "$(cat "${1}.basecommit")" = "$2" ]' _ "$G4_NEW_GEN" "$G4_HEAD2"
assert "G4: new gen .basecommit != HEAD1 (drift-proof)" \
    bash -c '[ "$(cat "${1}.basecommit")" != "$2" ]' _ "$G4_NEW_GEN" "$G4_HEAD1"

# G5: GC reaps retired gen + its .basecommit sibling (no orphan accumulation).
# After two refreshes with no reader holding gen.1.lock, gen.1 and its .basecommit
# must both be gone while the live gen still has its .basecommit.
G5_TMP="$(mktemp -d /tmp/test-refresh-warm-base-g5-XXXXXX)"
_TMPDIRS+=("$G5_TMP")
G5_LANE="$(mk_git_advancing "$G5_TMP")"
G5_ADV="$G5_LANE/advancing"
G5_HEAD="$(git -C "$G5_LANE" rev-parse HEAD)"
echo "content" > "$G5_ADV/file.txt"
G5_BASE="$G5_TMP/base"

# First refresh: creates gen.N with .basecommit; capture gen path before next refresh
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$G5_ADV" "$G5_BASE" --landed-commit "$G5_HEAD"
assert "G5: first refresh exits 0" test "$RC" -eq 0
G5_GEN1="$(readlink "$G5_BASE")"
assert "G5: gen.1 .basecommit exists" test -f "${G5_GEN1}.basecommit"

# Second refresh: GC sweeps gen.1 (no reader holds gen.1.lock → exclusive flock succeeds)
echo "content2" > "$G5_ADV/file2.txt"
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$G5_ADV" "$G5_BASE" --landed-commit "$G5_HEAD"
assert "G5: second refresh exits 0" test "$RC" -eq 0
G5_GEN2="$(readlink "$G5_BASE")"
# Live gen retains its .basecommit
assert "G5: live gen .basecommit present" test -f "${G5_GEN2}.basecommit"
# Retired gen.1 is fully reaped — dir AND .basecommit sibling
assert "G5: retired gen.1 dir GONE (reaped by GC)" test ! -d "$G5_GEN1"
assert "G5: retired gen.1 .basecommit GONE (reaped with its gen — no orphan)" \
    test ! -f "${G5_GEN1}.basecommit"

# ──────────────────────────────────────────────────────────────────────────────
# Block H — buildroot provenance: per-gen build-worktree stamp written at promote time
# Mirrors Block G (.basecommit) for the new <base>.gen.<N>.buildroot sidecar
# (task 4983): records realpath(dirname(advancing_target_dir)) — the advancing
# worktree ROOT under which the base binaries were compiled — so seed-warm-lane.sh
# can detect a build-worktree mismatch and relink env!()-baked-path test binaries.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: buildroot provenance ---"

H_TMP="$(mktemp -d /tmp/test-refresh-warm-base-h-XXXXXX)"
_TMPDIRS+=("$H_TMP")
H_LANE="$(mk_git_advancing "$H_TMP")"
H_ADV="$H_LANE/advancing"
H_HEAD="$(git -C "$H_LANE" rev-parse HEAD)"
echo "content" > "$H_ADV/file.txt"
H_BASE="$H_TMP/base"

# H1: after a successful refresh, the per-gen .buildroot stamp exists as a sibling
# of the gen dir (mirrors G1's .basecommit existence check).
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$H_ADV" "$H_BASE" --landed-commit "$H_HEAD"
assert "H1: refresh exits 0" test "$RC" -eq 0
H1_GEN="$(readlink "$H_BASE")"
assert "H1: per-gen .buildroot exists after refresh" test -f "${H1_GEN}.buildroot"

# H2: stamp content == realpath of the advancing WORKTREE ROOT
# (dirname(advancing_target_dir) = H_LANE, NOT the advancing/ subdir itself).
assert "H2: .buildroot content == realpath(advancing worktree root)" \
    bash -c '[ "$(cat "${1}.buildroot")" = "$2" ]' _ "$H1_GEN" "$(realpath "$H_LANE")"

# H3: GC reaps retired gen + its .buildroot sibling (no orphan accumulation),
# mirroring G5's .basecommit reap assertions.  After two refreshes with no
# reader holding gen.1.lock, gen.1 and its .buildroot must both be gone while
# the live gen still has its .buildroot.
H3_TMP="$(mktemp -d /tmp/test-refresh-warm-base-h3-XXXXXX)"
_TMPDIRS+=("$H3_TMP")
H3_LANE="$(mk_git_advancing "$H3_TMP")"
H3_ADV="$H3_LANE/advancing"
H3_HEAD="$(git -C "$H3_LANE" rev-parse HEAD)"
echo "content" > "$H3_ADV/file.txt"
H3_BASE="$H3_TMP/base"

# First refresh: creates gen.N with .buildroot; capture gen path before next refresh
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$H3_ADV" "$H3_BASE" --landed-commit "$H3_HEAD"
assert "H3: first refresh exits 0" test "$RC" -eq 0
H3_GEN1="$(readlink "$H3_BASE")"
assert "H3: gen.1 .buildroot exists" test -f "${H3_GEN1}.buildroot"

# Second refresh: GC sweeps gen.1 (no reader holds gen.1.lock → exclusive flock succeeds)
echo "content2" > "$H3_ADV/file2.txt"
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$H3_ADV" "$H3_BASE" --landed-commit "$H3_HEAD"
assert "H3: second refresh exits 0" test "$RC" -eq 0
H3_GEN2="$(readlink "$H3_BASE")"
# Live gen retains its .buildroot
assert "H3: live gen .buildroot present" test -f "${H3_GEN2}.buildroot"
# Retired gen.1 is fully reaped — dir AND .buildroot sibling
assert "H3: retired gen.1 dir GONE (reaped by GC)" test ! -d "$H3_GEN1"
assert "H3: retired gen.1 .buildroot GONE (reaped with its gen — no orphan)" \
    test ! -f "${H3_GEN1}.buildroot"

# ──────────────────────────────────────────────────────────────────────────────
# Block I — the provenance WIP refusal message (task 5981)
#
# This refusal fires INSIDE a warm lane (the advancing dir is a lane's target/),
# so whatever remedy it names is read by exactly the population that filled the
# shared stash stack: refs/stash is ONE ref in the shared .git, not per-worktree,
# so every lane on this host pushes onto one LIFO stack (esc-5785-6, nine entries
# over ~1 month). The guard's REFUSAL PATH had no coverage at all before this
# block — behavioural asserts on the script's real stderr, in the shape of
# test_land_script.sh's "dirty tree -> error says 'dirty'".
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block I: WIP refusal advises committing, never stashing ---"

I_TMP="$(mktemp -d /tmp/test-refresh-warm-base-i-XXXXXX)"
_TMPDIRS+=("$I_TMP")
I_LANE="$(mk_git_advancing "$I_TMP")"
I_ADV="$I_LANE/advancing"
I_HEAD="$(git -C "$I_LANE" rev-parse HEAD)"
# Dirty a TRACKED file. The guard runs `git status --porcelain
# --untracked-files=no`, so untracked content (the advancing subdir itself)
# would NOT trip it — this must be the committed .placeholder.
printf 'uncommitted WIP\n' >> "$I_LANE/.placeholder"

reset_calls
run_helper "$I_ADV" "$I_TMP/base" --landed-commit "$I_HEAD"
assert "I1: advancing worktree with tracked WIP is refused (non-zero)" test "$RC" -ne 0
assert "I2: refusal names the WIP condition" \
    bash -c 'printf "%s\n" "$1" | grep -qi "WIP"' _ "$ERR_OUT"
assert "I3: refusal advises committing" \
    bash -c 'printf "%s\n" "$1" | grep -qi "commit"' _ "$ERR_OUT"
# Bare-token check, same rationale as test_land_script.sh's: a reworded advisory
# that reaches for the word again should fail here and be reconsidered.
assert "I4: refusal does NOT advise stashing" \
    bash -c '! printf "%s\n" "$1" | grep -qi "stash"' _ "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block J — superseded hash-generation prune (keep newest 2) (task 7426)
#
# scripts/refresh-warm-base.sh prunes superseded cargo hash-generations from
# <partial>/debug/deps during the refresh, keeping only the newest 2 per
# (stem, ext) group ordered by mtime. This reclaims the 96.6%-by-bytes prize
# (extensionless test/bench binaries) on the live warm base while leaving a
# fallback generation so a lane whose fingerprint misses the single newest
# survivor does not rebuild cold.
#
# J1 — the core N=2 boundary on the extensionless case: three generations of
# one unit prune to the newest two; a sibling two-generation group is left
# entirely intact (the property that distinguishes keep-newest-2 from
# keep-newest-1).
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block J: superseded hash-generation prune (keep newest 2) ---"

J_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j-XXXXXX)"
_TMPDIRS+=("$J_TMP")
J_LANE="$(mk_git_advancing "$J_TMP")"
J_ADV="$J_LANE/advancing"
J_HEAD="$(git -C "$J_LANE" rev-parse HEAD)"
mkdir -p "$J_ADV/debug/deps"

# Three hash-generations of ONE unit, extensionless (the 96.6%-by-bytes case:
# test/bench binaries), distinct non-empty content, distinct mtimes (oldest to
# newest). No sleeps: mtimes are stamped explicitly via `touch -d` (T8).
echo "gen1 content" > "$J_ADV/debug/deps/reify_kernel_tests-1111111111111111"
echo "gen2 content" > "$J_ADV/debug/deps/reify_kernel_tests-2222222222222222"
echo "gen3 content" > "$J_ADV/debug/deps/reify_kernel_tests-3333333333333333"
touch -d '2026-01-01 00:00:00' "$J_ADV/debug/deps/reify_kernel_tests-1111111111111111"
touch -d '2026-02-01 00:00:00' "$J_ADV/debug/deps/reify_kernel_tests-2222222222222222"
touch -d '2026-03-01 00:00:00' "$J_ADV/debug/deps/reify_kernel_tests-3333333333333333"

# A second unit with only TWO generations — both must survive untouched; this
# is what distinguishes keep-newest-2 from keep-newest-1 (J1e).
echo "other gen a" > "$J_ADV/debug/deps/reify_other_crate-aaaaaaaaaaaaaaaa"
echo "other gen b" > "$J_ADV/debug/deps/reify_other_crate-bbbbbbbbbbbbbbbb"
touch -d '2026-01-15 00:00:00' "$J_ADV/debug/deps/reify_other_crate-aaaaaaaaaaaaaaaa"
touch -d '2026-02-15 00:00:00' "$J_ADV/debug/deps/reify_other_crate-bbbbbbbbbbbbbbbb"

J_BASE="$J_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$J_ADV" "$J_BASE" --landed-commit "$J_HEAD"
assert "J1a: refresh with superseded hash-generations exits 0" test "$RC" -eq 0

J_GEN="$(readlink "$J_BASE")"
J_DEPS="$J_GEN/debug/deps"

assert "J1b: newest two generations survive (-2222..., -3333...)" \
    bash -c 'test -f "$1/reify_kernel_tests-2222222222222222" && test -f "$1/reify_kernel_tests-3333333333333333"' _ "$J_DEPS"

assert "J1c: oldest generation is pruned (-1111...)" \
    bash -c 'test ! -f "$1/reify_kernel_tests-1111111111111111"' _ "$J_DEPS"

assert "J1d: exactly 2 files remain in the reify_kernel_tests group" \
    bash -c '[ "$(find "$1" -maxdepth 1 -type f -name "reify_kernel_tests-*" | wc -l)" -eq 2 ]' _ "$J_DEPS"

assert "J1e: a 2-generation group is left entirely intact (keep-2, not keep-1)" \
    bash -c 'test -f "$1/reify_other_crate-aaaaaaaaaaaaaaaa" && test -f "$1/reify_other_crate-bbbbbbbbbbbbbbbb"' _ "$J_DEPS"

# J2 — pin the artefact-name grammar. This is the assertion set that protects
# the ruled-and-measured 64.8 GiB mechanism from a greedier regex, and it is
# the discrepancy that reconciles the task's 11,193 groups with the
# 3,607 + 7,586 split (the 7,586 are all `.dwo` singletons — see J2b).
J2_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j2-XXXXXX)"
_TMPDIRS+=("$J2_TMP")
J2_LANE="$(mk_git_advancing "$J2_TMP")"
J2_ADV="$J2_LANE/advancing"
J2_HEAD="$(git -C "$J2_LANE" rev-parse HEAD)"
mkdir -p "$J2_ADV/debug/deps"
J2_SRC="$J2_ADV/debug/deps"

# J2a — EXTENSION SPLIT: (stem, ext) are independent groups. Three hashes,
# each present as BOTH .rlib and .rmeta (six files, one stem). A grouper keyed
# on stem alone (as of step-2) pools all six and keeps only 2 total; the
# correct grammar keeps the newest 2 .rlib AND the newest 2 .rmeta
# independently (4 files survive, not 2).
echo "rlib a" > "$J2_SRC/libreify_core-aaaaaaaaaaaaaaaa.rlib"
echo "rmeta a" > "$J2_SRC/libreify_core-aaaaaaaaaaaaaaaa.rmeta"
touch -d '2026-01-01 00:00:00' "$J2_SRC/libreify_core-aaaaaaaaaaaaaaaa.rlib" "$J2_SRC/libreify_core-aaaaaaaaaaaaaaaa.rmeta"
echo "rlib b" > "$J2_SRC/libreify_core-bbbbbbbbbbbbbbbb.rlib"
echo "rmeta b" > "$J2_SRC/libreify_core-bbbbbbbbbbbbbbbb.rmeta"
touch -d '2026-02-01 00:00:00' "$J2_SRC/libreify_core-bbbbbbbbbbbbbbbb.rlib" "$J2_SRC/libreify_core-bbbbbbbbbbbbbbbb.rmeta"
echo "rlib c" > "$J2_SRC/libreify_core-cccccccccccccccc.rlib"
echo "rmeta c" > "$J2_SRC/libreify_core-cccccccccccccccc.rmeta"
touch -d '2026-03-01 00:00:00' "$J2_SRC/libreify_core-cccccccccccccccc.rlib" "$J2_SRC/libreify_core-cccccccccccccccc.rmeta"

# J2b — .dwo NON-CANDIDATE: split-debuginfo-shaped names, three different
# leading hashes. Stem-before-last-dot ends "-cgu.09.rcgu" (not "-<16hex>"),
# so these are never a prune candidate — exactly the boundary a permissive
# `^(.+)-[0-9a-f]{16}(\..*)?$` would cross, silently changing the measured
# reclaim.
echo "dwo 1" > "$J2_SRC/axum-0082f0d2178b90e5.axum.191af5780e3108ae-cgu.09.rcgu.dwo"
echo "dwo 2" > "$J2_SRC/axum-1111111111111111.axum.191af5780e3108ae-cgu.09.rcgu.dwo"
echo "dwo 3" > "$J2_SRC/axum-2222222222222222.axum.191af5780e3108ae-cgu.09.rcgu.dwo"
touch -d '2026-01-01 00:00:00' "$J2_SRC/axum-0082f0d2178b90e5.axum.191af5780e3108ae-cgu.09.rcgu.dwo"
touch -d '2026-01-02 00:00:00' "$J2_SRC/axum-1111111111111111.axum.191af5780e3108ae-cgu.09.rcgu.dwo"
touch -d '2026-01-03 00:00:00' "$J2_SRC/axum-2222222222222222.axum.191af5780e3108ae-cgu.09.rcgu.dwo"

# J2c — NON-CONFORMING NAMES NEVER DELETED: no hash at all, and a
# short/long/uppercase pseudo-hash. Every one survives regardless of mtime.
echo "readme" > "$J2_SRC/README"
touch -d '2026-01-01 00:00:00' "$J2_SRC/README"
echo "plan1" > "$J2_SRC/build-plan-1.json"
echo "plan2" > "$J2_SRC/build-plan-2.json"
echo "plan3" > "$J2_SRC/build-plan-3.json"
touch -d '2026-01-05 00:00:00' "$J2_SRC/build-plan-1.json"
touch -d '2026-01-10 00:00:00' "$J2_SRC/build-plan-2.json"
touch -d '2026-01-15 00:00:00' "$J2_SRC/build-plan-3.json"
echo "short" > "$J2_SRC/foo-abc123.rlib"
touch -d '2026-01-20 00:00:00' "$J2_SRC/foo-abc123.rlib"
echo "upper" > "$J2_SRC/foo-AAAABBBBCCCCDDDD"
touch -d '2026-01-25 00:00:00' "$J2_SRC/foo-AAAABBBBCCCCDDDD"
echo "long" > "$J2_SRC/foo-0123456789abcdef0.rlib"
touch -d '2026-01-30 00:00:00' "$J2_SRC/foo-0123456789abcdef0.rlib"

# J2d — DISTINCT UNITS DO NOT MERGE: two extensionless units, three
# generations each; each keeps exactly 2, proving the group key is the stem
# and not a global pool.
echo "a1" > "$J2_SRC/reify_a-1111111111111111"
echo "a2" > "$J2_SRC/reify_a-2222222222222222"
echo "a3" > "$J2_SRC/reify_a-3333333333333333"
touch -d '2026-01-01 00:00:00' "$J2_SRC/reify_a-1111111111111111"
touch -d '2026-02-01 00:00:00' "$J2_SRC/reify_a-2222222222222222"
touch -d '2026-03-01 00:00:00' "$J2_SRC/reify_a-3333333333333333"
echo "b1" > "$J2_SRC/reify_b-4444444444444444"
echo "b2" > "$J2_SRC/reify_b-5555555555555555"
echo "b3" > "$J2_SRC/reify_b-6666666666666666"
touch -d '2026-01-01 00:00:00' "$J2_SRC/reify_b-4444444444444444"
touch -d '2026-02-01 00:00:00' "$J2_SRC/reify_b-5555555555555555"
touch -d '2026-03-01 00:00:00' "$J2_SRC/reify_b-6666666666666666"

J2_BASE="$J2_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$J2_ADV" "$J2_BASE" --landed-commit "$J2_HEAD"
assert "J2: refresh exits 0" test "$RC" -eq 0

J2_GEN="$(readlink "$J2_BASE")"
J2_DEPS="$J2_GEN/debug/deps"

assert "J2a: newest 2 .rlib survive (bbbb, cccc)" \
    bash -c 'test -f "$1/libreify_core-bbbbbbbbbbbbbbbb.rlib" && test -f "$1/libreify_core-cccccccccccccccc.rlib"' _ "$J2_DEPS"
assert "J2a: oldest .rlib pruned (aaaa)" \
    bash -c 'test ! -f "$1/libreify_core-aaaaaaaaaaaaaaaa.rlib"' _ "$J2_DEPS"
assert "J2a: newest 2 .rmeta survive (bbbb, cccc)" \
    bash -c 'test -f "$1/libreify_core-bbbbbbbbbbbbbbbb.rmeta" && test -f "$1/libreify_core-cccccccccccccccc.rmeta"' _ "$J2_DEPS"
assert "J2a: oldest .rmeta pruned (aaaa)" \
    bash -c 'test ! -f "$1/libreify_core-aaaaaaaaaaaaaaaa.rmeta"' _ "$J2_DEPS"
assert "J2a: exactly 4 libreify_core files remain (2 rlib + 2 rmeta, not 2 total)" \
    bash -c '[ "$(find "$1" -maxdepth 1 -type f -name "libreify_core-*" | wc -l)" -eq 4 ]' _ "$J2_DEPS"

assert "J2b: all three .dwo split-debuginfo shards survive" \
    bash -c 'n=$(find "$1" -maxdepth 1 -type f -name "axum-*.dwo" | wc -l); [ "$n" -eq 3 ]' _ "$J2_DEPS"

assert "J2c: README (no hash) survives" test -f "$J2_DEPS/README"
assert "J2c: all three non-conforming build-plan files survive" \
    bash -c 'test -f "$1/build-plan-1.json" && test -f "$1/build-plan-2.json" && test -f "$1/build-plan-3.json"' _ "$J2_DEPS"
assert "J2c: short pseudo-hash (6 chars) survives" test -f "$J2_DEPS/foo-abc123.rlib"
assert "J2c: uppercase pseudo-hash survives" test -f "$J2_DEPS/foo-AAAABBBBCCCCDDDD"
assert "J2c: long pseudo-hash (17 chars) survives" test -f "$J2_DEPS/foo-0123456789abcdef0.rlib"

assert "J2d: reify_a keeps newest 2 (2222, 3333), prunes oldest (1111)" \
    bash -c 'test -f "$1/reify_a-2222222222222222" && test -f "$1/reify_a-3333333333333333" && test ! -f "$1/reify_a-1111111111111111"' _ "$J2_DEPS"
assert "J2d: reify_b keeps newest 2 (5555, 6666), prunes oldest (4444)" \
    bash -c 'test -f "$1/reify_b-5555555555555555" && test -f "$1/reify_b-6666666666666666" && test ! -f "$1/reify_b-4444444444444444"' _ "$J2_DEPS"
assert "J2d: exactly 4 reify_a/reify_b files total (2 each, distinct pools)" \
    bash -c '[ "$(find "$1" -maxdepth 1 -type f \( -name "reify_a-*" -o -name "reify_b-*" \) | wc -l)" -eq 4 ]' _ "$J2_DEPS"

# J3 — scope containment. The prune must reach debug/deps and nothing else;
# this is the assertion set that keeps a future widening from quietly eating
# the fingerprint/build/release trees. One fixture advancing dir carries three
# same-unit generations in each of six locations; only the debug/deps copy
# (the control) may be pruned.
_j3_mint_three() {
    local dir="$1"
    mkdir -p "$dir"
    echo "gen1" > "$dir/reify_scope_unit-1111111111111111"
    echo "gen2" > "$dir/reify_scope_unit-2222222222222222"
    echo "gen3" > "$dir/reify_scope_unit-3333333333333333"
    touch -d '2026-01-01 00:00:00' "$dir/reify_scope_unit-1111111111111111"
    touch -d '2026-02-01 00:00:00' "$dir/reify_scope_unit-2222222222222222"
    touch -d '2026-03-01 00:00:00' "$dir/reify_scope_unit-3333333333333333"
}

J3_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j3-XXXXXX)"
_TMPDIRS+=("$J3_TMP")
J3_LANE="$(mk_git_advancing "$J3_TMP")"
J3_ADV="$J3_LANE/advancing"
J3_HEAD="$(git -C "$J3_LANE" rev-parse HEAD)"

_j3_mint_three "$J3_ADV/debug/deps"          # J3a: the control — pruned to 2
_j3_mint_three "$J3_ADV/debug/.fingerprint"  # J3b: sibling of deps — untouched
_j3_mint_three "$J3_ADV/debug/build"         # J3c: sibling of deps — untouched
_j3_mint_three "$J3_ADV/release/deps"        # J3d: different top-level tree — untouched
_j3_mint_three "$J3_ADV/debug/deps/nested"   # J3e: depth-2 under deps — untouched
_j3_mint_three "$J3_ADV/debug"               # J3f: hashed file directly in debug/ — untouched

J3_BASE="$J3_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$J3_ADV" "$J3_BASE" --landed-commit "$J3_HEAD"
assert "J3: refresh exits 0" test "$RC" -eq 0

J3_GEN="$(readlink "$J3_BASE")"

assert "J3a: debug/deps (the prune target) is pruned to 2 — control proving the instrument fires" \
    bash -c '[ "$(find "$1/debug/deps" -maxdepth 1 -type f -name "reify_scope_unit-*" | wc -l)" -eq 2 ] && [ ! -f "$1/debug/deps/reify_scope_unit-1111111111111111" ]' _ "$J3_GEN"
assert "J3b: debug/.fingerprint is untouched — all 3 generations survive" \
    bash -c '[ "$(find "$1/debug/.fingerprint" -maxdepth 1 -type f -name "reify_scope_unit-*" | wc -l)" -eq 3 ]' _ "$J3_GEN"
assert "J3c: debug/build is untouched — all 3 generations survive" \
    bash -c '[ "$(find "$1/debug/build" -maxdepth 1 -type f -name "reify_scope_unit-*" | wc -l)" -eq 3 ]' _ "$J3_GEN"
assert "J3d: release/deps is untouched — all 3 generations survive (out of scope)" \
    bash -c '[ "$(find "$1/release/deps" -maxdepth 1 -type f -name "reify_scope_unit-*" | wc -l)" -eq 3 ]' _ "$J3_GEN"
assert "J3e: debug/deps/nested is untouched — depth-1 only" \
    bash -c '[ "$(find "$1/debug/deps/nested" -maxdepth 1 -type f -name "reify_scope_unit-*" | wc -l)" -eq 3 ]' _ "$J3_GEN"
assert "J3f: a hashed file directly in debug/ (not deps/) survives" \
    bash -c '[ "$(find "$1/debug" -maxdepth 1 -type f -name "reify_scope_unit-*" | wc -l)" -eq 3 ]' _ "$J3_GEN"

# J3g — structural: an advancing dir with NO debug/deps at all refreshes
# exit 0 and produces a correct base (the graceful-no-op path every existing
# Block B-I fixture already depends on).
J3G_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j3g-XXXXXX)"
_TMPDIRS+=("$J3G_TMP")
J3G_LANE="$(mk_git_advancing "$J3G_TMP")"
J3G_ADV="$J3G_LANE/advancing"
J3G_HEAD="$(git -C "$J3G_LANE" rev-parse HEAD)"
echo "content" > "$J3G_ADV/unrelated.txt"
J3G_BASE="$J3G_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$J3G_ADV" "$J3G_BASE" --landed-commit "$J3G_HEAD"
assert "J3g: refresh with no debug/deps at all exits 0" test "$RC" -eq 0
assert "J3g: base has advancing content (graceful no-op path)" \
    bash -c '[ "$(cat "$1/unrelated.txt")" = "content" ]' _ "$J3G_BASE"

# J3h — structural: an EMPTY debug/deps directory refreshes exit 0 and leaves
# the directory present and empty (no `rm -rf` of the dir itself; the prune
# removes files, never the container).
J3H_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j3h-XXXXXX)"
_TMPDIRS+=("$J3H_TMP")
J3H_LANE="$(mk_git_advancing "$J3H_TMP")"
J3H_ADV="$J3H_LANE/advancing"
J3H_HEAD="$(git -C "$J3H_LANE" rev-parse HEAD)"
mkdir -p "$J3H_ADV/debug/deps"
J3H_BASE="$J3H_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$J3H_ADV" "$J3H_BASE" --landed-commit "$J3H_HEAD"
assert "J3h: refresh with an empty debug/deps exits 0" test "$RC" -eq 0
J3H_GEN="$(readlink "$J3H_BASE")"
assert "J3h: debug/deps directory itself still present (not rm -rf'd)" \
    test -d "$J3H_GEN/debug/deps"
assert "J3h: debug/deps is empty (no phantom files created)" \
    bash -c '[ -z "$(find "$1" -maxdepth 1 -type f)" ]' _ "$J3H_GEN/debug/deps"

# J4a-c — the operator-facing prune summary on stderr: a machine-greppable
# line naming both the deleted-file count and the reclaimed bytes (the signal
# an operator reads to apply the E19 stop rule), stdout stays empty (B7), and
# the no-debug/deps path is non-silent about the (skipped) stage.
J4_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j4-XXXXXX)"
_TMPDIRS+=("$J4_TMP")
J4_LANE="$(mk_git_advancing "$J4_TMP")"
J4_ADV="$J4_LANE/advancing"
J4_HEAD="$(git -C "$J4_LANE" rev-parse HEAD)"
mkdir -p "$J4_ADV/debug/deps"
echo "gen1 content" > "$J4_ADV/debug/deps/reify_summary_unit-1111111111111111"
echo "gen2 content" > "$J4_ADV/debug/deps/reify_summary_unit-2222222222222222"
echo "gen3 content" > "$J4_ADV/debug/deps/reify_summary_unit-3333333333333333"
touch -d '2026-01-01 00:00:00' "$J4_ADV/debug/deps/reify_summary_unit-1111111111111111"
touch -d '2026-02-01 00:00:00' "$J4_ADV/debug/deps/reify_summary_unit-2222222222222222"
touch -d '2026-03-01 00:00:00' "$J4_ADV/debug/deps/reify_summary_unit-3333333333333333"
# Computed, not hardcoded, so the assertion tracks the fixture's actual content.
J4_VICTIM_BYTES="$(stat -c %s "$J4_ADV/debug/deps/reify_summary_unit-1111111111111111")"
J4_BASE="$J4_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$J4_ADV" "$J4_BASE" --landed-commit "$J4_HEAD"
assert "J4a: refresh exits 0" test "$RC" -eq 0
assert "J4a: stderr carries a machine-greppable prune summary (prune...deps...files=1)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "prune.*deps.*files=1"' _ "$ERR_OUT"
assert "J4a: prune summary names the reclaimed bytes (exact victim size)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "bytes=$2([^0-9]|\$)"' _ "$ERR_OUT" "$J4_VICTIM_BYTES"
assert "J4b: stdout stays empty on the refresh path (B7 contract)" \
    bash -c '[ -z "$1" ]' _ "$OUT"

J4C_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j4c-XXXXXX)"
_TMPDIRS+=("$J4C_TMP")
J4C_LANE="$(mk_git_advancing "$J4C_TMP")"
J4C_ADV="$J4C_LANE/advancing"
J4C_HEAD="$(git -C "$J4C_LANE" rev-parse HEAD)"
echo "content" > "$J4C_ADV/f.txt"
J4C_BASE="$J4C_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$J4C_ADV" "$J4C_BASE" --landed-commit "$J4C_HEAD"
assert "J4c: refresh with no debug/deps exits 0" test "$RC" -eq 0
assert "J4c: stderr is non-silent about the (skipped) prune stage" \
    bash -c 'printf "%s\n" "$1" | grep -qi "skip"' _ "$ERR_OUT"

# J4d-f — staging placement (fail-closed): refresh over a PRE-EXISTING base
# with its own (unpruned) debug/deps generations. The prune reads/writes only
# the .partial staging copy — the previous generation is never edited in
# place. A shared flock on the retired gen's lock file (the documented
# reader-refcount protocol, script Step 6) defers the GC reap so this test
# can observe gen.1's untouched content instead of racing its removal.
J4D_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j4d-XXXXXX)"
_TMPDIRS+=("$J4D_TMP")
J4D_LANE="$(mk_git_advancing "$J4D_TMP")"
J4D_ADV="$J4D_LANE/advancing"
J4D_HEAD="$(git -C "$J4D_LANE" rev-parse HEAD)"
mkdir -p "$J4D_ADV/debug/deps"
echo "new1" > "$J4D_ADV/debug/deps/reify_new_unit-7777777777777777"
echo "new2" > "$J4D_ADV/debug/deps/reify_new_unit-8888888888888888"
echo "new3" > "$J4D_ADV/debug/deps/reify_new_unit-9999999999999999"
touch -d '2026-01-01 00:00:00' "$J4D_ADV/debug/deps/reify_new_unit-7777777777777777"
touch -d '2026-02-01 00:00:00' "$J4D_ADV/debug/deps/reify_new_unit-8888888888888888"
touch -d '2026-03-01 00:00:00' "$J4D_ADV/debug/deps/reify_new_unit-9999999999999999"

J4D_BASE="$J4D_TMP/base"
mkdir -p "$J4D_BASE/debug/deps"
echo "old1" > "$J4D_BASE/debug/deps/reify_old_unit-1111111111111111"
echo "old2" > "$J4D_BASE/debug/deps/reify_old_unit-2222222222222222"
echo "old3" > "$J4D_BASE/debug/deps/reify_old_unit-3333333333333333"
touch -d '2026-01-01 00:00:00' "$J4D_BASE/debug/deps/reify_old_unit-1111111111111111"
touch -d '2026-02-01 00:00:00' "$J4D_BASE/debug/deps/reify_old_unit-2222222222222222"
touch -d '2026-03-01 00:00:00' "$J4D_BASE/debug/deps/reify_old_unit-3333333333333333"

# Bootstrap numbering (mirrors Block B8): a pre-existing REAL base dir becomes
# .gen.1 on the FIRST refresh, and the new gen becomes .gen.2.
J4D_RETIRED_GEN="${J4D_BASE}.gen.1"
J4D_RETIRED_LOCK="${J4D_RETIRED_GEN}.lock"
touch "$J4D_RETIRED_LOCK"
exec {J4D_LOCK_FD}<>"$J4D_RETIRED_LOCK"
flock -s "$J4D_LOCK_FD"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$J4D_ADV" "$J4D_BASE" --landed-commit "$J4D_HEAD"
assert "J4d: refresh over a pre-existing base exits 0" test "$RC" -eq 0
assert "J4d: retired gen.1 (the old base) still has all 3 of its OWN generations — prune never touched it" \
    bash -c '[ "$(find "$1" -maxdepth 1 -type f -name "reify_old_unit-*" | wc -l)" -eq 3 ]' _ "$J4D_RETIRED_GEN/debug/deps"

J4D_NEW_GEN="$(readlink "$J4D_BASE")"
assert "J4e: new gen carries the pruned (2-file) set of the advancing unit" \
    bash -c '[ "$(find "$1" -maxdepth 1 -type f -name "reify_new_unit-*" | wc -l)" -eq 2 ] && [ ! -f "$1/reify_new_unit-7777777777777777" ]' _ "$J4D_NEW_GEN/debug/deps"

assert "J4f: no <base>.gen.*.partial remains after refresh" \
    bash -c '_n=0; for _p in "${1}".gen.*.partial; do [ -d "$_p" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$J4D_BASE"
assert "J4f: <base> is a symlink to a <base>.gen.N dir" \
    bash -c '[ -L "$1" ] && readlink "$1" | grep -qE "[.]gen[.][0-9]+$"' _ "$J4D_BASE"

# Release the shared lock now that assertions are done (fixture cleanup no
# longer needs to race the GC this was deferring).
flock -u "$J4D_LOCK_FD"
exec {J4D_LOCK_FD}<&-

# J4g — PRUNE FAILURE LEAVES NO RESIDUE. The REIFY_TEST_PRUNE_RM_FAIL=1 rm
# stub fails only the prune stage's own non-recursive `rm -f` (never the EXIT
# trap's recursive `rm -rf` cleanup — see the stub's own comment), so the
# refusal is isolated to the prune's own unlink and the trap's cleanup is
# genuinely exercised with the real rm.
J4G_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j4g-XXXXXX)"
_TMPDIRS+=("$J4G_TMP")
J4G_LANE="$(mk_git_advancing "$J4G_TMP")"
J4G_ADV="$J4G_LANE/advancing"
J4G_HEAD="$(git -C "$J4G_LANE" rev-parse HEAD)"
mkdir -p "$J4G_ADV/debug/deps"
echo "g1" > "$J4G_ADV/debug/deps/reify_fail_unit-1111111111111111"
echo "g2" > "$J4G_ADV/debug/deps/reify_fail_unit-2222222222222222"
echo "g3" > "$J4G_ADV/debug/deps/reify_fail_unit-3333333333333333"
touch -d '2026-01-01 00:00:00' "$J4G_ADV/debug/deps/reify_fail_unit-1111111111111111"
touch -d '2026-02-01 00:00:00' "$J4G_ADV/debug/deps/reify_fail_unit-2222222222222222"
touch -d '2026-03-01 00:00:00' "$J4G_ADV/debug/deps/reify_fail_unit-3333333333333333"
J4G_BASE="$J4G_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 REIFY_TEST_PRUNE_RM_FAIL=1 run_helper "$J4G_ADV" "$J4G_BASE" --landed-commit "$J4G_HEAD"
assert "J4g: prune rm failure makes the refresh exit non-zero" test "$RC" -ne 0
assert "J4g: <base> not created/advanced after prune failure" test ! -e "$J4G_BASE"
assert "J4g: no <base>.gen.*.partial residue after prune failure (EXIT trap cleanup)" \
    bash -c '_n=0; for _p in "${1}".gen.*.partial; do [ -e "$_p" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$J4G_BASE"

# J5 — mtime-tie determinism. NOT a theoretical edge case: measured on the live
# base, 1 of 3,607 (stem, ext) groups contains duplicate mtimes. The current
# sort has no DELIBERATE secondary key — ties fall through to whichever field
# happens to sit next in the `find -printf` stream (byte size), which is an
# accident of field order, not a rule anyone chose. J5c pins the intended
# rule (mtime desc, filename desc) and is red against that accident.
J5_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j5-XXXXXX)"
_TMPDIRS+=("$J5_TMP")
J5_LANE="$(mk_git_advancing "$J5_TMP")"
J5_ADV="$J5_LANE/advancing"
J5_HEAD="$(git -C "$J5_LANE" rev-parse HEAD)"
mkdir -p "$J5_ADV/debug/deps"

# Three generations of one unit: the two OLDEST tied at an IDENTICAL mtime
# (touch -d the same timestamp on both), the newest distinct. Content sizes
# are deliberately swapped relative to filename order — the LEXICALLY
# SMALLER hash suffix (-1111...) gets the LARGER byte count, and the
# LEXICALLY LARGER suffix (-2222...) gets the SMALLER byte count — so that
# "tiebreak by byte size" and "tiebreak by filename" pick DIFFERENT
# survivors. No sleeps; explicit `touch -d` timestamps only (T8).
printf '%s' "aaaaa" > "$J5_ADV/debug/deps/reify_tie_unit-1111111111111111"                 # 5 bytes
printf '%s' "bbbbbbbbbbbbbbbbbbbb" > "$J5_ADV/debug/deps/reify_tie_unit-2222222222222222"  # 20 bytes
printf '%s' "newest content" > "$J5_ADV/debug/deps/reify_tie_unit-3333333333333333"
touch -d '2026-01-01 00:00:00' \
    "$J5_ADV/debug/deps/reify_tie_unit-1111111111111111" \
    "$J5_ADV/debug/deps/reify_tie_unit-2222222222222222"
touch -d '2026-03-01 00:00:00' "$J5_ADV/debug/deps/reify_tie_unit-3333333333333333"

J5_BASE="$J5_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$J5_ADV" "$J5_BASE" --landed-commit "$J5_HEAD"
assert "J5a: refresh with a tied-mtime group exits 0" test "$RC" -eq 0

J5_GEN="$(readlink "$J5_BASE")"
J5_DEPS="$J5_GEN/debug/deps"

assert "J5a: exactly 2 files remain in the tied group (keep count honoured despite the tie)" \
    bash -c '[ "$(find "$1" -maxdepth 1 -type f -name "reify_tie_unit-*" | wc -l)" -eq 2 ]' _ "$J5_DEPS"
assert "J5a: the distinct-newest generation survives" \
    test -f "$J5_DEPS/reify_tie_unit-3333333333333333"

# J5c — the tiebreak must be filename-based (mtime desc, filename desc), not
# an accident of byte size. -2222... is the lexically LARGER of the tied-old
# pair, so the intended rule keeps it and prunes -1111.... Today's grouper
# has no filename tiebreak and falls through to comparing byte size as the
# next `find -printf` field, under which -1111... (5 bytes, string "5") sorts
# AFTER -2222... (20 bytes, string "20") — "5" > "20" lexically — making
# -1111... the one kept and -2222... the one pruned: the opposite of the
# intended rule. RED until step-10 gives the sort a deliberate secondary key.
assert "J5c: the lexically-larger tied filename survives (-2222...)" \
    test -f "$J5_DEPS/reify_tie_unit-2222222222222222"
assert "J5c: the lexically-smaller tied filename is pruned (-1111...)" \
    test ! -f "$J5_DEPS/reify_tie_unit-1111111111111111"

# J5b — the survivor SET is deterministic across independent refreshes: a
# second, byte-identical advancing fixture (same names, same stamped mtimes,
# same content), built from an independently-created lane so this exercises
# a genuinely separate `find`/readdir traversal, must produce the same
# surviving filename set as the first.
J5B_TMP="$(mktemp -d /tmp/test-refresh-warm-base-j5b-XXXXXX)"
_TMPDIRS+=("$J5B_TMP")
J5B_LANE="$(mk_git_advancing "$J5B_TMP")"
J5B_ADV="$J5B_LANE/advancing"
J5B_HEAD="$(git -C "$J5B_LANE" rev-parse HEAD)"
mkdir -p "$J5B_ADV/debug/deps"
printf '%s' "aaaaa" > "$J5B_ADV/debug/deps/reify_tie_unit-1111111111111111"
printf '%s' "bbbbbbbbbbbbbbbbbbbb" > "$J5B_ADV/debug/deps/reify_tie_unit-2222222222222222"
printf '%s' "newest content" > "$J5B_ADV/debug/deps/reify_tie_unit-3333333333333333"
touch -d '2026-01-01 00:00:00' \
    "$J5B_ADV/debug/deps/reify_tie_unit-1111111111111111" \
    "$J5B_ADV/debug/deps/reify_tie_unit-2222222222222222"
touch -d '2026-03-01 00:00:00' "$J5B_ADV/debug/deps/reify_tie_unit-3333333333333333"

J5B_BASE="$J5B_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$J5B_ADV" "$J5B_BASE" --landed-commit "$J5B_HEAD"
assert "J5b: second independent refresh of a byte-identical fixture exits 0" test "$RC" -eq 0

J5B_GEN="$(readlink "$J5B_BASE")"
J5B_DEPS="$J5B_GEN/debug/deps"

assert "J5b: survivor filename SET is identical across two independent refreshes" \
    bash -c '
        s1="$(cd "$1" && find . -maxdepth 1 -type f -name "reify_tie_unit-*" -printf "%f\n" | sort)"
        s2="$(cd "$2" && find . -maxdepth 1 -type f -name "reify_tie_unit-*" -printf "%f\n" | sort)"
        [ "$s1" = "$s2" ]
    ' _ "$J5_DEPS" "$J5B_DEPS"

# ─────────────────────────────────────────────────────────────────────────────
# Block TRASH: shared-trash litter guard (task 5612). Two asserts, deliberately
# kept as two independently-reported signals: TRASH2 can realistically only ever
# report "clean", which is indistinguishable from a checker that stopped working
# — TRASH1 is the hermetic control proving the instrument still fires.
# Full rationale and honest scope: the CANONICAL WIRING CONTRACT comment in
# tests/infra/test_helpers.sh.
# ─────────────────────────────────────────────────────────────────────────────
assert "TRASH1: shared-trash litter detector is live (self-test fires on a synthetic bare-/tmp lane)" \
    assert_shared_trash_litter_detector_live
assert "TRASH2: no lane in this suite littered the machine-shared /tmp/.reseed-trash" \
    assert_no_shared_trash_litter

test_summary
