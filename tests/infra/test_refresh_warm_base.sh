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
#   rm       — records argv; when REIFY_TEST_PRUNE_RM_FAIL=1, fails exactly the prune
#              stage's own `rm -f --` call (the xargs batch) and execs the real rm for
#              every other call (the EXIT trap's cleanup, GC's lock/gen removal) —
#              isolates a simulated prune-unlink failure from the trap's own cleanup.
#   findmnt  — records argv; REIFY_TEST_FINDMNT_FAIL=1 simulates findmnt failing (or
#              being absent); else prints REIFY_TEST_FINDMNT_OPTIONS (default
#              rw,relatime) as the mount's option list — an EMPTY value prints an
#              empty line. Keeps the suite hermetic whatever /tmp's real mount options.
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
#   G — basecommit provenance: per-gen .basecommit stamp == --landed-commit, reaped with its gen
#   H — buildroot provenance: per-gen .buildroot stamp == realpath(advancing worktree root)
#   I — WIP refusal: advancing worktree with tracked WIP is refused; wording advises committing, never stashing
#   DEPS — debug/deps faithful copy: every hash variant in the advancing debug/deps reaches the new gen, incl. same-mtime registry variants (#8366)
#   LIVE — liveness prune: debug/deps units whose fingerprint was not consulted within 7 days of the tree's newest consult are pruned from the base gen; live variants, non-candidates and other dirs survive
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

# rm stub: record argv; when REIFY_TEST_PRUNE_RM_FAIL=1, fail ONLY the prune
# stage's own `xargs -0 rm -f -- <files>` (first args "-f" "--") while every other
# invocation — the EXIT trap's `rm -rf "$_p"` and `rm -f "$_PRUNE_SUMMARY_FILE"`,
# GC's `rm -f lock` — execs the real rm. This isolates "the prune's own unlink
# failed" from "the trap's cleanup afterward also failed": a filesystem-permission
# fault cannot make that distinction, since it defeats `rm -rf` identically.
# Real rm path embedded at stub-creation time (mirrors the cp stub).
_REAL_RM="$(command -v rm)"
cat > "$STUB_DIR/rm" << STUB_EOF
#!/usr/bin/env bash
echo "rm \$*" >> "\${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "\${REIFY_TEST_PRUNE_RM_FAIL:-}" = "1" ] && [ "\${1:-}" = "-f" ] && [ "\${2:-}" = "--" ]; then
    echo "rm: SIMULATED failure (REIFY_TEST_PRUNE_RM_FAIL=1)" >&2
    exit 1
fi
exec "${_REAL_RM}" "\$@"
STUB_EOF
chmod +x "$STUB_DIR/rm"

# findmnt stub: record argv; REIFY_TEST_FINDMNT_FAIL=1 simulates findmnt failing
# (including not being installed); otherwise print REIFY_TEST_FINDMNT_OPTIONS as the
# mount's one-line option list. `${VAR-default}` (no colon), so an empty
# REIFY_TEST_FINDMNT_OPTIONS prints an EMPTY line rather than the default. Stubbing
# it keeps the suite hermetic regardless of the real mount options of /tmp.
cat > "$STUB_DIR/findmnt" << 'STUB_EOF'
#!/usr/bin/env bash
echo "findmnt $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_FINDMNT_FAIL:-}" = "1" ]; then
    echo "findmnt: SIMULATED failure (REIFY_TEST_FINDMNT_FAIL=1)" >&2
    exit 1
fi
echo "${REIFY_TEST_FINDMNT_OPTIONS-rw,relatime}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/findmnt"

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
# Block DEPS — debug/deps faithful copy (task 8366)
#
# The new gen's debug/deps must hold every artefact the advancing target holds.
# One stem can carry several concurrently-live hashes (semver / feature / host
# variants) that all keep the same cold-build mtime, so no mtime rule can tell
# them from superseded generations; #7426's keep-newest-2 prune deleted them and
# every seeded lane rebuilt them. This fixture carries no debug/.fingerprint, so
# Block LIVE's prune has no liveness evidence here and must copy debug/deps
# faithfully: DEPS2 and DEPS3 stay green.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block DEPS: debug/deps faithful copy ---"

DEPS_TMP="$(mktemp -d /tmp/test-refresh-warm-base-deps-XXXXXX)"
_TMPDIRS+=("$DEPS_TMP")
DEPS_LANE="$(mk_git_advancing "$DEPS_TMP")"
DEPS_ADV="$DEPS_LANE/advancing"
DEPS_HEAD="$(git -C "$DEPS_LANE" rev-parse HEAD)"
mkdir -p "$DEPS_ADV/debug/deps"

# Five concurrently-live hashes of ONE registry stem, each with .rlib and
# .rmeta, all stamped with the SAME mtime (a cold build writes them together).
# Every hash is 16 lowercase hex chars so a prune keyed on cargo's `-<16hex>`
# suffix grammar genuinely matches them — the RED must never be vacuous.
DEPS_REGISTRY_HASHES=(ece35e16166057f6 985ac887cd451fe2 0123456789abcdef fedcba9876543210 5a5a5a5a5a5a5a5a)
for h in "${DEPS_REGISTRY_HASHES[@]}"; do
    for ext in rlib rmeta; do
        echo "getrandom $h $ext" > "$DEPS_ADV/debug/deps/libgetrandom-$h.$ext"
    done
