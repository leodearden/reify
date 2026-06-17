#!/usr/bin/env bash
# tests/infra/test_cpu_load_governance.sh — §8 integration-gate leaf (task 4634).
#
# Proves that the α/β/γ primitives COMPOSE:
#   α  scripts/cpu-admit.sh          — PSI admission gate
#   β  scripts/agent-bin/cargo       — agent cargo shim (heavy-subcmd gate)
#   γ  scripts/cpu-governed-exec.sh  — cgroup-v2 cpu.weight placement wrapper
#
# §8 boundary-table rows covered:
#   Row 1  lone governed source, box idle → busy-core fraction ≥ 0.95·nproc,
#           cpu.max == max (no quota throttle)                        host-gated
#   Row 2  heavy mix → after warm-up avg10 < AGENT_THRESHOLD         host-gated
#   Row 3  governed probe under mix → slowdown within fair-share band host-gated
#   Row 4  merge-favored share ≥ W_merge/(W_merge+W_task)−tol        host-gated
#
# ALWAYS-ON (even on substrate-absent CI):
#   Cycle SELF  — pure-analyzer + instrument-reuse self-tests via
#                 cpu_gov_instrument.py selftest (hermetic, never vacuous)
#   Cycle FIXTURE — fixture-generator contract (PSI/proc-stat gated)
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).
# Helper files (cpu_load_fixture.sh, cpu_gov_instrument.py) are NOT test_*.sh
# so are not auto-run.
#
# §8 rows map to Cycles ROW1/ROW2_3/ROW4, each individually skipped when
# the host precondition is unmet — never false-fails on a hot shared host.
#
# Design decisions honored here:
#   G6 CRUX: all bounds PSI-relative/ratio/self-relative with a STATED
#             fair-share floor; NEVER absolute load==32.
#   Q5: warm-up default 8 s (knob REIFY_CPU_GOV_TEST_WARMUP_S).
#   Q2: W_task=100 / W_merge=300 (γ defaults, not retuned).
#   Row 4: private hermetic slices via REIFY_CPU_GOVERN_SLICE_TASK/MERGE overrides.
#
# KNOBS:
#   REIFY_CPU_GOV_TEST_WARMUP_S         warm-up window seconds (default 8)
#   REIFY_CPU_GOV_TEST_BUDGET_S         overall live-section timeout (default 120)
#   REIFY_CPU_GOV_TEST_MIXFACTOR        oversubscription factor (default 1.5)
#   REIFY_CPU_GOV_TEST_SLOWDOWN_K       slowdown upper-band multiplier (default 4)
#   REIFY_CPU_GOV_TEST_QUIET_CEILING    avg10 max for quiet-box precondition (default 20)
#   REIFY_CPU_GOV_TEST_BURN_S           per-fixture burn duration seconds (default 4)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

CPU_ADMIT="$REPO_ROOT/scripts/cpu-admit.sh"
CPU_GOV_EXEC="$REPO_ROOT/scripts/cpu-governed-exec.sh"
LIB_CGROUP="$REPO_ROOT/scripts/lib_cgroup.sh"
FIXTURE="$SCRIPT_DIR/cpu_load_fixture.sh"
INSTRUMENT="$SCRIPT_DIR/cpu_gov_instrument.py"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== cpu-load-governance integration tests (task 4634) ==="

# ---------------------------------------------------------------------------
# Substrate skip-guards (a) and (b) — always checked first.
# ---------------------------------------------------------------------------

# (a) PSI must be readable — required for cpu-admit.sh and Row 2 avg10 sampling.
if [ ! -r /proc/pressure/cpu ]; then
    echo "SKIP: kernel lacks /proc/pressure/cpu (PSI gate is Linux-only)"
    # Still run the pure-analyzer self-tests below (they do NOT need PSI).
    _PSI_AVAILABLE=0
else
    _PSI_AVAILABLE=1
fi

# (b) python3 must be on PATH — required for cpu_gov_instrument.py.
if ! command -v python3 >/dev/null 2>&1; then
    echo "SKIP: python3 not on PATH — all instrument-based cycles will be skipped"
    _PYTHON_AVAILABLE=0
