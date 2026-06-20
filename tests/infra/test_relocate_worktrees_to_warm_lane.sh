#!/usr/bin/env bash
# tests/infra/test_relocate_worktrees_to_warm_lane.sh
# Hermetic tests for scripts/relocate-worktrees-to-warm-lane.sh.
#
# PATH-stubs cp record argv to CALLS_FILE; env-driven stub behaviour:
#   REIFY_TEST_REFLINK_OK  — cp stub: "1" -> exit 0; else print error + exit 1
#
# run_helper captures STDOUT and STDERR SEPARATELY:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: file exists + --help, unknown flag, nonexistent mount
#   B — Fresh happy path: no .worktrees yet → creates symlink, stdout=DEST
#   C — Probe fail-loud: non-reflink mount → exits non-zero, no symlink
#   D — Idempotency: symlink already correct → no-op; wrong target → refuses
#   E — Migration: real directory with contents → mv to mount, symlink created
#   F — Real-git end-to-end acceptance (user-observable signal)
#   H — orchestrator.yaml config contract (PyYAML-guarded)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/relocate-worktrees-to-warm-lane.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/relocate-worktrees-to-warm-lane.sh hermetic tests (task 4696) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-relocate-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-relocate-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-relocate-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

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

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard: script exists, --help, unknown flag, nonexistent mount
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: script exists and is executable
assert "A1: script exists" test -f "$SCRIPT"
assert "A1: script is executable" test -x "$SCRIPT"

# A2: --help exits 0 and prints usage to stderr
reset_calls
run_helper --help
assert "A2: --help exits 0" test "$RC" -eq 0
assert "A2: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A3: unknown flag exits 2
reset_calls
run_helper --unknown-flag-xyz
assert "A3: unknown flag exits 2" test "$RC" -eq 2

# A4: --mount pointing at nonexistent directory exits non-zero
#     with an actionable message mentioning provision
A_TMP="$(mktemp -d /tmp/test-relocate-a-XXXXXX)"
_TMPDIRS+=("$A_TMP")
reset_calls
REIFY_TEST_REFLINK_OK=1 \
    run_helper --repo "$A_TMP" --mount "$A_TMP/nonexistent-mount-dir"
assert "A4: nonexistent mount exits non-zero" test "$RC" -ne 0
assert "A4: nonexistent mount stderr mentions 'provision'" \
    bash -c 'printf "%s\n" "$1" | grep -qi "provision"' _ "$ERR_OUT"


# ──────────────────────────────────────────────────────────────────────────────
# Block B — Fresh happy path: no .worktrees yet → symlink created, stdout=DEST
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: fresh happy path ---"

B_TMP="$(mktemp -d /tmp/test-relocate-b-XXXXXX)"
_TMPDIRS+=("$B_TMP")
B_REPO="$B_TMP/repo"
B_MNT="$B_TMP/mnt"
mkdir -p "$B_REPO" "$B_MNT"

# B1: fresh case (no .worktrees, reflink ok) exits 0
reset_calls
REIFY_TEST_REFLINK_OK=1 \
    run_helper --repo "$B_REPO" --mount "$B_MNT"
assert "B1: fresh case exits 0" test "$RC" -eq 0

# B2: <repo>/.worktrees is now a symlink
assert "B2: .worktrees is a symlink" test -L "$B_REPO/.worktrees"

# B3: symlink target is <mount>/worktrees
assert "B3: symlink target is <mount>/worktrees" \
    bash -c '[ "$(readlink -f "$1")" = "$(readlink -f "$2")" ]' \
    _ "$B_REPO/.worktrees" "$B_MNT/worktrees"

# B4: <mount>/worktrees directory exists
assert "B4: <mount>/worktrees exists" test -d "$B_MNT/worktrees"

# B5: stdout is exactly the <mount>/worktrees path
assert "B5: stdout is exactly <mount>/worktrees path" \
    bash -c '[ "$1" = "$2/worktrees" ]' _ "$OUT" "$B_MNT"

# B6: stderr is non-empty (diagnostics emitted)
assert "B6: stderr is non-empty (diagnostics)" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# B7: cp --reflink=always probe was recorded
assert "B7: cp --reflink=always probe invoked" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"


# ──────────────────────────────────────────────────────────────────────────────
# Block C — Probe fail-loud: non-reflink mount → exits non-zero, no symlink
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: probe fail-loud (P2 invariant) ---"

C_TMP="$(mktemp -d /tmp/test-relocate-c-XXXXXX)"
_TMPDIRS+=("$C_TMP")
C_REPO="$C_TMP/repo"
C_MNT="$C_TMP/mnt"
mkdir -p "$C_REPO" "$C_MNT"

