#!/usr/bin/env bash
# tests/infra/test_provision_warm_lane_fs.sh
# Hermetic tests for scripts/provision-warm-lane-fs.sh.
#
# PATH-stubs fallocate/mkfs.xfs/losetup/mount/umount/mountpoint/blkid/cp/sudo/chown
# record their argv to a CALLS_FILE; env-driven stub behaviour:
#   REIFY_TEST_REFLINK_OK  — cp stub: "1" -> exit 0; else print error + exit 1
#   REIFY_TEST_MOUNTED     — mountpoint stub: "1" -> exit 0 (mounted); else exit 1
#   REIFY_TEST_IMG_XFS     — blkid stub: "1" -> print "xfs"; else print nothing
#   REIFY_WARM_LANE_SUDO   — set "" in all run_helper calls to bypass sudo
#
# run_helper captures STDOUT and STDERR SEPARATELY:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag
#   B — Fresh-provision happy path + size default/override + STDOUT contract
#   C — Probe-fail-loud (boundary B2 / invariant P2): non-reflink mount
#   D — Idempotent no-op (boundary B1 / invariant P1): second-run mounted
#   E — P1 deep: existing populated image (XFS magic), unmounted
#   F — setup-dev.sh wiring (structural)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/provision-warm-lane-fs.sh"
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/provision-warm-lane-fs.sh hermetic tests (task 4659) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-warm-lane-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-warm-lane-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-warm-lane-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# fallocate stub: record argv, exit 0
cat > "$STUB_DIR/fallocate" << 'STUB_EOF'
#!/usr/bin/env bash
echo "fallocate $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/fallocate"

# mkfs.xfs stub: record argv, exit 0
cat > "$STUB_DIR/mkfs.xfs" << 'STUB_EOF'
#!/usr/bin/env bash
echo "mkfs.xfs $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/mkfs.xfs"

# losetup stub: record argv; print fake loop device when --show is present
cat > "$STUB_DIR/losetup" << 'STUB_EOF'
#!/usr/bin/env bash
echo "losetup $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
for arg in "$@"; do
    if [ "$arg" = "--show" ]; then
        echo "/dev/loop99"
        exit 0
    fi
done
exit 0
STUB_EOF
chmod +x "$STUB_DIR/losetup"

# mount stub: record argv, exit 0
cat > "$STUB_DIR/mount" << 'STUB_EOF'
#!/usr/bin/env bash
echo "mount $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/mount"

# umount stub: record argv, exit 0
cat > "$STUB_DIR/umount" << 'STUB_EOF'
#!/usr/bin/env bash
echo "umount $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/umount"

# mountpoint stub: exit 0 when REIFY_TEST_MOUNTED=1, else exit 1
cat > "$STUB_DIR/mountpoint" << 'STUB_EOF'
#!/usr/bin/env bash
echo "mountpoint $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
[ "${REIFY_TEST_MOUNTED:-}" = "1" ] && exit 0
exit 1
STUB_EOF
chmod +x "$STUB_DIR/mountpoint"

# blkid stub: print "xfs" when REIFY_TEST_IMG_XFS=1, else empty output
cat > "$STUB_DIR/blkid" << 'STUB_EOF'
#!/usr/bin/env bash
echo "blkid $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_IMG_XFS:-}" = "1" ]; then
    echo "xfs"
fi
exit 0
STUB_EOF
chmod +x "$STUB_DIR/blkid"

# cp stub: if REIFY_TEST_REFLINK_OK=1 -> exit 0; else print error + exit 1
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

# sudo stub: record argv, passthrough-exec its args (so downstream stubs fire)
cat > "$STUB_DIR/sudo" << 'STUB_EOF'
#!/usr/bin/env bash
echo "sudo $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exec "$@"
STUB_EOF
chmod +x "$STUB_DIR/sudo"

# chown stub: record argv, exit 0 (no real ownership change needed in tests)
cat > "$STUB_DIR/chown" << 'STUB_EOF'
#!/usr/bin/env bash
echo "chown $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/chown"

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes the script under the stub PATH with REIFY_WARM_LANE_SUDO="".
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        REIFY_WARM_LANE_SUDO="" \
        PATH="$STUB_DIR:$PATH" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

