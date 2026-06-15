#!/usr/bin/env bash
# CI gate for the δ historical-false-premise regression corpus (task 4609).
# Verifies that scripts/prd-capability-check.py returns FAIL/UNPROVABLE for
# every row in tests/prd-gate/corpus-probe-set.json.
#
# A row flipping to PASS means the substrate changed (update corpus) or the
# checker regressed (gate fires). Wired into CI via tests/infra/run_all.sh
# (auto-discovery) → verify.sh:983.
#
# Skip-guards on toolchain presence — mirroring α's @skipUnless guards:
#   - REIFY_BIN env var, or target/release/reify, or target/debug/reify
#   - tree-sitter-reify/src/parser.c (grammar must be generated)
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

echo "=== test_prd_gate_corpus ==="

# ── Toolchain skip-guard ───────────────────────────────────────────────────
# Mirror α's _REIFY_BUILT / _TS_GRAMMAR_AVAILABLE skip pattern.
_REIFY_BIN=""
if [ -n "${REIFY_BIN:-}" ]; then
    _REIFY_BIN="${REIFY_BIN}"
elif [ -f "$REPO_ROOT/target/release/reify" ]; then
    _REIFY_BIN="$REPO_ROOT/target/release/reify"
elif [ -f "$REPO_ROOT/target/debug/reify" ]; then
    _REIFY_BIN="$REPO_ROOT/target/debug/reify"
fi

if [ -z "$_REIFY_BIN" ] || [ ! -f "$REPO_ROOT/tree-sitter-reify/src/parser.c" ]; then
    echo "SKIP: reify/tree-sitter toolchain not built — need target/{release,debug}/reify AND tree-sitter-reify/src/parser.c"
    exit 0
fi

CORPUS="$REPO_ROOT/tests/prd-gate/corpus-probe-set.json"

# ── Run α with --json to get machine-readable verdict output ───────────────
# Capture stdout (JSON) only; stderr flows to terminal for diagnostics.
ALPHA_EXIT=0
ALPHA_JSON="$(python3 "$REPO_ROOT/scripts/prd-capability-check.py" --json "$CORPUS")" \
    || ALPHA_EXIT=$?

# α exits 64 (EX_USAGE: corpus missing, unreadable, or invalid probe-set) or
# 70 (EX_SOFTWARE: HARNESS_ERROR) → treat as gate failure.
if [ "$ALPHA_EXIT" -eq 64 ] || [ "$ALPHA_EXIT" -eq 70 ]; then
    echo "  FAIL: alpha exited $ALPHA_EXIT — corpus missing, invalid, or harness error"
    FAIL=$((FAIL + 1))
    test_summary
fi

# ── Assert: every verdict ∈ {FAIL, UNPROVABLE}, zero PASS/HARNESS_ERROR, ≥1 probe ─
_PY_GATE=$(cat << 'PYEOF'
import json, sys

try:
    data = json.loads(sys.stdin.read())
except Exception as e:
    print(f"GATE_FAIL: cannot parse alpha JSON output: {e}")
    sys.exit(1)

results = data.get("results", [])
if not results:
    print("GATE_FAIL: no results in alpha output (empty corpus?)")
    sys.exit(1)

errors = []

# (a) every verdict in {FAIL, UNPROVABLE}; (b) zero PASS; (c) zero HARNESS_ERROR
allowed = {"FAIL", "UNPROVABLE"}
for r in results:
    v = r["verdict"]
    if v not in allowed:
        errors.append(f"verdict {v!r} for {r['capability']!r} — expected FAIL or UNPROVABLE")

# (d) per task-id presence+verdict checks — all 7 expected corpus ids
for tid in ("3979", "4575", "4577", "4437", "4358", "4497", "4375"):
    if not any(tid in r["capability"] and r["verdict"] in allowed for r in results):
        errors.append(f"no FAIL/UNPROVABLE result found for task-id {tid}")

# (e) completeness: corpus must contain exactly 7 probes — no silent drops or extras
if len(results) != 7:
    errors.append(f"expected exactly 7 probe results, got {len(results)}")

if errors:
    for e in errors:
        print(f"GATE_FAIL: {e}")
    sys.exit(1)

print(f"GATE_PASS: {len(results)}/7 probe(s), all FAIL/UNPROVABLE, all expected task-ids present")
PYEOF
)

_GATE_EXIT=0
_GATE_STATUS="$(echo "$ALPHA_JSON" | python3 -c "$_PY_GATE")" || _GATE_EXIT=$?

if [ "$_GATE_EXIT" -ne 0 ] || echo "$_GATE_STATUS" | grep -q "^GATE_FAIL"; then
    echo "  FAIL: corpus gate assertions failed"
    echo "$_GATE_STATUS" | grep "^GATE_FAIL" | sed 's/^/        /'
    FAIL=$((FAIL + 1))
else
    _PASS_MSG="$(echo "$_GATE_STATUS" | grep "^GATE_PASS" | sed 's/GATE_PASS: //')"
    echo "  PASS: corpus gate — ${_PASS_MSG}"
    PASS=$((PASS + 1))
fi

test_summary
