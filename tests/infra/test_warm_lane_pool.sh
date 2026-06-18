#!/usr/bin/env bash
# tests/infra/test_warm_lane_pool.sh
# End-to-end integration gate for the warm-lane CoW pool mechanism.
# Task: #4662
#
# Architecture — two layers:
#
#   ALWAYS-RUN layer (no substrate needed, runs everywhere):
#     Block A  — script-presence / CLI-stability preconditions for all 4
#                warm-lane scripts (provision/seed/refresh/preflight).
#     Block FC — fail-closed wiring (B2 non-reflink-loud, B5 RUSTFLAGS-mismatch,
#                B5 preflight against unmounted mount) via the PATH-stub idiom.
#
#   SUBSTRATE-GATED real end-to-end layer (skips gracefully when no reflink
#   substrate or no cargo; runs on the provisioned host or with opt-in):
#     Block B3+B4 — warm-skip + path-independence (heavy dep fresh:true, B4 fresh
#                   count equality, B3 wall direction).
#     Block PS    — identical test pass-set warm vs cold.
#     Block B7    — reset-in-place stability over K cycles.
#     Block B6+B1 — lifecycle: in-flight clone independence + provision idempotency.
#
# Env knobs:
#   REIFY_WARM_LANE_MOUNT        — pre-existing XFS-reflink mount to use as
#                                  substrate (skips provision step).
#   REIFY_RUN_WARM_LANE_GATE     — set to 1 to opt-in to provisioning a small
#                                  ephemeral loopback via provision-warm-lane-fs.sh
#                                  when no mount is available.
#   REIFY_WARM_LANE_GATE_DEP_FNS — number of trivial fns in the heavy dep crate
#                                  (default: 200; tune for timing signal).
#   REIFY_WARM_LANE_GATE_RESET_CYCLES — number of reset-in-place cycles for B7
#                                  (default: 3).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== warm-lane pool end-to-end integration gate (task #4662) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Resolved paths for the four warm-lane scripts (systems-under-test; read-only)
# ─────────────────────────────────────────────────────────────────────────────
PROVISION_SCRIPT="$REPO_ROOT/scripts/provision-warm-lane-fs.sh"
SEED_SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"
REFRESH_SCRIPT="$REPO_ROOT/scripts/refresh-warm-base.sh"
PREFLIGHT_SCRIPT="$REPO_ROOT/scripts/warm-lane-preflight.sh"

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup trap
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# ─────────────────────────────────────────────────────────────────────────────
# PATH-stub infrastructure (reused from test_seed_warm_lane.sh)
# Used by Block FC to exercise the fail-closed guards of seed-warm-lane.sh
# without a real reflink substrate.
#
# Stubs record every invocation to CALLS_FILE. Behaviour knobs:
#   REIFY_TEST_REFLINK_OK   — cp stub: "1" → exit 0; else print error + exit 1
#   REIFY_TEST_GIT_DIFF_FILES — git stub: emitted on diff --name-only
#   REIFY_TEST_GIT_HEAD     — git stub: emitted on rev-parse HEAD
# ─────────────────────────────────────────────────────────────────────────────
STUB_DIR="$(mktemp -d /tmp/test-warm-pool-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-warm-pool-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-warm-pool-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# cp stub: record argv; REIFY_TEST_REFLINK_OK=1 → exit 0, else error + exit 1
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

# find stub: record argv, exit 0 (no-op; real mtime tests use real find)
cat > "$STUB_DIR/find" << 'STUB_EOF'
#!/usr/bin/env bash
echo "find $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/find"

# touch stub: record argv, exit 0 (no-op)
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
for arg in "$@"; do
    if [ "$arg" = "--name-only" ]; then
        printf "%s\n" "${REIFY_TEST_GIT_DIFF_FILES:-}"
        exit 0
    fi
done
for arg in "$@"; do
    if [ "$arg" = "rev-parse" ]; then
        echo "${REIFY_TEST_GIT_HEAD:-abc1234}"
        exit 0
    fi
done
exit 0
STUB_EOF
chmod +x "$STUB_DIR/git"

# run_helper — invoke SEED_SCRIPT under stub PATH; capture OUT/ERR_OUT/RC.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        PATH="$STUB_DIR:$PATH" \
            bash "$SEED_SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

reset_calls() { > "$CALLS_FILE"; }

# ─────────────────────────────────────────────────────────────────────────────
# Block A — Script-presence / CLI-stability preconditions (ALWAYS-RUN)
# Each of the 4 warm-lane scripts must exist as an executable, and --help must
# exit 0 and print "usage" or "Usage" on stderr.
# The verify-pipeline-infra-tests.txt map must contain a drift-guard row that
# routes a warm-lane script edit to this gate.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: script-presence / CLI-stability ---"

