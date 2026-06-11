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

# ── Blocks will be added by steps 05, 07, 09, 11 ──────────────────────────

test_summary