else
    _PYTHON_AVAILABLE=1
fi

# ---------------------------------------------------------------------------
# host_supports_governance — gate helper for live cgroup placement scenarios.
# Copies the idiom from test_cpu_governed_exec.sh:46-54 verbatim.
# Returns 0 if the host can run governed placement, 1 otherwise.
# ---------------------------------------------------------------------------
host_supports_governance() {
    [ -f "$LIB_CGROUP" ] || return 1
    (
        # shellcheck source=scripts/lib_cgroup.sh
        source "$LIB_CGROUP"
        cgroup_governance_supported
    )
}

# ---------------------------------------------------------------------------
# live_or_skip — wrapper for the entire live (cgroup-dependent) section.
#
# Usage:
#   live_or_skip <label> <timeout_s> <function_name>
#
# Checks host_supports_governance; if not supported prints SKIP and returns 0.
# Otherwise runs <function_name> wrapped in a timeout of <timeout_s> seconds.
# If the timeout fires (exit 124) prints SKIP (not FAIL) and returns 0.
# This protects the shared 20-min run_all.sh wall on a slow/contended host.
#
# (stub at skeleton stage — implementations added per-row in later steps)
# ---------------------------------------------------------------------------
_LIVE_BUDGET_S="${REIFY_CPU_GOV_TEST_BUDGET_S:-120}"

live_or_skip() {
    local label="$1"
    local budget_s="$2"
    local fn_name="$3"

    if ! host_supports_governance; then
        echo "  SKIP ${label}: host does not support cgroup governance"
        return 0
    fi

    local rc=0
    timeout "${budget_s}" bash -c "
        # Re-source helpers inside the timeout subshell.
        source '${SCRIPT_DIR}/test_helpers.sh'
        ${fn_name}
    " || rc=$?

    if [ "$rc" -eq 124 ]; then
        echo "  SKIP ${label}: live section budget (${budget_s}s) expired — host too slow/contended"
        return 0
    fi
    return "$rc"
}

# ---------------------------------------------------------------------------
# Hermetic workdir — cleaned up on EXIT.
# ---------------------------------------------------------------------------
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# ============================================================================
# Cycle SELF — pure-analyzer + instrument-reuse self-tests.
# Always runs regardless of PSI/cgroup substrate availability.
# (Tests added in step-1, implementation in step-2.)
# ============================================================================
echo ""
echo "--- Cycle SELF: pure-analyzer self-tests via cpu_gov_instrument.py ---"

# ============================================================================
# Cycle FIXTURE — fixture-generator contract.
# (Tests added in step-3, implementation in step-4.)
# ============================================================================
echo ""
echo "--- Cycle FIXTURE: cpu_load_fixture.sh contract ---"

# ============================================================================
# Cycle ROW1 — §8 Row 1: lone governed source, box idle.
# (Tests added in step-5, implementation in step-6.)
# ============================================================================
echo ""
echo "--- Cycle ROW1: §8 Row 1 (lone governed source, box idle) ---"

if ! host_supports_governance; then
    echo "  SKIP ROW1: host does not support cgroup governance"
fi

# ============================================================================
# Cycle ROW2_3 — §8 Rows 2+3: heavy mix + bounded slowdown.
# (Tests added in step-7, implementation in step-8.)
# ============================================================================
echo ""
echo "--- Cycle ROW2_3: §8 Rows 2+3 (heavy mix + bounded slowdown) ---"

if ! host_supports_governance; then
    echo "  SKIP ROW2_3: host does not support cgroup governance"
fi

# ============================================================================
# Cycle ROW4 — §8 Row 4: merge-favored share in private slices.
# (Tests added in step-9, implementation in step-10.)
# ============================================================================
echo ""
echo "--- Cycle ROW4: §8 Row 4 (merge-favored share, private slices) ---"

if ! host_supports_governance; then
    echo "  SKIP ROW4: host does not support cgroup governance"
fi

# ---------------------------------------------------------------------------
# Final summary — PASS/FAIL count from test_helpers.sh.
# ---------------------------------------------------------------------------
test_summary