done
touch -d '2026-08-18 00:00:00' "$DEPS_ADV"/debug/deps/libgetrandom-*

# Three extensionless test-binary generations at distinct mtimes, stamped
# explicitly via `touch -d` — no sleeps (T8).
echo "gen1 content" > "$DEPS_ADV/debug/deps/reify_kernel_tests-1111111111111111"
echo "gen2 content" > "$DEPS_ADV/debug/deps/reify_kernel_tests-2222222222222222"
echo "gen3 content" > "$DEPS_ADV/debug/deps/reify_kernel_tests-3333333333333333"
touch -d '2026-01-01 00:00:00' "$DEPS_ADV/debug/deps/reify_kernel_tests-1111111111111111"
touch -d '2026-02-01 00:00:00' "$DEPS_ADV/debug/deps/reify_kernel_tests-2222222222222222"
touch -d '2026-03-01 00:00:00' "$DEPS_ADV/debug/deps/reify_kernel_tests-3333333333333333"

DEPS_BASE="$DEPS_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$DEPS_ADV" "$DEPS_BASE" --landed-commit "$DEPS_HEAD"
assert "DEPS1: refresh with same-mtime hash variants exits 0" test "$RC" -eq 0

DEPS_GEN_DEPS="$(readlink "$DEPS_BASE")/debug/deps"

assert "DEPS2: every same-mtime libgetrandom hash variant reaches the new gen, on both .rlib and .rmeta" \
    bash -c '
        deps="$1"; shift
        for h in "$@"; do
            for ext in rlib rmeta; do
                [ -f "$deps/libgetrandom-$h.$ext" ] || { echo "missing: libgetrandom-$h.$ext"; exit 1; }
            done
        done
    ' _ "$DEPS_GEN_DEPS" "${DEPS_REGISTRY_HASHES[@]}"

assert "DEPS3: gen debug/deps filename set equals the advancing debug/deps filename set" \
    bash -c '
        s1="$(cd "$1" && find . -maxdepth 1 -type f -printf "%f\n" | sort)"
        s2="$(cd "$2" && find . -maxdepth 1 -type f -printf "%f\n" | sort)"
        [ -n "$s1" ] || { echo "advancing debug/deps fixture is empty"; exit 1; }
        [ "$s1" = "$s2" ] || { diff <(printf "%s\n" "$s1") <(printf "%s\n" "$s2"); exit 1; }
    ' _ "$DEPS_ADV/debug/deps" "$DEPS_GEN_DEPS"

# ──────────────────────────────────────────────────────────────────────────────
# Block LIVE — fingerprint-consult liveness prune of debug/deps (task 8352)
#
# Executable spec for scripts/refresh-warm-base.sh Step 3b. A depth-1 debug/deps
# file is a candidate iff its name, final dot-suffix stripped, ends in
# `-<16 lowercase hex>`; it is pruned iff that hash has a debug/.fingerprint
# dir AND no file of that dir was read or written (max of atime and mtime)
# within 7 days of the copied tree's newest consult, the ANCHOR. Cargo reads a
# unit's fingerprint on every build whose graph contains the unit, so a hash
# the lanes still build is consulted continuously whatever its artefact mtime.
#
# Fixtures use fixed past dates: no sleeps, and the window being anchored on
# the tree rather than on `date` is itself under test (LIVE1).
#
# Two guards keep the evidence honest: the stage is skipped when the advancing
# dir's mount does not maintain atime (LIVE13-17), and a future-dated consult
# never moves the anchor (LIVE18).
#
# FIXTURE RULE: a case with a dead unit also carries a unit consulted at the
# ANCHOR. Without one, the dead unit IS the newest consult, becomes the anchor
# and is correctly kept, so a 'gone' assertion would fail for the wrong reason.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block LIVE: fingerprint-consult liveness prune of debug/deps ---"

LIVE_ANCHOR='2026-03-10 00:00:00'   # the copied tree's newest consult
LIVE_RECENT='2026-03-08 00:00:00'   # 2 days before the anchor: inside the 7-day window
LIVE_DEAD='2026-02-01 00:00:00'     # 37 days before the anchor: outside it
LIVE_COLD='2026-01-01 00:00:00'     # cold-build mtime of artefacts nobody rebuilt

# _mint_unit <profile_dir> <pkg> <hash> <fp_mtime> <fp_atime> [<deps_name>...]
# One cargo unit: <profile_dir>/.fingerprint/<pkg>-<hash>/lib-<pkg>, stamped with
# the given mtime and atime (-m before -a, so the mtime stamp cannot clobber the
# atime), plus each named file under <profile_dir>/deps stamped with fp_mtime.
_mint_unit() {
    local profile="$1" pkg="$2" hash="$3" fp_mtime="$4" fp_atime="$5" fp name
    shift 5
    fp="$profile/.fingerprint/$pkg-$hash/lib-$pkg"
    mkdir -p "$(dirname "$fp")" "$profile/deps"
    echo "fingerprint $pkg-$hash" > "$fp"
    touch -m -d "$fp_mtime" "$fp"
    touch -a -d "$fp_atime" "$fp"
    for name in "$@"; do
        echo "deps $name" > "$profile/deps/$name"
        touch -d "$fp_mtime" "$profile/deps/$name"
    done
}

