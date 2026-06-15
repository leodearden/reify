#!/usr/bin/env bash
# Infrastructure test for task 4608 (prd-gate-exec γ — decompose-phase verification).
# Verifies that:
#   1. python3 is on PATH
#   2. scripts/test_prd_decompose_verify.py (stdlib unittest) exits 0
#   3. CLI smoke: scripts/prd-decompose-verify.py --help exits 0
#   4. CLI smoke: synthesize on an all-PASS results fixture exits 0
#   5. CLI smoke: synthesize on a FAIL results fixture exits 1
#   6. (skip-guarded) node --check scripts/prd-decompose-verify.mjs exits 0
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

echo "=== test_prd_decompose_verify ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "scripts/test_prd_decompose_verify.py exits 0" \
    python3 "$REPO_ROOT/scripts/test_prd_decompose_verify.py"

# ── CLI smoke: --help ─────────────────────────────────────────────────────
assert "scripts/prd-decompose-verify.py --help exits 0" \
    python3 "$REPO_ROOT/scripts/prd-decompose-verify.py" --help

# ── CLI smoke: synthesize all-PASS → exit 0 ───────────────────────────────
# Write a synthetic all-PASS results fixture to a temp file.
_TMP_PASS="$(mktemp /tmp/pdv_smoke_pass_XXXXXX.json)"
cat > "$_TMP_PASS" <<'EOJSON'
{
    "prover": [
        {
            "capability": "smoke-test-capability",
            "probe_kind": "check",
            "verdict": "PASS",
            "command": ["reify", "check", "/tmp/fixture.ri"],
            "exit_code": 0,
            "stdout": "All constraints satisfied.",
            "stderr": ""
        }
    ],
    "adversary": []
}
EOJSON

assert "prd-decompose-verify.py synthesize all-PASS exits 0" \
    python3 "$REPO_ROOT/scripts/prd-decompose-verify.py" synthesize "$_TMP_PASS"
rm -f "$_TMP_PASS"

# ── CLI smoke: synthesize FAIL → exit 1 ───────────────────────────────────
# Write a synthetic FAIL results fixture to a temp file.
_TMP_FAIL="$(mktemp /tmp/pdv_smoke_fail_XXXXXX.json)"
cat > "$_TMP_FAIL" <<'EOJSON'
{
    "prover": [
        {
            "capability": "arg-vs-param rejection (4575 class)",
            "probe_kind": "check",
            "verdict": "FAIL",
            "command": ["reify", "check", "/tmp/revolute_silent_accept.ri"],
            "exit_code": 0,
            "stdout": "All constraints satisfied.",
            "stderr": ""
        }
    ],
    "adversary": []
}
EOJSON

# synthesize exits 1 when at least one probe blocks — invert for assert.
if python3 "$REPO_ROOT/scripts/prd-decompose-verify.py" synthesize "$_TMP_FAIL" \
        >/dev/null 2>&1; then
    echo "  FAIL: prd-decompose-verify.py synthesize FAIL should exit 1 (got 0)"
    FAIL=$((FAIL + 1))
else
    echo "  PASS: prd-decompose-verify.py synthesize FAIL exits 1 (blocking)"
    PASS=$((PASS + 1))
fi
rm -f "$_TMP_FAIL"

# ── node --check (skip-guarded) ───────────────────────────────────────────
if command -v node >/dev/null 2>&1; then
    assert "node --check scripts/prd-decompose-verify.mjs exits 0" \
        node --check "$REPO_ROOT/scripts/prd-decompose-verify.mjs"
else
    echo "  SKIP: node not on PATH — skipping .mjs syntax check"
fi

test_summary
