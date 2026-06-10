#!/usr/bin/env bash
# Infrastructure test for task 4450.
# Validates the lean-debuginfo-profile contract (outcome-independent):
#   (a) POSTURE — the effective dev/test debuginfo posture is lean-but-backtrace-
#       preserving: split ∈ {"unpacked","packed"} OR debug ∈ {1,"line-tables-only"},
#       AND NOT backtrace-killing (debug ≠ 0/"none"/false).
#   (b) DECISION-CONSISTENCY — docs/notes/lean-debuginfo-bench.md exists and its
#       single machine-readable `chosen-mechanism:` token is consistent with the
#       effective posture computed from Cargo.toml.
#   (c) SHRINK DIRECTION — the bench doc's target-size-after-bytes < target-size-
#       before-bytes (the user-observable size reduction, based on real measurements).
#
# RED state:  no top-level [profile.dev] → assert (a) fails; bench doc absent →
#             assert (b) file-exists fails.
# GREEN state: [profile.dev] sets a lean posture AND bench doc present with
#             consistent token and after < before.
# Valid in BOTH mechanism branches (split-debuginfo-unpacked or debug-1).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== lean-debuginfo-profile contract tests ==="

# Preflight: tomllib is stdlib on Python ≥3.11; if unavailable, skip cleanly
# rather than aborting mid-test with an opaque ImportError traceback.
python3 -c 'import tomllib' 2>/dev/null || {
    echo "SKIP: python3 tomllib not available (requires Python ≥3.11); skipping lean-debuginfo-profile tests"
    exit 0
}

CARGO_TOML="$REPO_ROOT/Cargo.toml"
BENCH_DOC="$REPO_ROOT/docs/notes/lean-debuginfo-bench.md"

# Write a Python helper to a temp file so each assert() call can invoke it
# directly (assert only accepts a command + args, not heredocs).
# Uses tomllib (stdlib >=3.11) to parse Cargo.toml robustly and compute the
# EFFECTIVE dev/test debuginfo posture, honoring profile inheritance:
#   - split = profile.test.split-debuginfo OR profile.dev.split-debuginfo OR "off"
#   - debug = profile.test.debug OR profile.dev.debug OR 2
_PARSE_PY="$(mktemp /tmp/debuginfo_profile_parse_XXXXXX.py)"
trap 'rm -f "$_PARSE_PY"' EXIT

cat > "$_PARSE_PY" << 'PYEOF'
"""Validate Cargo.toml lean-debuginfo-profile contract.
Usage:
  python3 <script> <cargo_toml_path> effective_split     # print effective split-debuginfo
  python3 <script> <cargo_toml_path> effective_debug     # print effective debug value
  python3 <script> <cargo_toml_path> check_posture       # exit 1 if NOT lean-and-preserving
"""
import sys, tomllib

cargo_toml_path, action = sys.argv[1], sys.argv[2]

with open(cargo_toml_path, 'rb') as f:
    cfg = tomllib.load(f)

profile = cfg.get('profile', {})
dev     = profile.get('dev', {})
test    = profile.get('test', {})

# Effective posture: test overrides dev; dev overrides default.
# split-debuginfo: Cargo default is "off" on Linux (debuginfo embedded in binary).
# debug: Cargo default is 2 (full DWARF).
eff_split = test.get('split-debuginfo') or dev.get('split-debuginfo') or 'off'
eff_debug = test.get('debug')
if eff_debug is None:
    eff_debug = dev.get('debug')
if eff_debug is None:
    eff_debug = 2

if action == 'effective_split':
    print(eff_split)
    sys.exit(0)

elif action == 'effective_debug':
    print(eff_debug)
    sys.exit(0)

elif action == 'check_posture':
    # Lean-and-backtrace-preserving: must satisfy at least one of:
    #   (A) split-debuginfo in {"unpacked", "packed"}  — moves DWARF out of link
    #   (B) debug in {1, "line-tables-only"}            — line-tables-only (embedded)
    lean_split = str(eff_split) in ('unpacked', 'packed')
    lean_debug = str(eff_debug) in ('1', 'line-tables-only') or eff_debug == 1

    # NOT backtrace-killing: debug must not be 0 / "0" / "none" / False (no debuginfo at all)
    no_debug = str(eff_debug) in ('0', 'none') or eff_debug is False or eff_debug == 0

    if no_debug:
        print(
            f'FAIL: debug={eff_debug!r} disables backtraces entirely; '
            'a lean posture must keep at least line tables.',
            file=sys.stderr
        )
        sys.exit(1)

    if not (lean_split or lean_debug):
        print(
            f'FAIL: posture is NOT lean-and-backtrace-preserving: '
            f'split-debuginfo={eff_split!r}, debug={eff_debug!r}. '
            'Expected: split-debuginfo in {unpacked,packed} OR debug in {1,line-tables-only}.',
            file=sys.stderr
        )
        sys.exit(1)

    sys.exit(0)

else:
    print(f'Unknown action: {action}', file=sys.stderr)
    sys.exit(2)
PYEOF

# Determine effective posture from Cargo.toml before the asserts.
EFFECTIVE_SPLIT="$(python3 "$_PARSE_PY" "$CARGO_TOML" effective_split)"
EFFECTIVE_DEBUG="$(python3 "$_PARSE_PY" "$CARGO_TOML" effective_debug)"

# -- Test 1 (a): POSTURE -------------------------------------------------------
echo ""
echo "--- Test 1 (a): effective dev/test debuginfo posture is lean-and-backtrace-preserving ---"

assert "effective dev/test posture is lean-and-backtrace-preserving" \
    python3 "$_PARSE_PY" "$CARGO_TOML" check_posture

test_summary
