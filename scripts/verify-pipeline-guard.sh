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
#   --list-plan-derived           — print ONLY the plan-derived contribution to
#                                   the 'emitted' clause below (4b): the *.sh
#                                   paths named by verify.sh's RESOLVED
#                                   --print-plan output. One repo-relative path
#                                   per line, sorted-unique; a strict SUBSET of
#                                   --list. Always exit 0, and legitimately
#                                   EMPTY on the fail-soft route (a failed
#                                   --print-plan is not an error here — the
#                                   union still answers correctly from 4a).
#                                   Diagnostic only, never a diff verdict. It
#                                   exists because under the union a broken 4b
#                                   is INVISIBLE in --list: this is what lets a
#                                   test — and an operator debugging a
#                                   surprising classification — see that half
#                                   on its own.
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
#   emitted:  every repo-relative *.sh path invoked by an EMITTED plan line in
#             verify.sh — i.e. an add() / add_tool() argument — whatever
#             directory it lives in (scripts/, tests/, …)
#             (auto-derived live; self-healing — a future lint/gate script
#             wired into the plan is automatically load-bearing without any
#             manifest edit). These gate scripts are never `source`d, so the
#             'sourced' clause above cannot see them; without this clause a
#             gate-script-only diff is fast-path eligible and lands without
#             ever having been run (task 6320; the #4618/#4624 -> #4288
#             ambush class, doc-side analogue in 'doc-sync' below).
#             A UNION OF TWO DERIVATIONS (task 6426):
#               (4a) SOURCE-TEXT — grep verify.sh's add()/add_tool() STATEMENTS
#                    for literal paths. Covers every literal path in every plan
#                    shape, needs no fork, and works on a verify.sh that cannot
#                    run at all.
#               (4b) PLAN-DERIVED — extract paths from the RESOLVED plan printed
#                    by ONE canonical widest invocation:
#                      DF_VERIFY_ROLE=merge bash verify.sh all --scope all \
#                          --profile both --include-infra --print-plan
#                    This is what covers a plan line assembled through a
#                    VARIABLE (`_cmd="./scripts/x.sh"; add_tool "$_cmd"` — the
#                    _gui_cmd / _sidecar_cmd / _ts_cmd shape), which begins with
#                    the assignment rather than with `add_tool` and is therefore
#                    invisible to 4a. Inspect it alone with --list-plan-derived.
#             MONOTONICITY is the safety argument for the whole design: 4b can
#             only ever ADD to the set. If --print-plan fails, times out, or
#             emits nothing, the classifier degrades to 4a's source-text floor —
#             never below what it covered before task 6426, and never fail-open.
#             That is why the two are a union and not a replacement: an empty
#             derivation reads as "fast-path safe", so a replacement would
#             re-open the very ambush class this clause exists to close.
#             RESIDUAL LIMITATION (deliberate, pinned by Pair E's RESIDUAL
#             LIMITATION case in tests/infra/test_verify_pipeline_guard.sh): 4b
#             derives whatever the ONE canonical invocation RESOLVES, so a plan
#             line reachable only under some other shape — a branch of
#             build_plan that invocation never takes — is underived by 4b, and
#             underived by 4a too if its path is behind a variable. A gate wired
#             that way still needs a verify-pipeline-paths.txt row (or a rewrite
#             to a literal path, or to a branch the canonical invocation
#             reaches). Note the widenings in that invocation are load-bearing
#             precisely because they shrink this residual: measured, dropping
#             --include-infra and role=merge takes the derived set from 12 gates
#             to 6.
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
#             ALL THREE live derivations that consult verify.sh: the sourced-lib
#             clause (3), the source-text emitted-gate clause (4a) and the
#             plan-derived emitted-gate clause (4b) (testability / operator
#             override; mirrors the REIFY_* knob idiom used throughout verify.sh
#             and its libs). One knob, three clauses — a synthetic verify.sh
#             injected here drives all three alike, and they can never disagree
#             about WHICH verify.sh they describe.
#             SEMANTICS WIDENED (task 6426) — READ BEFORE SETTING THIS: clauses
#             3 and 4a only READ the file; clause 4b EXECUTES it
#             (`bash "$VALUE" … --print-plan`). Pointing this knob somewhere is
#             therefore choosing WHAT CODE THE GUARD RUNS, not merely what it
#             parses. In practice that adds no attack surface — whoever sets the
#             guard's environment already controls the guard invocation itself —
#             but it must not be discovered by surprise. Verified non-recursive:
#             verify.sh never invokes verify-pipeline-guard.sh (grepped, zero
#             hits), so executing it from inside the guard cannot loop.
#   REIFY_VERIFY_PIPELINE_GUARD_PRINT_PLAN_TIMEOUT — seconds; wall-clock ceiling
#             on clause 4b's single --print-plan fork. Default 45, roughly 100x
#             the measured ~0.4s cost of the real invocation. This guard runs on
#             EVERY dark-factory merge-worker classification, so a wedged
#             verify.sh must not be able to hang it; on expiry clause 4b derives
#             nothing and the classifier degrades to its clause-4a source-text
#             floor (bounded, never fail-open). Lowered by
#             tests/infra/test_verify_pipeline_guard.sh Pair E (c-bis)'s BOUNDED
#             case so that assertion stays cheap rather than paying the default.
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

