#!/usr/bin/env bash
# Tests for scripts/jobserver-tuning-harness.py — the ε measurement harness
# (task 4519, PRD docs/prds/jobserver-merge-priority-balancer.md §9/§10).
#
# RED/GREEN tests drive PURE functions over SYNTHETIC fixtures (deterministic,
# CI-fast).  No real cargo is invoked; run_regime uses a stub command.
# Committed-artifact tests (step-15) run --check on the committed JSON.
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

HARNESS="$REPO_ROOT/scripts/jobserver-tuning-harness.py"
BALANCER="$REPO_ROOT/scripts/jobserver-balancer.py"

echo "=== jobserver-tuning-harness.py tests ==="

# ──────────────────────────────────────────────────────────────────────────────
# Block 1: script contract
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 1: script contract ---"

assert "scripts/jobserver-tuning-harness.py exists" \
    test -f "$HARNESS"

assert "scripts/jobserver-tuning-harness.py is executable" \
    test -x "$HARNESS"

assert "first line is '#!/usr/bin/env python3'" \
    bash -c "head -1 '$HARNESS' | grep -qxF '#!/usr/bin/env python3'"

# ──────────────────────────────────────────────────────────────────────────────
# Block 2: busy_fraction() — /proc/stat busy-core arithmetic
#
# /proc/stat cpu line layout (positions 0-based after the 'cpu' label):
#   user nice system idle iowait irq softirq steal guest guest_nice
#   [0]  [1]  [2]    [3]  [4]    [5] [6]     [7]
#
# busy  = Σ(user+nice+system+irq+softirq+steal) delta
# idle  = Σ(idle+iowait) delta
# fraction  = busy / (busy + idle)   → float in [0, 1]
# busy_cores = fraction × nproc      → float (not rounded)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 2: busy_fraction() /proc/stat parser ---"

