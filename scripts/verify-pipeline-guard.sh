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
#   is-registered <path>          — read-only membership query over reify's
#                                   registries: exit 0 if <path> is registered
#                                   as load-bearing by ANY route below, 1 if it
#                                   is registered by none, 2 on a usage error.
#                                   Takes EXACTLY ONE path (no stdin mode); zero
#                                   or two-or-more is exit 2 — see the arity note
#                                   under the matched sets below. Writes NOTHING
#                                   to stdout on either verdict route; the
#                                   which-registry diagnostic goes to stderr.
#                                   CAUTION — EXIT 0 HERE MEANS "REGISTERED",
#                                   NOT "FULL GATE REQUIRED". This subcommand
#                                   and requires-full-gate share an exit-0
#                                   spelling and answer DIFFERENT questions: a
#                                   path registered only as a
#                                   verify-pipeline-infra-tests.txt row is
#                                   load-bearing at the cheaper SURGICAL cost
#                                   point (it SELECTS its guarding infra test at
#                                   task scope) and is deliberately still exit 1
#                                   for requires-full-gate. Never route a diff
#                                   on this subcommand's exit code.
#                                   MATCHED SETS: the static load-bearing set
#                                   (clauses 1/2/3/4a/5 below) UNION the
#                                   open-ended tests/infra/*.sh glob UNION the
#                                   ACTIVE-ROW KEYS of
#                                   scripts/verify-pipeline-infra-tests.txt.
#                                   Deliberately WIDER than the two registries:
#                                   a path load-bearing by any existing route
#                                   must answer 0, or the anti-drift sweep that
#                                   consumes this (tests/infra/test_verify_pipeline_guard.sh
#                                   Pair C clause (d), task 6857) reds on a
#                                   legitimate registration. The map keys are
#                                   read LAZILY here and are never folded into
#                                   the requires-full-gate set.
#
# Exit-code contract:
#   0 — full gate REQUIRED (at least one load-bearing file in the diff)
#   1 — fast-path safe (no load-bearing file found)
#   2 — usage error (unknown subcommand or flag)
# For is-registered the 0/1 pair re-reads as REGISTERED / NOT REGISTERED (2 is
# unchanged). Same three codes, different question — see its CAUTION above.
#
# The load-bearing set is the union of:
#   anchor:   scripts/verify.sh (always load-bearing)
#   manifest: all non-comment/non-blank lines in scripts/verify-pipeline-paths.txt
#   sourced:  scripts/<lib> for each 'source "$SCRIPT_DIR/<lib>"' line in verify.sh
#             (auto-derived live; self-healing — future sourced libs are
#             automatically load-bearing without any manifest edit)
#   emitted:  every repo-relative *.sh path invoked by an EMITTED plan line in
#             verify.sh, whatever directory it lives in (scripts/, tests/, …).
#             Auto-derived live and self-healing: a future lint/gate script
#             wired into the plan is load-bearing with no manifest edit. These
#             gate scripts are never `source`d, so the 'sourced' clause cannot
#             see them; without this clause a gate-script-only diff is
#             fast-path eligible and lands without ever having been run (task
#             6320; the #4618/#4624 -> #4288 ambush class, doc-side analogue in
#             'doc-sync' below).
#             It is a UNION of two derivations (task 6426) — (4a) a grep of
#             verify.sh's SOURCE TEXT for literal paths in add()/add_tool()
#             statements, and (4b) the paths named by the RESOLVED plan that
#             one canonical widest --print-plan invocation prints, which is
#             what covers a plan line assembled through a VARIABLE. The union
#             is MONOTONE: 4b can only ADD, so a failed derivation degrades to
#             4a's floor and never fails open. Inspect 4b alone with
#             --list-plan-derived.
#             >>> CANONICAL WRITE-UP: clause 4b's block comment below. It owns
#             the rationale, the measurements and the residual limitation; this
#             bullet is a summary and must not grow a second copy of them.
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
#   REIFY_VERIFY_PIPELINE_GUARD_INFRA_TESTS_MAP — override path to the
#             citing-test map read by `is-registered` clause (iii)
#             (testability / synthetic-injection + operator override; the exact
#             sibling of the doc-sync knob above, same
#             ${VAR:-$SCRIPT_DIR/...} default shape and same graceful
#             degradation to "no keys" when the file is absent). Defaults to
#             scripts/verify-pipeline-infra-tests.txt.
#             UNLIKE REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH, this one is
#             READ-ONLY: the map is parsed, never executed, and nothing named
#             in it is run. Pointing it somewhere chooses what the guard
#             PARSES, not what it RUNS — so it does not carry that knob's
#             "you are choosing what code the guard executes" caveat.
#             It affects `is-registered` ONLY. requires-full-gate, --list and
#             --list-plan-derived never read this map, by design (see the
#             is-registered arm's note (1)), so setting this knob cannot change
#             any diff verdict.
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