# C1: exits non-zero when cp probe fails
reset_calls
REIFY_TEST_REFLINK_OK=0 \
    run_helper --repo "$C_REPO" --mount "$C_MNT"
assert "C1: probe failure exits non-zero" test "$RC" -ne 0

# C2: stderr names reflink failure (actionable message)
assert "C2: stderr names reflink failure (matches /reflink|Operation not supported/i)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "reflink|Operation not supported"' _ "$ERR_OUT"

# C3: NO symlink was created (fail-closed, no silent fallback)
assert "C3: no symlink created on probe failure (fail-closed)" \
    bash -c '! test -L "$1/.worktrees"' _ "$C_REPO"

# C4: cp --reflink=always probe was invoked (failure came from probe, not a pre-guard)
assert "C4: cp --reflink=always probe was invoked before failure" \
    bash -c 'grep "^cp" "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"


# ──────────────────────────────────────────────────────────────────────────────
# Block D — Idempotency + wrong-target guard
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: idempotency and wrong-target guard ---"

D_TMP="$(mktemp -d /tmp/test-relocate-d-XXXXXX)"
_TMPDIRS+=("$D_TMP")
D_REPO="$D_TMP/repo"
D_MNT="$D_TMP/mnt"
mkdir -p "$D_REPO" "$D_MNT"

# D-idempotent: pre-create <repo>/.worktrees as symlink already → <mount>/worktrees
D_DEST="$D_MNT/worktrees"
mkdir -p "$D_DEST"
ln -s "$D_DEST" "$D_REPO/.worktrees"

# D1: second run exits 0
reset_calls
REIFY_TEST_REFLINK_OK=1 \
    run_helper --repo "$D_REPO" --mount "$D_MNT"
assert "D1: idempotent run exits 0" test "$RC" -eq 0

# D2: symlink unchanged (still → <mount>/worktrees)
assert "D2: symlink still points to <mount>/worktrees" \
    bash -c '[ "$(readlink -f "$1")" = "$(readlink -f "$2")" ]' \
    _ "$D_REPO/.worktrees" "$D_DEST"

# D3: stdout is still the DEST path
assert "D3: idempotent stdout is DEST path" \
    bash -c '[ "$1" = "$2" ]' _ "$OUT" "$D_DEST"

# D4: no destructive operation — symlink type unchanged
assert "D4: .worktrees is still a symlink (no destructive op)" \
    test -L "$D_REPO/.worktrees"


# D-wrong-target: symlink pointing at DIFFERENT dir → refuse to clobber
D_OTHER="$D_TMP/other-dir"
mkdir -p "$D_OTHER"
D_REPO2="$D_TMP/repo2"
mkdir -p "$D_REPO2"
ln -s "$D_OTHER" "$D_REPO2/.worktrees"

# D5: exits non-zero when symlink points elsewhere
reset_calls
REIFY_TEST_REFLINK_OK=1 \
    run_helper --repo "$D_REPO2" --mount "$D_MNT"
assert "D5: wrong-target symlink exits non-zero" test "$RC" -ne 0

# D6: stderr says "refusing to clobber"
assert "D6: stderr says 'refusing to clobber' (or similar)" \
    bash -c 'printf "%s\n" "$1" | grep -qi "refus"' _ "$ERR_OUT"

# D7: symlink is LEFT UNTOUCHED (still → D_OTHER)
assert "D7: wrong-target symlink is left untouched" \
    bash -c '[ "$(readlink -f "$1")" = "$(readlink -f "$2")" ]' \
    _ "$D_REPO2/.worktrees" "$D_OTHER"


# ──────────────────────────────────────────────────────────────────────────────
# Block E — Migration of a real directory with contents
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: directory migration ---"

E_TMP="$(mktemp -d /tmp/test-relocate-e-XXXXXX)"
_TMPDIRS+=("$E_TMP")
E_REPO="$E_TMP/repo"
E_MNT="$E_TMP/mnt"
mkdir -p "$E_REPO" "$E_MNT"

# Pre-create .worktrees as a real directory with two entries
mkdir -p "$E_REPO/.worktrees/_merge-verify/target"
echo "marker" > "$E_REPO/.worktrees/_merge-verify/target/cache-marker"
mkdir -p "$E_REPO/.worktrees/other-worktree"
echo "other" > "$E_REPO/.worktrees/other-worktree/data"

# E1: migration exits 0
reset_calls
REIFY_TEST_REFLINK_OK=1 \
    run_helper --repo "$E_REPO" --mount "$E_MNT"