# _mint_anchor <profile_dir>: the unit consulted at the ANCHOR (FIXTURE RULE).
_mint_anchor() {
    _mint_unit "$1" anchor 00000000000000a1 "$LIVE_ANCHOR" "$LIVE_ANCHOR" libanchor-00000000000000a1.rlib
}

# _live_case: a fresh hermetic advancing lane for one case. Sets LIVE_ADV (the
# advancing target dir), LIVE_PROFILE (its debug/, created by the first
# _mint_unit so a case may leave it absent), LIVE_HEAD and LIVE_BASE.
_live_case() {
    local tmp lane
    tmp="$(mktemp -d /tmp/test-refresh-warm-base-live-XXXXXX)"
    _TMPDIRS+=("$tmp")
    lane="$(mk_git_advancing "$tmp")"
    LIVE_ADV="$lane/advancing"
    LIVE_PROFILE="$LIVE_ADV/debug"
    LIVE_HEAD="$(git -C "$lane" rev-parse HEAD)"
    LIVE_BASE="$tmp/base"
}

# _live_refresh: run the real script over the current case (env prefixes pass
# through). Sets LIVE_GEN to the gen dir the base now points at — empty when the
# refresh did not advance the base.
_live_refresh() {
    reset_calls
    REIFY_TEST_REFLINK_OK=1 run_helper "$LIVE_ADV" "$LIVE_BASE" --landed-commit "$LIVE_HEAD"
    LIVE_GEN="$(readlink "$LIVE_BASE" || true)"
}

# _all_present / _none_present <dir> <relpath>...: name each offender and fail if
# any. Both refuse an absent <dir>, so an empty LIVE_GEN can never make a 'gone'
# check pass vacuously.
_all_present() {
    local dir="$1" rel rc=0
    shift
    [ -d "$dir" ] || { echo "no such dir: '$dir'"; return 1; }
    for rel in "$@"; do
        [ -f "$dir/$rel" ] || { echo "missing: $rel"; rc=1; }
    done
    return "$rc"
}
_none_present() {
    local dir="$1" rel rc=0
    shift
    [ -d "$dir" ] || { echo "no such dir: '$dir'"; return 1; }
    for rel in "$@"; do
        [ ! -e "$dir/$rel" ] || { echo "still present: $rel"; rc=1; }
    done
    return "$rc"
}

# _stderr_matches <ere>: the last run_helper's stderr matches, case-insensitively.
_stderr_matches() {
    grep -qiE -- "$1" <<<"$ERR_OUT"
}

# LIVE1 — E19 regression, the core. Nine hashes of ONE registry stem (five with
# .rlib+.rmeta, four .rmeta-only), every artefact and fingerprint mtime the
# cold-build date, every fingerprint consulted at the ANCHOR by lanes that still
# build them: all fourteen files must survive. The dead control proves the stage
# is active, so the survivors did not survive by the stage being skipped.
_live_case
LIVE1_FULL_HASHES=(ece35e16166057f6 985ac887cd451fe2 0123456789abcdef fedcba9876543210 5a5a5a5a5a5a5a5a)
LIVE1_META_HASHES=(a1a1a1a1a1a1a1a1 b2b2b2b2b2b2b2b2 c3c3c3c3c3c3c3c3 d4d4d4d4d4d4d4d4)
LIVE1_FILES=()
for h in "${LIVE1_FULL_HASHES[@]}"; do
    _mint_unit "$LIVE_PROFILE" getrandom "$h" "$LIVE_COLD" "$LIVE_ANCHOR" \
        "libgetrandom-$h.rlib" "libgetrandom-$h.rmeta"
    LIVE1_FILES+=("libgetrandom-$h.rlib" "libgetrandom-$h.rmeta")
done
for h in "${LIVE1_META_HASHES[@]}"; do
    _mint_unit "$LIVE_PROFILE" getrandom "$h" "$LIVE_COLD" "$LIVE_ANCHOR" "libgetrandom-$h.rmeta"
    LIVE1_FILES+=("libgetrandom-$h.rmeta")
done
_mint_unit "$LIVE_PROFILE" reify_old_tests deadbeef00000001 "$LIVE_DEAD" "$LIVE_DEAD" \
    reify_old_tests-deadbeef00000001
_live_refresh
LIVE1_DEPS="$LIVE_GEN/debug/deps"
assert "LIVE1: refresh exits 0" test "$RC" -eq 0
assert "LIVE1: all 14 same-mtime libgetrandom files survive (E19: every variant is still consulted)" \
    _all_present "$LIVE1_DEPS" "${LIVE1_FILES[@]}"
assert "LIVE1: the dead control reify_old_tests-deadbeef00000001 is pruned (the stage is active)" \
    _none_present "$LIVE1_DEPS" reify_old_tests-deadbeef00000001