# --- Shared *.sh path extraction, used by BOTH emitted-gate halves ---------
# ONE maintained copy of the path regex and its normalizer (task 6426 review).
# Clauses 4a and 4b previously carried byte-identical private copies of both;
# nothing enforced that, and the over-match hazards the boundaries defend
# against (see (ii) at clause 4a) are subtle enough that an edit to one copy
# would plausibly have missed the other.
#
# Reads candidate lines on stdin, writes one normalized repo-relative *.sh path
# per match on stdout. `grep` exits 1 on no match, which is a legitimate empty
# result here, not an error — both call sites consume this in a process
# substitution whose status the enclosing shell never inspects, and 4b adds an
# explicit `|| true` besides.
#
#   (i) '(^|[^A-Za-z0-9_./-])' LEFT and '([^A-Za-z0-9_.-]|$)' RIGHT path
#   boundaries. Without the left one, grep -o matches the 'scripts/x.sh' TAIL of
#   an 'other/scripts/x.sh' and collapses it to the top-level 'scripts/x.sh',
#   promoting an unrelated script. Without the right one — the character class
#   contains '.' — a 'scripts/x.sha256sums' backtracks to a 'scripts/x.sh'
#   match, the same over-match on the other side. Each class consumes one
#   adjacent character, which the fully-anchored sed capture then strips along
#   with any './' prefix.
#
#   (ii) The '+(/…)+' shape requires the path to be DIRECTORY-QUALIFIED, which
#   is what keeps a bare basename inside a plan line's diagnostic string
#   ("WARNING: sync_comments_test.sh not found") out of the derived set.
#
# Both properties are pinned by Pair E (c) in
# tests/infra/test_verify_pipeline_guard.sh, through BOTH halves: the
# 'other/scripts/zzz-nested.sh' and 'scripts/zzz-right.sha256sums' cases for 4a,
# and their plan-derived counterparts for 4b.
_SH_PATH_ERE='(^|[^A-Za-z0-9_./-])(\./)?[A-Za-z0-9_.-]+(/[A-Za-z0-9_.-]+)+\.sh([^A-Za-z0-9_.-]|$)'
_SH_PATH_NORMALIZE_SED='s|^[^A-Za-z0-9_./-]?(\./)?([A-Za-z0-9_.-]+(/[A-Za-z0-9_.-]+)+\.sh)[^A-Za-z0-9_.-]?$|\2|'
_extract_sh_paths() {
    grep -oE "$_SH_PATH_ERE" | sed -E "$_SH_PATH_NORMALIZE_SED"
}

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
#    NOT scripts/-only: tests/sync_comments_test.sh (grep `sync_comments_test.sh`
#    in scripts/verify.sh) is an
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
    #    a live idiom (grep -E "^\s*add(_tool)? '" scripts/verify.sh) and
    #    demanding a '"' silently dropped
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
             | _extract_sh_paths)
fi

