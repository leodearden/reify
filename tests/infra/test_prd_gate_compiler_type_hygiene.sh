#!/usr/bin/env bash
# CI gate for the compiler-type-hygiene §8 boundary-table probe-set (task 5070 λ).
# Verifies that scripts/prd-capability-check.py returns PASS for every row in
# tests/prd-gate/compiler-type-hygiene-probe-set.json — confirming that the
# whole §8 boundary table is green on ONE commit: the grammar probe parses,
# the three flipped check probes (E_TYPE_ARG_ON_TRAIT / E_ArithOperandKind /
# 4490 CmpOperandKind) reject at exit 1, and the end-to-end integration
# fixture surfaces all three diagnostics in a single `reify check` run.
#
# Like the F-inherit gate (test_prd_gate_objective_inheritance.sh) this asserts
# all-PASS: after the α/β2 rejections landed (5049, 5055) every probe asserts a
# now-true POST-state capability — the OPPOSITE of the corpus gate's all-FAIL
# semantics.
#
# Skip-guards (hybrid of the two existing prd_gate wrappers):
#   - reify binary: REIFY_BIN env var, or target/release/reify, or
#     target/debug/reify, PLUS a target/.reify-bin-sha freshness check proving
#     the resolved binary's build-time HEAD matches the current tree (task
#     #5133 — see scripts/reify-bin-freshness.sh; guards against a
#     cross-candidate leftover binary in the shared _merge-verify warm lane).
#     An explicit REIFY_BIN handoff bypasses the freshness check.
#   - tree-sitter-reify/src/parser.c (grammar must be generated) — probe 1 is a
#     grammar probe (SpecLike<Foo> parses).
#   - tree-sitter CLI on PATH (the grammar probe calls 'tree-sitter parse').
# A missing toolchain is a clean SKIP (exit 0), never a spurious FAIL.
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

echo "=== test_prd_gate_compiler_type_hygiene ==="

# ── Toolchain skip-guard ───────────────────────────────────────────────────
# Binary discovery+freshness (task #5133) lives in reify-bin-freshness.sh: it
# refuses (clean SKIP) a resolved binary whose target/.reify-bin-sha sidecar is
# absent or doesn't match HEAD — provenance unproven, possibly a
# cross-candidate leftover from a sibling merge candidate in the shared
# _merge-verify warm lane. An explicit REIFY_BIN handoff bypasses this and is
# trusted outright.
source "$REPO_ROOT/scripts/reify-bin-freshness.sh"
resolve_trusted_reify_bin "$REPO_ROOT" || { echo "SKIP: $REIFY_BIN_SKIP_REASON"; exit 0; }
_REIFY_BIN="$REIFY_BIN_RESOLVED"

# Probe 1 is a grammar probe (SpecLike<Foo> parses) — it needs the generated
# grammar and the tree-sitter CLI. Mirror the corpus gate's guards so a missing
# toolchain is a clean SKIP rather than a HARNESS_ERROR (exit 70) → spurious FAIL.
if [ ! -f "$REPO_ROOT/tree-sitter-reify/src/parser.c" ]; then
    echo "SKIP: tree-sitter grammar not built — need tree-sitter-reify/src/parser.c"
    exit 0
fi

_TS_BIN="${TREE_SITTER_BIN:-tree-sitter}"
if ! command -v "$_TS_BIN" >/dev/null 2>&1; then
    echo "SKIP: tree-sitter CLI ('$_TS_BIN') not on PATH — grammar probe requires 'tree-sitter parse'"
    exit 0
fi

PROBE_SET="$REPO_ROOT/tests/prd-gate/compiler-type-hygiene-probe-set.json"

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
            f"(the §8 boundary-table capability must hold at the CLI surface)"
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
    echo "  FAIL: compiler-type-hygiene gate assertions failed"
    echo "$_GATE_STATUS" | grep "^GATE_FAIL" | sed 's/^/        /'
    FAIL=$((FAIL + 1))
else
    _PASS_MSG="$(echo "$_GATE_STATUS" | grep "^GATE_PASS" | sed 's/GATE_PASS: //')"
    echo "  PASS: compiler-type-hygiene gate — ${_PASS_MSG}"
    PASS=$((PASS + 1))
fi

test_summary
