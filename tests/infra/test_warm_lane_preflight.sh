#!/usr/bin/env bash
# tests/infra/test_warm_lane_preflight.sh
# Hermetic tests for scripts/warm-lane-preflight.sh.
#
# PATH stubs:
#   mountpoint — exit 0 when REIFY_TEST_MOUNTED=1; else exit 1
#   cp         — reflink probe: exit 0 when REIFY_TEST_REFLINK_OK=1; else error+exit 1
#   Both record argv to CALLS_FILE.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag
#   B — all-pass happy path: all 5 checks pass, exit 0
#   C — fail-closed failure modes: each failing check exits non-zero with
#         actionable stderr naming the remediation script
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-preflight.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-preflight.sh hermetic tests (task 4661) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-warm-lane-preflight-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-warm-lane-preflight-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-warm-lane-preflight-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# mountpoint stub: exit 0 when REIFY_TEST_MOUNTED=1; else exit 1
cat > "$STUB_DIR/mountpoint" << 'STUB_EOF'
#!/usr/bin/env bash
echo "mountpoint $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
[ "${REIFY_TEST_MOUNTED:-}" = "1" ] && exit 0
exit 1
STUB_EOF
chmod +x "$STUB_DIR/mountpoint"

# cp stub: reflink probe exits 0 when REIFY_TEST_REFLINK_OK=1; else error+exit 1
cat > "$STUB_DIR/cp" << 'STUB_EOF'
#!/usr/bin/env bash
echo "cp $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    exit 0
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# ── Block D stubs: leak oracle + fail oracle ───────────────────────────────────
# leak-oracle.sh: given task-id $1, looks up status in ORACLE_MAP file (one
# "id status" pair per line). Exits 0 with empty output for unknown ids.
cat > "$STUB_DIR/leak-oracle.sh" << 'STUB_EOF'
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
chmod +x "$STUB_DIR/leak-oracle.sh"

# leak-oracle-fail.sh: always exits non-zero — drives the set -e/pipefail
# hardening test (oracle failure must NOT abort the script).
cat > "$STUB_DIR/leak-oracle-fail.sh" << 'STUB_EOF'
#!/usr/bin/env bash
exit 1
STUB_EOF
chmod +x "$STUB_DIR/leak-oracle-fail.sh"

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

# ── make_lane DIR BRANCH — real git-repo factory for Block D lane fixtures ─────
# Creates a minimal git repo at DIR (one initial commit on main), then
# positions HEAD according to BRANCH:
#   task/NNNN   → checkout a new branch named task/NNNN
#   DETACH      → detach HEAD at the initial commit
#   main / ""   → leave on main (no extra checkout)
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
# Block B — all-pass happy path: all 5 checks pass, exit 0
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: all-pass happy path ---"

B_TMP="$(mktemp -d /tmp/test-warm-lane-preflight-b-XXXXXX)"
_TMPDIRS+=("$B_TMP")

# Build a tmp mount dir + base dir (non-empty) + stamp files
B_MNT="$B_TMP/mount"
B_BASE="$B_MNT/base/target"
mkdir -p "$B_BASE"
echo "some content" > "$B_BASE/rustc"

# Write matching stamps
printf '%s' "-C target-cpu=native" > "$B_MNT/base/target.rustflags"
printf '%s' "sha256:cafebabe" > "$B_MNT/base/target.invocation"

# B1: all-pass happy path exits 0
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    run_helper --mount "$B_MNT" --base-dir "$B_BASE" --invocation "sha256:cafebabe"
assert "B1: all-pass exits 0" test "$RC" -eq 0

# B2: cp --reflink=always probe was run (check #2 — reflink-capable)
assert "B2: cp --reflink=always probe ran" \
    bash -c 'grep "^cp " "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# B3: mountpoint was checked (check #1 — volume mounted)
