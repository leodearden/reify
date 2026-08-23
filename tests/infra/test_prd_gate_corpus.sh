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
#   - reify binary (WHOLE-SCRIPT skip): REIFY_BIN env var, or
#     target/release/reify, or target/debug/reify, PLUS a target/.reify-bin-sha
#     freshness check proving the resolved binary's build-time HEAD matches the
#     current tree (task #5133 — see scripts/reify-bin-freshness.sh; guards
#     against a cross-candidate leftover binary in the shared _merge-verify warm
#     lane). An explicit REIFY_BIN handoff bypasses the freshness check. This one
#     gates the whole script because probes of BOTH kinds run `reify`.
#   - grammar substrate (PER-ROW skip, task 5897): one
#     `prd-capability-check.py --grammar-substrate-status` preflight, replacing
#     the two hand-rolled guards this file used to carry (an isfile() on
#     tree-sitter-reify/src/parser.c and a `command -v` on the tree-sitter CLI).
#     It answers a strictly larger question than either — including whether
#     tree-sitter can actually LOAD the grammar, which a sandboxed role's
#     unwritable ~/.cache/tree-sitter/lock/ prevents — at no extra cost on a lane
#     with no grammar at all.
#
# AN UNUSABLE SUBSTRATE DROPS ONLY THE GRAMMAR ROW; THE CHECK ROWS STILL RUN,
# and are asserted exactly as they always were. This corpus is 1 grammar + 2
# check probes, and check probes never touch the substrate (build_command sends
# them to `reify check`, only grammar probes to `tree-sitter parse`), so the
# older whole-script `exit 0` would have thrown away two perfectly runnable rows
# — trading a spurious RED for a silent coverage hole in exactly the sandboxed
# roles the guard exists to serve. The degradation is announced with a loud
# banner on both stdout and stderr, because a quiet skip makes a
# partial-coverage green run indistinguishable from a full one.
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
# Mirror α's _REIFY_BUILT / _TS_GRAMMAR_AVAILABLE skip pattern. Binary
# discovery+freshness (task #5133) lives in reify-bin-freshness.sh: it
# refuses (clean SKIP) a resolved binary whose target/.reify-bin-sha sidecar
# is absent or doesn't match HEAD — provenance unproven, possibly a
# cross-candidate leftover from a sibling merge candidate in the shared
# _merge-verify warm lane. An explicit REIFY_BIN handoff bypasses this and is
# trusted outright.
source "$REPO_ROOT/scripts/reify-bin-freshness.sh"
resolve_trusted_reify_bin "$REPO_ROOT" || { echo "SKIP: $REIFY_BIN_SKIP_REASON"; exit 0; }
_REIFY_BIN="$REIFY_BIN_RESOLVED"

CORPUS="$REPO_ROOT/tests/prd-gate/corpus-probe-set.json"

# ── Grammar-substrate preflight (task 5897) ────────────────────────────────
# One call answers what the old parser.c isfile() and `command -v tree-sitter`
# guards answered, plus the case neither could see: a present CLI and a present
# grammar that tree-sitter still cannot LOAD, because a sandboxed role's landlock
# write set does not grant ~/.cache/tree-sitter/lock/. That produced a
# HARNESS_ERROR → exit 70 → spurious gate FAIL, which is what this preflight
# converts into a clean, loud, PARTIAL run.
#
# The filtered probe-set is fed to BOTH the checker argument and the CORPUS_PATH
# env var, so the completeness assertions below — which derive expected_count and
# expected_ids from the JSON at CORPUS_PATH, deliberately, "to avoid triple
# bookkeeping" — recalibrate for free. The embedded python needs no change at all.
# The preflight, the filtered-copy mint, its EXIT-trap cleanup and the banner
# are all owned by prd_gate_resolve_probe_set — one composed call, so this gate
# and the hygiene gate cannot drift, and a third gate needs no new copy of it.
source "$REPO_ROOT/scripts/prd-gate-substrate-guard.sh"
_GUARD_RC=0
prd_gate_resolve_probe_set "test_prd_gate_corpus" "$REPO_ROOT" "$CORPUS" || _GUARD_RC=$?
case "$_GUARD_RC" in
    0)
        # Either the committed corpus (usable substrate) or the grammar-filtered
        # copy, with the banner already emitted.
        CORPUS="$PRD_GATE_PROBE_SET"
        ;;
    1)
        # Degenerate: nothing left to run. Not reachable with today's corpus
        # (1 grammar + 2 check), but an all-grammar corpus must skip the script
        # rather than hand the checker an empty probe-set, which it rejects with
        # exit 64 — a gate FAIL, i.e. the very spurious RED this guard prevents.
        echo "SKIP: every corpus row is a grammar probe — nothing left to run"
        exit 0
        ;;
    *)
        # NOT a substrate skip: the committed corpus is missing, unreadable, or
        # not JSON. Before this guard existed that reached the checker and came
        # back exit 64, which the branch below maps to a gate FAIL — so it must
        # still FAIL here. Laundering it into the SKIP above would turn a broken
        # committed artifact green while naming the wrong cause, and only on the
        # sandboxed lanes this guard was written to serve.
        echo "  FAIL: corpus missing, invalid, or harness error — could not filter $CORPUS"
        FAIL=$((FAIL + 1))
        test_summary
        ;;
