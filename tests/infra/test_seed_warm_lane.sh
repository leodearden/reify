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
# Seed a build artifact so run_helper_real's /bin/cp -a propagates it to
# $D_LANE/target; allows D4 to assert that target/ files keep their mtime.
mkdir -p "$D_BASE/debug"
echo "artifact" > "$D_BASE/debug/artifact.a"
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

# D4: files under target/ keep their pre-clone mtime — find prunes target/ entirely.
# $D_BASE/debug/artifact.a was seeded above; run_helper_real's /bin/cp -a
# propagates it to $D_LANE/target/debug/artifact.a.
D4_TARGET_MTIME="$(stat -c '%Y' "$D_LANE/target/debug/artifact.a")"
assert "D4: target/debug/artifact.a mtime > 2020-01-01 (pruned from bulk stamp)" \
    test "$D4_TARGET_MTIME" -gt "$EPOCH_2020"

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

# ─────────────────────────────────────────────────────────────────────────────
# Block G — --record-base writer
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: --record-base writer ---"

# Fixture: a base_target_dir to record provenance for
G_BASE_PARENT="$(mktemp -d /tmp/test-seed-G-parent-XXXXXX)"
G_BASE="$G_BASE_PARENT/target"
_TMPDIRS+=("$G_BASE_PARENT")
mkdir -p "$G_BASE"

EXPECTED_SIDECAR="$G_BASE_PARENT/.warm-base-meta"

# G1: --record-base exits 0
reset_calls
RUSTFLAGS="my-rustflags" REIFY_WARM_LANE_INVOCATION="my-invocation" \
    run_helper --record-base "$G_BASE"
assert "G1: --record-base exits 0" test "$RC" -eq 0

# G2: sidecar file was created beside the base target dir
assert "G2: sidecar created at $(dirname $G_BASE)/.warm-base-meta" \
    test -f "$EXPECTED_SIDECAR"

# G3: sidecar records RUSTFLAGS
assert "G3: sidecar records RUSTFLAGS=my-rustflags" \
    bash -c 'grep -q "^RUSTFLAGS=my-rustflags$" "$1"' _ "$EXPECTED_SIDECAR"

# G4: sidecar records INVOCATION
assert "G4: sidecar records INVOCATION=my-invocation" \
    bash -c 'grep -q "^INVOCATION=my-invocation$" "$1"' _ "$EXPECTED_SIDECAR"

# G5: STDOUT is the sidecar path (exactly)
assert "G5: STDOUT is exactly the sidecar path" \
    bash -c '[ "$1" = "'"$EXPECTED_SIDECAR"'" ]' _ "$OUT"

# G6: round-trip — a subsequent seed against the recorded base passes the guards
G_LANE="$(mktemp -d /tmp/test-seed-G-lane-XXXXXX)"
_TMPDIRS+=("$G_LANE")
reset_calls
RUSTFLAGS="my-rustflags" REIFY_WARM_LANE_INVOCATION="my-invocation" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$G_BASE" "$G_LANE" --fresh-checkout
assert "G6: round-trip: matching env passes both guards → cp invoked" \
    bash -c 'grep -q "^cp" "$1"' _ "$CALLS_FILE"

# G7: round-trip mismatch — different RUSTFLAGS is still refused after record-base
G_LANE2="$(mktemp -d /tmp/test-seed-G-lane2-XXXXXX)"
_TMPDIRS+=("$G_LANE2")
reset_calls
RUSTFLAGS="wrong-flags" REIFY_WARM_LANE_INVOCATION="my-invocation" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$G_BASE" "$G_LANE2" --fresh-checkout
assert "G7: round-trip: mismatched RUSTFLAGS still refused after record-base" \
    test "$RC" -ne 0

# ─────────────────────────────────────────────────────────────────────────────
# Block H — build-script output-dir invalidation (non-relocatable absolute paths)
# Uses run_helper_real (real cp/find/touch + stub git).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: build-script output-dir invalidation (tauri-* + reify-gui-*) ---"

# Fixture: a base target/ with build dirs under debug/build and release/build.
# The sidecar has empty RUSTFLAGS/INVOCATION so guards pass.
H_BASE_PARENT="$(mktemp -d /tmp/test-seed-H-parent-XXXXXX)"
H_BASE="$H_BASE_PARENT/target"
H_LANE="$(mktemp -d /tmp/test-seed-H-lane-XXXXXX)"
_TMPDIRS+=("$H_BASE_PARENT" "$H_LANE")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H_BASE_PARENT/.warm-base-meta"

