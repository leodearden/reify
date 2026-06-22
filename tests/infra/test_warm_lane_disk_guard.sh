#!/usr/bin/env bash
# tests/infra/test_warm_lane_disk_guard.sh
# Hermetic tests for scripts/warm-lane-disk-guard.sh.
#
# df stub:
#   Wired via REIFY_WARM_LANE_DISK_GUARD_DF env var (not via PATH).
#   Emits a 2-line df-like block from env-controlled vars:
#     REIFY_TEST_AVAIL_BYTES   — avail bytes to report (default: 107374182400 = 100 GiB)
#     REIFY_TEST_AVAIL_INODES  — avail inodes to report (default: 1000000)
#     REIFY_TEST_DF_FAIL       — set to 1 to exit non-zero (simulate df failure)
#     REIFY_TEST_DF_GARBAGE    — set to 1 to emit unparseable/non-integer output
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag, missing/unknown subcommand, missing mount
#   B — happy path: ample bytes AND inodes → exits 0, stdout empty
#   C1 — bytes below floor → exits 75
#   C2 — inodes below floor → exits 75
#   C3 — fail-closed measurement failure → exits 75
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-disk-guard.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-disk-guard.sh hermetic tests (task 4716) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-warm-lane-disk-guard-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

ERR_FILE="$(mktemp /tmp/test-warm-lane-disk-guard-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── df stub ────────────────────────────────────────────────────────────────────
# Pointed to by REIFY_WARM_LANE_DISK_GUARD_DF; mimics `df -B1 --output=avail,iavail`.
# Full-featured: supports all test scenarios from Block A through C3.
DF_STUB="$STUB_DIR/df_stub"
cat > "$DF_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
# df stub for warm-lane-disk-guard.sh tests
if [ "${REIFY_TEST_DF_FAIL:-}" = "1" ]; then
    echo "df: error: permission denied" >&2
    exit 1
fi
if [ "${REIFY_TEST_DF_GARBAGE:-}" = "1" ]; then
    printf '      Avail      IFree\n'
    printf 'not-an-integer not-an-integer\n'
    exit 0
fi
printf '      Avail      IFree\n'
printf ' %s %s\n' \
    "${REIFY_TEST_AVAIL_BYTES:-107374182400}" \
    "${REIFY_TEST_AVAIL_INODES:-1000000}"
STUB_EOF
chmod +x "$DF_STUB"

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes the script with the df stub wired via REIFY_WARM_LANE_DISK_GUARD_DF.
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
# Callers may prefix inline env vars (e.g. REIFY_TEST_AVAIL_BYTES=...) to
# control the stub; those are inherited by the subshell.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_WARM_LANE_DISK_GUARD_DF="$DF_STUB" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0 and prints usage on stderr
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits 2
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits 2" test "$RC" -eq 2

# A3: no subcommand (bare invocation) exits 2
run_helper
assert "A3: no subcommand exits 2" test "$RC" -eq 2

# A4: unknown subcommand exits 2
run_helper frobulate
assert "A4: unknown subcommand exits 2" test "$RC" -eq 2

# A5: check without mount (no REIFY_WARM_LANE_MOUNT, no --mount) exits 2
REIFY_WARM_LANE_MOUNT="" run_helper check
assert "A5: check without mount exits 2" test "$RC" -eq 2

# A6: --min-free-gib with no trailing value exits 2
run_helper check --mount /tmp --min-free-gib
assert "A6: --min-free-gib missing value exits 2" test "$RC" -eq 2

# A7: non-integer --min-free-gib (e.g. typo "50G") exits 2 — must be loud, not fail-open
run_helper check --mount /tmp --min-free-gib 50G --min-free-inodes 100000
assert "A7: non-integer --min-free-gib exits 2" test "$RC" -eq 2
assert "A7: non-integer --min-free-gib writes error to stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "integer\|invalid\|min.free.gib"' _ "$ERR_OUT"

# A8: non-integer REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB env exits 2
REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB=50G run_helper check --mount /tmp --min-free-inodes 100000
assert "A8: non-integer env MIN_FREE_GIB exits 2" test "$RC" -eq 2

# ──────────────────────────────────────────────────────────────────────────────
# Block B — happy path: ample bytes AND inodes → exits 0
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: happy path ---"

B_TMP="$(mktemp -d /tmp/test-warm-lane-disk-guard-b-XXXXXX)"
_TMPDIRS+=("$B_TMP")

# B1: ample bytes AND ample inodes, modest thresholds → exits 0
# 100 GiB bytes, 1M inodes; thresholds 10 GiB / 100k
REIFY_TEST_AVAIL_BYTES=107374182400 REIFY_TEST_AVAIL_INODES=1000000 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB=10 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES=100000 \
    run_helper check --mount "$B_TMP"
assert "B1: happy path exits 0" test "$RC" -eq 0

# B2: stdout is empty (all diagnostics on stderr)
assert "B2: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

