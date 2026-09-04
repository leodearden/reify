#!/usr/bin/env bash
# Infrastructure test for task 4608 (prd-gate-exec γ — decompose-phase verification).
# Verifies that:
#   1. python3 is on PATH
#   2. scripts/test_prd_decompose_verify.py (stdlib unittest) exits 0
#   3. CLI smoke: scripts/prd-decompose-verify.py --help exits 0
#   4. CLI smoke: synthesize on an all-PASS results fixture exits 0
#   5. CLI smoke: synthesize on a FAIL results fixture exits 1
#   6. CLI smoke (task #7257): a mixed batch reports a counts header, blocks on
#      ONLY the evidence-backed record, and renders a string command verbatim
#   7. (skip-guarded) node --check scripts/prd-decompose-verify.mjs exits 0
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

# ── CLI smoke (task #7257): the evidence gate, end to end ─────────────────
# Five records: three unexecuted promises, one probe whose fixture is missing,
# and ONE evidence-backed FAIL whose `command` arrived as a STRING.  Before the
# fix all five were tabulated as blocking and the string command was rendered
# character-by-character ("t a r g e t / r e l e a s e / ...").
_TMP_MIXED="$(mktemp /tmp/pdv_smoke_mixed_XXXXXX.json)"
cat > "$_TMP_MIXED" <<'EOJSON'
{
    "prover": [
        {
            "capability": "vacuous-1",
            "probe_kind": "check",
            "verdict": "FAIL",
            "command": [],
            "exit_code": null,
            "stdout": "",
            "stderr": ""
        },
        {
            "capability": "vacuous-2",
            "probe_kind": "ir",
            "verdict": "UNPROVABLE",
            "command": [],
            "stdout": "",
            "stderr": ""
        },
        {
            "capability": "vacuous-3",
            "probe_kind": "check",
            "verdict": "FAIL",
            "exit_code": null,
            "stdout": "",
            "stderr": ""
        },
        {
            "capability": "fixture-absent cap",
            "probe_kind": "ir",
            "verdict": "FAIL",
            "command": ["reify", "eval", "tests/prd-gate/fixtures/not-yet.ri"],
            "exit_code": 1,
            "stdout": "",
            "stderr": "Error: No such file or directory (os error 2)"
        }
    ],
    "adversary": [
        {
            "capability": "string-command cap",
            "probe_kind": "ir",
            "verdict": "FAIL",
            "command": "target/release/reify eval f.ri",
            "exit_code": 1,
            "stdout": "",
            "stderr": "assertion did not hold"
        }
    ]
}
EOJSON

_MIXED_OUT="$(mktemp /tmp/pdv_smoke_mixed_out_XXXXXX.json)"
if python3 "$REPO_ROOT/scripts/prd-decompose-verify.py" synthesize "$_TMP_MIXED" \
        > "$_MIXED_OUT" 2>/dev/null; then
    echo "  FAIL: mixed batch should exit 1 (one evidence-backed FAIL); got 0"
    FAIL=$((FAIL + 1))
else
    echo "  PASS: mixed batch exits 1 (blocking on the evidence-backed record)"
    PASS=$((PASS + 1))
fi

assert "mixed batch report carries the counts header" \
    grep -q "records: 5 total, 2 with executed-probe evidence, 1 blocking, 3 malformed, 1 fixture-absent" \
    "$_MIXED_OUT"

# Exactly one capability blocks — the other four are malformed or fixture-absent.
assert "mixed batch blocks on exactly one capability" \
    python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); sys.exit(0 if d["blocking"]==["string-command cap"] else 1)' \
    "$_MIXED_OUT"

assert "mixed batch renders the string command verbatim" \
    grep -q "target/release/reify eval f.ri" "$_MIXED_OUT"

# The character-exploded rendering must be gone.
if grep -q "t a r g e t" "$_MIXED_OUT"; then
    echo "  FAIL: string command was rendered character-by-character"
    FAIL=$((FAIL + 1))
else
    echo "  PASS: string command is not character-exploded"
    PASS=$((PASS + 1))
fi

rm -f "$_TMP_MIXED" "$_MIXED_OUT"

# ── node --check wrapped form (skip-guarded) ─────────────────────────────
# The .mjs has a top-level `return` (Workflow harness wraps body in AsyncFunction).
# Raw `node --check` rejects top-level `return` as SyntaxError: Illegal return
# statement.  Validate harness-faithful syntax: strip `export const meta` →
# `const meta`, wrap in `async function __wf() { ... }`, then node --check that.
if command -v node >/dev/null 2>&1; then
    _TMP_MJS_WRAPPED="$(mktemp /tmp/pdv_mjs_wrapped_XXXXXX.mjs)"
    {
        echo "async function __wf() {"
        sed 's/export const meta/const meta/' \
            "$REPO_ROOT/scripts/prd-decompose-verify.mjs"
        echo "}"
    } > "$_TMP_MJS_WRAPPED"
    assert "node --check .mjs (wrapped-form) exits 0" \
        node --check "$_TMP_MJS_WRAPPED"
    rm -f "$_TMP_MJS_WRAPPED"
else
    echo "  SKIP: node not on PATH — skipping .mjs syntax check"
fi

test_summary