_b2_exit=0
{
python3 - "$HARNESS" <<'PY'
import importlib.util, sys, math

spec = importlib.util.spec_from_file_location("jth", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)   # runs module-level config only, not main()

errors = []

# ── Fixture A: simple delta, 32-core host ────────────────────────────────────
# before: user=1000 nice=0 system=200 idle=8000 iowait=100 irq=50 softirq=10 steal=0
# after:  user=1100 nice=0 system=210 idle=8080 iowait=105 irq=55 softirq=11 steal=0
# delta:  user=100  nice=0 system=10  idle=80   iowait=5   irq=5  softirq=1  steal=0
#
# busy  = 100 + 0 + 10 + 5 + 1 + 0 = 116
# idle  = 80 + 5                    = 85
# total = 201
# fraction  = 116 / 201
# busy_cores = fraction * 32

STAT_BEFORE_A = "cpu  1000 0 200 8000 100 50 10 0 0 0"
STAT_AFTER_A  = "cpu  1100 0 210 8080 105 55 11 0 0 0"
NPROC_A = 32

fraction_a, busy_cores_a = mod.busy_fraction(STAT_BEFORE_A, STAT_AFTER_A, NPROC_A)

expected_busy_a = 100 + 0 + 10 + 5 + 1 + 0        # 116
expected_idle_a = 80 + 5                            # 85
expected_total_a = expected_busy_a + expected_idle_a  # 201
expected_fraction_a = expected_busy_a / expected_total_a
expected_cores_a = expected_fraction_a * NPROC_A

EPS = 1e-10
if abs(fraction_a - expected_fraction_a) > EPS:
    errors.append(
        f"Fixture A fraction: expected {expected_fraction_a:.10f}, got {fraction_a:.10f}"
    )
if abs(busy_cores_a - expected_cores_a) > EPS:
    errors.append(
        f"Fixture A busy_cores: expected {expected_cores_a:.10f}, got {busy_cores_a:.10f}"
    )

# ── Fixture B: 100% idle ─────────────────────────────────────────────────────
# before: user=500 nice=0 system=100 idle=5000 iowait=50 irq=10 softirq=5 steal=0
# after:  user=500 nice=0 system=100 idle=5200 iowait=50 irq=10 softirq=5 steal=0
# delta:  idle=200, all busy counters zero
# fraction = 0.0, busy_cores = 0.0

STAT_BEFORE_B = "cpu  500 0 100 5000 50 10 5 0 0 0"
STAT_AFTER_B  = "cpu  500 0 100 5200 50 10 5 0 0 0"
NPROC_B = 16

fraction_b, busy_cores_b = mod.busy_fraction(STAT_BEFORE_B, STAT_AFTER_B, NPROC_B)

if abs(fraction_b - 0.0) > EPS:
    errors.append(f"Fixture B (100% idle) fraction: expected 0.0, got {fraction_b}")
if abs(busy_cores_b - 0.0) > EPS:
    errors.append(f"Fixture B (100% idle) busy_cores: expected 0.0, got {busy_cores_b}")

# ── Fixture C: 100% busy (all delta in busy counters, zero idle delta) ────────
# before: user=0 nice=0 system=0 idle=1000 iowait=0 irq=0 softirq=0 steal=0
# after:  user=400 nice=0 system=0 idle=1000 iowait=0 irq=0 softirq=0 steal=0
# delta:  user=400 everything else=0
# fraction = 1.0, busy_cores = nproc

STAT_BEFORE_C = "cpu  0 0 0 1000 0 0 0 0 0 0"
STAT_AFTER_C  = "cpu  400 0 0 1000 0 0 0 0 0 0"
NPROC_C = 8

fraction_c, busy_cores_c = mod.busy_fraction(STAT_BEFORE_C, STAT_AFTER_C, NPROC_C)

if abs(fraction_c - 1.0) > EPS:
    errors.append(f"Fixture C (100% busy) fraction: expected 1.0, got {fraction_c}")
if abs(busy_cores_c - float(NPROC_C)) > EPS:
    errors.append(f"Fixture C (100% busy) busy_cores: expected {NPROC_C}, got {busy_cores_c}")

# ── Fixture D: steal ticks count as busy ─────────────────────────────────────
# steal represents CPU cycles stolen by a hypervisor — counts as busy (not idle)
# before: user=0 nice=0 system=0 idle=1000 iowait=0 irq=0 softirq=0 steal=0
# after:  user=0 nice=0 system=0 idle=1000 iowait=0 irq=0 softirq=0 steal=50
# busy = 50, idle = 0, fraction = 1.0

STAT_BEFORE_D = "cpu  0 0 0 1000 0 0 0 0 0 0"
STAT_AFTER_D  = "cpu  0 0 0 1000 0 0 0 50 0 0"
NPROC_D = 4

fraction_d, busy_cores_d = mod.busy_fraction(STAT_BEFORE_D, STAT_AFTER_D, NPROC_D)

if abs(fraction_d - 1.0) > EPS:
    errors.append(
        f"Fixture D (steal ticks) fraction: expected 1.0, got {fraction_d}"
    )

if errors:
    for e in errors:
        print("FAIL:", e, file=__import__('sys').stderr)
    raise SystemExit(1)

print("busy_fraction: all assertions passed")
PY
} || _b2_exit=$?

assert "busy_fraction: /proc/stat arithmetic correct (all fixtures pass)" \
    test "$_b2_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 3: instruments — sample_pool_occupancy, wall-clock timer, exit-124
#
# (a) sample_pool_occupancy(merge_fifo, task_fifo) over hermetic FIFOs pre-seeded
#     with known token counts returns {merge, task, sum, timestamp}.
# (b) A wall-clock timer wrapper records elapsed seconds for a stub command.
# (c) exit-124 detection: stub exiting 124 is counted as a timeout; exit 0 is not.
#
# All assertions are hermetic (tmp FIFOs + stub commands, no real cargo).
# Fails: samplers absent → AttributeError.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 3: instruments (sample_pool_occupancy, wall-clock, exit-124) ---"