# Build dirs under two profiles:
#   debug/build:   tauri-AAAA, tauri-plugin-fs-BBBB, reify-gui-CCCC, serde-DDDD
#   release/build: tauri-EEEE, serde-FFFF
# Each dir contains an 'output' file (non-empty, as cargo would produce).
mkdir -p "$H_BASE/debug/build/tauri-AAAA"
mkdir -p "$H_BASE/debug/build/tauri-plugin-fs-BBBB"
mkdir -p "$H_BASE/debug/build/reify-gui-CCCC"
mkdir -p "$H_BASE/debug/build/serde-DDDD"
mkdir -p "$H_BASE/release/build/tauri-EEEE"
mkdir -p "$H_BASE/release/build/serde-FFFF"
echo "out" > "$H_BASE/debug/build/tauri-AAAA/output"
echo "out" > "$H_BASE/debug/build/tauri-plugin-fs-BBBB/output"
echo "out" > "$H_BASE/debug/build/reify-gui-CCCC/output"
echo "out" > "$H_BASE/debug/build/serde-DDDD/output"
echo "out" > "$H_BASE/release/build/tauri-EEEE/output"
echo "out" > "$H_BASE/release/build/serde-FFFF/output"

# H1: seed with --fresh-checkout; real cp physically copies dirs to the lane.
reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H_BASE" "$H_LANE" --fresh-checkout

# H1c: success contract — exit 0, stdout == <lane>/target
assert "H1c: --fresh-checkout exits 0 (build-dir invalidation)" test "$RC" -eq 0
assert "H1c: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "'"$H_LANE/target"'" ]' _ "$OUT"

# H1a: allow-listed dirs REMOVED across both profiles
assert "H1a: debug/build/tauri-AAAA GONE (allow-listed tauri-*)" \
    bash -c '[ ! -e "'"$H_LANE/target/debug/build/tauri-AAAA"'" ]'
assert "H1a: debug/build/tauri-plugin-fs-BBBB GONE (allow-listed tauri-*)" \
    bash -c '[ ! -e "'"$H_LANE/target/debug/build/tauri-plugin-fs-BBBB"'" ]'
assert "H1a: debug/build/reify-gui-CCCC GONE (allow-listed reify-gui-*)" \
    bash -c '[ ! -e "'"$H_LANE/target/debug/build/reify-gui-CCCC"'" ]'
assert "H1a: release/build/tauri-EEEE GONE (allow-listed tauri-*)" \
    bash -c '[ ! -e "'"$H_LANE/target/release/build/tauri-EEEE"'" ]'

# H1b: unlisted dirs PRESERVED (warmth retained for non-offending crates)
assert "H1b: debug/build/serde-DDDD PRESERVED (not allow-listed)" \
    test -d "$H_LANE/target/debug/build/serde-DDDD"
assert "H1b: release/build/serde-FFFF PRESERVED (not allow-listed)" \
    test -d "$H_LANE/target/release/build/serde-FFFF"

# H1d: info line reports the correct non-zero invalidated count.
# This locks in that the matcher FIRED (dirs absent could also mean cp failed),
# and would catch a silent 0-count regression caused by the assignment-time-glob
# bug (unquoted 'tauri-*' in array assignment expanding against the CWD and
# replacing the intended literal patterns with CWD matches → 0 dirs found).
# Expected count: 4 dirs removed (debug/build: tauri-AAAA + tauri-plugin-fs-BBBB
# + reify-gui-CCCC; release/build: tauri-EEEE).
assert "H1d: info line reports Invalidated 4 non-relocatable dirs (matcher fired)" \
    bash -c 'printf "%s\n" "$1" | grep -q "Invalidated 4 "' _ "$ERR_OUT"

# ── H3a: --reset-in-place does NOT invalidate (scope guard) ──────────────────
# The invalidation block must live entirely inside `if [ -n "$FRESH_CHECKOUT" ]`.
H3a_BASE_PARENT="$(mktemp -d /tmp/test-seed-H3a-parent-XXXXXX)"
H3a_BASE="$H3a_BASE_PARENT/target"
H3a_LANE="$(mktemp -d /tmp/test-seed-H3a-lane-XXXXXX)"
_TMPDIRS+=("$H3a_BASE_PARENT" "$H3a_LANE")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H3a_BASE_PARENT/.warm-base-meta"
mkdir -p "$H3a_BASE/debug/build/tauri-XXXX"
echo "out" > "$H3a_BASE/debug/build/tauri-XXXX/output"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H3a_BASE" "$H3a_LANE" --reset-in-place
assert "H3a: --reset-in-place exits 0" test "$RC" -eq 0
assert "H3a: debug/build/tauri-XXXX PRESERVED under --reset-in-place (scope guard)" \
    test -d "$H3a_LANE/target/debug/build/tauri-XXXX"