assert "LIVE1: fixture dates are months before wall-clock now (precondition for the next assert)" \
    test "$(( $(date +%s) - $(date -d "$LIVE_ANCHOR" +%s) ))" -gt "$(( 7 * 24 * 3600 ))"
assert "LIVE1: window is anchored on the tree's newest consult, not wall clock — libgetrandom survives although every date is months before today" \
    _all_present "$LIVE1_DEPS" "${LIVE1_FILES[@]}"

# LIVE2 — stem collision (reify_ast). One stem, three hashes with unlike shapes
# (extensionless + .d, and two .d-only), all consulted: all four files survive.
_live_case
_mint_unit "$LIVE_PROFILE" reify_ast 1a2b3c4d5e6f7a8b "$LIVE_COLD" "$LIVE_ANCHOR" \
    reify_ast-1a2b3c4d5e6f7a8b reify_ast-1a2b3c4d5e6f7a8b.d
_mint_unit "$LIVE_PROFILE" reify_ast 2b3c4d5e6f7a8b9c "$LIVE_RECENT" "$LIVE_RECENT" \
    reify_ast-2b3c4d5e6f7a8b9c.d
_mint_unit "$LIVE_PROFILE" reify_ast 3c4d5e6f7a8b9cad "$LIVE_RECENT" "$LIVE_RECENT" \
    reify_ast-3c4d5e6f7a8b9cad.d
_mint_unit "$LIVE_PROFILE" reify_old_tests deadbeef00000002 "$LIVE_DEAD" "$LIVE_DEAD" \
    reify_old_tests-deadbeef00000002
_live_refresh
assert "LIVE2: refresh exits 0" test "$RC" -eq 0
assert "LIVE2: all four reify_ast files survive (stem collision is not a liveness signal)" \
    _all_present "$LIVE_GEN/debug/deps" reify_ast-1a2b3c4d5e6f7a8b reify_ast-1a2b3c4d5e6f7a8b.d \
        reify_ast-2b3c4d5e6f7a8b9c.d reify_ast-3c4d5e6f7a8b9cad.d
assert "LIVE2: the dead control is pruned" \
    _none_present "$LIVE_GEN/debug/deps" reify_old_tests-deadbeef00000002

# LIVE3 — superseded generation. One stem, one dead hash and two live hashes,
# each extensionless + .d: the dead hash goes on BOTH extensions, the live stay.
_live_case
_mint_unit "$LIVE_PROFILE" reify_kernel_tests 1111aaaa1111aaaa "$LIVE_DEAD" "$LIVE_DEAD" \
    reify_kernel_tests-1111aaaa1111aaaa reify_kernel_tests-1111aaaa1111aaaa.d
_mint_unit "$LIVE_PROFILE" reify_kernel_tests 2222bbbb2222bbbb "$LIVE_COLD" "$LIVE_ANCHOR" \
    reify_kernel_tests-2222bbbb2222bbbb reify_kernel_tests-2222bbbb2222bbbb.d
_mint_unit "$LIVE_PROFILE" reify_kernel_tests 3333cccc3333cccc "$LIVE_RECENT" "$LIVE_RECENT" \
    reify_kernel_tests-3333cccc3333cccc reify_kernel_tests-3333cccc3333cccc.d
_live_refresh
assert "LIVE3: refresh exits 0" test "$RC" -eq 0
assert "LIVE3: the superseded hash is pruned on both its extensionless and .d files" \
    _none_present "$LIVE_GEN/debug/deps" reify_kernel_tests-1111aaaa1111aaaa reify_kernel_tests-1111aaaa1111aaaa.d
assert "LIVE3: both live hashes keep both their files" \
    _all_present "$LIVE_GEN/debug/deps" reify_kernel_tests-2222bbbb2222bbbb reify_kernel_tests-2222bbbb2222bbbb.d \
        reify_kernel_tests-3333cccc3333cccc reify_kernel_tests-3333cccc3333cccc.d

# LIVE4 — the window. 6 days before the anchor is inside it, 8 days is outside.
_live_case
_mint_anchor "$LIVE_PROFILE"
_mint_unit "$LIVE_PROFILE" inwindow 4a4a4a4a4a4a4a4a '2026-03-04 00:00:00' '2026-03-04 00:00:00' \
    libinwindow-4a4a4a4a4a4a4a4a.rlib
_mint_unit "$LIVE_PROFILE" outwindow 4b4b4b4b4b4b4b4b '2026-03-02 00:00:00' '2026-03-02 00:00:00' \
    liboutwindow-4b4b4b4b4b4b4b4b.rlib
_live_refresh
assert "LIVE4: refresh exits 0" test "$RC" -eq 0
assert "LIVE4: a unit last used 6 days before the anchor survives" \
    _all_present "$LIVE_GEN/debug/deps" libinwindow-4a4a4a4a4a4a4a4a.rlib
assert "LIVE4: a unit last used 8 days before the anchor is pruned" \
    _none_present "$LIVE_GEN/debug/deps" liboutwindow-4b4b4b4b4b4b4b4b.rlib

