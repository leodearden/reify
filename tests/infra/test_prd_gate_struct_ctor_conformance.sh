#!/usr/bin/env bash
# CI gate for the struct-ctor-field-type-conformance §7 boundary-row probe-set
# (task δ / #5306).
#
# Verifies that scripts/prd-capability-check.py returns PASS for every row in
# tests/prd-gate/struct-ctor-conformance-probe-set.json — confirming, on ONE
# commit, that the five §7 boundary rows δ promotes (1 pose-at-selector,
# 2 int-at-String, 3 string-at-selector, 11 unknown named argument, 12
# over-arity) all REJECT at the CLI surface: `reify check` exits 1 and prints
# the structured diagnostic.
#
# This is the PRD's G2 headline signal made repeatable. Before δ flipped
# CTOR_FIELD_CONFORMANCE_SEVERITY every one of these fixtures exited 0 with
# "All constraints satisfied." — the exit code reads Error severity only
# (crates/reify-cli/src/main.rs:794), so a Warning is invisible there.
#
# Modelled on tests/infra/test_prd_gate_compiler_type_hygiene.sh (task 5070 λ),
# minus its grammar-substrate machinery: δ's probe-set is all `probe_kind:
# "check"`, so no row touches tree-sitter and prd_gate_resolve_probe_set has
# nothing to filter.
#
# WHAT THIS GATE CANNOT SEE: `reify check` renders diagnostics as
# `eprintln!("{}: {}", diag.severity, diag.message)` (crates/reify-cli/src/main.rs:211,
# :284) — never the DiagnosticLabel, the SourceSpan or the DiagnosticCode. So the
# PRD's C3 field-name / expected-type / found-type content is assertable here
# (it lives in the message), but C3's span-at-the-offending-argument is NOT, and
# stays a Rust-level assertion in
# crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs.
#
# Skip-guard: reify binary (WHOLE-SCRIPT skip) — REIFY_BIN env var, or
# target/release/reify, or target/debug/reify, PLUS the target/.reify-bin-sha
# freshness check proving the resolved binary's build-time HEAD matches the
# current tree (task #5133 — scripts/reify-bin-freshness.sh; guards against a
# cross-candidate leftover binary in the shared _merge-verify warm lane). An
# explicit REIFY_BIN handoff bypasses the freshness check. A MISSING TOOLCHAIN
# IS A CLEAN SKIP (exit 0), NEVER A SPURIOUS FAIL — every probe runs `reify`, so
# there is nothing left to assert without it.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob, and runs
# under the merge --scope all gate (no verify-pipeline-infra-tests.txt edit
# needed). It DOES need a row in tests/infra/run-all-classification.manifest —
# tests/infra/test_run_all_classification.sh reds on an unclassified script.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_prd_gate_struct_ctor_conformance ==="

# ── Toolchain skip-guard ───────────────────────────────────────────────────
source "$REPO_ROOT/scripts/reify-bin-freshness.sh"
resolve_trusted_reify_bin "$REPO_ROOT" || { echo "SKIP: $REIFY_BIN_SKIP_REASON"; exit 0; }
_REIFY_BIN="$REIFY_BIN_RESOLVED"

PROBE_SET="$REPO_ROOT/tests/prd-gate/struct-ctor-conformance-probe-set.json"

# ── Run prd-capability-check.py with --json ────────────────────────────────
# Capture stdout (JSON) only; stderr flows to terminal for diagnostics.
ALPHA_EXIT=0
ALPHA_JSON="$(REIFY_BIN="$_REIFY_BIN" python3 "$REPO_ROOT/scripts/prd-capability-check.py" --json "$PROBE_SET")" \
    || ALPHA_EXIT=$?

# α exits 64 (EX_USAGE: probe-set missing, unreadable, or invalid) or
# 70 (EX_SOFTWARE: HARNESS_ERROR) → treat as gate failure. Neither is a probe
# verdict, so neither may be laundered into a green skip.
if [ "$ALPHA_EXIT" -eq 64 ] || [ "$ALPHA_EXIT" -eq 70 ]; then
    echo "  FAIL: alpha exited $ALPHA_EXIT — probe-set missing, invalid, or harness error"
    FAIL=$((FAIL + 1))
    test_summary
fi

# ── Assert: every verdict == PASS, zero FAIL/UNPROVABLE/HARNESS_ERROR, ≥1 probe ──
# Count derived from the probe-set JSON (via CORPUS_PATH env var) — self-calibrating,
# never a hardcoded integer, so adding a §7 row to the probe-set cannot silently
# leave the completeness assertion behind.
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
            f"(the §7 boundary row must reject at the CLI surface post-δ)"
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
    echo "  FAIL: struct-ctor-conformance gate assertions failed"
    echo "$_GATE_STATUS" | grep "^GATE_FAIL" | sed 's/^/        /'
    FAIL=$((FAIL + 1))
else
    _PASS_MSG="$(echo "$_GATE_STATUS" | grep "^GATE_PASS" | sed 's/GATE_PASS: //')"
    echo "  PASS: struct-ctor-conformance gate — ${_PASS_MSG}"
    PASS=$((PASS + 1))
fi

test_summary