# B3: stderr is non-empty (ok/info diagnostics)
assert "B3: stderr is non-empty" bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# B4: REIFY_WARM_LANE_MOUNT env var works (no --mount flag needed)
REIFY_TEST_AVAIL_BYTES=107374182400 REIFY_TEST_AVAIL_INODES=1000000 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB=10 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES=100000 \
    REIFY_WARM_LANE_MOUNT="$B_TMP" \
    run_helper check
assert "B4: env-var mount exits 0" test "$RC" -eq 0

# B5: --min-free-gib and --min-free-inodes flags are exercised directly (not just env vars)
# Regression guard for the flag parse branches (wrong shift, swapped var, etc.)
REIFY_TEST_AVAIL_BYTES=107374182400 REIFY_TEST_AVAIL_INODES=1000000 \
    run_helper check --mount "$B_TMP" --min-free-gib 10 --min-free-inodes 100000
assert "B5: flag-supplied thresholds exit 0" test "$RC" -eq 0
assert "B5: stdout is empty with flag thresholds" bash -c '[ -z "$1" ]' _ "$OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block C1 — free BYTES below floor → backpressure (exit 75)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C1: bytes below floor ---"

C1_TMP="$(mktemp -d /tmp/test-warm-lane-disk-guard-c1-XXXXXX)"
_TMPDIRS+=("$C1_TMP")

# C1a: tiny avail_bytes but ample inodes → exit 75, stderr names bytes shortfall
# 1 GiB available, threshold 10 GiB; inodes 1M >> 100k threshold
REIFY_TEST_AVAIL_BYTES=1073741824 REIFY_TEST_AVAIL_INODES=1000000 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB=10 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES=100000 \
    run_helper check --mount "$C1_TMP"
assert "C1a: bytes below floor exits 75" test "$RC" -eq 75
assert "C1a: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"
assert "C1a: stderr mentions bytes shortfall" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "bytes|GiB|space"' _ "$ERR_OUT"

# C1b: exactly at the floor (avail == min) → exit 0 (floor is exclusive lower bound)
# 10 GiB = 10737418240 bytes; threshold 10 GiB → should pass
REIFY_TEST_AVAIL_BYTES=10737418240 REIFY_TEST_AVAIL_INODES=1000000 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB=10 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES=100000 \
    run_helper check --mount "$C1_TMP"
assert "C1b: exactly at bytes floor exits 0" test "$RC" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block C2 — free INODES below floor → backpressure (exit 75)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C2: inodes below floor ---"

C2_TMP="$(mktemp -d /tmp/test-warm-lane-disk-guard-c2-XXXXXX)"
_TMPDIRS+=("$C2_TMP")

# C2a: ample bytes but tiny inodes → exit 75, stderr names inode shortfall
# 100 GiB bytes >> 10 GiB threshold; 50k inodes < 100k threshold
REIFY_TEST_AVAIL_BYTES=107374182400 REIFY_TEST_AVAIL_INODES=50000 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB=10 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES=100000 \
    run_helper check --mount "$C2_TMP"
assert "C2a: inodes below floor exits 75" test "$RC" -eq 75
assert "C2a: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"
assert "C2a: stderr mentions inode shortfall" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "inode"' _ "$ERR_OUT"

# C2b: both bytes AND inodes below floor → exit 75
REIFY_TEST_AVAIL_BYTES=1073741824 REIFY_TEST_AVAIL_INODES=50000 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB=10 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES=100000 \
    run_helper check --mount "$C2_TMP"
assert "C2b: both below floor exits 75" test "$RC" -eq 75
assert "C2b: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

# C2c: exactly at inodes floor (avail == min) → exit 0
REIFY_TEST_AVAIL_BYTES=107374182400 REIFY_TEST_AVAIL_INODES=100000 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB=10 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES=100000 \
    run_helper check --mount "$C2_TMP"
assert "C2c: exactly at inodes floor exits 0" test "$RC" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block C3 — fail-closed measurement failure → backpressure (exit 75)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C3: fail-closed measurement failure ---"

C3_TMP="$(mktemp -d /tmp/test-warm-lane-disk-guard-c3-XXXXXX)"
_TMPDIRS+=("$C3_TMP")

# C3a: df exits non-zero (REIFY_TEST_DF_FAIL=1) → exit 75, not a raw set -e death
REIFY_TEST_DF_FAIL=1 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB=10 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES=100000 \
    run_helper check --mount "$C3_TMP"
assert "C3a: df failure exits 75" test "$RC" -eq 75
assert "C3a: stdout is empty on df failure" bash -c '[ -z "$1" ]' _ "$OUT"
assert "C3a: stderr names df failure" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "df|health|fail|denied|admission"' _ "$ERR_OUT"

# C3b: df emits non-integer/unparseable output → exit 75
REIFY_TEST_DF_GARBAGE=1 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB=10 \
    REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES=100000 \
    run_helper check --mount "$C3_TMP"
assert "C3b: garbage df output exits 75" test "$RC" -eq 75
assert "C3b: stdout is empty on garbage output" bash -c '[ -z "$1" ]' _ "$OUT"
assert "C3b: stderr names parse failure" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "integer|parse|health|fail|denied|admission"' _ "$ERR_OUT"

test_summary
