#!/usr/bin/env bash
# scripts/confirm-failed-manifest.sh — the ONE reader of a cargo-nextest JUnit
# report (task 7423; PRD docs/prds/verify-confirm-failed-self-discovery.md
# §4.1.2 / §4.2 step 5).
#
# PURPOSE. Answer exactly one question — "which tests did this nextest pass
# report as failed?" — and answer it the same way for both callers, because
# they compare their answers across runs and a disagreement would silently
# mean "this test stopped failing".
#
# ITS TWO CALLERS, both inside scripts/verify.sh:
#   1. RECORDING. The manifest write fused inline into the offline role's own
#      nextest PLAN command. It runs `record`, which gates on that pass's own
#      exit code and, when the report is trustworthy, writes the profile's
#      confirm manifest and stamps a confirm-owned tree-OID sidecar.
#   2. CONFIRM. The `--confirm-failed` code path, after it has re-run the
#      manifest's subset. It runs `extract` against THAT run's own report and
#      prints the result as its answer to dark-factory.
#
# STDOUT IS A WIRE CONTRACT. dark-factory spawns the confirm run with its two
# streams MERGED and parses every non-blank line as one confirmed-failing test
# ID, so `extract` puts test IDs and nothing else on stdout; every diagnostic
# goes to stderr. A stray line here becomes a bogus test name that DF files a
# fix task against.
#
# WHY A REAL XML PARSER AND NOT grep/sed. Two measured properties of
# cargo-nextest 0.9.136's report (probed live 2026-09-18) defeat line-oriented
# reading:
#   - A failing <testcase>'s <system-out>/<system-err> children repeat the bare
#     test name on its own indented line, inside a `failures:` block. A grep for
#     names over-reports wildly.
#   - A PASSING <testcase> is NOT self-closing; nextest writes an open/close
#     pair. (PRD §3 says otherwise; §3 is wrong for this version.) So "failed"
#     is decided by the presence of a <failure>/<error> CHILD, nothing else.
# python3 is already a hard dependency of this pipeline — scripts/occt-scope-lib.sh,
# sourced by verify.sh, drives one of these same embedded programs.
#
# Usage:
#   confirm-failed-manifest.sh extract <junit-report>
#       Print the failing tests' BARE ids (testcase/@name verbatim, the exact
#       string nextest's `test(=<id>)` filterset takes), one per line, sorted
#       and deduplicated. Exit 0 on a readable report, even when it names zero
#       failures. Exit 65 when the report is missing, empty or unparseable —
#       with nothing on stdout, so an unreadable report can never be mistaken
#       for "confirmed clean".
#
#   confirm-failed-manifest.sh record --nextest-rc <n> --junit <report> \
#       --manifest <path> --sidecar <path> --profiles <str>
#       Write <manifest> (the failing bare ids, one per line — possibly empty)
#       and stamp <sidecar> with the current tree OID, but ONLY when
#       <n> ∈ {0, 100}. Exit 0 on a write and on a declined write alike; 65
#       when a required write could not be completed.
#
# SORTED, NOT DOCUMENT ORDER. A JUnit report lists testcases in completion
# order, which varies run to run under parallel execution. Sorting makes the
# manifest a stable, diffable artifact and makes the confirm run's output
# independent of scheduling.
#
# WHY THE {0, 100} GATE, AND WHY IT LIVES HERE. 0 means "ran everything, all
# passed" and 100 means "ran everything, some failed" — both leave a complete
# report. Any other code (the outer `timeout`'s 124, a signal, an OOM) means
# nextest was cut short and the report is a partial one that would be recorded
# as if it were the whole truth. The rule lives in this script, not in the
# emitted plan string, so the two magic numbers exist in exactly one place.
#
# WHAT THIS SCRIPT DELIBERATELY DOES NOT CHECK: whether `--no-fail-fast` was
# active. A fail-fast pass truncated after its first failure ALSO exits 100 and
# is indistinguishable here by any evidence the report carries. That gate is
# therefore structural and upstream: verify.sh only emits the `record` call at
# all when its offline `--no-fail-fast` fragment is non-empty, so "the flag was
# active" and "a manifest may be written" are the same fact rather than two
# facts that could drift (PRD §4.1.2 / §5.3, boundary case B11).

set -euo pipefail

err() { printf 'confirm-failed-manifest.sh: %s\n' "$*" >&2; }