assert "B3: mountpoint was checked" \
    bash -c 'grep "^mountpoint " "$1" | grep -qF "'"$B_MNT"'"' _ "$CALLS_FILE"

# B4: stdout is empty (all diagnostics on stderr)
assert "B4: stdout is empty (diagnostics on stderr)" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# B5: stderr is non-empty (progress diagnostics)
assert "B5: stderr is non-empty (preflight progress)" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# B6: env var defaults (REIFY_WARM_LANE_MOUNT, REIFY_WARM_LANE_BASE, REIFY_WARM_LANE_INVOCATION)
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    REIFY_WARM_LANE_MOUNT="$B_MNT" \
    REIFY_WARM_LANE_BASE="$B_BASE" \
    REIFY_WARM_LANE_INVOCATION="sha256:cafebabe" \
    run_helper
assert "B6: env-var defaults path exits 0" test "$RC" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block C — fail-closed failure modes
# Each sub-case asserts: non-zero exit + actionable stderr naming the cause
# and the remediation script.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: fail-closed failure modes ---"

C_TMP="$(mktemp -d /tmp/test-warm-lane-preflight-c-XXXXXX)"
_TMPDIRS+=("$C_TMP")
C_MNT="$C_TMP/mount"
C_BASE="$C_MNT/base/target"
mkdir -p "$C_BASE"
echo "content" > "$C_BASE/rustc"
printf '%s' "sha256:deadbeef" > "$C_MNT/base/target.invocation"
printf '%s' "" > "$C_MNT/base/target.rustflags"

# ── C1: volume not mounted → mentions provision-warm-lane-fs.sh ───────────────
reset_calls
REIFY_TEST_MOUNTED="" REIFY_TEST_REFLINK_OK=1 \
    run_helper --mount "$C_MNT" --base-dir "$C_BASE" --invocation "sha256:deadbeef"
assert "C1: not mounted exits non-zero" test "$RC" -ne 0
assert "C1: stderr names provision-warm-lane-fs.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "provision-warm-lane-fs.sh"' _ "$ERR_OUT"

# ── C2: not reflink-capable → no silent fallback, mentions provision script ────
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=0 \
    run_helper --mount "$C_MNT" --base-dir "$C_BASE" --invocation "sha256:deadbeef"
assert "C2: not reflink-capable exits non-zero" test "$RC" -ne 0
assert "C2: stderr mentions reflink or Operation not supported" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "reflink|Operation not supported"' _ "$ERR_OUT"
assert "C2: no silent fallback (stdout empty)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "C2: stderr names provision-warm-lane-fs.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "provision-warm-lane-fs.sh"' _ "$ERR_OUT"

# ── C3: base missing → names refresh-warm-base.sh ─────────────────────────────
C3_TMP="$(mktemp -d /tmp/test-warm-lane-preflight-c3-XXXXXX)"
_TMPDIRS+=("$C3_TMP")
C3_MNT="$C3_TMP/mount"
mkdir -p "$C3_MNT"
C3_BASE="$C3_MNT/base/target"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    run_helper --mount "$C3_MNT" --base-dir "$C3_BASE" --invocation "sha256:deadbeef"
assert "C3: base missing exits non-zero" test "$RC" -ne 0
assert "C3: stderr names refresh-warm-base.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "refresh-warm-base.sh"' _ "$ERR_OUT"

# ── C3b: base empty (exists but no files) → names refresh-warm-base.sh ─────────
C3B_TMP="$(mktemp -d /tmp/test-warm-lane-preflight-c3b-XXXXXX)"
_TMPDIRS+=("$C3B_TMP")
C3B_MNT="$C3B_TMP/mount"
C3B_BASE="$C3B_MNT/base/target"
mkdir -p "$C3B_BASE"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    run_helper --mount "$C3B_MNT" --base-dir "$C3B_BASE" --invocation ""
assert "C3b: empty base exits non-zero" test "$RC" -ne 0
assert "C3b: stderr names refresh-warm-base.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "refresh-warm-base.sh"' _ "$ERR_OUT"

