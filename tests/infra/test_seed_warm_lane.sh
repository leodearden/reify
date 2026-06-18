#!/usr/bin/env bash
# tests/infra/test_seed_warm_lane.sh
# Hermetic tests for scripts/seed-warm-lane.sh.
#
# PATH-stubs: cp/find/touch/git (record argv to CALLS_FILE).
# Env-driven stub behaviour:
#   REIFY_TEST_REFLINK_OK    — cp stub: "1" → exit 0; else print error + exit 1
#   REIFY_TEST_GIT_DIFF_FILES — git stub: emitted as output of diff --name-only
#   REIFY_TEST_GIT_HEAD      — git stub: emitted as output of rev-parse HEAD
#
# run_helper captures STDOUT and STDERR SEPARATELY:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard (step-1 / step-2)
#   B — RUSTFLAGS guard / B5 (step-3 / step-4)
#   C — reflink clone + fail-closed / S2 (step-5 / step-6)
#   D — fresh-checkout mtime / D5 (step-7 / step-8)
#   E — reset-in-place / no bulk stamp (step-9 / step-10)
#   F — invocation fingerprint guard / S1 (step-11 / step-12)
#   G — --record-base writer (step-13 / step-14)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/seed-warm-lane.sh hermetic tests (task 4660) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-seed-warm-lane-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-seed-warm-lane-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-seed-warm-lane-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ────────────────────────────────────────────────────────────────

# cp stub: record argv; REIFY_TEST_REFLINK_OK=1 → exit 0, else error + exit 1
cat > "$STUB_DIR/cp" << 'STUB_EOF'
#!/usr/bin/env bash
echo "cp $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    # When REIFY_TEST_CP_CREATE_DEST=1, physically create the destination dir+file
    # so that mtime tests can assert on target/ contents.
    if [ "${REIFY_TEST_CP_CREATE_DEST:-}" = "1" ]; then
        dest="${*: -1}"
        mkdir -p "$dest/debug"
        echo "artifact" > "$dest/debug/artifact.a"
    fi
    exit 0
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# find stub: record argv, exit 0 (no-op; Block D uses real find)
cat > "$STUB_DIR/find" << 'STUB_EOF'
#!/usr/bin/env bash
echo "find $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/find"

# touch stub: record argv, exit 0 (no-op; Block D uses real touch)
cat > "$STUB_DIR/touch" << 'STUB_EOF'
#!/usr/bin/env bash
echo "touch $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/touch"

# git stub: record argv; controlled diff/rev-parse output via env vars
cat > "$STUB_DIR/git" << 'STUB_EOF'
#!/usr/bin/env bash
echo "git $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
# Detect diff --name-only and emit controlled file list
for arg in "$@"; do
    if [ "$arg" = "--name-only" ]; then
        if [ -n "${REIFY_TEST_GIT_DIFF_FILES:-}" ]; then
            printf "%s\n" "${REIFY_TEST_GIT_DIFF_FILES}"
        fi
        exit 0
    fi
done
# Detect rev-parse HEAD and emit controlled sha
for arg in "$@"; do
    if [ "$arg" = "rev-parse" ]; then
        echo "${REIFY_TEST_GIT_HEAD:-abc1234}"
        exit 0
    fi
done
exit 0
STUB_EOF
chmod +x "$STUB_DIR/git"

# ── run_helper ────────────────────────────────────────────────────────────────
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