# 4. Live emitted-gate derivation (task 6320): append every repo-relative
#    *.sh path invoked by verify.sh's EMITTED plan lines (add()/add_tool(),
#    the only two PLAN+= sites). These gate scripts are never `source`d, so
#    clause 3 cannot see them; without this clause a gate-script-only diff is
#    fast-path eligible (measured exit 1 at HEAD fee75336ca for
#    check-manifold-deps.sh and six siblings, while task 6243's two hand-added
#    manifest rows were the only covered ones). Self-healing: a future emitted
#    gate needs no verify-pipeline-paths.txt row.
#    Ordered immediately after clause 3 because the two live verify.sh
#    derivations share the $_verify_sh resolved just above, so the
#    REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH override applies to both with no
#    second env knob.
#    NOT scripts/-only: tests/sync_comments_test.sh (verify.sh:2627) is an
#    emitted gate of the identical ambush class living outside scripts/, and a
#    prefix-restricted clause missed it (measured exit 1). Deriving any
#    directory-qualified *.sh keeps the clause honest about the whole class.
#    tests/infra/*.sh paths so derived (run_all.sh) are a harmless duplicate of
#    the in-code infra-test glob clause at the dispatch site below — the set is
#    sort -u'd here, and that clause matches by regex regardless.
#    LIMITATION: literal paths only — see the header's 'emitted' bullet.
if [ -f "$_verify_sh" ]; then
    while IFS= read -r _gate; do
        [ -z "$_gate" ] && continue
        _SET="${_SET}"$'\n'"${_gate}"
    #    Both greps below are load-bearing, not decoration. Do not simplify
    #    either away. (This block sits at the TAIL of the loop body, not above
    #    the `if`, so it is adjacent to the pipeline it documents — where a
    #    future simplifier actually looks. Bash admits no comment between
    #    `done` and its `< <(...)` redirect, so this is as close as the syntax
    #    allows; please keep it here rather than hoisting it.)
    #
    #    (i) '^[[:space:]]*add(_tool)?[[:space:]]+' is a STATEMENT anchor: it
    #    matches real plan-emission statements only, never a '#'-prefixed
    #    comment mention — the same hardening clause 3's source-statement grep
    #    carries. The exclusion comes from the leading-'add' anchor ALONE, which
    #    is why no quote character is required here: `add './scripts/x.sh'` is
    #    a live idiom (cf. verify.sh:2610) and demanding a '"' silently dropped
    #    it while costing no precision (pinned both ways in Pair E (c)).
    #
    #    (ii) the '(^|[^A-Za-z0-9_./-])' LEFT and '([^A-Za-z0-9_.-]|$)' RIGHT
    #    path boundaries. Without the left one, grep -o matches the
    #    'scripts/x.sh' TAIL of an emitted 'other/scripts/x.sh' and collapses it
    #    to the top-level 'scripts/x.sh', promoting an unrelated script. Without
    #    the right one — the character class contains '.' — an emitted
    #    'scripts/x.sha256sums' backtracks to a 'scripts/x.sh' match, the same
    #    over-match on the other side. (The in-code infra-test glob clause
    #    carries the left property via its '^' anchor; both are pinned by
    #    Pair E (c) in
    #    tests/infra/test_verify_pipeline_guard.sh.) Each class consumes one
    #    adjacent character, which the fully-anchored sed capture then strips
    #    along with any './' prefix.
    #
    #    The '+(/…)+' shape requires the path to be DIRECTORY-QUALIFIED, which
    #    is what keeps a bare basename inside a plan line's diagnostic string
    #    ("WARNING: sync_comments_test.sh not found") out of the derived set.
    done < <(grep -E '^[[:space:]]*add(_tool)?[[:space:]]+' "$_verify_sh" \
             | grep -oE '(^|[^A-Za-z0-9_./-])(\./)?[A-Za-z0-9_.-]+(/[A-Za-z0-9_.-]+)+\.sh([^A-Za-z0-9_.-]|$)' \
             | sed -E 's|^[^A-Za-z0-9_./-]?(\./)?([A-Za-z0-9_.-]+(/[A-Za-z0-9_.-]+)+\.sh)[^A-Za-z0-9_.-]?$|\2|')
