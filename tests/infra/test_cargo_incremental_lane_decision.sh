#!/usr/bin/env bash
# Infrastructure test for task 4452.
# Validates the CARGO_INCREMENTAL global-forbid config contract (outcome-independent):
#   (a) GLOBAL-FORBID — CARGO_INCREMENTAL is NEVER enabled globally (PRD §11):
#       scripts/verify.sh exports CARGO_INCREMENTAL=0 and never sets CARGO_INCREMENTAL=1;
#       orchestrator.yaml verify_env sets CARGO_INCREMENTAL: "0";
#       .cargo/config.toml has no global incremental=true in [build] or any
#       [target.*] rustflags.
#   (b) DELIVERABLE — docs/notes/cargo-incremental-persistent-lane-bench.md exists.
#
# The test is GREEN against the current config and the existing bench doc.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== cargo-incremental lane-decision contract tests ==="

# Preflight: tomllib is stdlib on Python ≥3.11; if unavailable, skip cleanly
# rather than aborting mid-test with an opaque ImportError traceback.
python3 -c 'import tomllib' 2>/dev/null || {
    echo "SKIP: python3 tomllib not available (requires Python ≥3.11); skipping cargo-incremental tests"
    exit 0
}

CONFIG="$REPO_ROOT/.cargo/config.toml"
VERIFY_SH="$REPO_ROOT/scripts/verify.sh"
ORCH_YAML="$REPO_ROOT/orchestrator.yaml"
BENCH_DOC="$REPO_ROOT/docs/notes/cargo-incremental-persistent-lane-bench.md"

# Write a Python helper to a temp file so each assert() call can invoke it
# directly (assert only accepts a command + args, not heredocs).
# Uses tomllib (stdlib >=3.11) to parse .cargo/config.toml robustly.
_PARSE_PY="$(mktemp /tmp/incremental_config_parse_XXXXXX.py)"
trap 'rm -f "$_PARSE_PY"' EXIT

cat > "$_PARSE_PY" << 'PYEOF'
"""Validate .cargo/config.toml for CARGO_INCREMENTAL global-forbid contract.
Usage:
  python3 <script> <config_path> check_build_no_incremental
      # exit 1 if [build].incremental = true
  python3 <script> <config_path> check_target_no_incremental_flag
      # exit 1 if any [target.*] rustflags contain -Cincremental or incremental=true
"""
import sys, tomllib

config_path, action = sys.argv[1], sys.argv[2]

with open(config_path, 'rb') as f:
    cfg = tomllib.load(f)

if action == 'check_build_no_incremental':
    # Verify [build].incremental is not set to true.
    build_section = cfg.get('build', {})
    incremental_val = build_section.get('incremental', False)
    if incremental_val is True:
        print('FAIL: [build].incremental = true found in .cargo/config.toml', file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

elif action == 'check_target_no_incremental_flag':
    # Verify no [target.*] rustflags contain -Cincremental or incremental=true.
    target_section = cfg.get('target', {})
    for target_triple, target_cfg in target_section.items():
        if not isinstance(target_cfg, dict):
            continue
        flags = target_cfg.get('rustflags', [])
        if isinstance(flags, str):
            flags = [flags]
        for flag in flags:
            flag_s = str(flag)
            if '-Cincremental' in flag_s or 'incremental=true' in flag_s:
                print(
                    f'FAIL: incremental flag found in [target.{target_triple}].rustflags: {flag}',
                    file=sys.stderr,
                )
                sys.exit(1)
    sys.exit(0)

else:
    print(f'Unknown action: {action}', file=sys.stderr)
    sys.exit(2)
PYEOF

# -- Test 1 (a): GLOBAL-FORBID — scripts/verify.sh ---------------------------
echo ""
echo "--- Test 1 (a): scripts/verify.sh exports CARGO_INCREMENTAL=0 ---"

assert "scripts/verify.sh contains 'export CARGO_INCREMENTAL=0'" \
    grep -q 'export CARGO_INCREMENTAL=0' "$VERIFY_SH"

assert "scripts/verify.sh does not set CARGO_INCREMENTAL=1 anywhere" \
    bash -c "! grep -q 'CARGO_INCREMENTAL=1' \"$VERIFY_SH\""

# -- Test 2 (a): GLOBAL-FORBID — orchestrator.yaml ----------------------------
echo ""
echo "--- Test 2 (a): orchestrator.yaml verify_env sets CARGO_INCREMENTAL: \"0\" ---"

assert "orchestrator.yaml verify_env contains CARGO_INCREMENTAL: \"0\"" \
    grep -q 'CARGO_INCREMENTAL:.*"0"' "$ORCH_YAML"

# -- Test 3 (a): GLOBAL-FORBID — .cargo/config.toml --------------------------
echo ""
echo "--- Test 3 (a): .cargo/config.toml has no global incremental=true ---"

assert ".cargo/config.toml: [build].incremental is not true" \
    python3 "$_PARSE_PY" "$CONFIG" check_build_no_incremental

assert ".cargo/config.toml: no [target.*] rustflag enables incremental" \
    python3 "$_PARSE_PY" "$CONFIG" check_target_no_incremental_flag

# -- Test 4: DELIVERABLE — bench doc exists -----------------------------------
echo ""
echo "--- Test 4: bench doc exists ---"

assert "docs/notes/cargo-incremental-persistent-lane-bench.md exists" \
    test -f "$BENCH_DOC"

test_summary
