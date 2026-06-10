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

# ──────────────────────────────────────────────────────────────────────────────
# Block 4: run_regime() structural record — stub load, all regime/service combos
#
# Drives run_regime(regime, service, cache_state, load_cmd=<stub>) for each
# regime ∈ {just-task, just-merge, mixed} against BOTH single-pool (A) and
# dual-pool (B) services.
#
# A "stub load" is a python one-liner that exits immediately (0 or 124), so the
# test runs in under a second on any machine.  run_regime is expected to accept
# a load_cmd kwarg that replaces the real verify.sh invocation.
#
# Each returned record must carry ALL of:
#   service        ∈ {single-pool, dual-pool}
#   regime         ∈ {just-task, just-merge, mixed}
#   cache_state    ∈ {warm, cold}
#   busy_fraction  : float in [0.0, 1.0]  (CPU utilization sample)
#   occupancy      : list of {merge,task,sum,timestamp} dicts (≥1 sample)
#   merge_wall     : float ≥ 0  (wall-clock of merge verify, 0 if not run)
#   task_wall      : float ≥ 0  (wall-clock of SLOWEST task verify)
#   exit_124_count : int ≥ 0    (timeout count)
#   nproc          : int > 0    (total token budget for this run)
#
# Fails: run_regime + self-provisioning absent → AttributeError.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 4: run_regime() structural record ---"

