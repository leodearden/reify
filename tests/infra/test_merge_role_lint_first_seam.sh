#!/usr/bin/env bash
# Infrastructure test for task 5271.
# Rider-1 activation seam guard (docs/prds/merge-gate-riders.md task beta / Sec 3, Sec 6
# K1): validates the reify-side half of the merge-role `sequential_lint_first` contract in
# dark-factory-orchestrator.yaml. The DF-side reorder (lint -> test -> type, short-
# circuiting test+type as CheckRun.skipped when lint fails) is entirely dark_factory's
# verify.py (task alpha, dark_factory:2832); reify's leaf is only the config knob + this
# drift guard, which pins:
#   (A) STRUCTURE (PyYAML, SKIP-guarded) -- dark-factory-orchestrator.yaml parses as valid
#       YAML; sequential_lint_first == true; concurrent_verify == false (K1's precondition
#       for the DF-side reorder to take effect); lint_command is present and non-empty.
#   (B) KNOB-NAME / VALUE CROSS-CHECK -- always runs (bash grep, no python needed):
#       `sequential_lint_first: true` and `concurrent_verify: false` are both cited
#       verbatim in the yaml, and lint_command routes through `./scripts/verify.sh lint`.
#   (C) STRICT-SUPERSET CROSS-CHECK -- verify.sh's own --print-plan oracle (reusing the
#       LINT_PLAN idiom from test_release_mode_in_test_command.sh) proves the `--scope
#       all` lint plan still emits the full-workspace strict-clippy invocation. A pure
#       yaml grep of lint_command's string cannot catch a regression that narrows clippy
#       below --workspace or drops -D warnings inside verify.sh; this cross-check can.
#
# (A) is SKIPPED if python3 + PyYAML are unavailable (mirrors the SKIP idiom used by
#     test_cpu_governance_config.sh / test_cargo_incremental_lane_decision.sh:25).
# (B) and (C) always run -- plain bash grep / a verify.sh invocation, no python needed.
#
# NOTE ON SURVEY FIGURES (context only, never asserted): rider 1 catches the lint-red
# failure class (~4.8% of merge failures per the PRD survey: lint p/c=60 vs release
# nextest p/c=21) after ~4 min instead of ~50 min, by short-circuiting test+type once
# lint fails for role=merge. That ordering/short-circuit mechanism lives entirely in
# dark_factory's verify.py and is not reify-observable from this repo -- this guard only
# pins the yaml-contract seam feeding it.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== merge-role sequential_lint_first seam contract tests (rider 1 / task 5271) ==="

ORCH_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"

# ---------------------------------------------------------------------------
# (A) STRUCTURE -- parse YAML and assert key/value shape
# ---------------------------------------------------------------------------

if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping YAML structure assertions"
else
    echo "--- (A) structural assertions via PyYAML ---"

    _PARSE_PY="$(mktemp /tmp/lint_first_seam_parse_XXXXXX.py)"
    trap 'rm -f "$_PARSE_PY"' EXIT

    cat > "$_PARSE_PY" << 'PYEOF'
"""Validate dark-factory-orchestrator.yaml's sequential_lint_first (rider 1 / K1) contract.
Usage:
  python3 <script> <orch_yaml> <check>
Checks:
  parse_ok                    -- file parses as valid YAML (no exception)
  sequential_lint_first_true  -- top-level 'sequential_lint_first' key == true
  concurrent_verify_false     -- top-level 'concurrent_verify' key == false (K1 precondition)
  lint_command_present        -- top-level 'lint_command' key is a non-empty string
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

if check == "sequential_lint_first_true":
    sys.exit(0 if d.get("sequential_lint_first") is True else 1)

if check == "concurrent_verify_false":
    sys.exit(0 if d.get("concurrent_verify") is False else 1)

if check == "lint_command_present":
    lc = d.get("lint_command")
    sys.exit(0 if isinstance(lc, str) and lc.strip() != "" else 1)

print(f"unknown check: {check}", file=sys.stderr)
sys.exit(2)
PYEOF

    assert "dark-factory-orchestrator.yaml parses as valid YAML" \
        python3 "$_PARSE_PY" "$ORCH_YAML" parse_ok

    assert "top-level 'sequential_lint_first' == true" \
        python3 "$_PARSE_PY" "$ORCH_YAML" sequential_lint_first_true

    assert "top-level 'concurrent_verify' == false (K1 precondition)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" concurrent_verify_false

    assert "top-level 'lint_command' is present and non-empty" \
        python3 "$_PARSE_PY" "$ORCH_YAML" lint_command_present
fi

# ---------------------------------------------------------------------------
# (B) KNOB-NAME / VALUE CROSS-CHECK -- always runs (bash grep, no python needed)
# ---------------------------------------------------------------------------
echo "--- (B) knob-name / value cross-check (bash grep) ---"

assert "'sequential_lint_first: true' cited in dark-factory-orchestrator.yaml" \
    grep -qE '^[[:space:]]*sequential_lint_first:[[:space:]]*true[[:space:]]*$' "$ORCH_YAML"

assert "'concurrent_verify: false' cited in dark-factory-orchestrator.yaml" \
    grep -qE '^[[:space:]]*concurrent_verify:[[:space:]]*false[[:space:]]*$' "$ORCH_YAML"

assert "lint_command routes through './scripts/verify.sh lint'" \
    grep -qE '^lint_command:.*\./scripts/verify\.sh lint' "$ORCH_YAML"

# ---------------------------------------------------------------------------
# (C) STRICT-SUPERSET CROSS-CHECK -- verify.sh's own --print-plan oracle
# ---------------------------------------------------------------------------
echo "--- (C) lint_command strict-superset cross-check (verify.sh --print-plan oracle) ---"

# Reuses the LINT_PLAN idiom from test_release_mode_in_test_command.sh:30 -- --scope all
# forces the full plan; comment lines are stripped via `grep -v '^#'`.
LINT_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" lint --scope all --print-plan | grep -v '^#')"
export LINT_PLAN_SEGS

assert "lint plan (--scope all) contains the full-workspace strict-clippy invocation (K1 superset claim)" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -qE 'cargo clippy --workspace --all-targets -- -D warnings'"

test_summary
