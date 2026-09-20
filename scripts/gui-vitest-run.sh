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
# verbatim. The retry is bounded to one and is announced on stdout whether or
# not it rescues the run, so recurrences stay counted instead of being absorbed.
#
# WHAT GETS RE-RUN depends on what the verdict NAMES. When it names suites, the
# retry narrows the spec filters to them and carries the caller's own options
# through unchanged (see partition_args). When it names NONE, the retry re-runs
# the caller's original invocation verbatim -- on the merge gate that is the
# bare full suite, and on a REIFY_GUI_RETRY_SPECS-narrowed block it is that same
# narrowing, so the retry never answers a wider question than the one asked. A
# run whose only failures are run-level RPC timeouts has no suite to narrow to:
# `snapshotSaved` is issued after a file's tests have already passed, so its
# timeout has no module to attribute it to (task 7724). Widening the retry can
# never mask a failure -- the same argument partition_args already makes.
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

# Partition the caller's argv ONCE, at entry, into RETRY_OPTS (the tokens a
# retry must carry) and the positional spec filters (which the retry replaces
# with the classified suites). A token with a leading '-' is an OPTION, and the
# token following one is treated as that option's VALUE.
#
# Without this, `scripts/gui-test.sh -- -t someName` or `-- --coverage` lost the
# filter or the flag on retry and reported success for a different run than the
# one asked for. Dropping a flag's VALUE would be worse still: a dangling `-t`
# would swallow the first classified suite path as its argument.
#
# The value rule is deliberately over-inclusive. Without a per-flag arity table
# — an ad-hoc parser of vitest's CLI we decline to grow, and which would drift
# from it — `--coverage src/a.test.ts` cannot be told apart from `-t someName`,
# so a spec path in that position is kept as if it were a value. That only ever
# WIDENS the retry, because vitest ORs positional filters, and a wider retry
# can never mask a failure.
partition_args() {
    RETRY_OPTS=()
    local expect_value=0 tok
    for tok in "$@"; do
        case "$tok" in
            -*) RETRY_OPTS+=("$tok"); expect_value=1; continue ;;
        esac
        if [ "$expect_value" -eq 1 ]; then
            RETRY_OPTS+=("$tok")
            expect_value=0
        fi
    done
}

# Read the classified suite list, one per line, or fail. Parsed with node —
# a real JSON parser, not a grep — so the seam stays structured data.
#
# An EMPTY ARRAY is a legitimate verdict ("nothing to narrow to"). An entry that
# is empty or WHITESPACE is not, and this is the only place the two can still be
# told apart: the output is newline-joined, so `[""]` and `["\n"]` both reduce to
# the very string `[]` produces, and downstream would be read as "retry
# everything" -- promoting a rejected artifact into a full re-run. The joining
# cuts the other way too: `["a.ts",""]` loses its tail to command substitution,
# and one entry carrying an EMBEDDED newline is silently split into two specs.
# Rejecting whitespace here is not a new rule, only is_safe_spec's character
# class applied upstream, where the array is still structured data.
read_classified_suites() {
    node -e '
        const fs = require("node:fs");
        const artifact = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
        const suites = artifact.suites;
        if (!Array.isArray(suites)) process.exit(1);
        if (!suites.every((s) => typeof s === "string" && s.length > 0 && !/\s/.test(s))) process.exit(1);
        process.stdout.write(suites.join("\n"));
    ' "$ARTIFACT"
}

# A safe spec token is a plain relative path.
#
# DELIBERATE DUPLICATE, not a SPOT violation to be fixed by sharing: verify.sh
# validates REIFY_GUI_RETRY_SPECS as one space-separated STRING (so its class
# includes a space), while this validates one already-split TOKEN from a JSON
# array (so a space is a rejection) and adds the two rules a file source
# additionally needs — no absolute path, no parent-directory escape. The shared
# part is the character class, and tests/infra/test_gui_vitest_rpc_hardening.sh
# extracts both literals and compares them, so widening either side alone reds.
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

partition_args "$@"

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
    reject "the flake artifact at $ARTIFACT is unreadable or names an invalid suite"
    exit "$rc"
fi
# `mapfile <<<""` yields a ONE-element array holding the empty string, which is
# exactly the confusion the parser above exists to prevent. Guard the read.
suites=()
if [ -n "$classified" ]; then mapfile -t suites <<<"$classified"; fi

for spec in ${suites[@]+"${suites[@]}"}; do
    if ! is_safe_spec "$spec"; then
        reject "the flake artifact names '$spec', which is not a plain relative path"
        exit "$rc"
    fi
done

# Consume the artifact BEFORE re-running, so the retry's own reporter pass
# writes a fresh verdict and a stale list can never be read twice.
rm -f "$ARTIFACT"

# The retry invocation has ONE definition. `"$@"` is still the caller's
# original argv — this script never shifts — so the run-scope retry reproduces
# the request exactly, and RETRY_OPTS stays needed only on the one path where
# the positional filters are actually REPLACED.
if [ "${#suites[@]}" -eq 0 ]; then
    scope=run
    retry_argv=("$@")
    retry_what="the original invocation (the classifier named no suite to narrow to)"
else
    scope=suites
    retry_argv=(${RETRY_OPTS[@]+"${RETRY_OPTS[@]}"} "${suites[@]}")
    retry_what="${#suites[@]} classified suite(s)"
fi

echo "@@REIFY_GUI_FLAKE@@ kind=worker_rpc_timeout outcome=retrying scope=${scope} suites=${#suites[@]} lineage=${MARKER_LINEAGE}"
echo "gui-vitest-run.sh: classified as worker->host RPC starvation; re-running ${retry_what}: npm test ${retry_argv[*]+${retry_argv[*]}}" >&2

retry_rc=0
run_vitest ${retry_argv[@]+"${retry_argv[@]}"} || retry_rc=$?

if [ "$retry_rc" -eq 0 ]; then
    echo "@@REIFY_GUI_FLAKE@@ kind=worker_rpc_timeout outcome=retried scope=${scope} suites=${#suites[@]} lineage=${MARKER_LINEAGE}"
else
    echo "@@REIFY_GUI_FLAKE@@ kind=worker_rpc_timeout outcome=escalated scope=${scope} suites=${#suites[@]} exit=${retry_rc} lineage=${MARKER_LINEAGE}"
fi
exit "$retry_rc"