# 4b. Live emitted-gate derivation, PLAN-DERIVED half (task 6426): append every
#    repo-relative *.sh path named by a line of verify.sh's RESOLVED plan, as
#    produced by --print-plan.
#
#    THIS BLOCK IS THE CANONICAL WRITE-UP for the emitted-gate union. The
#    header's 'emitted' bullet, scripts/verify-pipeline-paths.txt's EMITTED GATE
#    SCRIPTS note, docs/notes/verify-pipeline-knobs.md and Pair E's RESIDUAL
#    LIMITATION comment all POINT HERE and deliberately carry no second copy of
#    the rationale or the measurements. Keep it that way: four independently
#    maintained copies of one claim is the drift shape this repo's own
#    no-lockstep-duplication norm warns about, and every measurement below goes
#    stale the moment verify.sh grows a variable-assembled plan line — which is
#    the entire point of the clause.
#
#    WHAT THIS BUYS OVER 4a: a plan line assembled through a VARIABLE
#    (`_cmd="./scripts/x.sh"; add_tool "$_cmd"`) is invisible to a source-text
#    grep, because the statement begins with the assignment rather than with
#    `add_tool`. The resolved plan names the path outright.
#
#    HOW MUCH IT BUYS TODAY: NOTHING — and that is the honest reading, not a
#    defect. Measured on this tree, `--list-plan-derived` is BYTE-IDENTICAL to
#    clause 4a's source-text set (12 paths). verify.sh has no live plan line
#    that names a *.sh path from behind a variable. (The `_gui_cmd` /
#    `_sidecar_cmd` / `_ts_cmd` triple — grep `_gui_cmd=` in scripts/verify.sh
#    — IS variable-assembled, but its value is a pure npm shell snippet
#    containing no *.sh path at all, so neither half can derive anything from
#    it; and its `add_tool "$_gui_cmd"` emission sits in the plain-path branch
#    guarded by `[ "$DO_LINT" -eq 0 ] || [ "$RUN_RUST" -eq 0 ]`, which the
#    canonical widest invocation below never takes. It is a RESIDUAL-bucket
#    example on both counts — do not cite it as a covered one.) The clause is
#    therefore FUTURE-PROOFING: it closes the idiom before someone reaches for
#    it. The only place the closure is actually exercised is the synthetic
#    `scripts/zzz-print-plan-variable.sh` case in Pair E of
#    tests/infra/test_verify_pipeline_guard.sh, which injects that shape into a
#    REACHED branch of build_plan.
#
#    UNIONED ONTO CLAUSE 4a, NEVER REPLACING IT — and that is a safety property,
#    not a style choice. The union makes the classifier MONOTONE: 4b can only
#    ever ADD a path, so a --print-plan failure (missing sibling libs, absent
#    cargo, a hard-failing nextest probe, a wedged run, an unwritable TMPDIR)
#    degrades to clause 4a's source-text floor and can NEVER classify something
#    as less load-bearing than it is today. A REPLACEMENT would fail OPEN, since
#    an empty derivation reads as "fast-path safe" — recreating the exact
#    #4618/#4624 -> #4288 ambush class this whole clause exists to prevent. Do
#    not collapse the two halves into one;
#    tests/infra/test_verify_pipeline_guard.sh Pair E (c-bis) pins the fail-soft
#    direction.
#
#    RESIDUAL LIMITATION (deliberate, pinned by Pair E's RESIDUAL LIMITATION
#    case): 4b derives whatever the ONE canonical invocation RESOLVES, so a plan
#    line reachable only under some other shape — a branch of build_plan that
#    invocation never takes — is underived by 4b, and underived by 4a too if its
#    path is behind a variable. A gate wired that way still needs a
#    verify-pipeline-paths.txt row (or a rewrite to a literal path, or to a
#    branch the canonical invocation reaches). The five widenings in that
#    invocation are load-bearing precisely because they shrink this residual:
#    measured, dropping --include-infra and role=merge takes the derived set
#    from 12 gates to 6.
#
#    Shares the $_verify_sh resolved at clause 3 — one knob, three clauses, no
#    second env var. NOTE that the knob's semantics WIDEN here from READ to
#    EXECUTE; see the header's Environment knobs block.
#    Captured into its OWN variable as well as unioned into _SET, so
#    --list-plan-derived can print this half ALONE without a second fork. That
#    isolation is not a nicety: under the union a broken 4b is invisible in
#    --list, and being able to see it alone is what makes the non-vacuity
#    assertion in Pair E (c-bis) possible at all.
#
#    EVALUATED LAZILY (task 6426 review), via derive_plan_paths below rather
#    than at top level. Deferring it costs nothing in correctness — the union is
#    order-independent, and the ONLY outcome 4b can change is flipping a
#    would-be exit 1 into exit 0, so consulting it after the static clauses miss
#    is exactly as load-bearing as consulting it first. Monotonicity, fail-soft
#    and the --list-plan-derived diagnostic are all preserved verbatim.
#    BE PRECISE ABOUT WHAT LAZINESS SAVES, because the intuitive reading is
#    backwards. The fork is skipped ONLY when a static clause has already
#    matched — i.e. only when the answer is already "full gate required". The
#    guard is consulted to decide fast-path ELIGIBILITY, so exit 1 is the
#    majority outcome, and exit 1 is precisely the branch that reaches
#    derive_plan_paths. Measured on this tree (3-run averages):
#        requires-full-gate crates/reify-eval/src/lib.rs -> exit 1, ~1.4s FORKED
#        requires-full-gate scripts/verify.sh            -> exit 0, ~0.09s no fork
#        requires-full-gate scripts/check-manifold-deps.sh -> exit 0, ~0.13s no fork
#        --bogus (usage error)                           -> exit 2, ~0.13s no fork
#    (the forked figure ranges ~0.3-1.4s with machine load). So most
#    classifications DO pay the fork; what laziness buys is that the answer
#    "full gate required" — and the usage-error branch, which never reads the
#    set — return without one. That is a real saving on a bounded, sub-second
#    cost, not an "the common case does not fork" optimisation.
#    ONE DIAGNOSTIC-ONLY CONSEQUENCE, deliberate: on a diff containing BOTH a
#    plan-derived-only path and a statically-matched one, requires-full-gate now
#    prints the statically-matched path rather than whichever came first in the
#    input. The exit code — the thing that decides routing — is unchanged, and
#    the two sets are byte-identical today so the case is not reachable yet.
_PLAN_DERIVED_SET=""
_SORTED_PLAN_DERIVED_SET=""
_PLAN_DERIVED_DONE=0

