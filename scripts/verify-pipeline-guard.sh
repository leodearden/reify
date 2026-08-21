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
#   emitted:  scripts/<x>.sh for each scripts/*.sh path invoked by an EMITTED
#             plan line in verify.sh — i.e. an add() / add_tool() argument
#             (auto-derived live; self-healing — a future lint/gate script
#             wired into the plan is automatically load-bearing without any
#             manifest edit). These gate scripts are never `source`d, so the
#             'sourced' clause above cannot see them; without this clause a
#             gate-script-only diff is fast-path eligible and lands without
#             ever having been run (task 6320; the #4618/#4624 -> #4288
#             ambush class, doc-side analogue in 'doc-sync' below).
#   doc-sync: docs cross-referenced by tests/infra doc-sync checks, from
#             scripts/doc-sync-paths.txt (the doc-side analogue of the
#             manifest source above — see that file's header for the
#             TRADEOFF BREADCRUMB: exit-0 here is the safe-default full-gate
#             route; a cheaper citing-test-subset alternative is supplied
#             separately via scripts/verify-pipeline-infra-tests.txt)
#   infra-tests: ANY tests/infra/*.sh path (open-ended glob, matched in code —
#             not enumerable in --list; not a manifest line, since the
#             literal-per-file manifest cannot cover not-yet-existent infra
#             tests). A new/renamed infra test changes the merge-gate suite
#             itself, so it is definitionally never config-only (task 5256;
#             recurrence prevention for the 2026-07-19 5247/5249 incident,
#             PRD docs/prds/merge-gate-health.md W3a).
#
# Environment knobs:
#   REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH — override path to verify.sh used for
#             BOTH live derivations that read verify.sh: the sourced-lib clause
#             and the emitted-gate clause (testability / operator override;
#             mirrors the REIFY_* knob idiom used throughout verify.sh and its
#             libs). One knob, both clauses — a synthetic verify.sh injected
#             here drives sourced-lib and emitted-gate coverage alike.
#   REIFY_VERIFY_PIPELINE_GUARD_DOC_SYNC_PATHS — override path to the doc-sync
#             manifest (testability / synthetic-injection + operator override;
#             mirrors REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH above). Defaults to
#             scripts/doc-sync-paths.txt.
#
# Usage by the dark-factory merge worker (cross-repo seam — wiring tracked
# separately; reify ships the oracle, dark-factory does the wiring):
#
#   exit_code=0
#   # $result holds the first matched load-bearing path (diagnostics on the
#   # exit-0 route only; the $exit_code branch below is what decides routing).
#   result=$(bash scripts/verify-pipeline-guard.sh requires-full-gate "${changed_files[@]}" < /dev/null) || exit_code=$?
#   # `< /dev/null`: an empty changed_files array expands to zero positional
#   # args, and with zero args the oracle falls back to reading stdin (see
#   # requires-full-gate above) — the redirect keeps a legitimate empty-diff
#   # call from blocking on an inherited/open caller stdin.
#   if [ "$exit_code" -eq 0 ]; then
#       : # Route to full --scope all gate (or run drift guards at minimum)
#   elif [ "$exit_code" -eq 1 ]; then
#       : # Config-only fast-path safe
#   else
#       : # exit 2 = usage error (mis-invocation, not a diff verdict): treat
#         # as full gate, fail-closed, and log loudly — never fall through
#         # silently.
#   fi
#
#   CAVEAT: `exit_code=0; result=$(...) || exit_code=$?` is NOT optional
#   boilerplate — a bare `result=$(...); exit_code=$?` (without the `||`)
#   would abort the caller's shell AT THE ASSIGNMENT under `set -e` whenever
#   the oracle exits non-zero (exit 1 = fast-path safe is the oracle's
#   normal non-zero outcome), because a command-substitution assignment is a
#   simple command for errexit purposes and the shell never reaches the
#   following `exit_code=$?` line. The `||` list exempts the assignment
#   from errexit.

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

# 5. Live emitted-gate derivation (task 6320): append scripts/<x>.sh for every
#    scripts/*.sh path invoked by verify.sh's EMITTED plan lines (add()/add_tool(),
#    the only two PLAN+= sites). These gate scripts are never `source`d, so
#    clause 3 cannot see them; without this clause a gate-script-only diff is
#    fast-path eligible (measured exit 1 at HEAD fee75336ca for
#    check-manifold-deps.sh and six siblings, while task 6243's two hand-added
#    manifest rows were the only covered ones). Self-healing: a future emitted
#    gate needs no verify-pipeline-paths.txt row.
#    Ordered here, ahead of clause 4, to sit with clause 3 — the two live
#    verify.sh derivations share the $_verify_sh resolved just above, so the
#    REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH override applies to both with no
#    second env knob.
#    The '^[[:space:]]*add(_tool)?[[:space:]]+"' statement anchor matches real
#    plan-emission STATEMENTS only (not '#'-prefixed comment mentions), the same
#    hardening clause 3's source-statement grep carries.
if [ -f "$_verify_sh" ]; then
    while IFS= read -r _gate; do
        [ -z "$_gate" ] && continue
        _SET="${_SET}"$'\n'"${_gate}"
    done < <(grep -E '^[[:space:]]*add(_tool)?[[:space:]]+"' "$_verify_sh" \
             | grep -oE '(\./)?scripts/[A-Za-z0-9_.-]+\.sh' \
             | sed -E 's|^\./||')
fi

# 4. Doc-sync manifest: non-comment/non-blank lines from doc-sync-paths.txt —
#    operational docs cross-referenced by tests/infra doc-sync checks (see
#    that file's header for the full rationale and the tradeoff breadcrumb).
#    REIFY_VERIFY_PIPELINE_GUARD_DOC_SYNC_PATHS overrides the manifest path
#    for testability (synthetic-doc injection) and operator use.
_doc_sync_paths="${REIFY_VERIFY_PIPELINE_GUARD_DOC_SYNC_PATHS:-$SCRIPT_DIR/doc-sync-paths.txt}"
if [ -f "$_doc_sync_paths" ]; then
    while IFS= read -r _line; do
        case "$_line" in
            '#'* | '') continue ;;
        esac
        _SET="${_SET}"$'\n'"${_line}"
    done < "$_doc_sync_paths"
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
        # Infra-test glob clause (task 5256; PRD merge-gate-health.md W3a):
        # ANY tests/infra/*.sh path is definitionally load-bearing — a new/renamed
        # infra test changes the merge-gate suite itself, so an infra-test diff is
        # never config-only. Open-ended glob (matches infra tests that don't exist
        # yet), hence a special-case here rather than a fixed-string manifest line.
        _infra_match=$(printf '%s\n' "$_normalized" \
                       | grep -m1 -E '^tests/infra/[^/]*\.sh$' 2>/dev/null \
                       || true)
        if [ -n "$_infra_match" ]; then
            echo "$_infra_match"
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
