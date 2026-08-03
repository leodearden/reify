#!/usr/bin/env bash
# Infrastructure test for task 4607 (prd-gate-exec α — capability probe runner).
# Verifies that:
#   1. python3 is on PATH
#   2. scripts/test_prd_capability_check.py (stdlib unittest) exits 0
#   3. scripts/prd-capability-check.py --help exits 0 (CLI smoke)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.
#
# Grammar-substrate preflight (5894).  In a sandboxed agent role tree-sitter
# cannot write ~/.cache/tree-sitter/lock/, so it cannot load the reify grammar
# and the suite's one grammar e2e used to report HARNESS_ERROR (exit 70) — a
# spurious FAIL where the house rule is a clean SKIP.  The suite now self-skips
# that test; this preflight exists so the log SAYS SO, because the original
# failure's real cost was attribution, not the red itself.
#
# UNLIKE tests/infra/test_prd_gate_corpus.sh, whose identical-looking guard
# early-exits, this one deliberately does NOT gate: the corpus gate's entire
# payload is the probe run, whereas here exactly ONE of the suite's unit tests
# needs the grammar substrate and every other one is hermetic (count left
# unstated on purpose — it moves).  Skipping the script would trade a spurious
# RED for a silent whole-suite coverage hole in exactly the sandboxed roles this
# task exists to serve.  The SKIP line is informational; both asserts below
# still run.
#
# The preflight below runs a real tree-sitter subprocess, but a time-bounded one
# (_SUBSTRATE_PROBE_TIMEOUT_S in prd-capability-check.py), so a tree-sitter
# wedged on its own grammar lock reports unusable instead of hanging this run.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_prd_capability_check ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Grammar-substrate preflight (informational — never gates) ─────────────
# The `|| SUBSTRATE_RC=$?` tail is load-bearing twice over: it keeps `set -e`
# from aborting on the expected exit 75, and it captures the real code —
# `... || true` followed by `$?` would read 0, since `$?` would then be the
# status of the `|| true` compound rather than of the command substitution.
SUBSTRATE_RC=0
SUBSTRATE_STATUS="$(python3 "$REPO_ROOT/scripts/prd-capability-check.py" \
    --grammar-substrate-status 2>/dev/null)" || SUBSTRATE_RC=$?
if [ "$SUBSTRATE_RC" -eq 75 ]; then
    echo "SKIP: grammar e2e — ${SUBSTRATE_STATUS#grammar substrate: unusable: }"
    echo "      (the other unit tests are hermetic and still run)"
fi

# ── Unit tests ────────────────────────────────────────────────────────────
assert "scripts/test_prd_capability_check.py exits 0" \
    python3 "$REPO_ROOT/scripts/test_prd_capability_check.py"

# ── CLI smoke ─────────────────────────────────────────────────────────────
assert "scripts/prd-capability-check.py --help exits 0" \
    python3 "$REPO_ROOT/scripts/prd-capability-check.py" --help

test_summary