# Memoized: forks --print-plan at most ONCE per guard process, so a caller that
# consults the set twice (or a future clause that does) pays for one fork.
derive_plan_paths() {
    if [ "$_PLAN_DERIVED_DONE" -eq 1 ]; then
        return 0
    fi
    _PLAN_DERIVED_DONE=1
    if [ ! -f "$_verify_sh" ]; then
        return 0
    fi
    # --- THE CANONICAL INVOCATION ------------------------------------------
    # action=all, --scope all, --profile both, --include-infra,
    # DF_VERIFY_ROLE=merge. Every one of those five widenings is load-bearing
    # rather than decoration. Measured on this tree: plain
    # `all --scope all --profile both` derives only 6 of the 12 gates; adding
    # --include-infra reaches 11; role=merge is what adds tests/infra/run_all.sh.
    # With all five, the derived set is byte-identical BOTH to clause 4a's
    # source-text set AND to the union over a 4-action x 3-scope x 4-role,
    # 48-invocation matrix — so ONE fork is the exact superset today, and an
    # N-way union would multiply the fork cost while buying nothing.
    #
    # --- AMBIENT-ENVIRONMENT SCRUB (task 6426 review) ----------------------
    # The fork must be shaped by its FLAGS, not by whatever the caller happened
    # to export. verify.sh reads ~38 REIFY_*/DF_* knobs and several of them
    # narrow the plan, so an inherited one silently shrinks this clause. That is
    # not hypothetical: MEASURED on this tree, an ambient
    # REIFY_INFRA_SUITE_ACTIVE=1 takes the derived set from 12 paths to 11 (it
    # is verify.sh's re-entrancy sentinel — see its RE-ENTRANCY GUARD comment —
    # and suppresses the very tests/infra/run_all.sh line that role=merge is
    # here to add). Monotonicity means such a loss can never fail OPEN, but
    # clause 4b's only value IS the extra coverage, so a quietly narrowed fork
    # is a clause quietly doing nothing.
    #
    # WHY A PREFIX SCRUB AND NOT A HAND-PICKED LIST. The list shape drifts and
    # is already known to miss: sibling captures in tests/infra each maintain
    # their own (`env -u REIFY_INFRA_SUITE_ACTIVE -u REIFY_RELEASE_DELTA_SKIP
    # -u REIFY_VERIFY_PREBUILD_TIMEOUT` in test_occt_flock_gate.sh, a shorter
    # one in test_run_all_ambient_isolation.sh), and the two knobs most likely
    # to be named first — REIFY_AFFECTED_CRATES_OVERRIDE and
    # REIFY_RELEASE_DELTA_SKIP — measure as NON-narrowing here (12 -> 12), while
    # the one that does narrow is neither. Scrubbing the whole REIFY_*/DF_*
    # prefix is self-healing in the same way clauses 3/4a/4b are: a future
    # narrowing knob is neutralized with no edit here.
    #
    # SAFE IN BOTH DIRECTIONS. Removing a narrowing knob can only WIDEN the
    # derived set, and 4b is monotone, so a wider set is always the safe error.
    # Nothing verify.sh needs to RUN is lost: the one provisioning-flavoured
    # knob in the prefix, REIFY_AMBIENT_LD_LIBRARY_PATH, is EXPORTED BY verify.sh
    # itself from LD_LIBRARY_PATH (scripts/verify.sh, search
    # REIFY_AMBIENT_LD_LIBRARY_PATH) rather than consumed as an inbound
    # setting, so it is recomputed. LD_LIBRARY_PATH, PATH, HOME and TMPDIR are
    # outside the prefix and pass through untouched. The guard's OWN
    # REIFY_VERIFY_PIPELINE_GUARD_* knobs are read by THIS shell before the fork
    # ($_verify_sh at clause 3, the timeout just below), so scrubbing them from
    # the child is a no-op for them too.
    #
    # `env` applies its -u removals BEFORE its NAME=VALUE assignments, so the
    # three explicit settings below always win over the scrub.
    local _env_scrub=()
    local _v
    while IFS= read -r _v; do
        [ -z "$_v" ] && continue
        _env_scrub+=( -u "$_v" )
    done < <(compgen -e | grep -E '^(REIFY|DF)_' || true)

    # --- CAPTURE TO A FILE, NOT A PIPE ------------------------------------
    # `timeout` bounds the verify.sh PROCESS; it does not bound OUR READ. Piping
    # --print-plan straight into the loop made the guard's exit depend on that
    # pipe reaching EOF, which requires EVERY inherited write end to close — so
    # a grandchild that escapes the process group (setsid/daemonized), or a
    # non-GNU `timeout` that signals only the direct child, could leave the
    # guard blocked with NO bound at all, in the merge-worker hot path. Note a
    # command-substitution capture would NOT have fixed this: `$(...)` reads to
    # EOF exactly like the loop did. A regular file does: it EOFs at its current
    # size no matter who still holds the descriptor.
    # mktemp failure is fail-soft like every other 4b failure — return with an
    # empty set and let the union answer from clause 4a's floor.
    local _plan_out
    _plan_out="$(mktemp)" || return 0

    # HARDENING ELEMENTS, each load-bearing:
    #   - `timeout -k 5 "${REIFY_VERIFY_PIPELINE_GUARD_PRINT_PLAN_TIMEOUT:-45}"`
    #     bounds the merge-worker hot path: this guard runs on EVERY
    #     classification, so a wedged verify.sh must not be able to hang it.
    #     45s is ~100x the measured ~0.4s cost of the real invocation, and
    #     /usr/bin/timeout is already the idiom throughout verify.sh's own plan
    #     lines. `-k 5` escalates to SIGKILL 5s after the SIGTERM, so a child
    #     that ignores or is wedged past TERM is still reaped. On expiry the
    #     clause derives nothing and the classifier degrades to its 4a floor —
    #     bounded, never fail-open.
    #   - `REIFY_NEXTEST_PROBE_RETRY_SLEEP=0`: verify.sh's nextest probe runs
    #     UNCONDITIONALLY in print mode and is explicitly NOT covered by the
    #     "hermetic oracle" guarantee (its own scope note at scripts/verify.sh —
    #     search "Scope note re: --print-plan hermeticity" — names this knob and
    #     says automation invoking --print-plan repeatedly should set it). Worst
    #     case without it: 4 cargo forks plus 2x the retry sleep before a hard
    #     fail.
    #   - `DF_VERIFY_ROLE=merge` is part of the canonical widest shape above;
    #     it is set explicitly (as well as scrubbed) so the shape is stated at
    #     the call site rather than left implicit in verify.sh's default.
    #   - `</dev/null`: the fork must never consume or block on the guard's own
    #     stdin. In `requires-full-gate <args>` mode stdin is untouched and
    #     still open — exactly the hazard the header's usage example tells
    #     CALLERS to close with `< /dev/null`, which the guard owes its own
    #     children in turn.
    #   - `2>/dev/null` + `|| true`: verify.sh warns on stderr for benign
    #     reasons and the guard must not pollute its caller's log with them; the
    #     `|| true` keeps a non-zero --print-plan (the fail-soft route) from
    #     aborting the guard under this file's `set -euo pipefail`, the same
    #     errexit hazard the header's CAVEAT documents for the guard's own
    #     callers.
    env "${_env_scrub[@]+"${_env_scrub[@]}"}" \
        DF_VERIFY_ROLE=merge REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 \
        timeout -k 5 "${REIFY_VERIFY_PIPELINE_GUARD_PRINT_PLAN_TIMEOUT:-45}" \
        bash "$_verify_sh" \
        all --scope all --profile both --include-infra --print-plan \
        >"$_plan_out" 2>/dev/null </dev/null || true

    local _gate
    while IFS= read -r _gate; do
        [ -z "$_gate" ] && continue
        _PLAN_DERIVED_SET="${_PLAN_DERIVED_SET}"$'\n'"${_gate}"
    #    Two things about the extraction below are load-bearing. (Tail-of-loop-
    #    body placement, same as clause 4a and for the same reason: bash admits
    #    no comment between `done` and its `< <(...)` redirect.)
    #
    #    (a) THE `grep -v '^#'` FILTER. --print-plan emits a header, a NOTE
    #    line, a scope-decision line, a narrowing line, an environment block and
    #    per-command annotations, all '#'-prefixed. No `.sh` path appears in any
    #    of them TODAY, so this filter is belt-and-braces rather than strictly
    #    load-bearing — but those lines are free prose, and a future annotation
    #    that named a script path would silently promote it to load-bearing.
    #    Keep the filter; it is not the no-op it looks like.
    #
    #    (b) `_extract_sh_paths` IS SHARED WITH CLAUSE 4a — one maintained copy
    #    of the boundary regex and its normalizer, so neither half can drift
    #    from the other (see the helper's own comment for the over-match
    #    rationale). THE ONE DELIBERATE DIFFERENCE between the halves lives at
    #    the CALL SITE, not in the helper: 4a prefilters with the
    #    '^[[:space:]]*add(_tool)?[[:space:]]+' STATEMENT ANCHOR and 4b must
    #    NOT. --print-plan emits RESOLVED COMMANDS, not add() statements, so
    #    that anchor would match nothing and silently zero this entire clause.
    #    Do not "restore" it for symmetry. The comment-exclusion job it does for
    #    4a is done here by (a)'s '^#' filter instead.
    done < <(grep -v '^#' "$_plan_out" | _extract_sh_paths || true)
    rm -f "$_plan_out"
    # Clause 4b's contribution alone, for the --list-plan-derived diagnostic.
    # `sed` rather than `grep -v '^$'` to drop the accumulator's leading blank:
    # sed always exits 0, whereas grep exits 1 on an all-empty set and
    # `set -o pipefail` would turn the documented fail-soft outcome (empty
    # derivation) into a hard abort.
    _SORTED_PLAN_DERIVED_SET="$(printf '%s\n' "$_PLAN_DERIVED_SET" | sed '/^$/d' | sort -u)"
    return 0
}

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