_b4_exit=0
{
python3 - "$HARNESS" "$BALANCER" <<'PY'
import importlib.util, os, sys, tempfile

spec = importlib.util.spec_from_file_location("jth", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

BALANCER_PATH = sys.argv[2]

errors = []

# Stub command: exits 0 immediately (hermetic, no cargo)
STUB_OK  = [sys.executable, "-c", "import sys; sys.exit(0)"]
STUB_124 = [sys.executable, "-c", "import sys; sys.exit(124)"]

REQUIRED_KEYS = {
    "service", "regime", "cache_state",
    "busy_fraction", "occupancy",
    "merge_wall", "task_wall",
    "exit_124_count", "nproc",
}

def check_record(rec, label):
    """Validate all required keys are present and sane."""
    local_errors = []
    missing = REQUIRED_KEYS - set(rec.keys())
    if missing:
        local_errors.append(f"{label}: missing keys {missing}")
        return local_errors

    # service ∈ {single-pool, dual-pool}
    if rec["service"] not in (mod.SERVICE_SINGLE_POOL, mod.SERVICE_DUAL_POOL):
        local_errors.append(f"{label}: service={rec['service']!r} not valid")

    # regime ∈ known regimes
    if rec["regime"] not in mod.REGIMES:
        local_errors.append(f"{label}: regime={rec['regime']!r} not valid")

    # cache_state ∈ {warm, cold}
    if rec["cache_state"] not in (mod.CACHE_WARM, mod.CACHE_COLD):
        local_errors.append(f"{label}: cache_state={rec['cache_state']!r} not valid")

    # busy_fraction ∈ [0, 1]
    if not (0.0 <= rec["busy_fraction"] <= 1.0):
        local_errors.append(
            f"{label}: busy_fraction={rec['busy_fraction']} not in [0,1]"
        )

    # occupancy: list with ≥1 sample, each a dict with merge/task/sum/timestamp
    occ = rec["occupancy"]
    if not isinstance(occ, list) or len(occ) < 1:
        local_errors.append(f"{label}: occupancy should be list ≥1 element, got {occ!r}")
    else:
        sample = occ[0]
        for k in ("merge", "task", "sum", "timestamp"):
            if k not in sample:
                local_errors.append(f"{label}: occupancy[0] missing key {k!r}")

    # merge_wall ≥ 0
    if not (isinstance(rec["merge_wall"], (int, float)) and rec["merge_wall"] >= 0):
        local_errors.append(f"{label}: merge_wall={rec['merge_wall']!r} not ≥ 0")

    # task_wall ≥ 0
    if not (isinstance(rec["task_wall"], (int, float)) and rec["task_wall"] >= 0):
        local_errors.append(f"{label}: task_wall={rec['task_wall']!r} not ≥ 0")

    # exit_124_count ≥ 0
    if not (isinstance(rec["exit_124_count"], int) and rec["exit_124_count"] >= 0):
        local_errors.append(
            f"{label}: exit_124_count={rec['exit_124_count']!r} not int ≥ 0"
        )

    # nproc > 0
    if not (isinstance(rec["nproc"], int) and rec["nproc"] > 0):
        local_errors.append(f"{label}: nproc={rec['nproc']!r} not int > 0")

    return local_errors


# ── Test each (regime, service) combo with warm cache and a stub load ─────────
COMBOS = [
    (mod.REGIME_JUST_TASK,  mod.SERVICE_SINGLE_POOL),
    (mod.REGIME_JUST_MERGE, mod.SERVICE_SINGLE_POOL),
    (mod.REGIME_MIXED,      mod.SERVICE_SINGLE_POOL),
    (mod.REGIME_JUST_TASK,  mod.SERVICE_DUAL_POOL),
    (mod.REGIME_JUST_MERGE, mod.SERVICE_DUAL_POOL),
    (mod.REGIME_MIXED,      mod.SERVICE_DUAL_POOL),
]

for regime, service in COMBOS:
    label = f"run_regime({regime},{service},warm,stub_ok)"
    try:
        rec = mod.run_regime(
            regime=regime,
            service=service,
            cache_state=mod.CACHE_WARM,
            load_cmd=STUB_OK,
            balancer_path=BALANCER_PATH,
        )
        errors.extend(check_record(rec, label))
    except Exception as exc:
        errors.append(f"{label}: raised {type(exc).__name__}: {exc}")

# ── Verify exit_124_count is non-zero when stub exits 124 ─────────────────────
label_124 = "run_regime(just-task,single-pool,warm,stub_124)"
try:
    rec_124 = mod.run_regime(
        regime=mod.REGIME_JUST_TASK,
        service=mod.SERVICE_SINGLE_POOL,
        cache_state=mod.CACHE_WARM,
        load_cmd=STUB_124,
        balancer_path=BALANCER_PATH,
    )
    if rec_124.get("exit_124_count", 0) < 1:
        errors.append(
            f"{label_124}: exit_124_count should be ≥ 1 when stub exits 124, "
            f"got {rec_124.get('exit_124_count')}"
        )
except Exception as exc:
    errors.append(f"{label_124}: raised {type(exc).__name__}: {exc}")

if errors:
    for e in errors:
        print("FAIL:", e, file=sys.stderr)
    raise SystemExit(1)

print("run_regime: all structural assertions passed")
PY
} || _b4_exit=$?

assert "run_regime returns valid structured record for all regime/service combos" \
    test "$_b4_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 5: derive_constants() — pure arithmetic over a SYNTHETIC measurements
# record, deterministic assertions.
#
# The synthetic record has:
#   - single-pool just-task cold  → worst_case_cold_task_secs = 120.0
#   - single-pool just-merge warm → measured_merge_secs_full_alloc = 240.0
#   - single-pool just-task warm  → busy_fraction = 0.875 (baseline reference)
# MARGIN defaults to 1.5, so:
#   - task_timeout_secs  = ceil(120.0 × 1.5) = 180
#   - merge_timeout_secs = ceil(240.0 × 1.5) = 360
#
# Asserts that the derived split is merge-favored, sums to nproc, task ≥ 1,
# poll_interval ≥ harness MIN_POLL_INTERVAL, epsilon ≥ 1, timeout budgets match
# the ceil(wall × MARGIN) formula, and utilization_threshold is derived from the
# baseline capture (not a hardcoded constant).
#
# Fails: derive_constants absent → AttributeError.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 5: derive_constants() pure arithmetic ---"

_b5_exit=0
{
python3 - "$HARNESS" <<'PY'
import importlib.util, sys, math

spec = importlib.util.spec_from_file_location("jth", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

errors = []

# ── Synthetic measurements record ────────────────────────────────────────────
NPROC_F = 32
WORST_CASE_COLD_TASK = 120.0    # single-pool just-task cold task_wall
MEASURED_MERGE       = 240.0    # single-pool just-merge warm merge_wall
BASELINE_UTIL        = 0.875    # single-pool just-task warm busy_fraction (28/32)

def _run(service, regime, cache, busy, merge_wall, task_wall, occupancy_sum):
    return {
        "service":        service,
        "regime":         regime,
        "cache_state":    cache,
        "busy_fraction":  busy,
        "occupancy":      [{"merge": occupancy_sum, "task": 0, "sum": occupancy_sum,
                            "timestamp": 1.0}],
        "merge_wall":     merge_wall,
        "task_wall":      task_wall,
        "exit_124_count": 0,
        "nproc":          NPROC_F,
    }

measurements = {
    "nproc": NPROC_F,
    "runs": [
        # baseline (single-pool) — the three regimes × warm/cold
        _run("single-pool","just-task",  "warm", BASELINE_UTIL, 0.0, 90.0,  NPROC_F),
        _run("single-pool","just-task",  "cold", 0.80,          0.0, WORST_CASE_COLD_TASK, NPROC_F),
        _run("single-pool","just-merge", "warm", 0.90,          MEASURED_MERGE,   0.0, NPROC_F),
        _run("single-pool","just-merge", "cold", 0.88,          220.0,            0.0, NPROC_F),
        _run("single-pool","mixed",      "warm", 0.82,          210.0,            100.0, NPROC_F),
        _run("single-pool","mixed",      "cold", 0.78,          230.0,            115.0, NPROC_F),
        # balancer (dual-pool) — same regimes × warm/cold
        _run("dual-pool","just-task",  "warm", 0.875, 0.0,   88.0, NPROC_F),
        _run("dual-pool","just-task",  "cold", 0.80,  0.0,   118.0, NPROC_F),
        _run("dual-pool","just-merge", "warm", 0.90,  235.0, 0.0,  NPROC_F),
        _run("dual-pool","just-merge", "cold", 0.87,  215.0, 0.0,  NPROC_F),
        _run("dual-pool","mixed",      "warm", 0.84,  205.0, 92.0, NPROC_F),
        _run("dual-pool","mixed",      "cold", 0.79,  225.0, 110.0, NPROC_F),
    ],
}

derived = mod.derive_constants(measurements)

# ── (a) baseline split properties ────────────────────────────────────────────
merge_b = derived.get("merge_baseline")
task_b  = derived.get("task_baseline")

if merge_b is None or task_b is None:
    errors.append("derive_constants missing merge_baseline or task_baseline")
else:
    if not (merge_b > task_b):
        errors.append(f"split not merge-favored: merge={merge_b}, task={task_b}")
    if merge_b + task_b != NPROC_F:
        errors.append(f"split sum {merge_b+task_b} != nproc {NPROC_F}")
    if task_b < 1:
        errors.append(f"task_baseline={task_b} < 1 (starvation)")

# ── (b) poll_interval ────────────────────────────────────────────────────────
pi = derived.get("poll_interval")
if pi is None:
    errors.append("derive_constants missing poll_interval")
elif not (pi >= mod.MIN_POLL_INTERVAL and pi > 0):
    errors.append(f"poll_interval={pi} < MIN_POLL_INTERVAL={mod.MIN_POLL_INTERVAL}")

# ── (c) epsilon ──────────────────────────────────────────────────────────────
eps = derived.get("epsilon")
if eps is None:
    errors.append("derive_constants missing epsilon")
elif eps < 1:
    errors.append(f"epsilon={eps} < 1")

# ── (d) task_timeout_secs — exact: ceil(worst_case_cold_task × MARGIN) ───────
task_to = derived.get("task_timeout_secs")
expected_task_to = math.ceil(WORST_CASE_COLD_TASK * mod.MARGIN)
if task_to is None:
    errors.append("derive_constants missing task_timeout_secs")
elif task_to != expected_task_to:
    errors.append(
        f"task_timeout_secs={task_to} != ceil({WORST_CASE_COLD_TASK}×{mod.MARGIN})"
        f"={expected_task_to}"
    )

# ── (e) merge_timeout_secs — exact: ceil(measured_merge × MARGIN) ────────────
merge_to = derived.get("merge_timeout_secs")
expected_merge_to = math.ceil(MEASURED_MERGE * mod.MARGIN)
if merge_to is None:
    errors.append("derive_constants missing merge_timeout_secs")
elif merge_to != expected_merge_to:
    errors.append(
        f"merge_timeout_secs={merge_to} != ceil({MEASURED_MERGE}×{mod.MARGIN})"
        f"={expected_merge_to}"
    )

# ── (f) utilization_threshold — derived from baseline, not hardcoded ─────────
util_t = derived.get("utilization_threshold")
if util_t is None:
    errors.append("derive_constants missing utilization_threshold")
elif not (0 < util_t <= BASELINE_UTIL):
    errors.append(
        f"utilization_threshold={util_t} out of range (expected 0 < t ≤ {BASELINE_UTIL})"
    )

# ── (g) sensitivity: change worst-case input → timeout changes accordingly ───
# Make a record with a shorter worst-case cold task (60.0 s)
short_cold = dict(measurements)
short_cold_runs = [
    r for r in measurements["runs"]
    if not (r["service"] == "single-pool" and r["regime"] == "just-task"
            and r["cache_state"] == "cold")
]
short_cold_runs.append(
    _run("single-pool","just-task","cold", 0.80, 0.0, 60.0, NPROC_F)
)
short_cold = {"nproc": NPROC_F, "runs": short_cold_runs}
derived2 = mod.derive_constants(short_cold)
expected_short_to = math.ceil(60.0 * mod.MARGIN)
if derived2.get("task_timeout_secs") == expected_task_to:
    errors.append(
        "derive_constants task_timeout_secs did not change when worst-case "
        "cold task changed from 120s to 60s — may be a hardcoded constant"
    )
if derived2.get("task_timeout_secs") != expected_short_to:
    errors.append(
        f"derive_constants sensitivity: expected task_timeout_secs="
        f"{expected_short_to} for 60s worst-case, got {derived2.get('task_timeout_secs')}"
    )

if errors:
    for e in errors:
        print("FAIL:", e, file=sys.stderr)
    raise SystemExit(1)

print("derive_constants: all assertions passed")
PY
} || _b5_exit=$?

assert "derive_constants: split/poll/epsilon/timeout/utilization derived correctly" \
    test "$_b5_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 6: evaluate_acceptance() — four synthetic scenarios
#
# (a) all runs clear floors in all 3 regimes (warm+cold) → ok=True, no ESCAPE_VALVE
# (b) utilization < threshold in one regime → ok=False
# (c) worst-case cold task at implicit-only > MAX_SANE_TIMEOUT → ok=True but
#     a structured FINDING with code ESCAPE_VALVE is emitted (§10.4)
# (d) a sum != nproc snapshot in occupancy series → TOKEN_CONSERVATION finding
#
# Asserts the measure→derive→ASSERT gate logic over synthetic numbers.
# Fails: evaluate_acceptance absent → AttributeError.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 6: evaluate_acceptance() acceptance gate ---"

_b6_exit=0
{
python3 - "$HARNESS" <<'PY'
import importlib.util, sys

spec = importlib.util.spec_from_file_location("jth", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

errors = []

NPROC_F = 32
THRESHOLD = 0.70   # utilization floor the derived dict will carry

# Reusable derived dict (matches the synthetic measurements from Block 5)
DERIVED = {
    "merge_baseline":        24,
    "task_baseline":         8,
    "poll_interval":         0.10,
    "epsilon":               1,
    "task_timeout_secs":     180,
    "merge_timeout_secs":    360,
    "utilization_threshold": THRESHOLD,
}

def _run(service, regime, cache, busy, merge_wall, task_wall, occ_sum=NPROC_F, exit_124=0):
    return {
        "service":        service,
        "regime":         regime,
        "cache_state":    cache,
        "busy_fraction":  busy,
        "occupancy":      [{"merge": occ_sum, "task": 0, "sum": occ_sum, "timestamp": 1.0}],
        "merge_wall":     merge_wall,
        "task_wall":      task_wall,
        "exit_124_count": exit_124,
        "nproc":          NPROC_F,
    }

# Helper: build a full 12-run record with all regimes clearing the threshold
def _good_measurements():
    return {
        "nproc": NPROC_F,
        "runs": [
            _run("single-pool","just-task",  "warm", 0.875, 0.0,  90.0),
            _run("single-pool","just-task",  "cold", 0.800, 0.0, 120.0),
            _run("single-pool","just-merge", "warm", 0.900, 240.0, 0.0),
            _run("single-pool","just-merge", "cold", 0.880, 220.0, 0.0),
            _run("single-pool","mixed",      "warm", 0.820, 210.0, 100.0),
            _run("single-pool","mixed",      "cold", 0.780, 230.0, 115.0),
            _run("dual-pool",  "just-task",  "warm", 0.875, 0.0,  88.0),
            _run("dual-pool",  "just-task",  "cold", 0.800, 0.0, 118.0),
            _run("dual-pool",  "just-merge", "warm", 0.900, 235.0, 0.0),
            _run("dual-pool",  "just-merge", "cold", 0.870, 215.0, 0.0),
            _run("dual-pool",  "mixed",      "warm", 0.840, 205.0,  92.0),
            _run("dual-pool",  "mixed",      "cold", 0.790, 225.0, 110.0),
        ],
    }

# ── (a) All clear → ok=True, no ESCAPE_VALVE finding ─────────────────────────
m_good = _good_measurements()
ok_a, findings_a = mod.evaluate_acceptance(m_good, DERIVED)

if not ok_a:
    errors.append("(a) all-clear: expected ok=True, got False")
escape_findings = [f for f in findings_a if f.get("code") == "ESCAPE_VALVE"]
if escape_findings:
    errors.append(f"(a) all-clear: unexpected ESCAPE_VALVE finding: {escape_findings}")

# ── (b) Utilization below threshold in one regime → ok=False ─────────────────
# Inject a run where busy_fraction < THRESHOLD
m_bad_util = dict(_good_measurements())
m_bad_util["runs"] = list(m_bad_util["runs"])  # copy
# Replace single-pool just-task warm with a below-threshold run
m_bad_util["runs"] = [
    r if not (r["service"] == "single-pool" and r["regime"] == "just-task"
              and r["cache_state"] == "warm")
    else _run("single-pool","just-task","warm", THRESHOLD - 0.10, 0.0, 90.0)
    for r in m_bad_util["runs"]
]
ok_b, findings_b = mod.evaluate_acceptance(m_bad_util, DERIVED)

if ok_b:
    errors.append("(b) below-threshold: expected ok=False, got True")
util_findings = [f for f in findings_b
                 if f.get("code") in ("UTILIZATION_FAIL", "UTILIZATION_LOW",
                                      "UTILIZATION")]
if not util_findings:
    errors.append("(b) below-threshold: no utilization finding in results")

# ── (c) worst-case cold task > MAX_SANE_TIMEOUT → escape-valve FINDING ────────
# Build a record where single-pool just-task cold has a huge wall-clock
huge_wall = mod.MAX_SANE_TIMEOUT + 600.0   # safely beyond the sane ceiling
m_insane = dict(_good_measurements())
m_insane["runs"] = [
    r if not (r["service"] == "single-pool" and r["regime"] == "just-task"
              and r["cache_state"] == "cold")
    else _run("single-pool","just-task","cold", 0.80, 0.0, huge_wall)
    for r in m_insane["runs"]
]
# Derive updated constants for this record
derived_c = mod.derive_constants(m_insane)

ok_c, findings_c = mod.evaluate_acceptance(m_insane, derived_c)

# Escape valve: finding MUST be emitted but ok should still be True (§10.4
# — the finding IS the honest signal, not a hard fail)
escape_c = [f for f in findings_c if f.get("code") == "ESCAPE_VALVE"]
if not escape_c:
    errors.append(
        f"(c) insane timeout: expected ESCAPE_VALVE finding for "
        f"task_wall={huge_wall:.0f}s > MAX_SANE_TIMEOUT={mod.MAX_SANE_TIMEOUT}"
    )
if not ok_c:
    errors.append(
        "(c) insane timeout: expected ok=True (escape-valve is honest surfacing, "
        "not a hard fail); got ok=False"
    )

# ── (d) Occupancy sum != nproc → TOKEN_CONSERVATION finding ──────────────────
m_bad_sum = dict(_good_measurements())
m_bad_sum["runs"] = [
    r if not (r["service"] == "single-pool" and r["regime"] == "just-task"
              and r["cache_state"] == "warm")
    else {**r, "occupancy": [{"merge": NPROC_F - 2, "task": 0, "sum": NPROC_F - 2,
                               "timestamp": 1.0}]}
    for r in m_bad_sum["runs"]
]
_ok_d, findings_d = mod.evaluate_acceptance(m_bad_sum, DERIVED)

token_findings = [f for f in findings_d
                  if f.get("code") in ("TOKEN_CONSERVATION", "TOKEN_LEAK",
                                       "CONSERVATION")]
if not token_findings:
    errors.append(
        f"(d) sum!=nproc: expected a token-conservation finding, "
        f"got findings={[f.get('code') for f in findings_d]}"
    )

if errors:
    for e in errors:
        print("FAIL:", e, file=sys.stderr)
    raise SystemExit(1)

print("evaluate_acceptance: all assertions passed")
PY
} || _b6_exit=$?

assert "evaluate_acceptance: ok/findings correct for all four synthetic scenarios" \
    test "$_b6_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 7: render_report() — output contract over a synthetic triple
#
# Asserts the returned markdown contains the required STRUCTURAL MARKERS:
#   - A/B comparison section (baseline vs balancer)
#   - All three regime names: just-task, just-merge, mixed
#   - warm AND cold cache-state rows
#   - A derived-constants block with the key names:
#       merge_baseline, task_baseline, poll_interval, epsilon,
#       task_timeout_secs, merge_timeout_secs, utilization_threshold
#   - A findings/escape-valve section
#
# Asserts output contract (section markers + key names), NOT prose wording.
# Fails: render_report absent → AttributeError.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 7: render_report() output contract ---"

_b7_exit=0
{
python3 - "$HARNESS" <<'PY'
import importlib.util, sys

spec = importlib.util.spec_from_file_location("jth", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

errors = []

# Reuse the good-measurements fixture and derive+evaluate
NPROC_F = 32
def _run(service, regime, cache, busy, merge_wall, task_wall, occ_sum=NPROC_F):
    return {
        "service":        service,
        "regime":         regime,
        "cache_state":    cache,
        "busy_fraction":  busy,
        "occupancy":      [{"merge": occ_sum, "task": 0, "sum": occ_sum,
                            "timestamp": 1.0}],
        "merge_wall":     merge_wall,
        "task_wall":      task_wall,
        "exit_124_count": 0,
        "nproc":          NPROC_F,
    }

measurements = {
    "nproc": NPROC_F,
    "runs": [
        _run("single-pool","just-task",  "warm", 0.875, 0.0,  90.0),
        _run("single-pool","just-task",  "cold", 0.800, 0.0, 120.0),
        _run("single-pool","just-merge", "warm", 0.900, 240.0, 0.0),
        _run("single-pool","just-merge", "cold", 0.880, 220.0, 0.0),
        _run("single-pool","mixed",      "warm", 0.820, 210.0, 100.0),
        _run("single-pool","mixed",      "cold", 0.780, 230.0, 115.0),
        _run("dual-pool",  "just-task",  "warm", 0.875, 0.0,  88.0),
        _run("dual-pool",  "just-task",  "cold", 0.800, 0.0, 118.0),
        _run("dual-pool",  "just-merge", "warm", 0.900, 235.0, 0.0),
        _run("dual-pool",  "just-merge", "cold", 0.870, 215.0, 0.0),
        _run("dual-pool",  "mixed",      "warm", 0.840, 205.0,  92.0),
        _run("dual-pool",  "mixed",      "cold", 0.790, 225.0, 110.0),
    ],
}
derived = mod.derive_constants(measurements)
ok, findings = mod.evaluate_acceptance(measurements, derived)

# Call render_report — returns a markdown string
report = mod.render_report(measurements, derived, ok, findings)

# ── Structural markers ────────────────────────────────────────────────────────
REQUIRED_MARKERS = [
    # A/B comparison section
    "single-pool",   "dual-pool",
    # All three regime names
    "just-task",     "just-merge",   "mixed",
    # Both cache states
    "warm",          "cold",
    # Derived-constants block — key names must appear in the report
    "merge_baseline", "task_baseline",
    "poll_interval",  "epsilon",
    "task_timeout_secs", "merge_timeout_secs",
    "utilization_threshold",
    # Findings / escape-valve section
    "findings",
]

for marker in REQUIRED_MARKERS:
    if marker.lower() not in report.lower():
        errors.append(f"render_report missing required marker: {marker!r}")

# ── Report must be a non-empty string ────────────────────────────────────────
if not isinstance(report, str) or len(report) < 100:
    errors.append(
        f"render_report returned too-short string ({len(report) if isinstance(report, str) else type(report).__name__})"
    )

if errors:
    for e in errors:
        print("FAIL:", e, file=sys.stderr)
    raise SystemExit(1)

print("render_report: all structural markers present")
PY
} || _b7_exit=$?

assert "render_report: markdown contains A/B + regime + cache + constants + findings sections" \
    test "$_b7_exit" -eq 0

test_summary
