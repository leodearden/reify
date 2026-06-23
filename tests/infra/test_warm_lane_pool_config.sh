#!/usr/bin/env bash
# Infrastructure test for task 4663.
# Validates the warm_lane_pool block config contract in orchestrator.yaml:
#   (A) STRUCTURE — top-level 'warm_lane_pool' key exists; shape/type assertions
#       (task_pool_size_source=="max_concurrent_tasks" — semantic contract;
#       merge_spec_pool_size_source=="_MERGE_AHEAD_BOUND" — semantic contract;
#       substrate.image_path is a non-empty string; substrate.size_gib is an int;
#       defrag_extent_threshold is an int — actual values validated by A2);
#       D9 negative guard: no hardcoded integer task_pool_size key;
#       top-level max_concurrent_tasks key present (derive-source readable at startup).
#   (A2) VALUE DRIFT — YAML default values MATCH the :-fallback defaults in the
#       owning scripts, so a numeric drift (e.g. YAML says 500, script still
#       defaults to 600) is caught.  Checked:
#         substrate.image_path    YAML vs scripts/provision-warm-lane-fs.sh IMG=
#         substrate.size_gib      YAML vs scripts/provision-warm-lane-fs.sh SIZE_GIB=
#         defrag_extent_threshold YAML vs scripts/refresh-warm-base.sh FRAG_THRESHOLD=
#   (B) KNOB-NAME CROSS-CHECK — each REIFY_* knob cited by name in
#       orchestrator.yaml MUST also appear in its owning script, so config↔script
#       names cannot drift silently.
#       Checked:
#         REIFY_WARM_LANE_MOUNT — scripts/provision-warm-lane-fs.sh
#       NOT grep-checked: _MERGE_AHEAD_BOUND, max_concurrent_tasks (DF-consumed /
#       orchestrator-level; no reify script reads them).
#
# (A) and (A2) are SKIPPED if python3 + PyYAML are unavailable (mirrors the
#     tomllib SKIP idiom in test_cargo_incremental_lane_decision.sh:25).
# (B) always runs — plain bash grep, no python needed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== warm_lane_pool config contract tests ==="

ORCH_YAML="$REPO_ROOT/orchestrator.yaml"
PROVISION_SH="$REPO_ROOT/scripts/provision-warm-lane-fs.sh"
REFRESH_SH="$REPO_ROOT/scripts/refresh-warm-base.sh"
INSTALLER_SH="$REPO_ROOT/scripts/install-warm-lane-units.sh"

# ---------------------------------------------------------------------------
# (A) STRUCTURE — parse YAML and assert key/value shape
# ---------------------------------------------------------------------------

# SKIP guard: require python3 + PyYAML
if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping YAML structure assertions"
else
    echo "--- (A) structural assertions via PyYAML ---"

    # Write a Python helper to a temp file so assert() can invoke it as a command.
    _PARSE_PY="$(mktemp /tmp/warm_lane_pool_config_parse_XXXXXX.py)"
    trap 'rm -f "$_PARSE_PY"' EXIT

    cat > "$_PARSE_PY" << 'PYEOF'
"""Validate orchestrator.yaml warm_lane_pool block.
Usage:
  python3 <script> <orch_yaml> <check> [<script_path>]
Checks (no <script_path>):
  parse_ok                           — file parses as valid YAML (no exception)
  has_warm_lane_pool                 — top-level 'warm_lane_pool' key exists
  has_max_concurrent_tasks           — top-level 'max_concurrent_tasks' key exists (D9 derive-source)
  task_pool_size_source_string       — warm_lane_pool.task_pool_size_source == "max_concurrent_tasks"
  merge_spec_pool_size_source_string — warm_lane_pool.merge_spec_pool_size_source == "_MERGE_AHEAD_BOUND"
  image_path_is_string               — warm_lane_pool.substrate.image_path is a non-empty string
  size_gib_is_int                    — warm_lane_pool.substrate.size_gib is an int (value validated by A2 drift check)
  defrag_threshold_is_int            — warm_lane_pool.defrag_extent_threshold is an int (value validated by A2 drift check)
  no_hardcoded_task_pool_size        — warm_lane_pool has NO integer 'task_pool_size' key (D9 negative guard)
Checks (with <script_path> — value-drift cross-check):
  image_path_yaml_vs_provision    — YAML image_path == provision-warm-lane-fs.sh IMG= default
  size_gib_yaml_vs_provision      — YAML size_gib == provision-warm-lane-fs.sh SIZE_GIB= default
  defrag_yaml_vs_refresh          — YAML defrag_extent_threshold == refresh-warm-base.sh FRAG_THRESHOLD= default
Exit 0 on pass, 1 on fail.
"""
import sys, yaml, re

orch_yaml_path = sys.argv[1]
check = sys.argv[2]

with open(orch_yaml_path) as f:
    d = yaml.safe_load(f)

if check == "parse_ok":
    # If we got here, the file parsed
    sys.exit(0)

if check == "has_max_concurrent_tasks":
    sys.exit(0 if "max_concurrent_tasks" in d else 1)

wlp = d.get("warm_lane_pool")