# LIVE5 — a write counts as a use. A fingerprint last READ long ago but last
# WRITTEN (rebuilt) inside the window is live.
_live_case
_mint_anchor "$LIVE_PROFILE"
_mint_unit "$LIVE_PROFILE" rebuilt 5c5c5c5c5c5c5c5c "$LIVE_RECENT" "$LIVE_DEAD" \
    librebuilt-5c5c5c5c5c5c5c5c.rlib
_mint_unit "$LIVE_PROFILE" reify_old_tests deadbeef00000005 "$LIVE_DEAD" "$LIVE_DEAD" \
    reify_old_tests-deadbeef00000005
_live_refresh
assert "LIVE5: refresh exits 0" test "$RC" -eq 0
assert "LIVE5: a unit with an old atime but a recent mtime survives" \
    _all_present "$LIVE_GEN/debug/deps" librebuilt-5c5c5c5c5c5c5c5c.rlib
assert "LIVE5: the dead control is pruned" \
    _none_present "$LIVE_GEN/debug/deps" reify_old_tests-deadbeef00000005

# LIVE6 — evidence required. A hashed deps file whose hash has NO fingerprint
# dir is never a victim, however old.
_live_case
_mint_anchor "$LIVE_PROFILE"
echo "no fingerprint" > "$LIVE_PROFILE/deps/libnofp-6a6a6a6a6a6a6a6a.rlib"
touch -d '2020-01-01 00:00:00' "$LIVE_PROFILE/deps/libnofp-6a6a6a6a6a6a6a6a.rlib"
_mint_unit "$LIVE_PROFILE" reify_old_tests deadbeef00000006 "$LIVE_DEAD" "$LIVE_DEAD" \
    reify_old_tests-deadbeef00000006
_live_refresh
assert "LIVE6: refresh exits 0" test "$RC" -eq 0
assert "LIVE6: a hashed file with no fingerprint dir survives" \
    _all_present "$LIVE_GEN/debug/deps" libnofp-6a6a6a6a6a6a6a6a.rlib
assert "LIVE6: the dead control is pruned" \
    _none_present "$LIVE_GEN/debug/deps" reify_old_tests-deadbeef00000006

# LIVE7 — grammar: names that are not `<stem>-<16 lowercase hex>[.<ext>]` are
# never candidates. Each near-miss that carries a hash-like token is tied to a
# DEAD fingerprint dir a looser grammar would key on, so its survival is
# discriminating rather than vacuous; the dead unit's own conforming file is
# the control.
_live_case
_mint_anchor "$LIVE_PROFILE"
LIVE7_DEAD=deadbeef00000007
_mint_unit "$LIVE_PROFILE" dead "$LIVE7_DEAD" "$LIVE_DEAD" "$LIVE_DEAD" "libdead-$LIVE7_DEAD.rlib"
echo "readme" > "$LIVE_PROFILE/deps/README"
echo "{}" > "$LIVE_PROFILE/deps/build-plan.json"
_mint_unit "$LIVE_PROFILE" foo abc123 "$LIVE_DEAD" "$LIVE_DEAD" foo-abc123.rlib
_mint_unit "$LIVE_PROFILE" foo AAAABBBBCCCCDDDD "$LIVE_DEAD" "$LIVE_DEAD" foo-AAAABBBBCCCCDDDD
_mint_unit "$LIVE_PROFILE" foo 0123456789abcdef "$LIVE_DEAD" "$LIVE_DEAD" foo-0123456789abcdef0.rlib
LIVE7_SHARD="axum-$LIVE7_DEAD.axum.7a7a7a7a7a7a7a7a-cgu.09.rcgu.dwo"
echo "shard" > "$LIVE_PROFILE/deps/$LIVE7_SHARD"
touch -d "$LIVE_DEAD" "$LIVE_PROFILE/deps/$LIVE7_SHARD"
_live_refresh
assert "LIVE7: refresh exits 0" test "$RC" -eq 0
assert "LIVE7: README, a .json, a short hash, an uppercase hash, a 17-hex hash and a split-debuginfo shard all survive" \
    _all_present "$LIVE_GEN/debug/deps" README build-plan.json foo-abc123.rlib foo-AAAABBBBCCCCDDDD \
        foo-0123456789abcdef0.rlib "$LIVE7_SHARD"
assert "LIVE7: the dead unit's own conforming file is pruned" \
    _none_present "$LIVE_GEN/debug/deps" "libdead-$LIVE7_DEAD.rlib"

