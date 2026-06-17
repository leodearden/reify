#!/usr/bin/env bash
# Infrastructure test for task 4633.
# Validates the cpu_governance block config contract in orchestrator.yaml:
#   (A) STRUCTURE — top-level 'cpu_governance' key exists; values match the
#       canonical shape (weights.task==100, weights.merge==300,
#       agent_admit.threshold==50, agent_admit enabled, DF_AGENT_CPU_GOVERN
#       present).
#   (B) KNOB-NAME CROSS-CHECK — each REIFY_* knob cited by name in
#       orchestrator.yaml MUST also appear in its owning script, so config↔script
#       names cannot drift silently.
#       Checked:
#         REIFY_CPU_GOVERN_W_TASK   — scripts/cpu-governed-exec.sh
#         REIFY_CPU_GOVERN_W_MERGE  — scripts/cpu-governed-exec.sh
#         REIFY_CPU_ADMIT_AGENT_THRESHOLD — scripts/agent-bin/cargo
#       NOT grep-checked: DF_AGENT_CPU_GOVERN (dark-factory consumed; no reify
#       script reads it).
#
# (A) is SKIPPED if python3 + PyYAML are unavailable (mirrors the tomllib SKIP
#     idiom in test_cargo_incremental_lane_decision.sh:25).
# (B) always runs — plain bash grep, no python needed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== cpu_governance config contract tests ==="

ORCH_YAML="$REPO_ROOT/orchestrator.yaml"
CPU_GOV="$REPO_ROOT/scripts/cpu-governed-exec.sh"
AGENT_CARGO="$REPO_ROOT/scripts/agent-bin/cargo"

# ---------------------------------------------------------------------------
# (A) STRUCTURE — parse YAML and assert key/value shape
# ---------------------------------------------------------------------------

# SKIP guard: require python3 + PyYAML
if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping YAML structure assertions"
else
    echo "--- (A) structural assertions via PyYAML ---"

    # Write a Python helper to a temp file so assert() can invoke it as a command.
    _PARSE_PY="$(mktemp /tmp/cpu_gov_config_parse_XXXXXX.py)"
    trap 'rm -f "$_PARSE_PY"' EXIT

    cat > "$_PARSE_PY" << 'PYEOF'
"""Validate orchestrator.yaml cpu_governance block.
Usage:
  python3 <script> <orch_yaml> <check>
Checks:
  parse_ok                 — file parses as valid YAML (no exception)
  has_cpu_governance       — top-level 'cpu_governance' key exists
  weights_task_100         — cpu_governance.weights.task == 100
  weights_merge_300        — cpu_governance.weights.merge == 300
  agent_admit_threshold_50 — cpu_governance.agent_admit.threshold == 50
  agent_admit_enabled      — cpu_governance.agent_admit.enabled is truthy
  df_agent_cpu_govern_present — 'DF_AGENT_CPU_GOVERN' key present in block
Exit 0 on pass, 1 on fail.
"""
import sys, yaml

orch_yaml_path = sys.argv[1]
check = sys.argv[2]

with open(orch_yaml_path) as f:
    d = yaml.safe_load(f)

if check == "parse_ok":
    # If we got here, the file parsed
    sys.exit(0)

cg = d.get("cpu_governance")

if check == "has_cpu_governance":
    sys.exit(0 if cg is not None else 1)

if cg is None:
    sys.exit(1)

if check == "weights_task_100":
    sys.exit(0 if cg.get("weights", {}).get("task") == 100 else 1)

if check == "weights_merge_300":
    sys.exit(0 if cg.get("weights", {}).get("merge") == 300 else 1)

if check == "agent_admit_threshold_50":
    sys.exit(0 if cg.get("agent_admit", {}).get("threshold") == 50 else 1)

if check == "agent_admit_enabled":
    sys.exit(0 if cg.get("agent_admit", {}).get("enabled") else 1)

if check == "df_agent_cpu_govern_present":
    sys.exit(0 if "DF_AGENT_CPU_GOVERN" in cg else 1)

print(f"unknown check: {check}", file=sys.stderr)
sys.exit(2)
PYEOF

    assert "orchestrator.yaml parses as valid YAML" \
        python3 "$_PARSE_PY" "$ORCH_YAML" parse_ok

    assert "top-level 'cpu_governance' key exists" \
        python3 "$_PARSE_PY" "$ORCH_YAML" has_cpu_governance

    assert "cpu_governance.weights.task == 100 (REIFY_CPU_GOVERN_W_TASK default)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" weights_task_100

    assert "cpu_governance.weights.merge == 300 (REIFY_CPU_GOVERN_W_MERGE default)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" weights_merge_300

    assert "cpu_governance.agent_admit.threshold == 50 (REIFY_CPU_ADMIT_AGENT_THRESHOLD default)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" agent_admit_threshold_50

    assert "cpu_governance.agent_admit.enabled is truthy" \
        python3 "$_PARSE_PY" "$ORCH_YAML" agent_admit_enabled

    assert "cpu_governance block contains DF_AGENT_CPU_GOVERN key" \
        python3 "$_PARSE_PY" "$ORCH_YAML" df_agent_cpu_govern_present
fi

# ---------------------------------------------------------------------------
# (B) KNOB-NAME CROSS-CHECK — always runs (bash grep, no python needed)
# ---------------------------------------------------------------------------
echo "--- (B) knob-name cross-check (config↔script) ---"

assert "REIFY_CPU_GOVERN_W_TASK cited in orchestrator.yaml" \
    grep -q "REIFY_CPU_GOVERN_W_TASK" "$ORCH_YAML"

assert "REIFY_CPU_GOVERN_W_TASK referenced in scripts/cpu-governed-exec.sh" \
    grep -q "REIFY_CPU_GOVERN_W_TASK" "$CPU_GOV"

assert "REIFY_CPU_GOVERN_W_MERGE cited in orchestrator.yaml" \
    grep -q "REIFY_CPU_GOVERN_W_MERGE" "$ORCH_YAML"

assert "REIFY_CPU_GOVERN_W_MERGE referenced in scripts/cpu-governed-exec.sh" \
    grep -q "REIFY_CPU_GOVERN_W_MERGE" "$CPU_GOV"

assert "REIFY_CPU_ADMIT_AGENT_THRESHOLD cited in orchestrator.yaml" \
    grep -q "REIFY_CPU_ADMIT_AGENT_THRESHOLD" "$ORCH_YAML"

assert "REIFY_CPU_ADMIT_AGENT_THRESHOLD referenced in scripts/agent-bin/cargo" \
    grep -q "REIFY_CPU_ADMIT_AGENT_THRESHOLD" "$AGENT_CARGO"

test_summary
