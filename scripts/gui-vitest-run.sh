#!/usr/bin/env bash
# The single definition of how the reify-gui vitest suites are invoked, shared
# by the merge gate (scripts/verify.sh's gui block) and the frontend checkpoint
# runner (scripts/gui-test.sh). Both call this; neither calls `npm test`
# directly, so the command shape cannot drift between them.
#
# Usage:
#   scripts/gui-vitest-run.sh                 # full suite
#   scripts/gui-vitest-run.sh <vitest args>   # forwarded verbatim to `npm test --`
#
# WHAT IT ADDS over a bare `npm test` (task 7630): a bounded, signature-gated
# retry of a host-starvation flake that cannot be prevented.
#
# Under heavy cross-worktree load the vitest host process stalls past birpc's
# hardcoded 60 s DEFAULT_TIMEOUT and the worker->host calls in flight reject
# with `[vitest-worker]: Timeout calling "<method>"`. vitest 3.2.4 exposes no
# knob for that bound, and the merge lane is exempt from CPU admission control
# by design, so the event is not preventable from here.
#
# THIS IS NOT A BLANKET RETRY, and deliberately not a timeout bump. It fires
# only when gui/vitest-worker-rpc-flake-reporter.ts has classified the run as
# unambiguously that event — every failed suite carrying an RPC timeout, zero
# failed TESTS, no unexplained unhandled error — conditions a genuine code
# defect cannot satisfy. The classification arrives as a JSON artifact, the one
# seam between the two halves; no vitest output is ever parsed. The artifact's
# ABSENCE is what vetoes a retry, so an unclassified failure propagates
# verbatim. The retry is bounded to one, covers only the named suites, and is
# announced on stdout whether or not it rescues the run, so recurrences stay
# counted instead of being absorbed.
#
# Knob: REIFY_GUI_RPC_FLAKE_RETRY=0 disables the retry (default 1).

set -euo pipefail

# Resolve the repo root from this script's path so it works from ANY cwd,
# including a warm-lane worktree (same idiom as scripts/gui-test.sh).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
GUI_DIR="$REPO_ROOT/gui"

# MUST match WORKER_RPC_FLAKE_ARTIFACT in gui/vitest-worker-rpc-flake-reporter.ts.
# tests/infra/test_gui_vitest_rpc_hardening.sh pins that the two agree.
ARTIFACT="$GUI_DIR/node_modules/.reify-gui-rpc-flake.json"

MARKER_LINEAGE="3185,4856,7630"

[ -d "$GUI_DIR" ] || { echo "gui-vitest-run.sh: no gui/ directory at $GUI_DIR" >&2; exit 1; }
cd "$GUI_DIR"

run_vitest() {
    if [ "$#" -gt 0 ]; then
        npm test -- "$@"
    else
        npm test
    fi
}

# Read the classified suite list, one per line, or fail. Parsed with node —
# a real JSON parser, not a grep — so the seam stays structured data.
read_classified_suites() {
    node -e '
        const fs = require("node:fs");
        const artifact = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
        const suites = artifact.suites;
        if (!Array.isArray(suites) || suites.length === 0) process.exit(1);
        if (!suites.every((s) => typeof s === "string")) process.exit(1);
        process.stdout.write(suites.join("\n"));
    ' "$ARTIFACT"
}

# A safe spec token is a plain relative path. Same allowlist and leading-dash
# rules as REIFY_GUI_RETRY_SPECS in verify.sh — one definition of "safe token"
# across the pipeline — plus the two rules a file source additionally needs:
# no absolute path, no parent-directory escape.
is_safe_spec() {
    local tok="$1"
    [ -n "$tok" ] || return 1
    [ -z "${tok//[A-Za-z0-9._\/-]/}" ] || return 1
    case "$tok" in
        -*|/*|../*|*/../*|*/..) return 1 ;;
    esac
    return 0
}

reject() {
    echo "gui-vitest-run.sh: WARNING — $1; not retrying, propagating the original failure" >&2
}

rc=0
run_vitest "$@" || rc=$?
if [ "$rc" -eq 0 ]; then exit 0; fi

# An unclassified failure is a real one: propagate it untouched.
if [ "${REIFY_GUI_RPC_FLAKE_RETRY:-1}" = "0" ] || [ ! -f "$ARTIFACT" ]; then exit "$rc"; fi

# Captured into a variable rather than piped straight into mapfile, because
# mapfile reports its own success, not the producer's — a node parse failure
# would otherwise be indistinguishable from an empty suite list.
classified=""
if ! classified="$(read_classified_suites)"; then
    reject "the flake artifact at $ARTIFACT is unreadable or names no suites"
    exit "$rc"
fi
mapfile -t suites <<<"$classified"

for spec in "${suites[@]}"; do
    if ! is_safe_spec "$spec"; then
        reject "the flake artifact names '$spec', which is not a plain relative path"
        exit "$rc"
    fi
done

# Consume the artifact BEFORE re-running, so the retry's own reporter pass
# writes a fresh verdict and a stale list can never be read twice.
rm -f "$ARTIFACT"

echo "@@REIFY_GUI_FLAKE@@ kind=worker_rpc_timeout outcome=retrying suites=${#suites[@]} lineage=${MARKER_LINEAGE}"
echo "gui-vitest-run.sh: re-running ${#suites[@]} suite(s) classified as worker->host RPC starvation: ${suites[*]}" >&2

retry_rc=0
run_vitest "${suites[@]}" || retry_rc=$?

if [ "$retry_rc" -eq 0 ]; then
    echo "@@REIFY_GUI_FLAKE@@ kind=worker_rpc_timeout outcome=retried suites=${#suites[@]} lineage=${MARKER_LINEAGE}"
else
    echo "@@REIFY_GUI_FLAKE@@ kind=worker_rpc_timeout outcome=escalated suites=${#suites[@]} exit=${retry_rc} lineage=${MARKER_LINEAGE}"
fi
exit "$retry_rc"