# LIVE8 — scope: only debug/deps at depth 1 is ever pruned. A dead hash's files
# elsewhere — build/, the profile root, a nested deps dir, the whole release
# profile, and the fingerprint dir (the evidence itself) — are untouched.
_live_case
_mint_anchor "$LIVE_PROFILE"
LIVE8_DEAD=deadbeef00000008
_mint_unit "$LIVE_PROFILE" dead "$LIVE8_DEAD" "$LIVE_DEAD" "$LIVE_DEAD" "libdead-$LIVE8_DEAD.rlib"
mkdir -p "$LIVE_PROFILE/build/dead-$LIVE8_DEAD" "$LIVE_PROFILE/deps/nested"
echo "out" > "$LIVE_PROFILE/build/dead-$LIVE8_DEAD/out.txt"
echo "root" > "$LIVE_PROFILE/dead-$LIVE8_DEAD"
echo "nested" > "$LIVE_PROFILE/deps/nested/libdead-$LIVE8_DEAD.rlib"
_mint_anchor "$LIVE_ADV/release"
_mint_unit "$LIVE_ADV/release" dead "$LIVE8_DEAD" "$LIVE_DEAD" "$LIVE_DEAD" "libdead-$LIVE8_DEAD.rlib"
_live_refresh
assert "LIVE8: refresh exits 0" test "$RC" -eq 0
assert "LIVE8: the dead hash's files outside debug/deps depth 1 all survive (build/, profile root, nested, release/, .fingerprint)" \
    _all_present "$LIVE_GEN" "debug/build/dead-$LIVE8_DEAD/out.txt" "debug/dead-$LIVE8_DEAD" \
        "debug/deps/nested/libdead-$LIVE8_DEAD.rlib" "release/deps/libdead-$LIVE8_DEAD.rlib" \
        "release/.fingerprint/dead-$LIVE8_DEAD/lib-dead" "debug/.fingerprint/dead-$LIVE8_DEAD/lib-dead"
assert "LIVE8: the dead hash's debug/deps/ file is pruned (control)" \
    _none_present "$LIVE_GEN" "debug/deps/libdead-$LIVE8_DEAD.rlib"
assert "LIVE8: only the depth-1 file is counted as a victim (files=1) — the nested twin is never even a candidate" \
    _stderr_matches 'prune deps=.*files=1( |$)'

# LIVE9 — operator contract: stderr carries the machine-greppable summary
# `prune deps=<path> files=N victim_bytes=N`, the byte figure being the summed
# apparent size of exactly the victims; stdout stays empty.
_live_case
_mint_anchor "$LIVE_PROFILE"
_mint_unit "$LIVE_PROFILE" deadone 9a9a9a9a9a9a9a9a "$LIVE_DEAD" "$LIVE_DEAD" libdeadone-9a9a9a9a9a9a9a9a.rlib
_mint_unit "$LIVE_PROFILE" deadtwo 9b9b9b9b9b9b9b9b "$LIVE_DEAD" "$LIVE_DEAD" deadtwo-9b9b9b9b9b9b9b9b
head -c 1500 /dev/zero > "$LIVE_PROFILE/deps/libdeadone-9a9a9a9a9a9a9a9a.rlib"
head -c 700 /dev/zero > "$LIVE_PROFILE/deps/deadtwo-9b9b9b9b9b9b9b9b"
LIVE9_BYTES=$(( $(stat -c %s "$LIVE_PROFILE/deps/libdeadone-9a9a9a9a9a9a9a9a.rlib") \
    + $(stat -c %s "$LIVE_PROFILE/deps/deadtwo-9b9b9b9b9b9b9b9b") ))
LIVE9_TMPDIR="$(mktemp -d /tmp/test-refresh-warm-base-live9-tmp-XXXXXX)"
_TMPDIRS+=("$LIVE9_TMPDIR")
TMPDIR="$LIVE9_TMPDIR" _live_refresh
assert "LIVE9: refresh exits 0" test "$RC" -eq 0
assert "LIVE9: the stage's summary temp file is removed after a successful prune" \
    test -z "$(ls -A "$LIVE9_TMPDIR")"
assert "LIVE9: stderr reports the prune with files=2" \
    _stderr_matches 'prune deps=.*files=2( |$)'
assert "LIVE9: stderr's victim_bytes is the summed apparent size of exactly the two dead files" \
    _stderr_matches "files=2 victim_bytes=${LIVE9_BYTES}( |\$)"
assert "LIVE9: stdout stays empty" test -z "$OUT"

# LIVE10 — skip paths: with no deps dir, or no .fingerprint to read liveness
# from, the stage deletes nothing, says so on stderr, and the refresh still
# succeeds.
_live_case
echo "content" > "$LIVE_ADV/f.txt"
_live_refresh
assert "LIVE10a: refresh with no debug/ at all exits 0" test "$RC" -eq 0
assert "LIVE10a: stderr says the stage was skipped" _stderr_matches 'skip'
assert "LIVE10a: the advancing content reaches the new gen" _all_present "$LIVE_GEN" f.txt
assert "LIVE10a: with nothing to prune findmnt is never consulted" \
    bash -c '! grep -q "^findmnt" "$1"' _ "$CALLS_FILE"

_live_case
mkdir -p "$LIVE_PROFILE/deps"
echo "dead-looking" > "$LIVE_PROFILE/deps/libdead-abababababababab.rlib"
touch -d '2020-01-01 00:00:00' "$LIVE_PROFILE/deps/libdead-abababababababab.rlib"
_live_refresh
assert "LIVE10b: refresh with debug/deps but no debug/.fingerprint exits 0" test "$RC" -eq 0
assert "LIVE10b: stderr says the stage was skipped" _stderr_matches 'skip'
assert "LIVE10b: with no liveness evidence the dead-looking file survives" \
    _all_present "$LIVE_GEN/debug/deps" libdead-abababababababab.rlib
assert "LIVE10b: with no evidence to read findmnt is never consulted" \
    bash -c '! grep -q "^findmnt" "$1"' _ "$CALLS_FILE"