# ── C4: invocation mismatch → shows both values, names refresh-warm-base.sh ────
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    run_helper --mount "$C_MNT" --base-dir "$C_BASE" --invocation "sha256:different"
assert "C4: invocation mismatch exits non-zero" test "$RC" -ne 0
assert "C4: stderr names refresh-warm-base.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "refresh-warm-base.sh"' _ "$ERR_OUT"
assert "C4: stderr mentions both invocation values" \
    bash -c 'printf "%s\n" "$1" | grep -q "sha256:deadbeef"' _ "$ERR_OUT"

# ── C4b: invocation stamp missing (treated as mismatch, fail-closed) ───────────
C4B_TMP="$(mktemp -d /tmp/test-warm-lane-preflight-c4b-XXXXXX)"
_TMPDIRS+=("$C4B_TMP")
C4B_MNT="$C4B_TMP/mount"
C4B_BASE="$C4B_MNT/base/target"
mkdir -p "$C4B_BASE"
echo "content" > "$C4B_BASE/rustc"
printf '%s' "" > "$C4B_MNT/base/target.rustflags"
# NO invocation stamp file

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    run_helper --mount "$C4B_MNT" --base-dir "$C4B_BASE" --invocation "sha256:expected"
assert "C4b: missing invocation stamp exits non-zero (fail-closed)" test "$RC" -ne 0
assert "C4b: stderr names refresh-warm-base.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "refresh-warm-base.sh"' _ "$ERR_OUT"

# ── C5: RUSTFLAGS mismatch → explains cold-rebuild risk, names refresh script ──
C5_TMP="$(mktemp -d /tmp/test-warm-lane-preflight-c5-XXXXXX)"
_TMPDIRS+=("$C5_TMP")
C5_MNT="$C5_TMP/mount"
C5_BASE="$C5_MNT/base/target"
mkdir -p "$C5_BASE"
echo "content" > "$C5_BASE/rustc"
printf '%s' "sha256:match" > "$C5_MNT/base/target.invocation"
printf '%s' "-C base-flags" > "$C5_MNT/base/target.rustflags"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C different-flags" \
    run_helper --mount "$C5_MNT" --base-dir "$C5_BASE" --invocation "sha256:match"
assert "C5: RUSTFLAGS mismatch exits non-zero" test "$RC" -ne 0
assert "C5: stderr names refresh-warm-base.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "refresh-warm-base.sh"' _ "$ERR_OUT"
assert "C5: stderr explains cold-rebuild risk (D4)" \
    bash -c 'printf "%s\n" "$1" | grep -qi "cold.rebuild\|RUSTFLAGS"' _ "$ERR_OUT"

# ── C5b: RUSTFLAGS stamp missing (treated as mismatch, fail-closed) ────────────
C5B_TMP="$(mktemp -d /tmp/test-warm-lane-preflight-c5b-XXXXXX)"
_TMPDIRS+=("$C5B_TMP")
C5B_MNT="$C5B_TMP/mount"
C5B_BASE="$C5B_MNT/base/target"
mkdir -p "$C5B_BASE"
echo "content" > "$C5B_BASE/rustc"
printf '%s' "sha256:match" > "$C5B_MNT/base/target.invocation"
# NO rustflags stamp file

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C some-flags" \
    run_helper --mount "$C5B_MNT" --base-dir "$C5B_BASE" --invocation "sha256:match"
assert "C5b: missing RUSTFLAGS stamp exits non-zero (fail-closed)" test "$RC" -ne 0
assert "C5b: stderr names refresh-warm-base.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "refresh-warm-base.sh"' _ "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block D — Check 6 advisory lane-leak detector
# All Block-D cases run on the Block-B all-pass mount fixture (checks 1-5 pass)
# and additionally thread REIFY_LANE_LEAK_WORKTREES + REIFY_LANE_LEAK_STATUS_CMD.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: Check 6 advisory lane-leak detector ---"