_VP_INFRA_MAP="$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt"

# ── A1: provision-warm-lane-fs.sh ────────────────────────────────────────────
assert "A1: provision-warm-lane-fs.sh exists and is executable" \
    test -x "$PROVISION_SCRIPT"
_A1_ERR="$(bash "$PROVISION_SCRIPT" --help 2>&1 >/dev/null)" || true
_A1_RC=0; bash "$PROVISION_SCRIPT" --help >/dev/null 2>&1 || _A1_RC=$?
assert "A1: provision-warm-lane-fs.sh --help exits 0" \
    test "$_A1_RC" -eq 0
assert "A1: provision-warm-lane-fs.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A1_ERR"

# ── A2: seed-warm-lane.sh ─────────────────────────────────────────────────────
assert "A2: seed-warm-lane.sh exists and is executable" \
    test -x "$SEED_SCRIPT"
_A2_ERR="$(bash "$SEED_SCRIPT" --help 2>&1 >/dev/null)" || true
_A2_RC=0; bash "$SEED_SCRIPT" --help >/dev/null 2>&1 || _A2_RC=$?
assert "A2: seed-warm-lane.sh --help exits 0" \
    test "$_A2_RC" -eq 0
assert "A2: seed-warm-lane.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A2_ERR"

# ── A3: refresh-warm-base.sh ──────────────────────────────────────────────────
assert "A3: refresh-warm-base.sh exists and is executable" \
    test -x "$REFRESH_SCRIPT"
_A3_ERR="$(bash "$REFRESH_SCRIPT" --help 2>&1 >/dev/null)" || true
_A3_RC=0; bash "$REFRESH_SCRIPT" --help >/dev/null 2>&1 || _A3_RC=$?
assert "A3: refresh-warm-base.sh --help exits 0" \
    test "$_A3_RC" -eq 0
assert "A3: refresh-warm-base.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A3_ERR"

# ── A4: warm-lane-preflight.sh ───────────────────────────────────────────────
assert "A4: warm-lane-preflight.sh exists and is executable" \
    test -x "$PREFLIGHT_SCRIPT"
_A4_ERR="$(bash "$PREFLIGHT_SCRIPT" --help 2>&1 >/dev/null)" || true
_A4_RC=0; bash "$PREFLIGHT_SCRIPT" --help >/dev/null 2>&1 || _A4_RC=$?
assert "A4: warm-lane-preflight.sh --help exits 0" \
    test "$_A4_RC" -eq 0
assert "A4: warm-lane-preflight.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A4_ERR"

# ── A5: drift-guard map contains a row for a warm-lane script → this gate ────
# At least one row in verify-pipeline-infra-tests.txt must map a warm-lane
# script artifact to a glob that matches tests/infra/test_warm_lane_pool.sh.
# This ensures that a future edit to provision/seed/refresh/preflight will
# re-exercise this integration gate at task-scope verify time.
assert "A5: verify-pipeline-infra-tests.txt exists" \
    test -f "$_VP_INFRA_MAP"