fi

# 4b. Live emitted-gate derivation, PLAN-DERIVED half (task 6426): append every
#    repo-relative *.sh path named by a line of verify.sh's RESOLVED plan, as
#    produced by --print-plan.
#
#    WHAT THIS BUYS OVER 4a: a plan line assembled through a VARIABLE
#    (`_cmd="./scripts/x.sh"; add_tool "$_cmd"` — the shape verify.sh uses for
#    _gui_cmd / _sidecar_cmd / _ts_cmd, assembled at scripts/verify.sh:2568-2574
#    and emitted at :2651-2653) is invisible to a source-text grep, because the
#    statement begins with the assignment rather than with `add_tool`. The
#    resolved plan names the path outright.
#
#    UNIONED ONTO CLAUSE 4a, NEVER REPLACING IT — and that is a safety property,
#    not a style choice. The union makes the classifier MONOTONE: 4b can only
#    ever ADD a path, so a --print-plan failure (missing sibling libs, absent
#    cargo, a hard-failing nextest probe, a wedged run) degrades to clause 4a's
#    source-text floor and can NEVER classify something as less load-bearing
#    than it is today. A REPLACEMENT would fail OPEN, since an empty derivation
#    reads as "fast-path safe" — recreating the exact #4618/#4624 -> #4288
#    ambush class this whole clause exists to prevent. Do not collapse the two
#    halves into one; tests/infra/test_verify_pipeline_guard.sh Pair E (c-bis)
#    pins the fail-soft direction.
#
#    Shares the $_verify_sh resolved at clause 3 — one knob, three clauses, no
#    second env var. NOTE that the knob's semantics WIDEN here from READ to
#    EXECUTE; see the header's Environment knobs block.
#    Captured into its OWN variable as well as unioned into _SET, so
#    --list-plan-derived can print this half ALONE without a second fork. That
#    isolation is not a nicety: under the union a broken 4b is invisible in
#    --list, and being able to see it alone is what makes the non-vacuity
#    assertion in Pair E (c-bis) possible at all.
_PLAN_DERIVED_SET=""
if [ -f "$_verify_sh" ]; then
    while IFS= read -r _gate; do
        [ -z "$_gate" ] && continue
        _PLAN_DERIVED_SET="${_PLAN_DERIVED_SET}"$'\n'"${_gate}"
        _SET="${_SET}"$'\n'"${_gate}"
    #    Three things about the pipeline below are load-bearing. (Same tail-of-
    #    loop-body placement as clause 4a, and for the same reason: bash admits
    #    no comment between `done` and its `< <(...)` redirect.)
    #
    #    (a) THE CANONICAL INVOCATION — action=all, --scope all, --profile both,
    #    --include-infra, DF_VERIFY_ROLE=merge. Every one of those five
    #    widenings is load-bearing rather than decoration. Measured on this
    #    tree: plain `all --scope all --profile both` derives only 6 of the 12
    #    gates; adding --include-infra reaches 11; role=merge is what adds
    #    tests/infra/run_all.sh. With all five, the derived set is
    #    byte-identical BOTH to clause 4a's source-text set AND to the union
    #    over a 4-action x 3-scope x 4-role, 48-invocation matrix — so ONE fork
    #    is the exact superset today, and an N-way union would multiply the
    #    fork cost while buying nothing.
    #
    #    (b) THE `grep -v '^#'` FILTER. --print-plan emits a header, a NOTE
    #    line, a scope-decision line, a narrowing line, an environment block and
    #    per-command annotations, all '#'-prefixed. No `.sh` path appears in any
    #    of them TODAY, so this filter is belt-and-braces rather than strictly
    #    load-bearing — but those lines are free prose, and a future annotation
    #    that named a script path would silently promote it to load-bearing.
    #    Keep the filter; it is not the no-op it looks like.
    #
    #    (c) THE REGEX AND NORMALIZER ARE CLAUSE 4a's, VERBATIM (see the
    #    pipeline just above and the over-match rationale written up with it),
    #    so both path boundaries and the directory-qualified '+(/…)+' shape
    #    carry over for free rather than being re-derived and drifting.
    #    THE ONE DELIBERATE DIFFERENCE: 4a's
    #    '^[[:space:]]*add(_tool)?[[:space:]]+' STATEMENT ANCHOR is absent here,
    #    and must STAY absent. --print-plan emits RESOLVED COMMANDS, not add()
    #    statements, so the anchor would match nothing and silently zero this
    #    entire clause. Do not "restore" it for symmetry. The comment-exclusion
    #    job the anchor does for 4a is done here by (b)'s '^#' filter instead.
    #
    #    (d) THE FOUR HARDENING ELEMENTS, each load-bearing:
    #      - `timeout "${REIFY_VERIFY_PIPELINE_GUARD_PRINT_PLAN_TIMEOUT:-45}"`
    #        bounds the merge-worker hot path: this guard runs on EVERY
    #        classification, so a wedged verify.sh must not be able to hang it.
    #        45s is ~100x the measured ~0.4s cost of the real invocation, and
    #        /usr/bin/timeout is already the idiom throughout verify.sh's own
    #        plan lines. On expiry the clause derives nothing and the classifier
    #        degrades to its 4a floor — bounded, never fail-open.
    #      - `REIFY_NEXTEST_PROBE_RETRY_SLEEP=0`: verify.sh's nextest probe runs
    #        UNCONDITIONALLY in print mode and is explicitly NOT covered by the
    #        "hermetic oracle" guarantee (its own scope note at
    #        scripts/verify.sh:1702-1714 names this knob and says automation
    #        invoking --print-plan repeatedly should set it). Worst case without
    #        it: 4 cargo forks plus 2x the retry sleep before a hard fail.
    #      - `DF_VERIFY_ROLE=merge` is part of the canonical widest shape (a),
    #        but setting it EXPLICITLY also stops an ambient DF_VERIFY_ROLE in
    #        the caller's environment from silently narrowing the derived set —
    #        an inherited role=task would drop tests/infra/run_all.sh.
    #      - `2>/dev/null` + `|| true`: verify.sh warns on stderr for benign
    #        reasons and the guard must not pollute its caller's log with them;
    #        the `|| true` keeps a non-zero --print-plan (the fail-soft route)
    #        from aborting the guard under this file's `set -euo pipefail`, the
    #        same errexit hazard the header's CAVEAT documents for the guard's
    #        own callers.
    done < <(DF_VERIFY_ROLE=merge REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 \
                 timeout "${REIFY_VERIFY_PIPELINE_GUARD_PRINT_PLAN_TIMEOUT:-45}" \
                 bash "$_verify_sh" \
                 all --scope all --profile both --include-infra --print-plan \
                 2>/dev/null \
             | grep -v '^#' \
             | grep -oE '(^|[^A-Za-z0-9_./-])(\./)?[A-Za-z0-9_.-]+(/[A-Za-z0-9_.-]+)+\.sh([^A-Za-z0-9_.-]|$)' \
             | sed -E 's|^[^A-Za-z0-9_./-]?(\./)?([A-Za-z0-9_.-]+(/[A-Za-z0-9_.-]+)+\.sh)[^A-Za-z0-9_.-]?$|\2|' \
             || true)
