#!/usr/bin/env bash
# Regression guard for task 4961 (esc-4906-45): test_run_all.sh's knob-UNSET
# sub-cases must behave identically whether REIFY_RUN_ALL_EXCLUDE_HOST_INFRA
# is genuinely unset OR inherited as an ambient export.
#
# Root cause this guards against: orchestrator.yaml's verify_env exports
# REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 into the whole verify.sh process tree by
# design (run-all-host-infra-partition PRD H9). test_run_all.sh's own
# knob-UNSET sub-cases (T9a/T9b/T13b/T14b/T17c) used to invoke run_all.sh via
# bare prefix-assignment (`RUN_ALL_CLASSIFICATION_MANIFEST=... bash
# "$RUN_ALL" ...`), which sets only the named vars and does NOT clear an
# inherited exported ambient var. Under the orchestrator those "unset"
# sub-cases silently inherited REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1, dropped
# their fixtures' host-exclusive members from discovery (run_all.sh's H3
# flip-seam, task 4925), and failed their discovered-count / hostx-header
# assertions -- blocking the verify gate for every --include-infra task.
#
# This file cannot live inside test_run_all.sh itself: self-invocation would
# recurse infinitely (test_run_all.sh IS the discovery runner's test
# subject). Instead it drives the REAL test_run_all.sh exactly once, as a
# subprocess, under a hostile ambient export -- the same shape the
# orchestrator's verify_env produces -- and asserts the nested suite still
# exits 0. test_run_all.sh never invokes run_all.sh against the real
# tests/infra/ directory (all of its sub-cases use temp-dir fixtures), so
# this single direct invocation does not itself recurse.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

TARGET="$SCRIPT_DIR/test_run_all.sh"
[ -f "$TARGET" ] || { echo "ERROR: test_run_all.sh not found at $TARGET"; exit 1; }

echo "=== run_all.sh ambient-isolation regression guard (task 4961 / esc-4906-45) ==="

# ---------------------------------------------------------------------------
# Test 1: test_run_all.sh passes identically under a hostile ambient
# REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 -- the exact shape orchestrator.yaml's
# verify_env produces. The `export ...; bash "$TARGET"` sequence runs inside
# the `$( ... )` command-substitution subshell only, so the export never
# leaks back out to this script. The normal infra suite already covers the
# knob-genuinely-unset case, so "passes identically" =
# hostile-run-passes AND normal-suite-passes.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: test_run_all.sh exits 0 under ambient REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 ---"

amb_rc=0
amb_out="$( export REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1; bash "$TARGET" 2>&1 )" || amb_rc=$?

assert "test_run_all.sh exits 0 under ambient REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 (got rc=$amb_rc)" \
    test "$amb_rc" -eq 0

# Anchored line match (not a substring grep) so an inner mock's own
# "0 failed"-shaped output could never false-pass this assertion -- only the
# nested test_run_all.sh's OWN test_summary line qualifies.
if printf '%s\n' "$amb_out" | grep -qE '^Results: [0-9]+ passed, 0 failed$'; then
    assert "test_run_all.sh reports 0 failed under ambient REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1" true
else
    assert "test_run_all.sh reports 0 failed under ambient REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 (got: $amb_out)" false
fi

test_summary