# ── H3b: clean no-op when nothing matches (set -euo pipefail safe) ───────────
# Case 1: build/ exists but contains only unlisted dirs (serde-YYYY)
H3b1_BASE_PARENT="$(mktemp -d /tmp/test-seed-H3b1-parent-XXXXXX)"
H3b1_BASE="$H3b1_BASE_PARENT/target"
H3b1_LANE="$(mktemp -d /tmp/test-seed-H3b1-lane-XXXXXX)"
_TMPDIRS+=("$H3b1_BASE_PARENT" "$H3b1_LANE")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H3b1_BASE_PARENT/.warm-base-meta"
mkdir -p "$H3b1_BASE/debug/build/serde-YYYY"
echo "out" > "$H3b1_BASE/debug/build/serde-YYYY/output"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H3b1_BASE" "$H3b1_LANE" --fresh-checkout
assert "H3b: no-match (only unlisted dirs) exits 0" test "$RC" -eq 0
assert "H3b: no-match: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "'"$H3b1_LANE/target"'" ]' _ "$OUT"

# Case 2: target/ has NO build/ dir at all
H3b2_BASE_PARENT="$(mktemp -d /tmp/test-seed-H3b2-parent-XXXXXX)"
H3b2_BASE="$H3b2_BASE_PARENT/target"
H3b2_LANE="$(mktemp -d /tmp/test-seed-H3b2-lane-XXXXXX)"
_TMPDIRS+=("$H3b2_BASE_PARENT" "$H3b2_LANE")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H3b2_BASE_PARENT/.warm-base-meta"
# Only a deps/ dir — no build/ dir at all
mkdir -p "$H3b2_BASE/debug/deps"
echo "libserde.rlib" > "$H3b2_BASE/debug/deps/libserde.rlib"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H3b2_BASE" "$H3b2_LANE" --fresh-checkout
assert "H3b: no-build-dir exits 0" test "$RC" -eq 0
assert "H3b: no-build-dir: STDOUT is exactly <lane>/target" \
    bash -c '[ "$1" = "'"$H3b2_LANE/target"'" ]' _ "$OUT"

# ── H3c: sibling non-build dirs untouched (deps/, .fingerprint/ preserved) ───
H3c_BASE_PARENT="$(mktemp -d /tmp/test-seed-H3c-parent-XXXXXX)"
H3c_BASE="$H3c_BASE_PARENT/target"
H3c_LANE="$(mktemp -d /tmp/test-seed-H3c-lane-XXXXXX)"
_TMPDIRS+=("$H3c_BASE_PARENT" "$H3c_LANE")
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$H3c_BASE_PARENT/.warm-base-meta"
mkdir -p "$H3c_BASE/debug/build/tauri-ZZZZ"
mkdir -p "$H3c_BASE/debug/deps"
mkdir -p "$H3c_BASE/debug/.fingerprint"
echo "out" > "$H3c_BASE/debug/build/tauri-ZZZZ/output"
echo "libserde.rlib" > "$H3c_BASE/debug/deps/libserde.rlib"
echo "fp" > "$H3c_BASE/debug/.fingerprint/serde-abc123"

reset_calls
RUSTFLAGS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper_real "$H3c_BASE" "$H3c_LANE" --fresh-checkout
assert "H3c: --fresh-checkout exits 0 (sibling dirs preserved)" test "$RC" -eq 0
assert "H3c: debug/deps PRESERVED (non-build sibling untouched)" \
    test -d "$H3c_LANE/target/debug/deps"
assert "H3c: debug/.fingerprint PRESERVED (non-build sibling untouched)" \
    test -d "$H3c_LANE/target/debug/.fingerprint"
assert "H3c: debug/build/tauri-ZZZZ GONE (allow-listed)" \
    bash -c '[ ! -e "'"$H3c_LANE/target/debug/build/tauri-ZZZZ"'" ]'

test_summary
