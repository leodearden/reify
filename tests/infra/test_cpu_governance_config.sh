#!/usr/bin/env bash
# Infrastructure test for task 4633.
# Validates the cpu_governance block config contract in dark-factory-orchestrator.yaml:
#   (A) STRUCTURE — top-level 'cpu_governance' key exists; values match the
#       canonical shape (weights.task==100, weights.merge==300,
#       agent_admit.threshold==50, agent_admit enabled, DF_AGENT_CPU_GOVERN
#       present).
#   (A2) VALUE DRIFT — YAML default values MATCH the :-fallback defaults in the
#       owning scripts, so a numeric drift (e.g. YAML says 200, script still
#       defaults to 100) is caught.  Checked:
#         REIFY_CPU_GOVERN_W_TASK   YAML vs scripts/lib_cgroup.sh :- default
#         REIFY_CPU_GOVERN_W_MERGE  YAML vs scripts/lib_cgroup.sh :- default
#         REIFY_CPU_ADMIT_AGENT_THRESHOLD YAML vs scripts/agent-bin/cargo :- default
#   (B) KNOB-NAME CROSS-CHECK — each REIFY_* knob cited by name in
#       dark-factory-orchestrator.yaml MUST also appear in its owning script, so config↔script
#       names cannot drift silently.
#       Checked:
#         REIFY_CPU_GOVERN_W_TASK   — scripts/cpu-governed-exec.sh
#         REIFY_CPU_GOVERN_W_MERGE  — scripts/cpu-governed-exec.sh
#         REIFY_CPU_ADMIT_AGENT_THRESHOLD — scripts/agent-bin/cargo
#       NOT grep-checked: DF_AGENT_CPU_GOVERN (dark-factory consumed; no reify
#       script reads it).
#   (C) CENSUS IGNORE-LIST STALENESS — every config_key_census.ignore entry
#       must still fnmatch.fnmatchcase-match >=1 live dotted path in
#       dark-factory-orchestrator.yaml (mirrors dark-factory task 2989's
#       ConfigKeyCensusConfig.ignore matching semantics via stdlib fnmatch
#       only — no cross-repo import). Only the STALE direction is guarded:
#       the COMPLETE direction (a new unlisted key) is self-announcing via
#       2989's born-at-L2 census escalation.
#       A separate vacuity check (census_ignore_is_nonempty_list) guards the
#       block itself: it FAILS loudly if config_key_census is absent, empty,
#       or malformed, so the staleness check above — which stays silently
#       green on zero entries — cannot degrade into a no-op if the block is
#       ever deleted.
#
# (A), (A2), and (C) are SKIPPED if python3 + PyYAML are unavailable (mirrors
#     the tomllib SKIP idiom in test_cargo_incremental_lane_decision.sh:25).
# (B) always runs — plain bash grep, no python needed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== cpu_governance config contract tests ==="

ORCH_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"
CPU_GOV="$REPO_ROOT/scripts/cpu-governed-exec.sh"
LIB_CGROUP="$REPO_ROOT/scripts/lib_cgroup.sh"      # actual home of W_TASK/W_MERGE :-fallbacks
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
"""Validate dark-factory-orchestrator.yaml cpu_governance block.
Usage:
  python3 <script> <orch_yaml> <check> [<script_path>]
Checks (no <script_path>):
  parse_ok                 — file parses as valid YAML (no exception)
  has_cpu_governance       — top-level 'cpu_governance' key exists
  weights_task_100         — cpu_governance.weights.task == 100
  weights_merge_300        — cpu_governance.weights.merge == 300
  agent_admit_threshold_50 — cpu_governance.agent_admit.threshold == 50
  agent_admit_enabled      — cpu_governance.agent_admit.enabled is truthy
  df_agent_cpu_govern_present — 'DF_AGENT_CPU_GOVERN' key present in block
  census_ignore_entries_resolve — every config_key_census.ignore entry
                                   still fnmatch.fnmatchcase-matches >=1 live
                                   dotted path (STALE direction only; see (C)
                                   in the file header)
  census_ignore_is_nonempty_list — config_key_census.ignore is present and a
                                    non-empty list of strings (the vacuity
                                    check: catches an absent/emptied block,
                                    which the staleness check above stays
                                    silent on by design)
Checks (with <script_path> — value-drift cross-check):
  w_task_yaml_vs_lib_cgroup       — YAML weights.task == lib_cgroup.sh :-fallback
  w_merge_yaml_vs_lib_cgroup      — YAML weights.merge == lib_cgroup.sh :-fallback
  threshold_yaml_vs_agent_cargo   — YAML agent_admit.threshold == agent-bin/cargo :-fallback
Exit 0 on pass, 1 on fail.
"""
import sys, yaml, re, fnmatch

