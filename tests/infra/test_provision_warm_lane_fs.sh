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


# ──────────────────────────────────────────────────────────────────────────────
# Block D — Idempotent no-op (boundary B1 / invariant P1): already mounted
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: idempotent no-op (B1/P1) ---"

D_TMP="$(mktemp -d /tmp/test-warm-lane-d-XXXXXX)"
_TMPDIRS+=("$D_TMP")
D_IMG="$D_TMP/img"
D_MNT="$D_TMP/mnt"
mkdir -p "$D_MNT"
# Simulate: img exists (second run) and is mounted
touch "$D_IMG"

# D1: idempotent no-op exits 0
reset_calls
REIFY_TEST_MOUNTED=1 REIFY_TEST_IMG_XFS="" REIFY_TEST_REFLINK_OK=1 \
    run_helper --img "$D_IMG" --mount "$D_MNT"
assert "D1: idempotent no-op exits 0" test "$RC" -eq 0

# D2: STDOUT is exactly the mount path
assert "D2: idempotent STDOUT is exactly the mount path" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$D_MNT"

# D3: NO mkfs.xfs (never reformat)
assert "D3: idempotent no-op: NO mkfs.xfs called" \
    bash -c '! grep -q "^mkfs.xfs" "$1"' _ "$CALLS_FILE"

# D4: NO fallocate (no re-allocation)
assert "D4: idempotent no-op: NO fallocate called" \
    bash -c '! grep -q "^fallocate" "$1"' _ "$CALLS_FILE"

# D5: cp --reflink=always probe STILL ran (re-verify even on idempotent path)
assert "D5: idempotent no-op: cp --reflink=always probe still ran" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"


# ──────────────────────────────────────────────────────────────────────────────
# Block E — P1 deep: existing populated image (XFS magic), unmounted
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: P1 deep — existing XFS image, unmounted ---"

E_TMP="$(mktemp -d /tmp/test-warm-lane-e-XXXXXX)"
_TMPDIRS+=("$E_TMP")
E_IMG="$E_TMP/img"
E_MNT="$E_TMP/mnt"
mkdir -p "$E_MNT"
# Simulate: img exists with XFS magic but is NOT mounted
touch "$E_IMG"

# E1: re-attach+mount existing XFS image exits 0
reset_calls
REIFY_TEST_MOUNTED="" REIFY_TEST_IMG_XFS=1 REIFY_TEST_REFLINK_OK=1 \
    run_helper --img "$E_IMG" --mount "$E_MNT"
assert "E1: re-attach existing XFS image exits 0" test "$RC" -eq 0

# E2: STDOUT is exactly the mount path
assert "E2: STDOUT is exactly the mount path" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$E_MNT"

# E3: NO mkfs.xfs (P1: never reformat a populated image)
assert "E3: P1 — NO mkfs.xfs for existing XFS image" \
    bash -c '! grep -q "^mkfs.xfs" "$1"' _ "$CALLS_FILE"

# E4: NO fallocate (P1: no re-allocation)
assert "E4: P1 — NO fallocate for existing XFS image" \
    bash -c '! grep -q "^fallocate" "$1"' _ "$CALLS_FILE"

# E5: losetup WAS invoked (re-attach the loop device)
assert "E5: losetup was invoked (re-attach existing image)" \
    bash -c 'grep -q "^losetup" "$1"' _ "$CALLS_FILE"

# E6: mount WAS invoked (re-mount the loop device)
assert "E6: mount was invoked (re-mount existing image)" \
    bash -c 'grep "^mount" "$1" | grep -qF "'"$E_MNT"'"' _ "$CALLS_FILE"

# E7: cp --reflink=always probe ran
assert "E7: cp --reflink=always probe ran after re-mount" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"


# ──────────────────────────────────────────────────────────────────────────────
# Block F — setup-dev.sh wiring (structural grep)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: setup-dev.sh wiring ---"

# F1: setup-dev.sh references provision-warm-lane-fs.sh
assert "F1: setup-dev.sh references provision-warm-lane-fs.sh" \
    bash -c 'grep -q "provision-warm-lane-fs.sh" "$1"' _ "$SETUP_DEV"

# F2: the invocation is gated on REIFY_PROVISION_WARM_LANES opt-in
assert "F2: invocation gated on REIFY_PROVISION_WARM_LANES" \
    bash -c 'grep -q "REIFY_PROVISION_WARM_LANES" "$1"' _ "$SETUP_DEV"

# F3: the call is non-fatal (guarded; a provisioning failure does not abort setup-dev)
# Check that the warm-lane section uses || (non-fatal) or does not bare-exit-1 on failure.
# We assert that the section around provision-warm-lane-fs.sh does NOT have a
# bare `exit 1` immediately after the invocation (it must be guarded with || or similar).
assert "F3: warm-lane provisioning call is non-fatal (uses || guard)" \
    bash -c 'grep -A5 "provision-warm-lane-fs.sh" "$1" | grep -qE "\|\||warn|true"' _ "$SETUP_DEV"

test_summary
