#!/usr/bin/env bash
# verify-pipeline-guard.sh — classifier oracle for the dark-factory merge-worker
# trivial-pass fast-path.
#
# Subcommands:
#   requires-full-gate [file...]  — read repo-relative changed-file paths from
#                                   "$@" (if any) or newline-separated stdin.
#                                   Caller path-form contract: pass clean
#                                   repo-relative paths (as emitted by 'git
#                                   diff --name-only').  Leading './' is
#                                   stripped defensively; absolute paths and
#                                   '../' forms will NOT match.
#                                   Exit 0 if ANY path is load-bearing (full gate
#                                   REQUIRED — do NOT fast-path the diff).
#                                   Exit 1 if none are load-bearing (fast-path safe).
#                                   Prints the first matched path to stdout for
#                                   diagnostics.
#   --list                        — print the canonical load-bearing path set,
#                                   one repo-relative path per line, sorted-unique.
#
# Exit-code contract:
#   0 — full gate REQUIRED (at least one load-bearing file in the diff)
#   1 — fast-path safe (no load-bearing file found)
#   2 — usage error (unknown subcommand or flag)
#
# The load-bearing set is the union of:
#   anchor:   scripts/verify.sh (always load-bearing)
#   manifest: all non-comment/non-blank lines in scripts/verify-pipeline-paths.txt
#   sourced:  scripts/<lib> for each 'source "$SCRIPT_DIR/<lib>"' line in verify.sh
#             (auto-derived live; self-healing — future sourced libs are
#             automatically load-bearing without any manifest edit)
#
# Environment knobs:
#   REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH — override path to verify.sh used for
#             live sourced-lib derivation (testability / operator override; mirrors
#             the REIFY_* knob idiom used throughout verify.sh and its libs).
#
# Usage by the dark-factory merge worker (cross-repo seam — wiring tracked
# separately; reify ships the oracle, dark-factory does the wiring):
#
#   result=$(bash scripts/verify-pipeline-guard.sh requires-full-gate "${changed_files[@]}")
#   exit_code=$?
#   if [ "$exit_code" -eq 0 ]; then
#       # Route to full --scope all gate (or run drift guards at minimum)
#   elif [ "$exit_code" -eq 1 ]; then
#       # Config-only fast-path safe
#   fi

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---------------------------------------------------------------------------
# Build the load-bearing set _SET (newline-separated, deduped at list time)
# ---------------------------------------------------------------------------

# 1. Anchor: scripts/verify.sh is always load-bearing.
_SET="scripts/verify.sh"

# 2. Static manifest: non-comment/non-blank lines from verify-pipeline-paths.txt.
_MANIFEST="$SCRIPT_DIR/verify-pipeline-paths.txt"
if [ -f "$_MANIFEST" ]; then
    while IFS= read -r _line; do
        case "$_line" in
            '#'* | '') continue ;;
        esac
        _SET="${_SET}"$'\n'"${_line}"
    done < "$_MANIFEST"
fi

# 3. Live sourced-lib derivation: append scripts/<lib> for each
#    'source "$SCRIPT_DIR/<lib>"' statement in verify.sh.
#    The anchored grep matches real source STATEMENTS only (not comment
#    mentions), inheriting the same hardening as make_branch_fixture's preflight
#    in tests/infra/test_verify_throughput.sh.
#    REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH overrides the verify.sh path
#    for testability (synthetic-lib injection) and operator use.
_verify_sh="${REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH:-$SCRIPT_DIR/verify.sh}"
if [ -f "$_verify_sh" ]; then
    while IFS= read -r _lib; do
        [ -z "$_lib" ] && continue
        _SET="${_SET}"$'\n'"scripts/${_lib}"
    done < <(grep -E '^[[:space:]]*source "\$SCRIPT_DIR/' "$_verify_sh" \
             | sed -n 's|.*source "\$SCRIPT_DIR/\([^"]*\)".*|\1|p')
fi

# Sort and deduplicate the set (a lib in both the manifest and sourced is fine).
_SORTED_SET="$(printf '%s\n' "$_SET" | sort -u)"

# ---------------------------------------------------------------------------
# Subcommand dispatch
# ---------------------------------------------------------------------------

_subcmd="${1:-}"

case "$_subcmd" in
    --list)
        printf '%s\n' "$_SORTED_SET"
        exit 0
        ;;
    requires-full-gate)
        shift
        # Collect all candidates from args or stdin, then do ONE grep pass —
        # O(N+M) instead of O(N*M) per-candidate subshell pipelines.
        if [ "$#" -gt 0 ]; then
            _raw=$(printf '%s\n' "$@")
        else
            # Stdin mode: newline-separated paths — supports large diffs that
            # would exceed ARG_MAX if passed as positional arguments.
            _raw=$(cat)
        fi
        # Normalize: strip a leading './' so callers that pass './foo/bar'
        # match the clean repo-relative form in _SORTED_SET.  'git diff
        # --name-only' emits clean paths; this is defensive hardening.
        # Absolute paths and '../'-prefixed forms will NOT match.
        _normalized=$(printf '%s\n' "$_raw" | sed 's|^\./||')
        # Single-pass match: -f reads _SORTED_SET as fixed-string patterns,
        # -x anchors to the full line, -m1 short-circuits after the first hit.
        # '|| true' prevents set -e from aborting on no-match (grep exit 1).
        _match=$(printf '%s\n' "$_normalized" \
                 | grep -xF -m1 -f <(printf '%s\n' "$_SORTED_SET") 2>/dev/null \
                 || true)
        if [ -n "$_match" ]; then
            echo "$_match"
            exit 0
        fi
        exit 1
        ;;
    *)
        printf 'Usage: %s requires-full-gate [file...] | --list\n' "$(basename "$0")" >&2
        printf '  requires-full-gate: exits 0 if any file is load-bearing (full gate required),\n' >&2
        printf '                      1 if none (fast-path safe); reads stdin when no args.\n' >&2
        printf '  --list: print the canonical load-bearing path set (one path per line).\n' >&2
        exit 2
        ;;
esac
