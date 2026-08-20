#!/usr/bin/env bash
# Infrastructure test for task 5383.
# Guards the reify-side cold merge-verify (DF_VERIFY_ROLE=merge) OUTER timeout
# wall. dark-factory orchestrator/src/orchestrator/verify.py's
# `_resolve_verify_timeout` resolves the cold merge-verify timeout via a
# cascade whose FIRST branch returns config.merge_verify_cold_command_timeout_secs
# when set (the is_merge_verify branch) — the reify yaml's
# verify_command_timeout_secs does NOT govern the merge path. This test
# asserts the reify-side dark-factory-orchestrator.yaml pins
# merge_verify_cold_command_timeout_secs present and >= 10800 (180m), so cold
# native-kernel merge-verifies (OCCT/OpenVDB/gmsh/manifold compile +
# debug(60m)+release(90m) inner nextest budget) are not preempted at the
# dark-factory default of 7200s.
#   (A) PyYAML-gated SKIP:
#       (a) dark-factory-orchestrator.yaml still parses as valid YAML.
#       (b) the top-level 'merge_verify_cold_command_timeout_secs' key IS
#           PRESENT in the parsed config, checked via dict MEMBERSHIP (not
#           truthiness).
#       (c) int(value) >= 10800.
#   (B) Always-runs (python-free) guard: an active
#       `merge_verify_cold_command_timeout_secs:` key-assignment line exists
#       somewhere in the file.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== merge_verify_cold_command_timeout_secs outer-wall guard (task 5383) ==="

ORCH_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"

# ---------------------------------------------------------------------------
# (A) PyYAML-gated: file parses + key present + value >= 10800
# ---------------------------------------------------------------------------
echo ""
echo "--- (A) parsed-config assertions via PyYAML ---"

if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping YAML parse assertions"
else
    assert "dark-factory-orchestrator.yaml still parses as valid YAML" \
        python3 -c 'import yaml,sys; yaml.safe_load(open(sys.argv[1]))' "$ORCH_YAML"

    # Membership check, NOT truthiness — keeps the presence check independent
    # of the value check below, so a present-but-wrong-type/negative value
    # fails the value assertion (C) rather than being masked here.
    assert "'merge_verify_cold_command_timeout_secs' key is present in parsed config" \
        python3 -c 'import yaml,sys; d = yaml.safe_load(open(sys.argv[1])); sys.exit(0 if "merge_verify_cold_command_timeout_secs" in d else 1)' "$ORCH_YAML"

    assert "'merge_verify_cold_command_timeout_secs' value is >= 10800" \
        python3 -c 'import yaml,sys; d = yaml.safe_load(open(sys.argv[1])); sys.exit(0 if int(d.get("merge_verify_cold_command_timeout_secs", -1)) >= 10800 else 1)' "$ORCH_YAML"
fi

# ---------------------------------------------------------------------------
# (B) An active merge_verify_cold_command_timeout_secs key-assignment line
#     exists (python-free floor)
# ---------------------------------------------------------------------------
echo ""
echo "--- (B) active key-assignment line exists ---"

assert "an active merge_verify_cold_command_timeout_secs key line exists" \
    bash -c "grep -Eq '^[[:space:]]*merge_verify_cold_command_timeout_secs[[:space:]]*:' '$ORCH_YAML'"

test_summary