_b3_exit=0
{
python3 - "$HARNESS" <<'PY'
import importlib.util, os, sys, tempfile, time

spec = importlib.util.spec_from_file_location("jth", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

errors = []

# ── (a) sample_pool_occupancy over hermetic pre-seeded FIFOs ─────────────────
# Create two temp FIFOs and seed them with known token counts.
merge_path = tempfile.mktemp(prefix="/tmp/test-harness-merge-")
task_path  = tempfile.mktemp(prefix="/tmp/test-harness-task-")
os.mkfifo(merge_path)
os.mkfifo(task_path)

# Open both FIFOs O_RDWR (non-blocking) so we can write and read without blocking.
merge_fd = os.open(merge_path, os.O_RDWR | os.O_NONBLOCK)
task_fd  = os.open(task_path,  os.O_RDWR | os.O_NONBLOCK)

MERGE_TOKENS = 6
TASK_TOKENS  = 2

# Seed FIFOs with known token bytes
os.write(merge_fd, b'\x00' * MERGE_TOKENS)
os.write(task_fd,  b'\x00' * TASK_TOKENS)

try:
    sample = mod.sample_pool_occupancy(merge_path, task_path)

    # Must return a dict with keys: merge, task, sum, timestamp
    required_keys = {"merge", "task", "sum", "timestamp"}
    missing = required_keys - set(sample.keys())
    if missing:
        errors.append(f"sample_pool_occupancy missing keys: {missing}")
    else:
        if sample["merge"] != MERGE_TOKENS:
            errors.append(
                f"sample_pool_occupancy merge: expected {MERGE_TOKENS}, "
                f"got {sample['merge']}"
            )
        if sample["task"] != TASK_TOKENS:
            errors.append(
                f"sample_pool_occupancy task: expected {TASK_TOKENS}, "
                f"got {sample['task']}"
            )
        expected_sum = MERGE_TOKENS + TASK_TOKENS
        if sample["sum"] != expected_sum:
            errors.append(
                f"sample_pool_occupancy sum: expected {expected_sum}, "
                f"got {sample['sum']}"
            )
        # timestamp should be a positive float close to now
        if not isinstance(sample["timestamp"], float) or sample["timestamp"] <= 0:
            errors.append(
                f"sample_pool_occupancy timestamp should be positive float, "
                f"got {sample['timestamp']!r}"
            )
finally:
    os.close(merge_fd)
    os.close(task_fd)
    os.unlink(merge_path)
    os.unlink(task_path)

# ── (b) wall-clock timer wrapper ─────────────────────────────────────────────
# timed_run(cmd_list) should return (elapsed_seconds, returncode).
# Use a stub that sleeps briefly.
t0 = time.monotonic()
elapsed, rc = mod.timed_run([sys.executable, "-c", "import time; time.sleep(0.05)"])
t1 = time.monotonic()

if elapsed < 0.0:
    errors.append(f"timed_run: elapsed should be >= 0, got {elapsed}")
# Wall-clock of the outer measurement should be at least as large
if elapsed > (t1 - t0) + 0.5:
    errors.append(f"timed_run: elapsed {elapsed:.3f}s suspiciously > outer wall-clock")
if rc != 0:
    errors.append(f"timed_run: expected rc=0 for sleep stub, got {rc}")

# ── (c) exit-124 detection ───────────────────────────────────────────────────
# is_timeout(returncode) -> True iff returncode == 124
if not mod.is_timeout(124):
    errors.append("is_timeout(124) should be True")
if mod.is_timeout(0):
    errors.append("is_timeout(0) should be False")
if mod.is_timeout(1):
    errors.append("is_timeout(1) should be False")
if mod.is_timeout(-1):
    errors.append("is_timeout(-1) should be False")

if errors:
    for e in errors:
        print("FAIL:", e, file=sys.stderr)
    raise SystemExit(1)

print("instruments: all assertions passed")
PY
} || _b3_exit=$?

assert "sample_pool_occupancy returns {merge,task,sum,timestamp} with correct counts" \
    test "$_b3_exit" -eq 0

test_summary