orch_yaml_path = sys.argv[1]
check = sys.argv[2]

with open(orch_yaml_path) as f:
    d = yaml.safe_load(f)

if check == "parse_ok":
    # If we got here, the file parsed
    sys.exit(0)

def _flatten(node, prefix=""):
    """Every dotted path in the config, INCLUDING intermediates."""
    out = set()
    if isinstance(node, dict):
        for k, v in node.items():
            p = f"{prefix}{k}"
            out.add(p)
            out |= _flatten(v, p + ".")
    return out

if check == "census_ignore_entries_resolve":
    census = d.get("config_key_census") or {}
    entries = census.get("ignore") or []
    paths = _flatten(d)
    stale = [e for e in entries
             if not any(fnmatch.fnmatchcase(p, e) for p in paths)]
    if stale:
        print("STALE config_key_census.ignore entries (match no live key in "
              "dark-factory-orchestrator.yaml): " + ", ".join(repr(e) for e in stale),
              file=sys.stderr)
        print("Each entry excuses a reify-owned key from dark-factory task 2989's "
              "unknown-config-key census. If the knob was deliberately renamed or "
              "removed, drop its ignore entry in the same commit (see e61fae8017).",
              file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "census_ignore_is_nonempty_list":
    census = d.get("config_key_census")
    if not isinstance(census, dict):
        print("config_key_census block is absent or not a mapping. If the census "
              "allowlist was deliberately retired, remove the (C) group from this "
              "test in the same commit rather than leaving a guard that passes "
              "vacuously.", file=sys.stderr)
        sys.exit(1)
    entries = census.get("ignore")
    if not isinstance(entries, list) or not entries:
        print(f"config_key_census.ignore must be a non-empty list, got {entries!r}",
              file=sys.stderr)
        sys.exit(1)
    bad = [e for e in entries if not isinstance(e, str)]
    if bad:
        print(f"config_key_census.ignore entries must be strings; got {bad!r}",
              file=sys.stderr)
        sys.exit(1)
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

# Value-drift checks: require sys.argv[3] = path to owning bash script.
if check == "w_task_yaml_vs_lib_cgroup":
    script_path = sys.argv[3]
    content = open(script_path).read()
    m = re.search(r'REIFY_CPU_GOVERN_W_TASK:-(\d+)', content)
    if not m:
        print(f"REIFY_CPU_GOVERN_W_TASK default not found in {script_path}", file=sys.stderr)
        sys.exit(1)
    script_val = int(m.group(1))
    yaml_val = cg.get("weights", {}).get("task")
    if yaml_val != script_val:
        print(f"Drift: YAML weights.task={yaml_val} != lib_cgroup.sh default={script_val}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "w_merge_yaml_vs_lib_cgroup":
    script_path = sys.argv[3]
    content = open(script_path).read()
    m = re.search(r'REIFY_CPU_GOVERN_W_MERGE:-(\d+)', content)
    if not m:
        print(f"REIFY_CPU_GOVERN_W_MERGE default not found in {script_path}", file=sys.stderr)
        sys.exit(1)
    script_val = int(m.group(1))
    yaml_val = cg.get("weights", {}).get("merge")
    if yaml_val != script_val:
        print(f"Drift: YAML weights.merge={yaml_val} != lib_cgroup.sh default={script_val}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "threshold_yaml_vs_agent_cargo":
    script_path = sys.argv[3]
    content = open(script_path).read()
    m = re.search(r'REIFY_CPU_ADMIT_AGENT_THRESHOLD:-(\d+)', content)
    if not m:
        print(f"REIFY_CPU_ADMIT_AGENT_THRESHOLD default not found in {script_path}", file=sys.stderr)
        sys.exit(1)
    script_val = int(m.group(1))
    yaml_val = cg.get("agent_admit", {}).get("threshold")
    if yaml_val != script_val:
        print(f"Drift: YAML agent_admit.threshold={yaml_val} != agent-bin/cargo default={script_val}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

print(f"unknown check: {check}", file=sys.stderr)
sys.exit(2)
PYEOF

    assert "dark-factory-orchestrator.yaml parses as valid YAML" \
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

    # (A2) VALUE DRIFT — YAML default values MUST match the :-fallback defaults
    # in the owning scripts.  Catches the case where YAML is updated (e.g.
    # weights.task: 200) but the script still defaults to the old value (:-100),
    # or vice versa — the two sources diverge silently without this cross-check.
    #
    # W_TASK / W_MERGE live in scripts/lib_cgroup.sh (cgroup_role_weight());
    # REIFY_CPU_ADMIT_AGENT_THRESHOLD lives in scripts/agent-bin/cargo.
    echo "--- (A2) value drift: YAML defaults == script :-fallbacks ---"

    assert "REIFY_CPU_GOVERN_W_TASK: YAML weights.task matches lib_cgroup.sh :-fallback" \
        python3 "$_PARSE_PY" "$ORCH_YAML" w_task_yaml_vs_lib_cgroup "$LIB_CGROUP"

    assert "REIFY_CPU_GOVERN_W_MERGE: YAML weights.merge matches lib_cgroup.sh :-fallback" \
        python3 "$_PARSE_PY" "$ORCH_YAML" w_merge_yaml_vs_lib_cgroup "$LIB_CGROUP"

    assert "REIFY_CPU_ADMIT_AGENT_THRESHOLD: YAML agent_admit.threshold matches agent-bin/cargo :-fallback" \
        python3 "$_PARSE_PY" "$ORCH_YAML" threshold_yaml_vs_agent_cargo "$AGENT_CARGO"

    # ---------------------------------------------------------------------------
    # (C) CENSUS IGNORE-LIST STALENESS — every config_key_census.ignore entry
    # must still match >=1 live dotted path in dark-factory-orchestrator.yaml,
    # so a knob rename/removal (e.g. e61fae8017's fairness.scheduler_v2) cannot
    # leave a dead excuse behind silently. Only the STALE direction is guarded
    # here; the COMPLETE direction (a new unlisted key) is self-announcing via
    # dark-factory task 2989's born-at-L2 census escalation.
    # ---------------------------------------------------------------------------
    echo "--- (C) census ignore-list staleness ---"

    assert "every config_key_census.ignore entry still matches a live key in dark-factory-orchestrator.yaml" \
        python3 "$_PARSE_PY" "$ORCH_YAML" census_ignore_entries_resolve

    # Negative fixture: the real YAML with one bogus, never-matching entry
    # appended to config_key_census.ignore. Built by loading+mutating the dict
    # and yaml.safe_dump-ing it back out (not text munging), so the nested-list
    # edit round-trips cleanly regardless of surrounding formatting.
    _CENSUS_STALE_FIXTURE="$(mktemp /tmp/cpu_gov_census_stale_XXXXXX.yaml)"
    trap 'rm -f "$_PARSE_PY" "${_CENSUS_STALE_FIXTURE:-}"' EXIT
    python3 -c 'import sys,yaml; d=yaml.safe_load(open(sys.argv[1])); d["config_key_census"]["ignore"].append("cpu_governance.__stale_fixture_knob__"); yaml.safe_dump(d, open(sys.argv[2],"w"))' \
        "$ORCH_YAML" "$_CENSUS_STALE_FIXTURE"

    assert "census check FAILS on a fixture with a stale ignore entry" \
        bash -c "! python3 '$_PARSE_PY' '$_CENSUS_STALE_FIXTURE' census_ignore_entries_resolve"

    assert "census failure names the stale entry" \
        bash -c "python3 '$_PARSE_PY' '$_CENSUS_STALE_FIXTURE' census_ignore_entries_resolve 2>&1 | grep -q 'cpu_governance.__stale_fixture_knob__'"

    assert "config_key_census.ignore is a non-empty list of strings" \
        python3 "$_PARSE_PY" "$ORCH_YAML" census_ignore_is_nonempty_list

    # Negative fixture: the real YAML with the entire config_key_census block
    # deleted, so the vacuity check must fail loudly rather than pass over an
    # absent block vacuously.
    _CENSUS_ABSENT_FIXTURE="$(mktemp /tmp/cpu_gov_census_absent_XXXXXX.yaml)"
    trap 'rm -f "$_PARSE_PY" "${_CENSUS_STALE_FIXTURE:-}" "${_CENSUS_ABSENT_FIXTURE:-}"' EXIT
    python3 -c 'import sys,yaml; d=yaml.safe_load(open(sys.argv[1])); del d["config_key_census"]; yaml.safe_dump(d, open(sys.argv[2],"w"))' \
        "$ORCH_YAML" "$_CENSUS_ABSENT_FIXTURE"

    assert "vacuity check FAILS when the config_key_census block is absent" \
        bash -c "! python3 '$_PARSE_PY' '$_CENSUS_ABSENT_FIXTURE' census_ignore_is_nonempty_list"

    assert "vacuity failure names the absent census block" \
        bash -c "python3 '$_PARSE_PY' '$_CENSUS_ABSENT_FIXTURE' census_ignore_is_nonempty_list 2>&1 | grep -q 'config_key_census'"

    # Division of labour: the staleness check stays deliberately SILENT (exit
    # 0) on an absent census block — census_ignore_is_nonempty_list above is
    # what catches that. Pinning this prevents a later refactor from
    # conflating the two diagnostics.
    assert "census_ignore_entries_resolve stays silent (exit 0) on an absent census block" \
        python3 "$_PARSE_PY" "$_CENSUS_ABSENT_FIXTURE" census_ignore_entries_resolve

fi

# ---------------------------------------------------------------------------
# (B) KNOB-NAME CROSS-CHECK — always runs (bash grep, no python needed)
# ---------------------------------------------------------------------------
echo "--- (B) knob-name cross-check (config↔script) ---"

assert "REIFY_CPU_GOVERN_W_TASK cited in dark-factory-orchestrator.yaml" \
    grep -q "REIFY_CPU_GOVERN_W_TASK" "$ORCH_YAML"

assert "REIFY_CPU_GOVERN_W_TASK referenced in scripts/cpu-governed-exec.sh" \
    grep -q "REIFY_CPU_GOVERN_W_TASK" "$CPU_GOV"

assert "REIFY_CPU_GOVERN_W_MERGE cited in dark-factory-orchestrator.yaml" \
    grep -q "REIFY_CPU_GOVERN_W_MERGE" "$ORCH_YAML"

assert "REIFY_CPU_GOVERN_W_MERGE referenced in scripts/cpu-governed-exec.sh" \
    grep -q "REIFY_CPU_GOVERN_W_MERGE" "$CPU_GOV"

assert "REIFY_CPU_ADMIT_AGENT_THRESHOLD cited in dark-factory-orchestrator.yaml" \
    grep -q "REIFY_CPU_ADMIT_AGENT_THRESHOLD" "$ORCH_YAML"

assert "REIFY_CPU_ADMIT_AGENT_THRESHOLD referenced in scripts/agent-bin/cargo" \
    grep -q "REIFY_CPU_ADMIT_AGENT_THRESHOLD" "$AGENT_CARGO"

test_summary