D_TMP="$(mktemp -d /tmp/test-warm-lane-preflight-d-XXXXXX)"
_TMPDIRS+=("$D_TMP")

# Re-use the B_MNT / B_BASE / B-stamp layout as the all-pass fixture.
# (B_MNT, B_BASE, and their stamp files are already set up above.)

# D1: Check 6 is opt-in — with REIFY_LANE_LEAK_STATUS_CMD UNSET,
#     the script exits 0, stderr contains "skipped" and
#     "REIFY_LANE_LEAK_STATUS_CMD", stdout is empty, and stderr still
#     contains "all checks passed" (checks 1-5 unaffected).
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    run_helper --mount "$B_MNT" --base-dir "$B_BASE" --invocation "sha256:cafebabe"
assert "D1: opt-in skip exits 0" test "$RC" -eq 0
assert "D1: stderr contains 'skipped'" \
    bash -c 'printf "%s\n" "$1" | grep -q "skipped"' _ "$ERR_OUT"
assert "D1: stderr names REIFY_LANE_LEAK_STATUS_CMD" \
    bash -c 'printf "%s\n" "$1" | grep -q "REIFY_LANE_LEAK_STATUS_CMD"' _ "$ERR_OUT"
assert "D1: stdout is empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "D1: stderr still contains 'all checks passed'" \
    bash -c 'printf "%s\n" "$1" | grep -q "all checks passed"' _ "$ERR_OUT"

# D2: with REIFY_LANE_LEAK_STATUS_CMD set but REIFY_LANE_LEAK_WORKTREES pointing
#     at a NON-EXISTENT dir, script exits 0 and stderr contains "skipped".
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    REIFY_LANE_LEAK_STATUS_CMD="$STUB_DIR/leak-oracle.sh" \
    REIFY_LANE_LEAK_WORKTREES="$D_TMP/nonexistent-dir" \
    run_helper --mount "$B_MNT" --base-dir "$B_BASE" --invocation "sha256:cafebabe"
assert "D2: nonexistent worktrees dir exits 0" test "$RC" -eq 0
assert "D2: stderr contains 'skipped'" \
    bash -c 'printf "%s\n" "$1" | grep -q "skipped"' _ "$ERR_OUT"

# ── D3: basic detection — lane on task/100 with status=done is flagged ─────────
D3_LANES="$D_TMP/lanes-d3"
mkdir -p "$D3_LANES"
_TMPDIRS+=("$D3_LANES")

make_lane "$D3_LANES/_lane-0" "task/100"

D3_MAP="$(mktemp /tmp/test-preflight-oracle-map-XXXXXX)"
_TMPDIRS+=("$D3_MAP")
printf '100 done\n' > "$D3_MAP"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    REIFY_LANE_LEAK_STATUS_CMD="$STUB_DIR/leak-oracle.sh" \
    REIFY_LANE_LEAK_WORKTREES="$D3_LANES" \
    ORACLE_MAP="$D3_MAP" \
    run_helper --mount "$B_MNT" --base-dir "$B_BASE" --invocation "sha256:cafebabe"
assert "D3: done-task lane exits 0 (advisory, non-fatal)" test "$RC" -eq 0
assert "D3: stderr contains 'LANE LEAK'" \
    bash -c 'printf "%s\n" "$1" | grep -q "LANE LEAK"' _ "$ERR_OUT"
assert "D3: stderr contains '1 lane'" \
    bash -c 'printf "%s\n" "$1" | grep -q "1 lane"' _ "$ERR_OUT"
assert "D3: stderr table row names _lane-0" \
    bash -c 'printf "%s\n" "$1" | grep -q "_lane-0"' _ "$ERR_OUT"
assert "D3: stderr table row names task id 100" \
    bash -c 'printf "%s\n" "$1" | grep -q "100"' _ "$ERR_OUT"