fi

# 5. Doc-sync manifest: non-comment/non-blank lines from doc-sync-paths.txt —
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

# Clause 4b's contribution alone, for the --list-plan-derived diagnostic. `sed`
# rather than `grep -v '^$'` to drop the accumulator's leading blank: sed always
# exits 0, whereas grep exits 1 on an all-empty set and `set -o pipefail` would
# turn the documented fail-soft outcome (empty derivation) into a hard abort.
_SORTED_PLAN_DERIVED_SET="$(printf '%s\n' "$_PLAN_DERIVED_SET" | sed '/^$/d' | sort -u)"

# ---------------------------------------------------------------------------
# Subcommand dispatch
# ---------------------------------------------------------------------------

_subcmd="${1:-}"

case "$_subcmd" in
    --list)
        printf '%s\n' "$_SORTED_SET"
        exit 0
        ;;
    --list-plan-derived)
        # Diagnostic: clause 4b's contribution in ISOLATION — a strict subset of
        # --list. Reuses the set built above; does NOT re-run --print-plan.
        # An EMPTY set is a legitimate exit-0 outcome, not an error: it is what
        # the documented fail-soft route produces when the plan derivation
        # fails, and the union means the classifier is still correct. Guarded by
        # an `if` rather than `[ -n ... ] && printf` so an empty set neither
        # prints a spurious blank line nor trips errexit before `exit 0`.
        if [ -n "$_SORTED_PLAN_DERIVED_SET" ]; then
            printf '%s\n' "$_SORTED_PLAN_DERIVED_SET"
        fi
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
        printf 'Usage: %s requires-full-gate [file...] | --list | --list-plan-derived\n' "$(basename "$0")" >&2
        printf '  requires-full-gate: exits 0 if any file is load-bearing (full gate required),\n' >&2
        printf '                      1 if none (fast-path safe); reads stdin when no args.\n' >&2
        printf '  --list: print the canonical load-bearing path set (one path per line).\n' >&2
        printf '  --list-plan-derived: print only the --print-plan-derived subset of that set\n' >&2
        printf '                       (diagnostic; exit 0, possibly empty on the fail-soft route).\n' >&2
        exit 2
        ;;
esac