# run_helper_real: like run_helper but without stubbing find/touch — for Block D
# which asserts actual mtime changes on a real fixture tree.
run_helper_real() {
    local rc=0
    > "$ERR_FILE"
    # Only stub cp and git; let find/touch be real binaries
    local real_stub_dir
    real_stub_dir="$(mktemp -d /tmp/test-seed-real-stub-XXXXXX)"
    # cp stub that physically copies src to dest (no --reflink needed for tests)
    cat > "$real_stub_dir/cp" << 'REAL_STUB_EOF'
#!/usr/bin/env bash
echo "cp $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    # Physically copy src→dest using plain cp -a (test environment is non-XFS)
    # Parse out: cp -a --reflink=always <src> <dest>
    src=""
    dest=""
    for arg in "$@"; do
        case "$arg" in
            -a|--reflink=always) ;;
            -*) ;;
            *) [ -z "$src" ] && src="$arg" || dest="$arg" ;;
        esac
    done
    if [ -n "$src" ] && [ -n "$dest" ]; then
        /bin/cp -a "$src" "$dest"
    fi
    exit 0
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
REAL_STUB_EOF
    chmod +x "$real_stub_dir/cp"
    cp "$STUB_DIR/git" "$real_stub_dir/git"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        PATH="$real_stub_dir:$PATH" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
    rm -rf "$real_stub_dir"
}

reset_calls() {
    > "$CALLS_FILE"
}

# ─────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0 with usage on stderr
reset_calls
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits non-zero
reset_calls
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits non-zero" test "$RC" -ne 0

# A3: missing positional args (only mode flag, no base/lane dirs) exits non-zero
reset_calls
run_helper --fresh-checkout
assert "A3: missing positional args exits non-zero" test "$RC" -ne 0

# A4: neither --fresh-checkout nor --reset-in-place exits non-zero
reset_calls
A_BASE="$(mktemp -d /tmp/test-seed-A-base-XXXXXX)"
A_LANE="$(mktemp -d /tmp/test-seed-A-lane-XXXXXX)"
_TMPDIRS+=("$A_BASE" "$A_LANE")
run_helper "$A_BASE" "$A_LANE"
assert "A4: neither mode flag exits non-zero" test "$RC" -ne 0

# A5: both --fresh-checkout and --reset-in-place exits non-zero
reset_calls
run_helper "$A_BASE" "$A_LANE" --fresh-checkout --reset-in-place
assert "A5: both mode flags exits non-zero" test "$RC" -ne 0

# ─────────────────────────────────────────────────────────────────────────────
# Block B — RUSTFLAGS guard (B5 / D4)
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: RUSTFLAGS guard (B5) ---"

# Fixture: a base dir with a sidecar recording RUSTFLAGS
B_BASE_PARENT="$(mktemp -d /tmp/test-seed-B-parent-XXXXXX)"
B_BASE="$B_BASE_PARENT/target"
B_LANE="$(mktemp -d /tmp/test-seed-B-lane-XXXXXX)"
_TMPDIRS+=("$B_BASE_PARENT" "$B_LANE")
mkdir -p "$B_BASE"
# Write sidecar with recorded RUSTFLAGS=old-flags
cat > "$B_BASE_PARENT/.warm-base-meta" <<'SIDECAR_EOF'
RUSTFLAGS=old-flags
INVOCATION=
SIDECAR_EOF

# B1: RUSTFLAGS mismatch → non-zero exit
reset_calls
RUSTFLAGS="new-flags" run_helper "$B_BASE" "$B_LANE" --fresh-checkout
assert "B1: RUSTFLAGS mismatch exits non-zero" test "$RC" -ne 0

# B2: stderr names the RUSTFLAGS mismatch (actionable message)
assert "B2: stderr names RUSTFLAGS mismatch" \
    bash -c 'printf "%s\n" "$1" | grep -qi "RUSTFLAGS"' _ "$ERR_OUT"

# B3: STDOUT is EMPTY on mismatch (fail-closed: no path emitted)
assert "B3: STDOUT is EMPTY on RUSTFLAGS mismatch (fail-closed)" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# B4: cp was NEVER invoked (guard fires before clone)
assert "B4: cp NEVER invoked on RUSTFLAGS mismatch" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# B5: matching RUSTFLAGS (recorded "old-flags" == env "old-flags") → guard passes → cp IS called
B_LANE2="$(mktemp -d /tmp/test-seed-B-lane2-XXXXXX)"
_TMPDIRS+=("$B_LANE2")
reset_calls
RUSTFLAGS="old-flags" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$B_BASE" "$B_LANE2" --fresh-checkout
assert "B5: matching RUSTFLAGS passes guard → cp invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