assert "E1: migration exits 0" test "$RC" -eq 0

# E2: <repo>/.worktrees is now a symlink
assert "E2: .worktrees is now a symlink after migration" \
    test -L "$E_REPO/.worktrees"

# E3: symlink target is <mount>/worktrees
assert "E3: symlink target is <mount>/worktrees" \
    bash -c '[ "$(readlink -f "$1")" = "$(readlink -f "$2/worktrees")" ]' \
    _ "$E_REPO/.worktrees" "$E_MNT"

# E4: _merge-verify is under the mount (entries moved)
assert "E4: _merge-verify marker resolves under <mount>" \
    bash -c 'path="$(readlink -f "$1/.worktrees/_merge-verify/target/cache-marker")"; [[ "$path" == "$2"/* ]]' \
    _ "$E_REPO" "$E_MNT"

# E5: other-worktree is also under the mount
assert "E5: other-worktree data resolves under <mount>" \
    bash -c 'path="$(readlink -f "$1/.worktrees/other-worktree/data")"; [[ "$path" == "$2"/* ]]' \
    _ "$E_REPO" "$E_MNT"

# E6: the original real directory no longer exists as a directory
assert "E6: original real directory removed (replaced by symlink)" \
    bash -c '! { [ ! -L "$1/.worktrees" ] && [ -d "$1/.worktrees" ]; }' _ "$E_REPO"


# E-collision: when <mount>/worktrees/<name> already exists → exit non-zero
E2_TMP="$(mktemp -d /tmp/test-relocate-e2-XXXXXX)"
_TMPDIRS+=("$E2_TMP")
E2_REPO="$E2_TMP/repo"
E2_MNT="$E2_TMP/mnt"
mkdir -p "$E2_REPO" "$E2_MNT"

# Create .worktrees as a real directory
mkdir -p "$E2_REPO/.worktrees/_merge-verify"
# Pre-existing collision: same name already in <mount>/worktrees/
mkdir -p "$E2_MNT/worktrees/_merge-verify"

# E7: collision → exits non-zero
reset_calls
REIFY_TEST_REFLINK_OK=1 \
    run_helper --repo "$E2_REPO" --mount "$E2_MNT"
assert "E7: collision exits non-zero" test "$RC" -ne 0

# E8: stderr contains collision message
assert "E8: stderr names collision" \
    bash -c 'printf "%s\n" "$1" | grep -qi "collision\|already exist\|clobber"' _ "$ERR_OUT"


# ──────────────────────────────────────────────────────────────────────────────
# Block F — Real-git end-to-end acceptance (user-observable signal)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: real-git end-to-end acceptance ---"

F_TMP="$(mktemp -d /tmp/test-relocate-f-XXXXXX)"
_TMPDIRS+=("$F_TMP")
F_REPO="$F_TMP/repo"
F_MNT="$F_TMP/mnt"
mkdir -p "$F_REPO" "$F_MNT"

# Build a throwaway git repo seeded with the REAL .gitignore
git -C "$F_REPO" init -q -b main
git -C "$F_REPO" config user.email test@test.com
git -C "$F_REPO" config user.name Test
cp "$REPO_ROOT/.gitignore" "$F_REPO/.gitignore"
echo "base" > "$F_REPO/base.txt"
git -C "$F_REPO" add .gitignore base.txt
git -C "$F_REPO" commit -q -m "base commit"

# Create a real git worktree inside .worktrees/
mkdir -p "$F_REPO/.worktrees"
git -C "$F_REPO" worktree add -q "$F_REPO/.worktrees/_merge-verify" -b _merge-verify

# Run relocate with cp stub ok
reset_calls
REIFY_TEST_REFLINK_OK=1 \
    run_helper --repo "$F_REPO" --mount "$F_MNT"

# F1: exits 0
assert "F1: relocate exits 0" test "$RC" -eq 0

# F1b: stdout is EXACTLY "<mount>/worktrees" — one bare line, no extra output.
# This guards the stdout contract against git worktree repair emitting to stdout
# (the script now redirects repair's stdout to stderr defensively).
assert "F1b: stdout is exactly <mount>/worktrees (stdout contract)" \
    bash -c '[ "$1" = "$2/worktrees" ]' _ "$OUT" "$F_MNT"

# F2: .worktrees is a symlink → <mount>/worktrees
assert "F2: .worktrees is a symlink" test -L "$F_REPO/.worktrees"
assert "F3: symlink target is <mount>/worktrees" \
    bash -c '[ "$(readlink -f "$1")" = "$(readlink -f "$2/worktrees")" ]' \
    _ "$F_REPO/.worktrees" "$F_MNT"

