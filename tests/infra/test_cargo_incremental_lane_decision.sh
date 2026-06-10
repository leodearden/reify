#!/usr/bin/env bash
# Infrastructure test for task 4452.
# Validates the CARGO_INCREMENTAL config contract (outcome-independent):
#   (a) GLOBAL-FORBID — CARGO_INCREMENTAL is NEVER enabled globally (PRD §11):
#       scripts/verify.sh exports CARGO_INCREMENTAL=0;
#       orchestrator.yaml verify_env sets CARGO_INCREMENTAL: "0";
#       .cargo/config.toml has no global incremental=true in [build] or any
#       [target.*] rustflags.
#   (b) LANE-SCOPE — any future incremental enablement is ONLY for the
#       dark-factory persistent _merge-verify lane (git.persistent_merge_worktree),
#       never global.  The bench doc names this lane.
#   (c) DECISION-CONSISTENCY — docs/notes/cargo-incremental-persistent-lane-bench.md
#       exists and carries exactly one machine-readable `decision: adopt|reject` token:
#         reject → global-forbid (a) intact + no reify-side lane-incremental enablement;
#         adopt  → doc names the DF-side lane-scoped seam AND global-forbid (a) intact.
#
# RED state:  bench doc absent → assert (c) file-exists fails.
# GREEN state: bench doc present with a valid decision token consistent with config.
# Valid in BOTH benchmark outcome branches (adopt or reject).

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

# -- Test 4 (c): DECISION-CONSISTENCY — bench doc exists ----------------------
echo ""
echo "--- Test 4 (c): bench doc exists (RED trigger if absent) ---"

assert "docs/notes/cargo-incremental-persistent-lane-bench.md exists" \
    test -f "$BENCH_DOC"

# Only run further decision checks if the doc is present (avoid cascading fails).
if [ -f "$BENCH_DOC" ]; then

    # -- Test 5 (c): decision token is valid ----------------------------------
    echo ""
    echo "--- Test 5 (c): bench doc carries a valid decision token ---"

    DECISION="$(grep -oE 'decision: [a-z]+' "$BENCH_DOC" | head -1 | sed 's/decision: //')"

    assert "decision token is 'adopt' or 'reject'" \
        bash -c "[ '$DECISION' = 'adopt' ] || [ '$DECISION' = 'reject' ]"

    # -- Test 6 (b)+(c): LANE-SCOPE — bench doc names the DF persistent lane --
    echo ""
    echo "--- Test 6 (b): bench doc names the DF persistent _merge-verify lane ---"

    assert "bench doc references '_merge-verify' (persistent lane name)" \
        grep -q '_merge-verify' "$BENCH_DOC"

    assert "bench doc references 'git.persistent_merge_worktree' (DF knob)" \
        grep -q 'git.persistent_merge_worktree' "$BENCH_DOC"

    # -- Test 7 (c): decision↔config consistency ------------------------------
    echo ""
    echo "--- Test 7 (c): decision token is consistent with config ---"

    if [ "$DECISION" = "reject" ]; then
        # reject → global-forbid invariants already checked in Tests 1-3 above.
        # Additionally verify no reify-side lane-incremental enablement in verify.sh:
        # there must be no conditional that sets CARGO_INCREMENTAL=1.
        assert "reject: verify.sh does not set CARGO_INCREMENTAL=1 anywhere" \
            bash -c "! grep -q 'CARGO_INCREMENTAL=1' '$VERIFY_SH'"

    elif [ "$DECISION" = "adopt" ]; then
        # adopt → doc must name the lane-scoped seam.
        # Global-forbid is still required (incremental is lane-scoped, never global).
        assert "adopt: bench doc names a DF-side lane seam (verify-env or git.* yaml)" \
            bash -c "grep -q 'verify.env\|verify_env\|git\.' '$BENCH_DOC'"

        assert "adopt: global-forbid intact (verify.sh CARGO_INCREMENTAL=0 still present)" \
            grep -q 'export CARGO_INCREMENTAL=0' "$VERIFY_SH"

        assert "adopt: global-forbid intact (orchestrator.yaml CARGO_INCREMENTAL: \"0\" still present)" \
            grep -q 'CARGO_INCREMENTAL:.*"0"' "$ORCH_YAML"
    fi

fi

test_summary
