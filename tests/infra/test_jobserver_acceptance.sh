#!/usr/bin/env bash
# Tests for scripts/jobserver-acceptance.py — the end-to-end mixed-load
# acceptance gate for the dual-FIFO jobserver priority balancer
# (task η/4521, PRD §9 leaf, docs/prds/jobserver-merge-priority-balancer.md).
#
# ALL tests here are HERMETIC: mktemp FIFOs, importlib-loaded Python stubs,
# PATH-stubbed systemctl where needed.  The real ~tens-of-minutes A/B campaign
# lives behind the harness's `--run` mode (capstone step-13), never here.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

ACCEPT="$REPO_ROOT/scripts/jobserver-acceptance.py"
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

[ -f "$ACCEPT" ] || { echo "ERROR: $ACCEPT not found"; exit 1; }
[ -f "$SETUP_DEV" ] || { echo "ERROR: $SETUP_DEV not found"; exit 1; }

# Verify the acceptance harness loads without error (importlib + argparse).
assert "jobserver-acceptance.py loads without error (--help exits 0)" \
    python3 "$ACCEPT" --help

# ──────────────────────────────────────────────────────────────────────────────
# Block 1: pure analyzer unit tests (step-03 / step-04)
#   Importlib-load jobserver-acceptance.py (hyphenated → not importable by
#   name) via spec_from_file_location.  All fixtures are inline literals.
#
#   (i)  merge_reached_full_allocation(series, nproc) — criterion (d)
#          True  when any during-contention sample has merge == nproc
#          False when merge never reaches nproc
#
#   (ii) utilization_ok(busy_frac, threshold) — criterion (a)
#          True  when busy_frac >= threshold
#          False otherwise
#
#   RED before step-04: neither symbol exists → AttributeError → exit 1.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 1: pure analyzer unit tests (merge_reached_full_allocation + utilization_ok) ---"

