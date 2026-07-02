#!/usr/bin/env bash
# scripts/run-offline-deep.sh — one-shot runner for the offline deep-test lane
# (task 4916/A5).
#
# This is the real reify-local EXECUTING consumer of the A2
# DF_VERIFY_ROLE=offline role (task 4913, scripts/verify.sh): the
# anti-orphan caller that actually RUNS the heavy off-gate test set, not
# merely `--print-plan`s it.
#
# Consumers:
#   - The manual bridge/operator entry point during the Part-B window (run
#     by hand, or from a local timer/cron).
#   - Later, the Part-B dark-factory offline-lane worker's subprocess entry
#     point.
#
# This is a THIN wrapper — it sets DF_VERIFY_ROLE=offline and delegates
# straight to verify.sh. It does NOT re-specify --profile, --scope, or the
# heavy filter: all of that comes from the A2 offline role in verify.sh,
# which stays the single source of truth for the offline lane's shape.
#
# Usage:
#   scripts/run-offline-deep.sh [verify.sh options...]
#
#   Trailing args are forwarded verbatim to `verify.sh test`. With zero
#   args, this runs the full offline heavy set (SCOPE defaults to 'all',
#   PROFILE defaults to 'release' under the offline role). Pass
#   `--print-plan` for a hermetic dry run.
#
# Reports pass/fail via BOTH the process exit code (for the DF worker
# subprocess entry) and a human-readable outcome line on stderr (for the
# operator/manual-bridge). All wrapper chatter goes to stderr so
# `--print-plan` stdout stays a pure plan.
#
# A forwarded --print-plan never runs any tests (verify.sh exits 0 after
# emitting the plan), so the outcome line reports a distinct "PLAN OK" dry
# -run marker rather than "PASS" for that case — a reader of only the
# stderr summary must not mistake a plan emission for a real test pass.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

export DF_VERIFY_ROLE=offline

is_print_plan=0
for _arg in "$@"; do
    if [ "$_arg" = "--print-plan" ]; then
        is_print_plan=1
        break
    fi
done

rc=0
"$SCRIPT_DIR/verify.sh" test "$@" || rc=$?

if [ "$rc" -eq 0 ]; then
    if [ "$is_print_plan" -eq 1 ]; then
        echo "==> offline deep-test lane: PLAN OK (dry run — no tests executed)" >&2
    else
        echo "==> offline deep-test lane: PASS" >&2
    fi
else
    echo "==> offline deep-test lane: FAIL (exit $rc)" >&2
fi

exit "$rc"