# F4: _merge-verify resolves under <mount>
assert "F4: _merge-verify physical path resolves under <mount>" \
    bash -c '[[ "$(readlink -f "$1/.worktrees/_merge-verify")" == "$2"/* ]]' \
    _ "$F_REPO" "$F_MNT"

# F5: git worktree list still lists _merge-verify
assert "F5: git worktree list still lists _merge-verify" \
    bash -c 'git -C "$1" worktree list 2>&1 | grep -q "_merge-verify"' _ "$F_REPO"

# F6: git status in _merge-verify worktree exits 0
assert "F6: git status in _merge-verify worktree exits 0" \
    bash -c 'git -C "$1/.worktrees/_merge-verify" status >/dev/null 2>&1' _ "$F_REPO"

# F7: git status --porcelain is EMPTY (symlink ignored — land.sh clean-tree gate)
# RED until step-10 drops trailing slash from .gitignore
assert "F7: git status --porcelain empty (land.sh clean-tree gate)" \
    bash -c '[ -z "$(git -C "$1" status --porcelain 2>&1)" ]' _ "$F_REPO"

# F8: git check-ignore -q .worktrees exits 0 (symlink covered by .gitignore)
# RED until step-10 drops trailing slash from .gitignore
assert "F8: git check-ignore -q .worktrees exits 0" \
    bash -c 'git -C "$1" check-ignore -q .worktrees 2>/dev/null' _ "$F_REPO"

# F9: a fresh git worktree add lands under <mount>
git -C "$F_REPO" worktree add -q "$F_REPO/.worktrees/probe" -b probe-wt 2>/dev/null || true
assert "F9: new worktree probe resolves under <mount>" \
    bash -c '[[ "$(readlink -f "$1/.worktrees/probe")" == "$2"/* ]]' \
    _ "$F_REPO" "$F_MNT"

# F10 (jq-guarded): setup-worktree-debug-port.sh against a relocated worktree
if ! command -v jq >/dev/null 2>&1; then
    echo "  SKIP: F10: jq not available; skipping setup-worktree-debug-port.sh check"
else
    # Seed a minimal .mcp.json with a reify-debug entry in the relocated worktree
    cat > "$F_REPO/.worktrees/_merge-verify/.mcp.json" << 'MCPEOF'
{"mcpServers":{"reify-debug":{"type":"http","url":"http://127.0.0.1:3939/mcp"}}}
MCPEOF
    _f10_port_out="$(REIFY_DEBUG_PORT=19876 \
        bash "$REPO_ROOT/scripts/setup-worktree-debug-port.sh" \
        "$F_REPO/.worktrees/_merge-verify" 2>/dev/null)" || true
    assert "F10: setup-worktree-debug-port.sh exits 0 (port printed)" \
        bash -c '[ "$1" = "19876" ]' _ "$_f10_port_out"
    assert "F10: .mcp.json reify-debug URL patched to port 19876" \
        bash -c 'jq -e ".mcpServers[\"reify-debug\"].url | contains(\"19876\")" "$1" >/dev/null' \
        _ "$F_REPO/.worktrees/_merge-verify/.mcp.json"
fi


# ──────────────────────────────────────────────────────────────────────────────
# Block G — --repo-only default mount resolution
#
# Exercises the fix for the deferred-MOUNT bug (suggestion 1): when --repo X is
# passed without --mount, the mount default must be computed relative to X (i.e.
# _default_mount(X)), NOT relative to REPO_ROOT (the script's own location).
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: --repo-only default mount resolution ---"

G_TMP="$(mktemp -d /tmp/test-relocate-g-XXXXXX)"
_TMPDIRS+=("$G_TMP")
G_REPO="$G_TMP/repo"
# Expected default: dirname(G_REPO) = G_TMP (not named "worktrees"), so
# _default_mount(G_REPO) = G_TMP/warm-lanes
G_EXPECTED_MOUNT="$G_TMP/warm-lanes"
mkdir -p "$G_REPO" "$G_EXPECTED_MOUNT"  # mount must exist for validation to pass

# G1: invoke with only --repo (no --mount); clear REIFY_WARM_LANE_MOUNT so the
#     computed default is used (not an inherited env var).
reset_calls
REIFY_TEST_REFLINK_OK=1 REIFY_WARM_LANE_MOUNT="" \
    run_helper --repo "$G_REPO"
assert "G1: --repo-only exits 0" test "$RC" -eq 0