assert "A5: drift-guard map has a warm-lane-script → test_warm_lane_pool.sh row" \
    bash -c '
        map="$1"
        # Look for any non-comment row whose artifact column matches a warm-lane script
        # and whose test-glob column would fnmatch tests/infra/test_warm_lane_pool.sh.
        while IFS= read -r line; do
            [[ "$line" =~ ^[[:space:]]*# ]] && continue
            [[ -z "${line// }" ]] && continue
            artifact=$(awk "{print \$1}" <<< "$line")
            glob=$(awk "{print \$2}" <<< "$line")
            case "$artifact" in
                scripts/*warm-lane*.sh|scripts/*warm_lane*.sh|scripts/provision-warm-lane-fs.sh|\
scripts/seed-warm-lane.sh|scripts/refresh-warm-base.sh|scripts/warm-lane-preflight.sh) ;;
                *) continue ;;
            esac
            # Check if the glob matches this gate file
            case "tests/infra/test_warm_lane_pool.sh" in
                $glob) exit 0 ;;
            esac
        done < "$map"
        exit 1
    ' _ "$_VP_INFRA_MAP"

# ─────────────────────────────────────────────────────────────────────────────
# Block FC — Fail-closed wiring (ALWAYS-RUN; no real substrate needed)
#
# Exercises the integration-level guards via the PATH-stub idiom reused from
# test_seed_warm_lane.sh:  STUB_DIR with cp/find/touch/git stubs recording
# argv to CALLS_FILE, run_helper capturing OUT/ERR_OUT/RC separately.
#
# Stubs + run_helper + reset_calls are defined in impl-failclosed (impl step).
# Referencing them here without prior definition → immediate error under
# set -euo pipefail → RED until the impl step defines them.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block FC: fail-closed wiring (B5/B2/preflight) ---"

# ── FC fixture: a base dir whose .warm-base-meta records a DIFFERENT RUSTFLAGS
FC_BASE_PARENT="$(mktemp -d /tmp/test-warm-pool-FC-base-XXXXXX)"
FC_BASE="$FC_BASE_PARENT/target"
FC_LANE="$(mktemp -d /tmp/test-warm-pool-FC-lane-XXXXXX)"
_TMPDIRS+=("$FC_BASE_PARENT" "$FC_LANE")
mkdir -p "$FC_BASE"
cat > "$FC_BASE_PARENT/.warm-base-meta" <<'SIDECAR_EOF'
RUSTFLAGS=original-flags
INVOCATION=
SIDECAR_EOF

# ── FC1: B5 — RUSTFLAGS mismatch → non-zero exit, actionable stderr, empty stdout, cp not called
reset_calls
RUSTFLAGS="different-flags" run_helper "$FC_BASE" "$FC_LANE" --fresh-checkout
assert "FC1: RUSTFLAGS mismatch exits non-zero (B5)" test "$RC" -ne 0
assert "FC1: stderr names RUSTFLAGS mismatch (actionable)" \
    bash -c 'printf "%s\n" "$1" | grep -qi "RUSTFLAGS"' _ "$ERR_OUT"
assert "FC1: STDOUT empty on RUSTFLAGS mismatch (fail-closed)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "FC1: cp never invoked on RUSTFLAGS mismatch (guard fires first)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# ── FC2: B2 — reflink-failure → non-zero exit with actionable message
FC_LANE2="$(mktemp -d /tmp/test-warm-pool-FC-lane2-XXXXXX)"
_TMPDIRS+=("$FC_LANE2")
reset_calls
RUSTFLAGS="original-flags" REIFY_TEST_REFLINK_OK=0 \
    run_helper "$FC_BASE" "$FC_LANE2" --fresh-checkout
assert "FC2: cp failure (non-reflink FS) exits non-zero (B2)" test "$RC" -ne 0
assert "FC2: stderr names reflink failure (actionable)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "reflink|Operation not supported"' _ "$ERR_OUT"

# ── FC3: preflight — unmounted mount → non-zero exit with actionable hint
FC_FAKE_MOUNT="$(mktemp -d /tmp/test-warm-pool-FC-mnt-XXXXXX)"
_TMPDIRS+=("$FC_FAKE_MOUNT")
# The fake mount dir exists but is NOT a real mountpoint → preflight check 1 fails.
FC_PF_RC=0
bash "$PREFLIGHT_SCRIPT" --mount "$FC_FAKE_MOUNT" 2>/dev/null || FC_PF_RC=$?
assert "FC3: preflight fails on unmounted dir (non-zero)" test "$FC_PF_RC" -ne 0
FC_PF_ERR="$(bash "$PREFLIGHT_SCRIPT" --mount "$FC_FAKE_MOUNT" 2>&1 >/dev/null)" || true
assert "FC3: preflight stderr names mount/provision remediation (actionable)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "mount|provision"' _ "$FC_PF_ERR"

# ─────────────────────────────────────────────────────────────────────────────
# Block SG — Substrate detector + skip path (ALWAYS-RUN)
#
# Unit-tests detect_substrate() and _skip() which are defined in the
# impl-substrate-gate step. Until then, placeholder values make every
# assertion RED.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block SG: substrate detection + skip path ---"

# Placeholder values that make the assertions fail (RED) until impl wires them.
# impl-substrate-gate replaces these with real detect_substrate/_skip calls.
_SG_DETECT_NO_SUB_RC=0    # detect_substrate should return non-zero (no substrate)
_SG_DETECT_WITH_SUB_RC=1  # detect_substrate should return 0 (substrate present)
_SG_SKIP_RC=1              # _skip should exit 0
_SG_SKIP_ERR=""            # _skip should emit "SKIP" on stderr
_SG_CARGO_MISS_RC=0       # command -v cargo in empty PATH must return non-zero

assert "SG1: detect_substrate returns non-zero when no substrate available" \
    test "$_SG_DETECT_NO_SUB_RC" -ne 0
assert "SG2: detect_substrate returns 0 when valid mount+reflink provided" \
    test "$_SG_DETECT_WITH_SUB_RC" -eq 0
assert "SG3: _skip exits 0 (graceful skip, not hard abort)" \
    test "$_SG_SKIP_RC" -eq 0
assert "SG3: _skip emits a SKIP line on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "SKIP"' _ "$_SG_SKIP_ERR"
assert "SG4: gate detects absent cargo (command -v cargo in empty PATH)" \
    test "$_SG_CARGO_MISS_RC" -ne 0

# ─────────────────────────────────────────────────────────────────────────────
test_summary