if check == "has_warm_lane_pool":
    sys.exit(0 if wlp is not None else 1)

if wlp is None:
    sys.exit(1)

if check == "task_pool_size_source_string":
    sys.exit(0 if wlp.get("task_pool_size_source") == "max_concurrent_tasks" else 1)

if check == "merge_spec_pool_size_source_string":
    sys.exit(0 if wlp.get("merge_spec_pool_size_source") == "_MERGE_AHEAD_BOUND" else 1)

if check == "image_path_is_string":
    val = wlp.get("substrate", {}).get("image_path")
    sys.exit(0 if isinstance(val, str) and val else 1)

if check == "size_gib_is_int":
    val = wlp.get("substrate", {}).get("size_gib")
    sys.exit(0 if isinstance(val, int) else 1)

if check == "defrag_threshold_is_int":
    val = wlp.get("defrag_extent_threshold")
    sys.exit(0 if isinstance(val, int) else 1)

if check == "no_hardcoded_task_pool_size":
    # D9 negative guard: pool size must stay derived, never re-frozen as a constant
    val = wlp.get("task_pool_size")
    # Fail if the key exists AND its value is an integer (a hardcoded count)
    if val is not None and isinstance(val, int):
        print(f"D9 violation: warm_lane_pool.task_pool_size is a hardcoded integer ({val}); use task_pool_size_source instead", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

# Value-drift checks: require sys.argv[3] = path to owning bash script.
if check == "image_path_yaml_vs_provision":
    script_path = sys.argv[3]
    content = open(script_path).read()
    m = re.search(r'^IMG="([^"]+)"', content, re.MULTILINE)
    if not m:
        print(f"IMG= default not found in {script_path}", file=sys.stderr)
        sys.exit(1)
    script_val = m.group(1)
    yaml_val = wlp.get("substrate", {}).get("image_path")
    if yaml_val != script_val:
        print(f"Drift: YAML substrate.image_path={yaml_val!r} != provision-warm-lane-fs.sh IMG={script_val!r}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "size_gib_yaml_vs_provision":
    script_path = sys.argv[3]
    content = open(script_path).read()
    m = re.search(r'^SIZE_GIB=([0-9]+)', content, re.MULTILINE)
    if not m:
        print(f"SIZE_GIB= default not found in {script_path}", file=sys.stderr)
        sys.exit(1)
    script_val = int(m.group(1))
    yaml_val = wlp.get("substrate", {}).get("size_gib")
    if yaml_val != script_val:
        print(f"Drift: YAML substrate.size_gib={yaml_val} != provision-warm-lane-fs.sh SIZE_GIB={script_val}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "defrag_yaml_vs_refresh":
    script_path = sys.argv[3]
    content = open(script_path).read()
    m = re.search(r'^FRAG_THRESHOLD=([0-9]+)', content, re.MULTILINE)
    if not m:
        print(f"FRAG_THRESHOLD= default not found in {script_path}", file=sys.stderr)
        sys.exit(1)
    script_val = int(m.group(1))
    yaml_val = wlp.get("defrag_extent_threshold")
    if yaml_val != script_val:
        print(f"Drift: YAML defrag_extent_threshold={yaml_val} != refresh-warm-base.sh FRAG_THRESHOLD={script_val}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "image_path_value":
    yaml_val = wlp.get("substrate", {}).get("image_path")
    expected = "/media/leo/data_lv_1/leo/reify-warm-lanes.img"
    if yaml_val != expected:
        print(f"Expected substrate.image_path={expected!r}, got {yaml_val!r}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "size_gib_value":
    yaml_val = wlp.get("substrate", {}).get("size_gib")
    expected = 4096
    if yaml_val != expected:
        print(f"Expected substrate.size_gib={expected}, got {yaml_val}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

# Installer-YAML constant consistency checks (suggestion 3, task #4720 amendment).
# The installer hardcodes WARM_LANE_IMG/WARM_LANE_SIZE_GIB to decouple the deployed
# boot unit from future script-default drift (design decision #3).  These checks catch
# an accidental one-sided edit where only the installer or only the YAML is updated.
if check == "image_path_installer_vs_yaml":
    script_path = sys.argv[3]
    content = open(script_path).read()
    m = re.search(r'^WARM_LANE_IMG="([^"]+)"', content, re.MULTILINE)
    if not m:
        print(f"WARM_LANE_IMG= constant not found in {script_path}", file=sys.stderr)
        sys.exit(1)
    installer_val = m.group(1)
    yaml_val = wlp.get("substrate", {}).get("image_path")
    if yaml_val != installer_val:
        print(f"Drift: YAML substrate.image_path={yaml_val!r} != install-warm-lane-units.sh WARM_LANE_IMG={installer_val!r}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "size_gib_installer_vs_yaml":
    script_path = sys.argv[3]
    content = open(script_path).read()
    m = re.search(r'^WARM_LANE_SIZE_GIB=([0-9]+)', content, re.MULTILINE)
    if not m:
        print(f"WARM_LANE_SIZE_GIB= constant not found in {script_path}", file=sys.stderr)
        sys.exit(1)
    installer_val = int(m.group(1))
    yaml_val = wlp.get("substrate", {}).get("size_gib")
    if yaml_val != installer_val:
        print(f"Drift: YAML substrate.size_gib={yaml_val} != install-warm-lane-units.sh WARM_LANE_SIZE_GIB={installer_val}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

print(f"unknown check: {check}", file=sys.stderr)
sys.exit(2)
PYEOF

    assert "orchestrator.yaml parses as valid YAML" \
        python3 "$_PARSE_PY" "$ORCH_YAML" parse_ok

    assert "top-level 'warm_lane_pool' key exists" \
        python3 "$_PARSE_PY" "$ORCH_YAML" has_warm_lane_pool

    assert "top-level 'max_concurrent_tasks' key exists (D9 derive-source)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" has_max_concurrent_tasks

    assert "warm_lane_pool.task_pool_size_source == 'max_concurrent_tasks' (D9: derived, not hardcoded)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" task_pool_size_source_string

    assert "warm_lane_pool.merge_spec_pool_size_source == '_MERGE_AHEAD_BOUND'" \
        python3 "$_PARSE_PY" "$ORCH_YAML" merge_spec_pool_size_source_string

    assert "warm_lane_pool.substrate.image_path is a non-empty string (value validated by A2 drift check)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" image_path_is_string

    assert "warm_lane_pool.substrate.size_gib is an int (value validated by A2 drift check)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" size_gib_is_int

    assert "warm_lane_pool.defrag_extent_threshold is an int (value validated by A2 drift check)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" defrag_threshold_is_int

    assert "warm_lane_pool has no hardcoded integer task_pool_size key (D9 negative guard)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" no_hardcoded_task_pool_size

    # (A2) VALUE DRIFT — YAML default values MUST match the :-fallback defaults
    # in the owning scripts.  Catches the case where YAML is updated (e.g.
    # size_gib: 500) but the script still defaults to the old value (SIZE_GIB=600),
    # or vice versa — the two sources diverge silently without this cross-check.
    #
    # image_path / size_gib live in scripts/provision-warm-lane-fs.sh;
    # FRAG_THRESHOLD lives in scripts/refresh-warm-base.sh.
    echo "--- (A2) value drift: YAML defaults == script defaults ---"

    assert "substrate.image_path: YAML matches provision-warm-lane-fs.sh IMG= default" \
        python3 "$_PARSE_PY" "$ORCH_YAML" image_path_yaml_vs_provision "$PROVISION_SH"

    assert "substrate.size_gib: YAML matches provision-warm-lane-fs.sh SIZE_GIB= default" \
        python3 "$_PARSE_PY" "$ORCH_YAML" size_gib_yaml_vs_provision "$PROVISION_SH"

    assert "defrag_extent_threshold: YAML matches refresh-warm-base.sh FRAG_THRESHOLD= default" \
        python3 "$_PARSE_PY" "$ORCH_YAML" defrag_yaml_vs_refresh "$REFRESH_SH"

    # (A3) PINNED VALUES — assert the literal new canonical defaults are in place.
    # These fail RED until both provision-warm-lane-fs.sh and orchestrator.yaml are
    # updated together (step-2, task #4720).
    echo "--- (A3) pinned values: new canonical defaults ---"

    assert "substrate.image_path == '/media/leo/data_lv_1/leo/reify-warm-lanes.img' (pinned value)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" image_path_value

    assert "substrate.size_gib == 4096 (pinned value)" \
        python3 "$_PARSE_PY" "$ORCH_YAML" size_gib_value

    # (A4) INSTALLER-YAML CONSTANT CONSISTENCY — the installer hardcodes the canonical
    # values in WARM_LANE_IMG/WARM_LANE_SIZE_GIB to decouple the deployed boot unit from
    # script-default drift.  These values are intentionally the same as orchestrator.yaml's
    # substrate block today; this cross-check catches an accidental one-sided edit.
    echo "--- (A4) installer-YAML constant consistency ---"

    assert "substrate.image_path: YAML matches install-warm-lane-units.sh WARM_LANE_IMG= constant" \
        python3 "$_PARSE_PY" "$ORCH_YAML" image_path_installer_vs_yaml "$INSTALLER_SH"

    assert "substrate.size_gib: YAML matches install-warm-lane-units.sh WARM_LANE_SIZE_GIB= constant" \
        python3 "$_PARSE_PY" "$ORCH_YAML" size_gib_installer_vs_yaml "$INSTALLER_SH"
fi

# ---------------------------------------------------------------------------
# (B) KNOB-NAME CROSS-CHECK — always runs (bash grep, no python needed)
# ---------------------------------------------------------------------------
echo "--- (B) knob-name cross-check (config↔script) ---"

assert "REIFY_WARM_LANE_MOUNT cited in orchestrator.yaml" \
    grep -q "REIFY_WARM_LANE_MOUNT" "$ORCH_YAML"

assert "REIFY_WARM_LANE_MOUNT referenced in scripts/provision-warm-lane-fs.sh" \
    grep -q "REIFY_WARM_LANE_MOUNT" "$PROVISION_SH"

test_summary