esac

# ── Run α with --json to get machine-readable verdict output ───────────────
# Capture stdout (JSON) only; stderr flows to terminal for diagnostics.
ALPHA_EXIT=0
ALPHA_JSON="$(REIFY_BIN="$_REIFY_BIN" python3 "$REPO_ROOT/scripts/prd-capability-check.py" --json "$CORPUS")" \
    || ALPHA_EXIT=$?

# α exits 64 (EX_USAGE: corpus missing, unreadable, or invalid probe-set) or
# 70 (EX_SOFTWARE: HARNESS_ERROR) → treat as gate failure.
if [ "$ALPHA_EXIT" -eq 64 ] || [ "$ALPHA_EXIT" -eq 70 ]; then
    echo "  FAIL: alpha exited $ALPHA_EXIT — corpus missing, invalid, or harness error"
    FAIL=$((FAIL + 1))
    test_summary
fi

# ── Assert: every verdict ∈ {FAIL, UNPROVABLE}, zero PASS/HARNESS_ERROR, ≥1 probe ─
# Expected task-ids and probe count are derived from the corpus JSON (via
# CORPUS_PATH env var) to avoid triple bookkeeping — when adding or removing a
# row, only corpus-probe-set.json and the README table need updating; the gate
# script self-calibrates. The explicit count check is kept as a silent-drop guard.
_PY_GATE=$(cat << 'PYEOF'
import json, sys, re, os

try:
    data = json.loads(sys.stdin.read())
except Exception as e:
    print(f"GATE_FAIL: cannot parse alpha JSON output: {e}")
    sys.exit(1)

results = data.get("results", [])
if not results:
    print("GATE_FAIL: no results in alpha output (empty corpus?)")
    sys.exit(1)

# Load corpus to derive expected probe count and task-ids — single source of truth.
corpus_path = os.environ.get("CORPUS_PATH", "")
try:
    with open(corpus_path) as f:
        corpus = json.load(f)
    corpus_probes = corpus.get("probes", [])
    expected_count = len(corpus_probes)
    expected_ids = set()
    for p in corpus_probes:
        m = re.match(r'^(\d+)', p.get("capability", ""))
        if m:
            expected_ids.add(m.group(1))
except Exception as e:
    print(f"GATE_FAIL: cannot load corpus JSON {corpus_path!r}: {e}")
    sys.exit(1)

errors = []

# (a) every verdict in {FAIL, UNPROVABLE}; (b) zero PASS; (c) zero HARNESS_ERROR
allowed = {"FAIL", "UNPROVABLE"}
for r in results:
    v = r["verdict"]
    if v not in allowed:
        errors.append(f"verdict {v!r} for {r['capability']!r} — expected FAIL or UNPROVABLE")

# (d) per task-id presence+verdict checks — derived from corpus (no hardcoded id list)
for tid in sorted(expected_ids):
    if not any(tid in r["capability"] and r["verdict"] in allowed for r in results):
        errors.append(f"no FAIL/UNPROVABLE result found for task-id {tid}")

# (e) completeness: count derived from corpus — catches silent drops or extras
if len(results) != expected_count:
    errors.append(f"expected exactly {expected_count} probe results (per corpus), got {len(results)}")

if errors:
    for e in errors:
        print(f"GATE_FAIL: {e}")
    sys.exit(1)

print(f"GATE_PASS: {len(results)}/{expected_count} probe(s), all FAIL/UNPROVABLE, all expected task-ids present")
PYEOF
)

_GATE_EXIT=0
_GATE_STATUS="$(echo "$ALPHA_JSON" | CORPUS_PATH="$CORPUS" python3 -c "$_PY_GATE")" || _GATE_EXIT=$?

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
