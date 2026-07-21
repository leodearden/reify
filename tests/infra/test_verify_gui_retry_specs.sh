#!/usr/bin/env bash
# Infrastructure test for task 5289.
# Validates REIFY_GUI_RETRY_SPECS failed-spec forwarding in verify.sh's gui block.
#
# PRD docs/prds/verify-retry-failed-only.md leaf γ (§4.2): on a merge-gate
# narrowed retry, the gui block runs ONLY the named failed vitest specs via
# `npm test -- <specs>` (== `vitest run <specs>`, since npm test == vitest run),
# driven by the REIFY_GUI_RETRY_SPECS env var that dark-factory sets in
# verify_env. The specs are gui-root-relative vitest paths (e.g.
# `src/__tests__/foo.test.ts`) — the block cd's into gui/ (wrap_subshell gui),
# so gui-relative positionals match. When REIFY_GUI_RETRY_SPECS is unset OR
# empty, the block runs the full suite (`npm test`) byte-identically to today —
# the §4.3 loud full-fallback for an empty gui subset (empty subset => run the
# full suite), which also guarantees zero plan drift for every existing
# non-retry invocation.
#
# Oracle: verify.sh --print-plan (hermetic — never runs cargo/npm; the gui
# block has no PRINT_PLAN special-casing, so it reads REIFY_GUI_RETRY_SPECS
# identically in print and execute modes, and the emitted plan line directly
# encodes the forwarding mechanism). Capture/grep idiom mirrors
# test_verify_failfast_order.sh:29-31,92-93.
#
# OUT OF SCOPE (owned by sibling leaves; not asserted here): the honest
# @@REIFY_RETRY_SCOPE=failed_only@@ marker + subset counts (leaf δ, §8), the
# tree-OID sidecar guard + loud full-fallback (leaf α), and the
# nextest/run_all member subsets (α/β). This leaf touches only the gui line.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== verify.sh gui REIFY_GUI_RETRY_SPECS forwarding tests (task 5289) ==="

# Capture four plans that differ ONLY in the REIFY_GUI_RETRY_SPECS environment.
# Strip comment lines (^#). --print-plan is hermetic (no cargo/npm executed).
RETRY_PLAN="$(REIFY_GUI_RETRY_SPECS='src/__tests__/foo.test.ts' bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
MULTI_PLAN="$(REIFY_GUI_RETRY_SPECS='src/__tests__/foo.test.ts src/__tests__/bar.test.ts' bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
EMPTY_PLAN="$(REIFY_GUI_RETRY_SPECS='' bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
PLAIN_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
export RETRY_PLAN MULTI_PLAN EMPTY_PLAN PLAIN_PLAN

# ===========================================================================
# Test 1: single failed spec forwarded via `npm test -- <spec>`  [primary RED]
# ===========================================================================
echo ""
echo "--- Test 1: single failed spec forwarded (npm test -- <spec>) ---"

# RED before step-2: today the gui block emits plain `npm test`, so the full
# forwarded chain is absent.
assert "retry plan: gui chain forwards the failed spec (npm ci && npm run typecheck && npm test -- src/__tests__/foo.test.ts)" \
    bash -c 'printf "%s\n" "$1" | grep -qF "npm ci && npm run typecheck && npm test -- src/__tests__/foo.test.ts"' _ "$RETRY_PLAN"

# ===========================================================================
# Test 2: multiple failed specs forwarded, order preserved  [RED]
# ===========================================================================
echo ""
echo "--- Test 2: multiple failed specs forwarded in order ---"

assert "multi plan: both failed specs forwarded in order (npm test -- foo bar)" \
    bash -c 'printf "%s\n" "$1" | grep -qF "npm test -- src/__tests__/foo.test.ts src/__tests__/bar.test.ts"' _ "$MULTI_PLAN"

# ===========================================================================
# Test 3: preservation — plain plan (no env) keeps the full suite, no subset
# (green before AND after; the non-retry path must stay byte-identical)
# ===========================================================================
echo ""
echo "--- Test 3: plain plan (no env) keeps full npm test, no spec forwarding ---"

assert "plain plan: gui chain intact (npm ci && npm run typecheck && npm test)" \
    bash -c 'printf "%s\n" "$1" | grep -qF "npm ci && npm run typecheck && npm test"' _ "$PLAIN_PLAN"

assert "plain plan: no failed-spec forwarding (no 'npm test --')" \
    bash -c '! printf "%s\n" "$1" | grep -qF "npm test --"' _ "$PLAIN_PLAN"

# ===========================================================================
# Test 4: §4.3 fallback — empty REIFY_GUI_RETRY_SPECS behaves like plain
# (empty subset => full-run fallback; also green before AND after)
# ===========================================================================
echo ""
echo "--- Test 4: empty REIFY_GUI_RETRY_SPECS => full suite (like plain) ---"

assert "empty-env plan: gui chain intact (npm ci && npm run typecheck && npm test)" \
    bash -c 'printf "%s\n" "$1" | grep -qF "npm ci && npm run typecheck && npm test"' _ "$EMPTY_PLAN"

assert "empty-env plan: no failed-spec forwarding (empty subset => full-run fallback, §4.3)" \
    bash -c '! printf "%s\n" "$1" | grep -qF "npm test --"' _ "$EMPTY_PLAN"

# ===========================================================================
# Test 5: isolation — retry touches ONLY the gui line; other suites intact
# (green before AND after; guards against γ leaking into non-gui suites)
# ===========================================================================
echo ""
echo "--- Test 5: retry touches only the gui line (nextest suite intact) ---"

assert "retry plan: cargo nextest run --workspace still present (γ touches only gui)" \
    bash -c 'printf "%s\n" "$1" | grep -qF "cargo nextest run --workspace"' _ "$RETRY_PLAN"

test_summary