# Extracts failing testcase/@name values from a JUnit report on argv[1].
# Prints ids to stdout, diagnostics to stderr; exits 65 on unreadable input.
_CONFIRM_EXTRACT_PY='
import sys
import xml.etree.ElementTree as ET

path = sys.argv[1]
try:
    root = ET.parse(path).getroot()
except (OSError, ET.ParseError) as exc:
    sys.stderr.write(
        "confirm-failed-manifest.sh: unreadable JUnit report %s: %s\n" % (path, exc))
    raise SystemExit(65)

failed = {
    tc.get("name")
    for tc in root.iter("testcase")
    if tc.find("failure") is not None or tc.find("error") is not None
}
failed.discard(None)
failed.discard("")
sys.stdout.write("".join(name + "\n" for name in sorted(failed)))
'

extract() {
    local _junit="${1-}"
    if [ -z "$_junit" ]; then
        err "extract: missing <junit-report> argument"
        return 64
    fi
    python3 -c "$_CONFIRM_EXTRACT_PY" "$_junit"
}

record() {
    local _nextest_rc="" _junit="" _manifest="" _sidecar="" _profiles=""
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --nextest-rc) _nextest_rc="${2:?--nextest-rc requires an argument}"; shift 2 ;;
            --junit)      _junit="${2:?--junit requires an argument}";           shift 2 ;;
            --manifest)   _manifest="${2:?--manifest requires an argument}";     shift 2 ;;
            --sidecar)    _sidecar="${2:?--sidecar requires an argument}";       shift 2 ;;
            --profiles)   _profiles="${2:?--profiles requires an argument}";     shift 2 ;;
            *) err "record: unknown argument '$1'"; return 64 ;;
        esac
    done
    # Variable names mirror their flag spellings, so the missing-flag message
    # is derived from the name rather than restated beside it.
    local _need
    for _need in _nextest_rc _junit _manifest _sidecar _profiles; do
        if [ -z "${!_need}" ]; then
            err "record: missing required --$(printf '%s' "${_need#_}" | tr '_' '-') argument"
            return 64
        fi
    done

    # The completeness gate. A declined write is a normal, expected outcome —
    # say so on stderr and succeed, so the caller's `|| true` is belt-and-braces
    # rather than the thing hiding a real error.
    case "$_nextest_rc" in
        0|100) ;;
        *)
            err "record: declined — nextest exited $_nextest_rc (not 0 or 100), so its JUnit report may be truncated; leaving any existing manifest untouched"
            return 0
            ;;
    esac

    local _ids
    if ! _ids="$(extract "$_junit")"; then
        err "record: declined — could not read $_junit; leaving any existing manifest untouched"
        return 65
    fi

    mkdir -p "$(dirname "$_manifest")" "$(dirname "$_sidecar")" || {
        err "record: cannot create the manifest/sidecar directories"
        return 65
    }

    # Written via a temp file + mv so a concurrent reader sees either the whole
    # previous manifest or the whole new one, never a half-written list — a
    # truncated read would silently mean "these tests stopped failing".
    local _tmp
    _tmp="$(mktemp "${_manifest}.XXXXXX")" || { err "record: cannot create a temp file beside $_manifest"; return 65; }
    if [ -n "$_ids" ]; then
        printf '%s\n' "$_ids" > "$_tmp"
    else
        : > "$_tmp"
    fi
    mv -f "$_tmp" "$_manifest" || { rm -f "$_tmp"; err "record: cannot write $_manifest"; return 65; }

    # Same {tree_oid, profiles, timestamp} shape as verify.sh's attempt-0
    # sidecar, field for field, so verify.sh's existing tolerant reader parses
    # this one unchanged. A DISTINCT path, never that file: the attempt-0
    # sidecar is gated to DF_VERIFY_ROLE=merge and is never written under the
    # offline role, so sharing it would pin nothing for this lane.
    printf '{"tree_oid":"%s","profiles":"%s","timestamp":"%s"}\n' \
        "$(git rev-parse HEAD: 2>/dev/null || echo unknown)" \
        "$_profiles" \
        "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        > "$_sidecar" || { err "record: cannot write $_sidecar"; return 65; }
}

case "${1-}" in
    extract) shift; extract "$@" ;;
    record)  shift; record "$@" ;;
    *)
        err "usage: confirm-failed-manifest.sh extract <junit-report>"
        err "       confirm-failed-manifest.sh record --nextest-rc <n> --junit <report> --manifest <path> --sidecar <path> --profiles <str>"
        exit 64
        ;;
esac