# Sort and deduplicate the STATIC set — clauses 1, 2, 3, 4a and 5 (a lib in both
# the manifest and sourced is fine). Clause 4b is deliberately NOT folded in
# here: it is derived lazily by derive_plan_paths and unioned in at the two
# dispatch sites that actually need it (--list, and requires-full-gate's
# last-resort pass), so no caller pays its fork unless the answer depends on it.
_SORTED_SET="$(printf '%s\n' "$_SET" | sort -u)"

# ---------------------------------------------------------------------------
# SURGICAL registry: active-row keys of verify-pipeline-infra-tests.txt
# ---------------------------------------------------------------------------
#
# Reify's SECOND registration cost point (task 4955). A row here does not route
# the diff to the full --scope all gate; it makes verify.sh SELECT the row's
# guarding infra test at task scope. That is a first-class registration, just a
# cheaper and more precise one — which is why `is-registered` counts it and
# `requires-full-gate` deliberately does not.
#
# READ LAZILY, by the is-registered arm ONLY. These keys are never appended to
# _SET: see that arm's note (1) for why folding them in would be a throughput
# regression and a silent rewrite of the cross-repo merge-worker contract.
#
# REIFY_VERIFY_PIPELINE_GUARD_INFRA_TESTS_MAP overrides the map path for
# testability (synthetic-row injection) and operator use — the exact sibling of
# the REIFY_VERIFY_PIPELINE_GUARD_DOC_SYNC_PATHS idiom at clause 5, and READ-ONLY
# (see the header entry).
_infra_tests_map="${REIFY_VERIFY_PIPELINE_GUARD_INFRA_TESTS_MAP:-$SCRIPT_DIR/verify-pipeline-infra-tests.txt}"

