#!/usr/bin/env bash
# CI gate for the F-inherit ζ passing-capability probe-set (task #4826).
# Verifies that scripts/prd-capability-check.py returns PASS for every row in
# tests/prd-gate/objective-inheritance-probe-set.json — confirming that the
# W_OBJECTIVE_INHERIT_AMBIGUOUS (BT8) and W_SUBBODY_OBJECTIVE_IGNORED (BT9)
# diagnostic codes fire at the `reify check` CLI surface.
#
# Unlike the all-FAIL corpus gate (test_prd_gate_corpus.sh), this gate asserts
# all-PASS: these are PASSING capability probes (the W_ codes are implemented).
#
# Skip-guard: requires a pre-built reify binary (REIFY_BIN env var, or
# target/release/reify, or target/debug/reify).  No tree-sitter guard needed
# — these are `check` probes only.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob, and runs
# under the merge --scope all gate (no verify-pipeline-infra-tests.txt edit
# needed — same auto-discovery wiring as test_prd_gate_corpus.sh).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_prd_gate_objective_inheritance ==="

# ── Toolchain skip-guard ───────────────────────────────────────────────────
_REIFY_BIN=""
if [ -n "${REIFY_BIN:-}" ]; then
    _REIFY_BIN="${REIFY_BIN}"
elif [ -f "$REPO_ROOT/target/release/reify" ]; then
    _REIFY_BIN="$REPO_ROOT/target/release/reify"
elif [ -f "$REPO_ROOT/target/debug/reify" ]; then
    _REIFY_BIN="$REPO_ROOT/target/debug/reify"
fi

if [ -z "$_REIFY_BIN" ]; then
    echo "SKIP: reify binary not built — need target/{release,debug}/reify or REIFY_BIN"
    exit 0
fi

PROBE_SET="$REPO_ROOT/tests/prd-gate/objective-inheritance-probe-set.json"

# ── Run prd-capability-check.py with --json ────────────────────────────────
# Capture stdout (JSON) only; stderr flows to terminal for diagnostics.
ALPHA_EXIT=0
ALPHA_JSON="$(REIFY_BIN="$_REIFY_BIN" python3 "$REPO_ROOT/scripts/prd-capability-check.py" --json "$PROBE_SET")" \
    || ALPHA_EXIT=$?

# α exits 64 (EX_USAGE: probe-set missing, unreadable, or invalid) or
# 70 (EX_SOFTWARE: HARNESS_ERROR) → treat as gate failure.
if [ "$ALPHA_EXIT" -eq 64 ] || [ "$ALPHA_EXIT" -eq 70 ]; then
    echo "  FAIL: alpha exited $ALPHA_EXIT — probe-set missing, invalid, or harness error"
    FAIL=$((FAIL + 1))
    test_summary
fi

# ── Assert: every verdict == PASS, zero FAIL/UNPROVABLE/HARNESS_ERROR, ≥1 probe ──
# Count derived from the probe-set JSON (via CORPUS_PATH env var) — self-calibrating.
_PY_GATE=$(cat << 'PYEOF'
import json, sys, os

try:
    data = json.loads(sys.stdin.read())
except Exception as e:
    print(f"GATE_FAIL: cannot parse alpha JSON output: {e}")
    sys.exit(1)

results = data.get("results", [])
if not results:
    print("GATE_FAIL: no results in alpha output (empty probe-set?)")
    sys.exit(1)

# Load probe-set to derive expected probe count — single source of truth.
corpus_path = os.environ.get("CORPUS_PATH", "")
try:
    with open(corpus_path) as f:
        corpus = json.load(f)
    expected_count = len(corpus.get("probes", []))
except Exception as e:
    print(f"GATE_FAIL: cannot load probe-set JSON {corpus_path!r}: {e}")
    sys.exit(1)

errors = []

# (a) every verdict must be PASS
for r in results:
    v = r["verdict"]
    if v != "PASS":
        errors.append(
            f"verdict {v!r} for {r['capability']!r} — expected PASS "
            f"(W_ code must fire at the CLI surface)"
        )

# (b) completeness: count derived from probe-set — catches silent drops or extras
if len(results) != expected_count:
    errors.append(
        f"expected exactly {expected_count} probe results (per probe-set), got {len(results)}"
    )

if errors:
    for e in errors:
        print(f"GATE_FAIL: {e}")
    sys.exit(1)

print(f"GATE_PASS: {len(results)}/{expected_count} probe(s), all PASS")
PYEOF
)

_GATE_EXIT=0
_GATE_STATUS="$(echo "$ALPHA_JSON" | CORPUS_PATH="$PROBE_SET" python3 -c "$_PY_GATE")" || _GATE_EXIT=$?

if [ "$_GATE_EXIT" -ne 0 ] || echo "$_GATE_STATUS" | grep -q "^GATE_FAIL"; then
    echo "  FAIL: objective-inheritance gate assertions failed"
    echo "$_GATE_STATUS" | grep "^GATE_FAIL" | sed 's/^/        /'
    FAIL=$((FAIL + 1))
else
    _PASS_MSG="$(echo "$_GATE_STATUS" | grep "^GATE_PASS" | sed 's/GATE_PASS: //')"
    echo "  PASS: objective-inheritance gate — ${_PASS_MSG}"
    PASS=$((PASS + 1))
fi

test_summary