# B6: also test: no sidecar → recorded RUSTFLAGS defaults to "" → empty-env RUSTFLAGS matches
B_BASE2_PARENT="$(mktemp -d /tmp/test-seed-B2-parent-XXXXXX)"
B_BASE2="$B_BASE2_PARENT/target"
B_LANE3="$(mktemp -d /tmp/test-seed-B-lane3-XXXXXX)"
_TMPDIRS+=("$B_BASE2_PARENT" "$B_LANE3")
mkdir -p "$B_BASE2"
# No sidecar: recorded defaults to ""
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$B_BASE2" "$B_LANE3" --fresh-checkout
assert "B6: no sidecar + empty RUSTFLAGS matches default → cp invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

# ─────────────────────────────────────────────────────────────────────────────
# Block C — reflink clone + fail-closed (S2)
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: reflink clone + fail-closed (S2) ---"

# Shared fixture: a base dir (with empty sidecar → guards pass) + a fresh lane dir
C_BASE_PARENT="$(mktemp -d /tmp/test-seed-C-parent-XXXXXX)"
C_BASE="$C_BASE_PARENT/target"
_TMPDIRS+=("$C_BASE_PARENT")
mkdir -p "$C_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$C_BASE_PARENT/.warm-base-meta"

# C1: cp invoked with --reflink=always and destination <lane_dir>/target
C_LANE1="$(mktemp -d /tmp/test-seed-C-lane1-XXXXXX)"
_TMPDIRS+=("$C_LANE1")
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$C_BASE" "$C_LANE1" --fresh-checkout
C1_OUT="$OUT"  # save before subsequent run_helpers overwrite OUT
assert "C1: cp invoked with --reflink=always" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# C2: cp NEVER invoked with --reflink=auto (always=always, not auto)
assert "C2: cp NEVER invoked with --reflink=auto" \
    bash -c '! grep "^cp" "$1" | grep -q -- "--reflink=auto"' _ "$CALLS_FILE"

# C3: destination is <lane_dir>/target
assert "C3: cp destination is <lane_dir>/target" \
    bash -c 'grep "^cp" "$1" | grep -qF "'"$C_LANE1/target"'"' _ "$CALLS_FILE"

# C4: cp failure (non-reflink FS) → script exits non-zero with EMPTY stdout (fail-closed)
C_LANE2="$(mktemp -d /tmp/test-seed-C-lane2-XXXXXX)"
_TMPDIRS+=("$C_LANE2")
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=0 \
    run_helper "$C_BASE" "$C_LANE2" --fresh-checkout
assert "C4: cp failure exits non-zero" test "$RC" -ne 0
assert "C4: STDOUT is EMPTY on cp failure (S2 fail-closed)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "C4: stderr names reflink failure" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "reflink|Operation not supported"' _ "$ERR_OUT"

# C5: pre-existing NON-EMPTY <lane_dir>/target → refused (clobber guard)
C_LANE3="$(mktemp -d /tmp/test-seed-C-lane3-XXXXXX)"
_TMPDIRS+=("$C_LANE3")
mkdir -p "$C_LANE3/target"
echo "existing artifact" > "$C_LANE3/target/artifact.a"
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$C_BASE" "$C_LANE3" --fresh-checkout
assert "C5: clobber guard exits non-zero" test "$RC" -ne 0
assert "C5: clobber guard: STDOUT is EMPTY" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "C5: clobber guard: cp NEVER invoked (refused before clone)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# C6: on success STDOUT is exactly the resolved <lane_dir>/target path
assert "C6: STDOUT is exactly <lane_dir>/target on success" \
    bash -c '[ "$1" = "'"$C_LANE1/target"'" ]' _ "$C1_OUT"

# ─────────────────────────────────────────────────────────────────────────────
# Block D — fresh-checkout mtime normalization (D5)
# Uses run_helper_real (real find + touch; stub cp + git).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: fresh-checkout mtime (D5) ---"