assert "D3: stderr table row contains status=done" \
    bash -c 'printf "%s\n" "$1" | grep -q "done"' _ "$ERR_OUT"
assert "D3: stdout is empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# ── D4: clean — lane on task/200 with status=pending is NOT flagged ────────────
D4_LANES="$D_TMP/lanes-d4"
mkdir -p "$D4_LANES"
_TMPDIRS+=("$D4_LANES")

make_lane "$D4_LANES/_lane-0" "task/200"

D4_MAP="$(mktemp /tmp/test-preflight-oracle-map-XXXXXX)"
_TMPDIRS+=("$D4_MAP")
printf '200 pending\n' > "$D4_MAP"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    REIFY_LANE_LEAK_STATUS_CMD="$STUB_DIR/leak-oracle.sh" \
    REIFY_LANE_LEAK_WORKTREES="$D4_LANES" \
    ORACLE_MAP="$D4_MAP" \
    run_helper --mount "$B_MNT" --base-dir "$B_BASE" --invocation "sha256:cafebabe"
assert "D4: pending-task lane exits 0" test "$RC" -eq 0
assert "D4: stderr contains 'no terminal-task lane leaks detected'" \
    bash -c 'printf "%s\n" "$1" | grep -q "no terminal-task lane leaks detected"' _ "$ERR_OUT"

# ── D5: skip-guards — detached lane, main lane, task/abc lane, plain dir ───────
D5_LANES="$D_TMP/lanes-d5"
mkdir -p "$D5_LANES"
_TMPDIRS+=("$D5_LANES")

make_lane "$D5_LANES/_lane-detach" "DETACH"
make_lane "$D5_LANES/_lane-main"   "main"
make_lane "$D5_LANES/_lane-abc"    "task/abc"   # non-numeric task id
# plain dir (non-git)
mkdir -p "$D5_LANES/_lane-plain"

D5_MAP="$(mktemp /tmp/test-preflight-oracle-map-XXXXXX)"
_TMPDIRS+=("$D5_MAP")
# even if oracle were called with these, they'd return empty; leave map empty
printf '' > "$D5_MAP"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    REIFY_LANE_LEAK_STATUS_CMD="$STUB_DIR/leak-oracle.sh" \
    REIFY_LANE_LEAK_WORKTREES="$D5_LANES" \
    ORACLE_MAP="$D5_MAP" \
    run_helper --mount "$B_MNT" --base-dir "$B_BASE" --invocation "sha256:cafebabe"
assert "D5: skip-guards all exit 0 with count 0" test "$RC" -eq 0
assert "D5: stderr reports no leaks (no false-positives)" \
    bash -c 'printf "%s\n" "$1" | grep -q "no terminal-task lane leaks detected"' _ "$ERR_OUT"

# ── D6: hardening — fail-oracle does NOT abort the script ──────────────────────
D6_LANES="$D_TMP/lanes-d6"
mkdir -p "$D6_LANES"
_TMPDIRS+=("$D6_LANES")

make_lane "$D6_LANES/_lane-0" "task/300"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    REIFY_LANE_LEAK_STATUS_CMD="$STUB_DIR/leak-oracle-fail.sh" \
    REIFY_LANE_LEAK_WORKTREES="$D6_LANES" \
    run_helper --mount "$B_MNT" --base-dir "$B_BASE" --invocation "sha256:cafebabe"
assert "D6: fail-oracle does NOT abort the script (RC 0)" test "$RC" -eq 0
assert "D6: fail-oracle lane is not flagged as a leak" \
    bash -c '! printf "%s\n" "$1" | grep -q "LANE LEAK"' _ "$ERR_OUT"

# ── D7: cancelled-task lane IS flagged as a leak ───────────────────────────────
D7_LANES="$D_TMP/lanes-d7"
mkdir -p "$D7_LANES"
_TMPDIRS+=("$D7_LANES")