# G2: stdout is the computed default mount's worktrees subdir
assert "G2: stdout is <default-mount>/worktrees (derived from --repo)" \
    bash -c '[ "$1" = "$2/worktrees" ]' _ "$OUT" "$G_EXPECTED_MOUNT"

# G3: .worktrees symlink created pointing into the default mount for --repo
#     (if the bug were present, it would point into REPO_ROOT's warm-lanes instead)
assert "G3: .worktrees symlink target resolves into default mount for --repo" \
    bash -c '[ "$(readlink -f "$1/.worktrees")" = "$(readlink -f "$2/worktrees")" ]' \
    _ "$G_REPO" "$G_EXPECTED_MOUNT"


# ──────────────────────────────────────────────────────────────────────────────
# Block H — orchestrator.yaml config contract (PyYAML-guarded)
# Asserts: git.warm_lane_base_target_dir is set correctly; git.warm_lane_pool is ON
# (enabled 2026-06-20 by the #4665 deploy); the top-level warm_lane_pool.enabled
# regression guard (a DISTINCT key) stays OFF until DF ζ task-dispatch wiring lands.
# Mirrors the PyYAML-with-SKIP-guard idiom from test_warm_lane_pool_config.sh.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: orchestrator.yaml config contract ---"

ORCH_YAML="$REPO_ROOT/orchestrator.yaml"
EXPECTED_BASE_TARGET_DIR="/home/leo/src/warm-lanes/base/target"

# SKIP guard: require python3 + PyYAML
if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "  SKIP: python3 'yaml' (PyYAML) not available; skipping YAML assertions"
else
    _H_PARSE_PY="$(mktemp /tmp/relocate_config_check_XXXXXX.py)"
    _TMPDIRS+=("$_H_PARSE_PY")

    cat > "$_H_PARSE_PY" << 'PYEOF'
"""Validate orchestrator.yaml for task 4696 (warm-lane R3).
Usage:
  python3 <script> <orch_yaml> <check> [<expected_value>]
Checks:
  parse_ok                  — file parses as valid YAML
  base_target_dir_set       — git.warm_lane_base_target_dir == argv[3]
  pool_on                   — git.warm_lane_pool is True (enabled by #4665 deploy)
  top_level_pool_not_on     — warm_lane_pool.enabled is absent or not True
Exit 0 on pass, 1 on fail.
"""
import sys, yaml

orch_yaml_path = sys.argv[1]
check = sys.argv[2]

with open(orch_yaml_path) as f:
    d = yaml.safe_load(f)

if check == "parse_ok":
    sys.exit(0)

if check == "base_target_dir_set":
    expected = sys.argv[3]
    git_block = d.get("git", {}) or {}
    actual = git_block.get("warm_lane_base_target_dir")
    if actual != expected:
        print(f"FAIL: git.warm_lane_base_target_dir={actual!r}, expected {expected!r}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "pool_on":
    git_block = d.get("git", {}) or {}
    pool_val = git_block.get("warm_lane_pool")
    if pool_val is not True:
        print(f"FAIL: git.warm_lane_pool={pool_val!r} — pool must be ON (enabled by #4665 deploy)", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "top_level_pool_not_on":
    wlp = d.get("warm_lane_pool") or {}
    enabled = wlp.get("enabled")
    if enabled is True:
        print(f"FAIL: warm_lane_pool.enabled=True — task 4696 must not turn the pool on", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

print(f"unknown check: {check}", file=sys.stderr)
sys.exit(2)
PYEOF

    # H1: orchestrator.yaml parses as valid YAML
    assert "H1: orchestrator.yaml parses as valid YAML" \
        python3 "$_H_PARSE_PY" "$ORCH_YAML" parse_ok

    # H2: git.warm_lane_base_target_dir == expected path
    # RED until step-12 adds the knob to orchestrator.yaml
    assert "H2: git.warm_lane_base_target_dir == $EXPECTED_BASE_TARGET_DIR" \
        python3 "$_H_PARSE_PY" "$ORCH_YAML" base_target_dir_set "$EXPECTED_BASE_TARGET_DIR"

    # H3: git.warm_lane_pool is True (pool enabled by the #4665 deploy)
    assert "H3: git.warm_lane_pool is True (pool enabled by #4665)" \
        python3 "$_H_PARSE_PY" "$ORCH_YAML" pool_on

    # H4: regression guard — top-level warm_lane_pool.enabled is not True
    assert "H4: warm_lane_pool.enabled is not True (regression guard)" \
        python3 "$_H_PARSE_PY" "$ORCH_YAML" top_level_pool_not_on
fi

test_summary