# LIVE11 — fail-closed: a prune whose unlink fails aborts the refresh BEFORE the
# rename, so the base is not advanced and the EXIT trap leaves no staging dir.
# The rm stub fails only the prune's own `rm -f --`.
_live_case
_mint_anchor "$LIVE_PROFILE"
_mint_unit "$LIVE_PROFILE" failunit deadbeef00000011 "$LIVE_DEAD" "$LIVE_DEAD" libfailunit-deadbeef00000011.rlib
LIVE11_TMPDIR="$(mktemp -d /tmp/test-refresh-warm-base-live11-tmp-XXXXXX)"
_TMPDIRS+=("$LIVE11_TMPDIR")
TMPDIR="$LIVE11_TMPDIR" REIFY_TEST_PRUNE_RM_FAIL=1 _live_refresh
assert "LIVE11: a failing prune unlink makes the refresh exit non-zero" test "$RC" -ne 0
assert "LIVE11: the failure is the prune's own rm (the stub fired)" _stderr_matches 'SIMULATED failure'
assert "LIVE11: <base> is not created when the prune fails" test ! -e "$LIVE_BASE"
assert "LIVE11: the stage's summary temp file is reclaimed by the EXIT trap even though the prune failed" \
    test -z "$(ls -A "$LIVE11_TMPDIR")"
assert "LIVE11: no <base>.gen.*.partial residue after a prune failure (EXIT trap cleanup)" \
    bash -c '_n=0; for _p in "${1}".gen.*.partial; do [ -e "$_p" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$LIVE_BASE"

# LIVE12 — staging siting: refreshing over a PRE-EXISTING base prunes only the
# .partial staging copy. The old base (bootstrapped to .gen.1) is never edited;
# a shared flock on its lock file defers the GC reap (script Step 6) so the
# retired gen can still be inspected.
_live_case
_mint_anchor "$LIVE_PROFILE"
_mint_unit "$LIVE_PROFILE" advdead deadbeef00000012 "$LIVE_DEAD" "$LIVE_DEAD" libadvdead-deadbeef00000012.rlib
_mint_unit "$LIVE_BASE/debug" baseanchor bbbb0000000000a1 "$LIVE_ANCHOR" "$LIVE_ANCHOR" libbaseanchor-bbbb0000000000a1.rlib
_mint_unit "$LIVE_BASE/debug" basedead bbbb00000000dead "$LIVE_DEAD" "$LIVE_DEAD" libbasedead-bbbb00000000dead.rlib
LIVE12_RETIRED_GEN="${LIVE_BASE}.gen.1"
touch "${LIVE12_RETIRED_GEN}.lock"
exec {LIVE12_LOCK_FD}<>"${LIVE12_RETIRED_GEN}.lock"
flock -s "$LIVE12_LOCK_FD"
_live_refresh
assert "LIVE12: refresh over a pre-existing base exits 0" test "$RC" -eq 0
assert "LIVE12: retired gen.1 (the old base) still holds its own dead unit — the prune never edits it" \
    _all_present "$LIVE12_RETIRED_GEN/debug/deps" libbasedead-bbbb00000000dead.rlib
assert "LIVE12: the new gen lacks the advancing dead unit" \
    _none_present "$LIVE_GEN/debug/deps" libadvdead-deadbeef00000012.rlib
assert "LIVE12: the new gen keeps the advancing anchor unit" \
    _all_present "$LIVE_GEN/debug/deps" libanchor-00000000000000a1.rlib
assert "LIVE12: no <base>.gen.*.partial remains after refresh" \
    bash -c '_n=0; for _p in "${1}".gen.*.partial; do [ -d "$_p" ] && _n=$((_n+1)); done; [ "$_n" -eq 0 ]' _ "$LIVE_BASE"
assert "LIVE12: <base> is a symlink to a <base>.gen.N dir" \
    bash -c '[ -L "$1" ] && readlink "$1" | grep -qE "[.]gen[.][0-9]+$"' _ "$LIVE_BASE"
flock -u "$LIVE12_LOCK_FD"
exec {LIVE12_LOCK_FD}<&-

# LIVE13-17 — the atime guard. Under noatime, last_use degrades to mtime alone and
# every unchanged registry unit would read as weeks stale, so the stage must skip
# rather than prune. One fixture shape (an anchor unit plus one dead unit) runs
# under each mount-option answer; LIVE13 is the control proving the dead unit IS
# pruned where atime is maintained, so LIVE14-16 cannot pass vacuously.
LIVE_MOUNT_DEAD=libmountdead-deadbeef00000013.rlib
_live_mount_case() {
    _live_case
    _mint_anchor "$LIVE_PROFILE"
    _mint_unit "$LIVE_PROFILE" mountdead deadbeef00000013 "$LIVE_DEAD" "$LIVE_DEAD" "$LIVE_MOUNT_DEAD"
}

_live_mount_case
_live_refresh
assert "LIVE13: refresh on a relatime mount exits 0" test "$RC" -eq 0
assert "LIVE13: control — where atime is maintained the dead unit is pruned" \
    _none_present "$LIVE_GEN/debug/deps" "$LIVE_MOUNT_DEAD"