_b1_exit=0
{
python3 - "$ACCEPT" <<'PY'
import importlib.util, sys

spec = importlib.util.spec_from_file_location("ja", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)   # runs module-level config only, not main()

errors = []
NPROC = 8

# ── merge_reached_full_allocation ─────────────────────────────────────────

# (a) Series with one sample showing merge==nproc, task==0 → True
series_full = [
    {"merge": 4, "task": 4, "ts": 0.0},
    {"merge": 8, "task": 0, "ts": 0.1},   # merge reached nproc == 8
    {"merge": 6, "task": 2, "ts": 0.2},
]
if not mod.merge_reached_full_allocation(series_full, NPROC):
    errors.append("(a) expected True for series with merge==nproc sample")

# (b) Series where merge never reaches nproc → False
series_short = [
    {"merge": 4, "task": 4, "ts": 0.0},
    {"merge": 6, "task": 2, "ts": 0.1},
    {"merge": 7, "task": 1, "ts": 0.2},
]
if mod.merge_reached_full_allocation(series_short, NPROC):
    errors.append("(b) expected False for series where merge never reaches nproc")

# (c) Empty series → False (no sample, cannot assert full allocation)
if mod.merge_reached_full_allocation([], NPROC):
    errors.append("(c) expected False for empty series")

# ── utilization_ok ────────────────────────────────────────────────────────

# (d) busy_frac >= threshold → True
if not mod.utilization_ok(0.92, 0.85):
    errors.append("(d) expected True for 0.92 >= 0.85")

# (e) busy_frac == threshold → True (boundary)
if not mod.utilization_ok(0.85, 0.85):
    errors.append("(e) expected True for exact boundary 0.85 == 0.85")

# (f) busy_frac < threshold → False
if mod.utilization_ok(0.70, 0.85):
    errors.append("(f) expected False for 0.70 < 0.85")

if errors:
    sys.stderr.write("FAIL analyzers:\n" + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
print("OK: analyzers")
PY
} || _b1_exit=$?

assert "merge_reached_full_allocation + utilization_ok unit tests pass" \
    test "$_b1_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 2: evaluate_acceptance_gate unit tests (step-05 / step-06)
#   Tests the gate evaluator on synthetic same-instrument A/B measurements.
#   Fixtures are inline Python literals.
#
#   (all-pass) → ok=True, all four verdicts pass, no findings
#   (a-fail)   → busy_fraction below threshold → ok=False, criterion a FAIL
#   (b-fail)   → dual-pool merge wall ≥ baseline → ok=False, criterion b FAIL
#   (c-fail)   → exit_124_count > 0 → ok=False, criterion c FAIL with §10.4
#               escape-valve note in findings
#   (d-fail)   → merge never reaches nproc → ok=False, criterion d FAIL
#
#   RED before step-06: evaluate_acceptance_gate absent → AttributeError.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 2: evaluate_acceptance_gate unit tests ---"

_b2_exit=0
{
python3 - "$ACCEPT" <<'PY'
import importlib.util, sys

spec = importlib.util.spec_from_file_location("ja", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

errors = []

NPROC = 8
THRESH = 0.85

def make_ok_measurements():
    """All-pass synthetic A/B fixture."""
    return {
        "nproc": NPROC,
        "utilization_threshold": THRESH,
        "baseline": {
            "service": "single-pool",
            "regime": "mixed",
            "cache_state": "warm",
            "busy_fraction": 0.96,
            "merge_wall": 120.0,
            "task_walls": [100.0, 115.0],
            "exit_124_count": 0,
            "occupancy": [{"merge": 4, "task": 4, "ts": 0.0}],
        },
        "dual_pool": {
            "service": "dual-pool",
            "regime": "mixed",
            "cache_state": "warm",
            "busy_fraction": 0.97,
            "merge_wall": 95.0,
            "task_walls": [105.0, 110.0],
            "exit_124_count": 0,
            "occupancy": [
                {"merge": 4, "task": 4, "ts": 0.0},
                {"merge": 8, "task": 0, "ts": 0.5},
            ],
        },
    }

# ── all-pass fixture ──────────────────────────────────────────────────────
m = make_ok_measurements()
ok, verdicts, findings = mod.evaluate_acceptance_gate(m)
if not ok:
    errors.append(f"(all-pass) ok=False, findings={findings}")
for crit in ("a", "b", "c", "d"):
    if verdicts.get(crit) != "PASS":
        errors.append(f"(all-pass) criterion {crit} verdict={verdicts.get(crit)!r}, want PASS")
if findings:
    errors.append(f"(all-pass) unexpected findings: {findings}")

# ── (a) fails: busy_fraction below threshold ──────────────────────────────
m_a = make_ok_measurements()
m_a["dual_pool"]["busy_fraction"] = 0.60   # below 0.85 threshold
ok_a, verdicts_a, findings_a = mod.evaluate_acceptance_gate(m_a)
if ok_a:
    errors.append("(a-fail) expected ok=False when busy_fraction < threshold")
if verdicts_a.get("a") != "FAIL":
    errors.append(f"(a-fail) criterion a verdict={verdicts_a.get('a')!r}, want FAIL")

# ── (b) fails: dual-pool merge wall ≥ baseline ────────────────────────────
m_b = make_ok_measurements()
m_b["dual_pool"]["merge_wall"] = 150.0   # worse than baseline 120.0
ok_b, verdicts_b, findings_b = mod.evaluate_acceptance_gate(m_b)
if ok_b:
    errors.append("(b-fail) expected ok=False when dual-pool merge_wall >= baseline")
if verdicts_b.get("b") != "FAIL":
    errors.append(f"(b-fail) criterion b verdict={verdicts_b.get('b')!r}, want FAIL")

# ── (c) fails: exit_124_count > 0 with §10.4 escape-valve note ───────────
m_c = make_ok_measurements()
m_c["dual_pool"]["exit_124_count"] = 1
ok_c, verdicts_c, findings_c = mod.evaluate_acceptance_gate(m_c)
if ok_c:
    errors.append("(c-fail) expected ok=False when exit_124_count > 0")
if verdicts_c.get("c") != "FAIL":
    errors.append(f"(c-fail) criterion c verdict={verdicts_c.get('c')!r}, want FAIL")
# §10.4 escape-valve note must appear in findings
escape_valve_present = any("10.4" in f or "escape" in f.lower() or "absolute priority" in f.lower() for f in findings_c)
if not escape_valve_present:
    errors.append(f"(c-fail) §10.4 escape-valve note missing from findings: {findings_c}")

# ── (d) fails: merge never reaches nproc ─────────────────────────────────
m_d = make_ok_measurements()
m_d["dual_pool"]["occupancy"] = [
    {"merge": 4, "task": 4, "ts": 0.0},
    {"merge": 6, "task": 2, "ts": 0.5},
]  # merge reaches max 6, not NPROC=8
ok_d, verdicts_d, findings_d = mod.evaluate_acceptance_gate(m_d)
if ok_d:
    errors.append("(d-fail) expected ok=False when merge never reaches nproc")
if verdicts_d.get("d") != "FAIL":
    errors.append(f"(d-fail) criterion d verdict={verdicts_d.get('d')!r}, want FAIL")

if errors:
    sys.stderr.write("FAIL evaluate_acceptance_gate:\n" + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
print("OK: evaluate_acceptance_gate")
PY
} || _b2_exit=$?

assert "evaluate_acceptance_gate unit tests pass" \
    test "$_b2_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 3: render_acceptance_report unit tests (step-07 / step-08)
#   Tests that the renderer produces markdown containing the required sections.
#   Fixture is the same all-pass inline literal from Block 2.
#
#   Required in the rendered markdown:
#     - four (a)–(d) verdicts with PASS/FAIL labels
#     - dual-pool merge wall-clock value
#     - slowest task wall-clock value (max of task_walls)
#     - cache_state column (warm/cold)
#     - A/B comparison section (baseline vs dual-pool)
#     - "ζ′/4520 budget floor" section with merge wall + slowest task wall
#
#   RED before step-08: render_acceptance_report absent → AttributeError.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 3: render_acceptance_report unit tests ---"

_b3_exit=0
{
python3 - "$ACCEPT" <<'PY'
import importlib.util, sys

spec = importlib.util.spec_from_file_location("ja", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

errors = []

measurements = {
    "nproc": 8,
    "utilization_threshold": 0.85,
    "baseline": {
        "service": "single-pool",
        "regime": "mixed",
        "cache_state": "warm",
        "busy_fraction": 0.96,
        "merge_wall": 120.0,
        "task_walls": [100.0, 115.0],
        "exit_124_count": 0,
        "occupancy": [{"merge": 4, "task": 4, "ts": 0.0}],
    },
    "dual_pool": {
        "service": "dual-pool",
        "regime": "mixed",
        "cache_state": "warm",
        "busy_fraction": 0.97,
        "merge_wall": 95.0,
        "task_walls": [105.0, 110.0],
        "exit_124_count": 0,
        "occupancy": [
            {"merge": 4, "task": 4, "ts": 0.0},
            {"merge": 8, "task": 0, "ts": 0.5},
        ],
    },
}
ok, verdicts, findings = mod.evaluate_acceptance_gate(measurements)
report = mod.render_acceptance_report(measurements, verdicts)

if not isinstance(report, str) or len(report) == 0:
    errors.append("report is empty or not a string")
    sys.stderr.write("FAIL render_acceptance_report:\n  " + "\n  ".join(errors) + "\n")
    sys.exit(1)

# Per-criterion PASS/FAIL labels
for crit in ("a", "b", "c", "d"):
    label = f"({crit})"
    if label not in report:
        errors.append(f"criterion label {label!r} not in report")
    if "PASS" not in report and "FAIL" not in report:
        errors.append("neither PASS nor FAIL appears in report")

# Dual-pool merge wall value
if "95" not in report:
    errors.append("dual-pool merge_wall=95.0 not found in report")

# Slowest task wall (max(105, 110) = 110)
if "110" not in report:
    errors.append("slowest task_wall=110.0 not found in report")

# cache_state column present
if "warm" not in report:
    errors.append("cache_state 'warm' not in report")

# A/B comparison section
if "baseline" not in report.lower() and "single-pool" not in report.lower():
    errors.append("A/B comparison section (baseline) missing from report")
if "dual-pool" not in report.lower() and "dual_pool" not in report.lower():
    errors.append("A/B comparison section (dual-pool) missing from report")

# ζ′/4520 budget floor section
if "4520" not in report:
    errors.append("ζ′/4520 budget floor section missing from report")
if "budget floor" not in report.lower():
    errors.append("'budget floor' heading missing from report")

if errors:
    sys.stderr.write("FAIL render_acceptance_report:\n" + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
print("OK: render_acceptance_report")
PY
} || _b3_exit=$?

assert "render_acceptance_report unit tests pass" \
    test "$_b3_exit" -eq 0

# ── Blocks will be added by steps 09, 11 ──────────────────────────────────

test_summary