# registry_keys — print one active-row KEY per line (unsorted, possibly empty).
#
# THE PARSE BELOW IS A DELIBERATE MIRROR of verify.sh's select_infra_tests()
# (grep `^select_infra_tests() {` in scripts/verify.sh) and MUST NOT DRIFT FROM
# IT. Specifically: same active-row filter, same two-field `read`, and the same
# requirement that BOTH fields be non-empty. A one-field row selects no test, so
# treating it as a registration would let the anti-drift sweep pass on a path
# whose "registration" buys nothing — the query point must not disagree with the
# consumer about what an active row means. Only the FIRST field is a key; the
# second is a test-selection glob, not a registered artifact.
#
# A missing map degrades to "no keys" rather than erroring, the same
# graceful-degradation shape select_infra_tests() and clause 5 already use.
registry_keys() {
    local _line _artifact _glob
    [ -f "$_infra_tests_map" ] || return 0
    while IFS= read -r _line; do
        read -r _artifact _glob <<< "$_line"
        [ -n "$_artifact" ] || continue
        [ -n "$_glob" ]     || continue
        printf '%s\n' "$_artifact"
    done < <(grep -v '^\s*#' "$_infra_tests_map" | grep -v '^\s*$')
}

# ---------------------------------------------------------------------------
# Subcommand dispatch
# ---------------------------------------------------------------------------