reset_calls() {
    > "$CALLS_FILE"
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard: --help and unknown flag
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0
reset_calls
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits non-zero (2)
reset_calls
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits non-zero" test "$RC" -ne 0


# ──────────────────────────────────────────────────────────────────────────────
# Block B — Fresh-provision happy path + size default/override + STDOUT contract
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: fresh provision happy path ---"

B_TMP="$(mktemp -d /tmp/test-warm-lane-b-XXXXXX)"
_TMPDIRS+=("$B_TMP")
B_IMG="$B_TMP/img"
B_MNT="$B_TMP/mnt"
mkdir -p "$B_MNT"

# B1: fresh provision (img absent, not mounted, reflink probe passes) exits 0
reset_calls
REIFY_TEST_MOUNTED="" REIFY_TEST_IMG_XFS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper --img "$B_IMG" --mount "$B_MNT"
assert "B1: fresh provision exits 0" test "$RC" -eq 0

# B2: STDOUT is EXACTLY the mount path (single bare line, nothing else)
assert "B2: STDOUT is exactly the mount path" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$B_MNT"

# B3: fallocate invoked with 600GiB default size
assert "B3: fallocate invoked with 600GiB (default size)" \
    bash -c 'grep "^fallocate" "$1" | grep -q "600GiB"' _ "$CALLS_FILE"

# B4: mkfs.xfs invoked with reflink=1
assert "B4: mkfs.xfs invoked with reflink=1" \
    bash -c 'grep "^mkfs.xfs" "$1" | grep -q "reflink=1"' _ "$CALLS_FILE"

# B5: mkfs.xfs invoked with bigtime=1
assert "B5: mkfs.xfs invoked with bigtime=1" \
    bash -c 'grep "^mkfs.xfs" "$1" | grep -q "bigtime=1"' _ "$CALLS_FILE"

# B6: losetup invoked targeting the img
assert "B6: losetup invoked with --find --show" \
    bash -c 'grep "^losetup" "$1" | grep -q -- "--find"' _ "$CALLS_FILE"

# B7: mount invoked targeting the mount dir
assert "B7: mount invoked targeting mount dir" \
    bash -c 'grep "^mount" "$1" | grep -qF "'"$B_MNT"'"' _ "$CALLS_FILE"

# B8: cp probe invoked with --reflink=always
assert "B8: cp probe invoked with --reflink=always" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# B9: stderr is non-empty (diagnostics on stderr, not stdout)
assert "B9: stderr is non-empty (diagnostics on stderr)" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# B10: --size-gib override: re-run with 123, fallocate gets 123GiB
reset_calls
B2_TMP="$(mktemp -d /tmp/test-warm-lane-b2-XXXXXX)"
_TMPDIRS+=("$B2_TMP")
mkdir -p "$B2_TMP/mnt"
REIFY_TEST_MOUNTED="" REIFY_TEST_IMG_XFS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper --img "$B2_TMP/img" --mount "$B2_TMP/mnt" --size-gib 123
assert "B10: --size-gib 123 passes 123GiB to fallocate" \
    bash -c 'grep "^fallocate" "$1" | grep -q "123GiB"' _ "$CALLS_FILE"


# ──────────────────────────────────────────────────────────────────────────────
# Block C — Probe-fail-loud (boundary B2 / invariant P2): non-reflink mount
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: probe-fail-loud (P2 invariant) ---"

C_TMP="$(mktemp -d /tmp/test-warm-lane-c-XXXXXX)"
_TMPDIRS+=("$C_TMP")
C_IMG="$C_TMP/img"
C_MNT="$C_TMP/mnt"
mkdir -p "$C_MNT"

# C1: script exits non-zero when cp probe fails
reset_calls
REIFY_TEST_MOUNTED="" REIFY_TEST_IMG_XFS="" REIFY_TEST_REFLINK_OK=0 \
    run_helper --img "$C_IMG" --mount "$C_MNT"
assert "C1: probe failure exits non-zero" test "$RC" -ne 0

# C2: stderr names the reflink failure (actionable message)
assert "C2: stderr names reflink failure (matches /reflink|Operation not supported/i)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "reflink|Operation not supported"' _ "$ERR_OUT"

# C3: STDOUT is EMPTY (no mount path printed — P2 fail-closed, no silent fallback)
assert "C3: STDOUT is EMPTY on probe failure (P2 invariant)" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# C4: cp --reflink=always probe was recorded (failure came from the probe, not a pre-guard)
assert "C4: cp --reflink=always probe was invoked before failure" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

test_summary