# Epoch for the bulk stamp: 2020-01-01T00:00:00 UTC
EPOCH_2020=1577836800

# Fixture: a base_target_dir + a lane_dir with real source files and target/ + .git/
D_BASE_PARENT="$(mktemp -d /tmp/test-seed-D-parent-XXXXXX)"
D_BASE="$D_BASE_PARENT/target"
D_LANE="$(mktemp -d /tmp/test-seed-D-lane-XXXXXX)"
_TMPDIRS+=("$D_BASE_PARENT" "$D_LANE")
mkdir -p "$D_BASE"
# Sidecar: no RUSTFLAGS/INVOCATION recorded (defaults "")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$D_BASE_PARENT/.warm-base-meta"
# Source files in lane_dir (these should be stamped to 2020-01-01)
mkdir -p "$D_LANE/src"
echo 'fn main() {}' > "$D_LANE/src/main.rs"
echo 'pub fn lib() {}' > "$D_LANE/src/lib.rs"
# .git/ files in lane_dir (pruned — must NOT be stamped)
mkdir -p "$D_LANE/.git"
echo '[core]' > "$D_LANE/.git/config"
# delta source file (will be passed via --touch; must be stamped to now)
D_DELTA="$D_LANE/src/changed.rs"
echo 'pub fn changed() {}' > "$D_DELTA"

# Record mtime of .git/config BEFORE the run (should be ~now; definitely > 2020)
D_GIT_MTIME_BEFORE="$(stat -c '%Y' "$D_LANE/.git/config")"

# A small sleep so "before" and "after" mtimes are distinguishable
sleep 1

# Run --fresh-checkout with real find/touch; pass D_DELTA via --touch
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$D_BASE" "$D_LANE" --fresh-checkout --touch "$D_DELTA"
assert "D0: script exits 0 (fresh-checkout succeeds)" test "$RC" -eq 0

# D1: source files are stamped to 2020-01-01 epoch
D1_MTIME_SRC="$(stat -c '%Y' "$D_LANE/src/main.rs")"
assert "D1: src/main.rs mtime == 2020-01-01 epoch ($EPOCH_2020)" \
    test "$D1_MTIME_SRC" -eq "$EPOCH_2020"

D1_MTIME_LIB="$(stat -c '%Y' "$D_LANE/src/lib.rs")"
assert "D1: src/lib.rs mtime == 2020-01-01 epoch ($EPOCH_2020)" \
    test "$D1_MTIME_LIB" -eq "$EPOCH_2020"

# D2: files under .git/ keep their original mtime (pruned — NOT stamped)
D2_GIT_MTIME_AFTER="$(stat -c '%Y' "$D_LANE/.git/config")"
assert "D2: .git/config mtime unchanged (pruned from bulk stamp)" \
    test "$D2_GIT_MTIME_AFTER" -eq "$D_GIT_MTIME_BEFORE"

# D3: delta file (--touch) is stamped to ~now (mtime > 2020-01-01 epoch)
D3_DELTA_MTIME="$(stat -c '%Y' "$D_DELTA")"
assert "D3: --touch delta file mtime > 2020-01-01 (stamped to now)" \
    test "$D3_DELTA_MTIME" -gt "$EPOCH_2020"

# D4: files under target/ (created by the cp stub) keep recent mtime (pruned)
# The cp stub with REIFY_TEST_REFLINK_OK=1 creates target/debug/artifact.a
if [ -f "$D_LANE/target/debug/artifact.a" ]; then
    D4_TARGET_MTIME="$(stat -c '%Y' "$D_LANE/target/debug/artifact.a")"
    assert "D4: target/debug/artifact.a mtime > 2020-01-01 (pruned)" \
        test "$D4_TARGET_MTIME" -gt "$EPOCH_2020"
fi

# ─────────────────────────────────────────────────────────────────────────────
# Block E — reset-in-place: NO bulk 2020-01-01 stamp (stub find+touch)
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: reset-in-place (no bulk stamp) ---"