make_lane "$D7_LANES/_lane-0" "task/400"

D7_MAP="$(mktemp /tmp/test-preflight-oracle-map-XXXXXX)"
_TMPDIRS+=("$D7_MAP")
printf '400 cancelled\n' > "$D7_MAP"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    REIFY_LANE_LEAK_STATUS_CMD="$STUB_DIR/leak-oracle.sh" \
    REIFY_LANE_LEAK_WORKTREES="$D7_LANES" \
    ORACLE_MAP="$D7_MAP" \
    run_helper --mount "$B_MNT" --base-dir "$B_BASE" --invocation "sha256:cafebabe"
assert "D7: cancelled-task lane exits 0 (advisory)" test "$RC" -eq 0
assert "D7: stderr contains 'LANE LEAK'" \
    bash -c 'printf "%s\n" "$1" | grep -q "LANE LEAK"' _ "$ERR_OUT"
assert "D7: stderr table row contains task id 400" \
    bash -c 'printf "%s\n" "$1" | grep -q "400"' _ "$ERR_OUT"
assert "D7: stderr table row contains status=cancelled" \
    bash -c 'printf "%s\n" "$1" | grep -q "cancelled"' _ "$ERR_OUT"

# ── D8: mixed pool — 5 lanes, exactly 2 leaks (done + cancelled), others skipped ─
D8_LANES="$D_TMP/lanes-d8"
mkdir -p "$D8_LANES"
_TMPDIRS+=("$D8_LANES")

make_lane "$D8_LANES/_lane-0" "task/100"     # done  → LEAK
make_lane "$D8_LANES/_lane-1" "task/400"     # cancelled → LEAK
make_lane "$D8_LANES/_lane-2" "task/200"     # pending → clean
make_lane "$D8_LANES/_lane-3" "DETACH"       # detached → skip
make_lane "$D8_LANES/_lane-4" "main"         # main → skip

D8_MAP="$(mktemp /tmp/test-preflight-oracle-map-XXXXXX)"
_TMPDIRS+=("$D8_MAP")
printf '100 done\n400 cancelled\n200 pending\n' > "$D8_MAP"

reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_REFLINK_OK=1 \
    RUSTFLAGS="-C target-cpu=native" \
    REIFY_LANE_LEAK_STATUS_CMD="$STUB_DIR/leak-oracle.sh" \
    REIFY_LANE_LEAK_WORKTREES="$D8_LANES" \
    ORACLE_MAP="$D8_MAP" \
    run_helper --mount "$B_MNT" --base-dir "$B_BASE" --invocation "sha256:cafebabe"
assert "D8: mixed pool exits 0 (advisory)" test "$RC" -eq 0
assert "D8: stderr contains 'LANE LEAK'" \
    bash -c 'printf "%s\n" "$1" | grep -q "LANE LEAK"' _ "$ERR_OUT"
assert "D8: stderr reports count of 2 lanes" \
    bash -c 'printf "%s\n" "$1" | grep -q "2 lane"' _ "$ERR_OUT"
assert "D8: table row for task 100 present" \
    bash -c 'printf "%s\n" "$1" | grep -q "100"' _ "$ERR_OUT"
assert "D8: table row for task 400 present" \
    bash -c 'printf "%s\n" "$1" | grep -q "400"' _ "$ERR_OUT"
assert "D8: task 200 (pending) NOT in alarm" \
    bash -c '! printf "%s\n" "$1" | grep -qE "200.*pending|pending.*200"' _ "$ERR_OUT"
assert "D8: stdout is empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "D8: stderr still contains 'all checks passed'" \
    bash -c 'printf "%s\n" "$1" | grep -q "all checks passed"' _ "$ERR_OUT"
assert "D8: table body has exactly 2 rows (one per leak)" \
    bash -c 'printf "%s\n" "$1" | grep -c -- "-> task " | grep -qx 2' _ "$ERR_OUT"

test_summary