assert "LIVE13: control — the anchor unit survives" \
    _all_present "$LIVE_GEN/debug/deps" libanchor-00000000000000a1.rlib
assert "LIVE13: the mount asked about is the advancing dir's (findmnt -T <advancing dir>)" \
    grep -qE -- "^findmnt( .*)? -T ${LIVE_ADV}( |\$)" "$CALLS_FILE"

_live_mount_case
REIFY_TEST_FINDMNT_OPTIONS='rw,noatime' _live_refresh
assert "LIVE14: refresh on a noatime mount exits 0" test "$RC" -eq 0
assert "LIVE14: on a noatime mount the dead unit survives (no atime, so no liveness evidence)" \
    _all_present "$LIVE_GEN/debug/deps" "$LIVE_MOUNT_DEAD"
assert "LIVE14: stderr says the stage was skipped" _stderr_matches 'skip'

_live_mount_case
REIFY_TEST_FINDMNT_OPTIONS='noatime,rw' _live_refresh
assert "LIVE14b: noatime as the FIRST option token is recognised too (dead unit survives)" \
    _all_present "$LIVE_GEN/debug/deps" "$LIVE_MOUNT_DEAD"

_live_mount_case
REIFY_TEST_FINDMNT_OPTIONS='noatime' _live_refresh
assert "LIVE14c: noatime as the ONLY option token is recognised too (dead unit survives)" \
    _all_present "$LIVE_GEN/debug/deps" "$LIVE_MOUNT_DEAD"

_live_mount_case
REIFY_TEST_FINDMNT_FAIL=1 _live_refresh
assert "LIVE15: refresh with findmnt failing exits 0" test "$RC" -eq 0
assert "LIVE15: with findmnt failing the dead unit survives (unknown mount, no evidence)" \
    _all_present "$LIVE_GEN/debug/deps" "$LIVE_MOUNT_DEAD"
assert "LIVE15: stderr says the stage was skipped" _stderr_matches 'skip'

_live_mount_case
REIFY_TEST_FINDMNT_OPTIONS='' _live_refresh
assert "LIVE16: refresh with findmnt printing nothing exits 0" test "$RC" -eq 0
assert "LIVE16: with an empty option list the dead unit survives (unknown mount, no evidence)" \
    _all_present "$LIVE_GEN/debug/deps" "$LIVE_MOUNT_DEAD"
assert "LIVE16: stderr says the stage was skipped" _stderr_matches 'skip'

_live_mount_case
REIFY_TEST_FINDMNT_OPTIONS='rw,relatime,nodiratime' _live_refresh
assert "LIVE17: refresh on a nodiratime mount exits 0" test "$RC" -eq 0
assert "LIVE17: nodiratime is not noatime — the dead unit is still pruned" \
    _none_present "$LIVE_GEN/debug/deps" "$LIVE_MOUNT_DEAD"

# LIVE18 — a future-dated consult (clock skew, a stray touch, an extracted archive)
# must not move the anchor, or every live unit would age out against it: a mass
# deletion of exactly the E19 class from one outlier. The future unit still counts
# as ITS OWN unit's use, so it survives.
_live_case
_mint_anchor "$LIVE_PROFILE"
_mint_unit "$LIVE_PROFILE" liveunit 1818181818181801 '2026-03-06 00:00:00' '2026-03-06 00:00:00' \
    libliveunit-1818181818181801.rlib
_mint_unit "$LIVE_PROFILE" deadunit deadbeef00000018 "$LIVE_DEAD" "$LIVE_DEAD" \
    libdeadunit-deadbeef00000018.rlib
_mint_unit "$LIVE_PROFILE" futureunit 1818181818181802 "$LIVE_COLD" '2099-01-01 00:00:00' \
    libfutureunit-1818181818181802.rlib
# A second, long-stale fingerprint file in the same dir: ignoring the future-dated
# one (rather than counting it for its own unit) would leave the unit looking dead.
echo "stale sibling" > "$LIVE_PROFILE/.fingerprint/futureunit-1818181818181802/dep-lib-futureunit"
touch -d "$LIVE_DEAD" "$LIVE_PROFILE/.fingerprint/futureunit-1818181818181802/dep-lib-futureunit"
assert "LIVE18: fixture check — the future-dated fingerprint reads back as later than now" \
    test "$(stat -c %X "$LIVE_PROFILE/.fingerprint/futureunit-1818181818181802/lib-futureunit")" -gt "$(date +%s)"
_live_refresh
assert "LIVE18: refresh exits 0" test "$RC" -eq 0
assert "LIVE18: the anchor unit and the live unit survive — a future-dated outlier does not move the anchor" \
    _all_present "$LIVE_GEN/debug/deps" libanchor-00000000000000a1.rlib libliveunit-1818181818181801.rlib
assert "LIVE18: the future-dated unit itself survives (a future consult still counts as its own use)" \
    _all_present "$LIVE_GEN/debug/deps" libfutureunit-1818181818181802.rlib
assert "LIVE18: the dead unit is still pruned (the stage is active)" \
    _none_present "$LIVE_GEN/debug/deps" libdeadunit-deadbeef00000018.rlib

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