_subcmd="${1:-}"

case "$_subcmd" in
    --list)
        # The full set = the static clauses plus clause 4b, which is derived
        # here on demand (see derive_plan_paths). `sed '/^$/d'` drops the blank
        # line the second `%s` contributes when the plan-derived half is empty
        # — the documented fail-soft outcome, which must print a clean list
        # rather than a stray blank.
        derive_plan_paths
        printf '%s\n%s\n' "$_SORTED_SET" "$_SORTED_PLAN_DERIVED_SET" \
            | sed '/^$/d' | sort -u
        exit 0
        ;;
    --list-plan-derived)
        # Diagnostic: clause 4b's contribution in ISOLATION — a strict subset of
        # --list. This is the one caller that always needs the fork, since the
        # set it prints IS clause 4b's output; derive_plan_paths memoizes, so
        # it still runs --print-plan exactly once.
        derive_plan_paths
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
        # LAST RESORT — clause 4b, the guard's only fork, consulted only now.
        # Every clause above has missed, so this is the sole remaining way the
        # verdict can still become exit 0; reaching this point is exactly the
        # condition under which the sub-second --print-plan is worth paying. A
        # would-be exit 1 is the ONLY answer 4b can change (monotonicity: it can
        # only ADD paths), so deferring it here cannot alter any verdict.
        # NOTE WHAT THAT DOES AND DOES NOT SAVE: this line is on the exit-1
        # path, which is the MAJORITY outcome for a guard whose job is deciding
        # fast-path eligibility — so most classifications reach here and do
        # fork. Laziness spares the already-decided "full gate required"
        # answers above and the `*)` usage-error branch, not the common case.
        # Measurements and the full rationale: clause 4b's block comment.
        # An empty set (the fail-soft route) falls straight through to exit 1,
        # which is what the pre-6426 guard would have said.
        derive_plan_paths
        if [ -n "$_SORTED_PLAN_DERIVED_SET" ]; then
            _plan_match=$(printf '%s\n' "$_normalized" \
                          | grep -xF -m1 -f <(printf '%s\n' "$_SORTED_PLAN_DERIVED_SET") 2>/dev/null \
                          || true)
            if [ -n "$_plan_match" ]; then
                echo "$_plan_match"
                exit 0
            fi
        fi
        exit 1
        ;;
    is-registered)
        # Read-only membership query over reify's registries (task 6857, filed
        # from esc-6758-2). See the header's is-registered entry for the full
        # contract; the two things worth repeating at the code are:
        #
        #   (1) EXIT 0 HERE IS NOT "FULL GATE REQUIRED". The infra-tests map
        #       keys are read LAZILY inside this arm and are NEVER appended to
        #       _SET, so requires-full-gate's derived set and --list stay
        #       byte-identical. That is deliberate and load-bearing: folding
        #       the surgical registry into _SET would route every edit of every
        #       surgically registered artifact — including a prose note whose
        #       only coupling is one link-rot grep — to the full global
        #       --scope all gate, spending exactly the throughput the doc-sync
        #       clause's precision exists to protect and silently rewriting the
        #       cross-repo merge-worker contract that consumes exit 0.
        #       Pinned by the NO-LEAK assertions in Pair C (e) of
        #       tests/infra/test_verify_pipeline_guard.sh.
        #
        #   (2) THIS QUERY IS FORK-FREE, and provably loses nothing by it: it
        #       deliberately never calls derive_plan_paths. Clause 4b derives
        #       only *.sh paths NAMED BY THE RESOLVED PLAN, so it can contribute
        #       neither a doc nor a registry key — the two path kinds this query
        #       is asked about. Skipping it therefore forfeits no coverage while
        #       keeping a per-path membership query out of the ~0.4-1.4s
        #       --print-plan fork that its only caller (the anti-drift sweep)
        #       would otherwise pay once per swept path.
        shift
        # ARITY: exactly one path. requires-full-gate uses ANY-semantics over
        # many paths because "does this DIFF need the gate" is genuinely a
        # disjunction; "is this path registered" reads as a conjunction, so a
        # multi-arg form would silently pick one of two plausible meanings.
        # Refusing >1 makes the surface state the question instead of guessing.
        # No stdin mode for the same reason — there is no set-shaped answer here.
        if [ "$#" -ne 1 ]; then
            printf '%s: is-registered takes EXACTLY ONE path (got %d); it is a\n' "$(basename "$0")" "$#" >&2
            printf '  single-path membership query, not a set predicate — there is no\n' >&2
            printf '  ANY/ALL reading to pick from, and no stdin mode. Query one path per call.\n' >&2
            exit 2
        fi
        # Same normalization idiom requires-full-gate uses: strip a leading
        # './' so './foo/bar' matches the clean repo-relative registered form.
        _query=$(printf '%s\n' "$1" | sed 's|^\./||')

        # MATCH IDIOM — capture-then-test with `|| true`, NOT `grep -q`, and the
        # `|| true` is NOT optional boilerplate. `set -o pipefail` is in force,
        # and every short-circuiting grep (-q / -m1) can close the pipe while
        # its producer is still writing; the producer then dies of SIGPIPE and
        # pipefail reports the pipeline as 141, turning a genuine MATCH into a
        # silent no-match. Measured here on clause (iii): `registry_keys | grep
        # -qxF <a-key-that-is-present>` returns 141, not 0. `|| true` absorbs
        # that, and the captured string — not the exit status — is the verdict.
        # This is the same idiom requires-full-gate's greps already use.
        #
        # (i) STATIC load-bearing set — clauses 1/2/3/4a/5 (anchor, manifest,
        #     sourced libs, emitted-gate source text, doc-sync docs). Reused
        #     wholesale rather than re-derived, so this query and
        #     requires-full-gate can never disagree about the static set.
        _hit=$(printf '%s\n' "$_SORTED_SET" | grep -xF -m1 -- "$_query" 2>/dev/null || true)
        if [ -n "$_hit" ]; then
            printf 'is-registered: %s — registered via the static load-bearing set (--list)\n' "$_query" >&2
            exit 0
        fi
        # (ii) The open-ended infra-test glob clause, same regex
        #      requires-full-gate carries (task 5256).
        _hit=$(printf '%s\n' "$_query" | grep -m1 -E '^tests/infra/[^/]*\.sh$' 2>/dev/null || true)
        if [ -n "$_hit" ]; then
            printf 'is-registered: %s — registered via the tests/infra/*.sh glob clause\n' "$_query" >&2
            exit 0
        fi
        # (iii) ACTIVE-ROW KEYS of the infra-tests map — the SURGICAL registry.
        _hit=$(registry_keys | grep -xF -m1 -- "$_query" 2>/dev/null || true)
        if [ -n "$_hit" ]; then
            printf 'is-registered: %s — registered SURGICALLY as a %s row (selects its guarding infra test at task scope; NOT a full-gate route)\n' \
                "$_query" "$(basename "$_infra_tests_map")" >&2
            exit 0
        fi
        printf 'is-registered: %s — NOT registered. Searched: the static load-bearing set\n' "$_query" >&2
        printf '  (scripts/doc-sync-paths.txt, scripts/verify-pipeline-paths.txt and the live\n' >&2
        printf '  verify.sh derivations — see --list), the tests/infra/*.sh glob, and the\n' >&2
        printf '  active rows of scripts/verify-pipeline-infra-tests.txt.\n' >&2
        exit 1
        ;;
    *)
        printf 'Usage: %s requires-full-gate [file...] | is-registered <path> | --list | --list-plan-derived\n' "$(basename "$0")" >&2
        printf '  requires-full-gate: exits 0 if any file is load-bearing (full gate required),\n' >&2
        printf '                      1 if none (fast-path safe); reads stdin when no args.\n' >&2
        printf '  is-registered: exits 0 if the ONE given path is registered as load-bearing by\n' >&2
        printf '                 any route (incl. a surgical verify-pipeline-infra-tests.txt row),\n' >&2
        printf '                 1 if by none. NOT a full-gate verdict — never route a diff on it.\n' >&2
        printf '  --list: print the canonical load-bearing path set (one path per line).\n' >&2
        printf '  --list-plan-derived: print only the --print-plan-derived subset of that set\n' >&2
        printf '                       (diagnostic; exit 0, possibly empty on the fail-soft route).\n' >&2
        exit 2
        ;;
esac