# Fixture: a fresh base (sidecar with no RUSTFLAGS/INVOCATION) + a lane dir
E_BASE_PARENT="$(mktemp -d /tmp/test-seed-E-parent-XXXXXX)"
E_BASE="$E_BASE_PARENT/target"
E_LANE="$(mktemp -d /tmp/test-seed-E-lane-XXXXXX)"
_TMPDIRS+=("$E_BASE_PARENT" "$E_LANE")
mkdir -p "$E_BASE"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$E_BASE_PARENT/.warm-base-meta"
mkdir -p "$E_LANE/src"
echo 'fn main() {}' > "$E_LANE/src/main.rs"

# E1: --reset-in-place exits 0
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$E_BASE" "$E_LANE" --reset-in-place
assert "E1: --reset-in-place exits 0" test "$RC" -eq 0

# E2: find was NOT invoked with a 2020-01-01 bulk stamp
# (the stub records every find call; if reset-in-place skips the bulk stamp,
# no find call with "2020-01-01" should appear)
assert "E2: find NOT called with 2020-01-01 bulk stamp (reset-in-place skips it)" \
    bash -c '! grep "^find" "$1" | grep -q "2020"' _ "$CALLS_FILE"

# E3: STDOUT is exactly <lane_dir>/target (success contract preserved)
assert "E3: STDOUT is exactly <lane_dir>/target" \
    bash -c '[ "$1" = "'"$E_LANE/target"'" ]' _ "$OUT"

# ─────────────────────────────────────────────────────────────────────────────
# Block F — invocation fingerprint guard (S1)
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: invocation fingerprint guard (S1) ---"

# Fixture: base with sidecar recording a specific invocation fingerprint
F_BASE_PARENT="$(mktemp -d /tmp/test-seed-F-parent-XXXXXX)"
F_BASE="$F_BASE_PARENT/target"
_TMPDIRS+=("$F_BASE_PARENT")
mkdir -p "$F_BASE"
cat > "$F_BASE_PARENT/.warm-base-meta" <<'SIDECAR_EOF'
RUSTFLAGS=
INVOCATION=my-invocation-fingerprint
SIDECAR_EOF

# F1: invocation mismatch → non-zero exit
F_LANE1="$(mktemp -d /tmp/test-seed-F-lane1-XXXXXX)"
_TMPDIRS+=("$F_LANE1")
reset_calls
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="wrong-invocation" \
    run_helper "$F_BASE" "$F_LANE1" --fresh-checkout
assert "F1: invocation mismatch exits non-zero" test "$RC" -ne 0

# F2: stderr names the invocation mismatch (actionable)
assert "F2: stderr names invocation mismatch" \
    bash -c 'printf "%s\n" "$1" | grep -qi "invocation"' _ "$ERR_OUT"

# F3: STDOUT is EMPTY on mismatch (fail-closed)
assert "F3: STDOUT is EMPTY on invocation mismatch" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# F4: cp NEVER invoked (guard fires before clone)
assert "F4: cp NEVER invoked on invocation mismatch" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# F5: matching invocation → guard passes → cp IS called
F_LANE2="$(mktemp -d /tmp/test-seed-F-lane2-XXXXXX)"
_TMPDIRS+=("$F_LANE2")
reset_calls
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="my-invocation-fingerprint" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$F_BASE" "$F_LANE2" --fresh-checkout
assert "F5: matching invocation passes guard → cp invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

# F6: no sidecar recorded invocation → defaults "" → empty env matches
F_BASE2_PARENT="$(mktemp -d /tmp/test-seed-F2-parent-XXXXXX)"
F_BASE2="$F_BASE2_PARENT/target"
F_LANE3="$(mktemp -d /tmp/test-seed-F-lane3-XXXXXX)"
_TMPDIRS+=("$F_BASE2_PARENT" "$F_LANE3")
mkdir -p "$F_BASE2"
# No sidecar → recorded invocation defaults to ""
reset_calls
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$F_BASE2" "$F_LANE3" --fresh-checkout
assert "F6: no sidecar + empty invocation matches default → cp invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

test_summary
