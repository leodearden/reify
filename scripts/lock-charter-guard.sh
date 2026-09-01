#!/usr/bin/env bash
# lock-charter-guard.sh — syntactic directory-vs-file predicate for task lock charters.
#
# Classifies a declared path string as a directory declaration (REJECT) or a
# file-level/empty declaration (ACCEPT), per contracts C-P1..C-P4 in the
# task-lock-charter-lifecycle PRD (docs/prds/task-lock-charter-lifecycle.md §4.1).
#
# Subcommands:
#   classify <path>        — single-path predicate.
#                            exit 0 = ACCEPT (file-level declaration)
#                            exit 1 = REJECT (directory declaration)
#                            prints "ACCEPT <path>" or "REJECT <path>" to stdout.
#   check [path...]        — metadata.files-list gate.
#                            Reads paths from positional args; if none, reads
#                            newline-separated stdin.
#                            Empty list (the [] defer-to-architect value) → exit 0.
#                            All-file list → exit 0.
#                            Any directory path → exit 1 (prints each REJECT <path>).
#   --list-extensions      — prints the canonical extension allowlist sorted-unique
#                            in BYTE order, one extension per line
#                            (shared α/γ test vector, PRD §11 Q2).
#   --list-extensionless   — prints the canonical extensionless-basename allowlist
#                            sorted-unique in BYTE order, one name per line
#                            (shared α/γ test vector, PRD §11 Q2).
#
# Exit-code contract:
#   0 — ACCEPT (file-level declaration or empty list)
#   1 — REJECT (directory declaration found)
#   2 — usage error (unknown subcommand, missing required argument)
#
# Mechanism (C-P3 — pure string, no stat, no model):
#   Strip trailing slash(es) → take final path segment (after last /) → match
#   against the extension allowlist (post-dot token) or, for a dotless segment,
#   against the extensionless-basename allowlist (whole segment).
#   NO test -f / test -d / -e, NO network, NO LLM call anywhere.
#   Conservative-reject: an extension-less final segment is treated as a directory
#   (REJECT) UNLESS it exactly matches the enumerated _EXTLESS allowlist below.
#   The exception is bounded by enumeration plus a measured zero-directory-name-
#   collision result across both tracked corpora — never by a heuristic.  Anything
#   outside both allowlists is still declared via [] — the safe under-declaration
#   direction (PRD §5 item 2; §5 is a numbered list, so the header's older "§5.2"
#   citations mean item 2, not a subsection).
#
# Cross-repo seam: γ (fused-memory / dark-factory submit_task backstop)
#   re-implements this predicate against the PRD §4.1 spec using the shared
#   --list-extensions / --list-extensionless test vectors (PRD §11 Q2 — the
#   allowlist-completeness question; §11 Q1 is the separate transport question
#   this paragraph's re-implement-don't-shell-out choice answers) rather than
#   taking a runtime dependency on this script.
#
#   There are now TWO shared vectors across this seam, at different stages.
#
#   STATUS 2026-08-07 (#6067) — EXTENSION vector: CONVERGED.  Both sides now
#   carry 59 entries, set-identical (measured, both sides).  dark_factory:3117
#   converged the two at 58 first; dark-factory commit 43410b3418 then widened
#   γ to 59 by adding `csv`, and this task (#6067) followed with the matching
#   α widening.  THE SEQUENCING INVERTED THIS TIME, and that is the finding,
#   not an aside: γ landed `csv` FIRST, to heal a ~13.7h DF main-red /
#   merge-lane deadlock (DF corpus plans/evidence/scheduler-scoring-2026-08-06/
#   *.csv, landed e1c51efa8d, RED'd DF's
#   test_every_tracked_extension_is_allowlisted), and α followed after.  That
#   is the opposite of this block's own "α ships the primitive, γ mirrors it"
#   rule (below) and of the repo-wide CLAUDE.md convention "reify ships the
#   primitive, dark-factory wires the invocation" — the prior convergence
#   (dark_factory:3117) was α-first, so this is the first documented case of
#   the inversion.  The former "γ still pins 36 entries" note and both of its
#   consequences remain obsolete and removed rather than dated.  Its
#   predecessor reify #5737 was cancelled as a cross-repo misfile and drained
#   INTO 3117 — cite 3117, never #5737.  (Origin ticket:
#   tkt_0RRT3KW6B9KF72BHDY5Q038R7Y.)
#
#   STATUS 2026-08-27 (#6758), SUPERSEDED SAME DAY (#6856, below) — EXTENSION
#   vector AT TIME OF WRITING: α LEADS AGAIN at 60.  α added `tombstones`
#   (reify-evidenced: docs/reify-language-spec.tombstones, the spec-conformance
#   anchor tombstone sidecar, whose filename is PRD-normative —
#   docs/prds/v0_6/spec-conformance-suite.md D3).  It landed as the repair for a
#   RED Cycle 10 corpus alarm (esc-6758-3), i.e. exactly the #5726 shape this
#   alarm exists to catch.  γ carried 59 at that moment; the mirroring follow-up
#   was filed from that escalation.  CONSEQUENCE while that held: a `.tombstones`
#   path was declarable in a lock charter at α but still REJECTed at the γ
#   submit_task backstop — the same reify-leads shape as #5726 -> 3117, handled
#   the same way.
#
#   MEASURED 2026-08-27 (#6856) — EXTENSION vector: CONVERGED again at 60.  The
#   γ mirror landed the SAME day as the α widening above (dark-factory commit
#   9b824da7db, "Widen lock-charter extension allowlist 59 -> 60: add
#   'tombstones'", confirmed an ancestor of dark-factory main).  Measured by
#   importing both γ copies named elsewhere in this block —
#   shared/src/shared/locking.py and
#   fused-memory/src/fused_memory/middleware/lock_charter_guard.py — and
#   diffing their FILE_EXTENSIONS against this script's --list-extensions
#   output: 60 entries each, set-identical, `tombstones` present on both sides.
#   So the "still REJECTed at the γ submit_task backstop" consequence above is
#   DISCHARGED, and the α-lead window it describes is CLOSED — no DF main-red
#   window is open on this vector as of that measurement.  The window was
#   real while it lasted, which is the point of the discipline recorded below,
#   not a reason to relax it.
#
#   STATUS 2026-07-31 (#5890), SUPERSEDED 2026-08-27 (#6856, below) —
#   EXTENSIONLESS vector AT TIME OF WRITING: α LEADS, γ LAGS.  This
#   script accepts the 8 _EXTLESS basenames as of this task; γ does not.
#   dark_factory:3248 is the mirroring task and had NOT landed at time of
#   writing (DF main b525c6ee92 still has CODE_EXTENSIONS, no rename to
#   FILE_EXTENSIONS, and no EXTENSIONLESS_FILENAMES in either
#   shared/src/shared/locking.py or
#   fused-memory/src/fused_memory/middleware/lock_charter_guard.py).
#   CONSEQUENCE while that holds: declaring e.g. hooks/project-checks in a lock
#   charter passes α here but still FAILS at the γ submit_task / scheduler
#   backstop.  This task alone does not make those paths declarable end-to-end.
#   This is the same reify-leads shape as #5726 → 3117 and is handled the same
#   way: α ships the primitive, γ mirrors it, and this block is the seam record.
#
#   MEASURED 2026-08-27 (#6856): the α LEADS / γ LAGS status above is stale,
#   not current.  Both γ copies this note named as lacking the vector —
#   shared/src/shared/locking.py and
#   fused-memory/src/fused_memory/middleware/lock_charter_guard.py — now
#   define EXTENSIONLESS_FILENAMES, holding the identical 8 names as this
#   script's _EXTLESS.  The vector is CONVERGED, and the "passes α here but
#   still FAILS at the γ submit_task / scheduler backstop" consequence above
#   is discharged.  Sequencing finding for the seam record: dark_factory:3248
#   was planned believing this was a dark_factory-only lead; that premise was
#   refuted by measurement on 2026-08-09 — reify #5890 had already landed the
#   identical vector and added --list-extensionless specifically so the other
#   side could pin against it.  So, unlike the extension vector's documented
#   γ-first inversion (STATUS 2026-08-07 (#6067) above), this vector was
#   α-first after all, matching this block's own rule.
#
#   This block used to argue it was SAFE to lead because γ's Tier-2
#   cross-source drift test (fused-memory/tests/test_lock_charter_guard.py
#   ::test_extension_drift_guard_vs_reify_script) was live and comparing
#   against this script.  MEASURED 2026-08-07 (#6067), SUPERSEDED 2026-08-27
#   (#6856, below): that premise was FALSE at the time.  That test resolved
#   this script's path relative to its own file location; the resolution did
#   not land on a real file, so its skipif marker dropped the test AT
#   COLLECTION on every run — it had never compared anything.  Repair was
#   recorded as dark-factory's, not reify's.  The measured paths, the
#   confirming SKIPPED line, and the located-symptom hypothesis are recorded
#   in escalation esc-6067-2 (which also carries the dark-factory tracking
#   ticket) and are deliberately NOT restated here: foreign-repo line cites
#   and host paths rot on the next edit to that file, and nothing on either
#   side gates them — which is exactly the failure this seam exists to catch.
#   CONSEQUENCE AT THE TIME, plainly: while that guard skipped, this seam had
#   NO live automated cross-source check on EITHER vector — γ's Tier-1 drift
#   test compared γ only to γ, and α's Cycle 4 in
#   tests/infra/test_lock_charter_guard.sh compared α only to a pin inside α.
#   The csv drift that motivated #6067 (α at 58 while γ had already moved to
#   59) was found by human/reconciliation review, not by a gate.
#
#   MEASURED 2026-08-27 (#6856): the premise above is stale, not current.
#   dark-factory tasks 3843 and 4080 replaced the broken parents[5] path
#   arithmetic with a layout-independent shared.reify_checkout resolver
#   (REIFY_ROOT env override), which re-armed the Tier-2 test.  A single
#   -rs run against dark-factory main so a skip could not hide confirms both
#   that the guard now runs (baseline: all its `drift`/`tracked`-keyed cases
#   pass, none skipped) and that it fires (pointing REIFY_ROOT at a reify
#   worktree missing an extension γ carries reds the extension-vector
#   comparison).  So: the extension vector now HAS a live automated
#   cross-source check, running from γ's side.  A unilateral α widening is
#   therefore no longer silent — it reds dark-factory's suite as soon as the
#   new tracked file reaches reify main, the same fleet-time incident shape
#   recorded 2026-07-29 in dark_factory task 3226.  That makes the α-first +
#   prompt-γ-mirror discipline in this block MORE important, not less: a lag
#   between an α widening landing and its γ mirror landing is now a live DF
#   main-red window, not a quiet gap.  The extensionless vector (the STATUS
#   2026-07-31 note above) is NOT exempt from this: γ's test module
#   (fused-memory/tests/test_lock_charter_guard.py) defines the two Tier-2
#   cross-source guards symmetrically, test_extension_drift_guard_vs_reify_script
#   and test_extensionless_drift_guard_vs_reify_script — the latter invoking
#   --list-extensionless against this very script — both under the same
#   import-time _REIFY_SKIP_REASON skipif and both routed through
#   _reify_guard_vector, which HARD-FAILS rather than skips when this script's
#   emitter exits non-zero, so dropping or renaming --list-extensionless is
#   loud, not a silent retirement.  A `-rs` run scoped to the extensionless
#   case (`pytest -rs -q -k extensionless`) shows 19 passed, 0 skipped — the
#   guard RUNS — and pointing REIFY_ROOT at a temp reify root whose _EXTLESS
#   drops `Dockerfile` reds test_extensionless_drift_guard_vs_reify_script on
#   the α/γ/bash drift assertion — the guard FIRES.  So the α-first +
#   prompt-γ-mirror discipline two sentences up applies to BOTH vectors, not
#   just the one that motivated this measurement: a unilateral α widening of
#   EITHER allowlist — _EXTS or _EXTLESS — now opens a live DF main-red
#   window until its γ mirror lands.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---------------------------------------------------------------------------
# Canonical extension allowlist (OQ#2 resolved — PRD §11 Q2).
# Single source of truth for the α (reify) enforcement point: used by
# _is_file_path(), classify, and --list-extensions.  60 entries, and α/γ are
# converged there (measured 2026-08-27, #6856):
# dark_factory:3117 landed at 58, then dark_factory 43410b3418 + #6067
# widened both sides to 59, then #6758 added `tombstones` at α with the γ mirror
# following the same day — see the "Cross-repo seam: γ" status notes in
# the header above, which are also where the extensionless vector's own
# convergence is recorded.
# PRD-explicit: rs ri toml cpp c h hpp md json yaml yml lock py sh ts tsx js txt step stl
# Corpus-evidenced: css mjs html jsonc gcode service
# Common source siblings: cc cxx hh mts cts cjs jsx scss svg png
# git-ls-files sweep 2026-07-28 (#5726) — 22 tracked-file extensions across reify +
# dark-factory that this list misclassified as directories; supersedes #4676's
# "OQ#2 resolved" completeness claim FOR THIS LIST, which was a 22-extension
# undercount:
#   conf diff envrc example example-systemd-config gitattributes gitignore gitkeep
#   gitmodules golden grammar icns ico jq jsonl log manifest npmrc python-version
#   template timer typed
# Widened 58 -> 59 on 2026-08-07 (#6067): `csv` — dark-factory-evidenced only.  reify tracks
# zero .csv files (measured: git ls-files '*.csv' | wc -l -> 0); the evidence is DF's
# plans/evidence/scheduler-scoring-2026-08-06/*.csv (landed e1c51efa8d), which RED'd DF's
# corpus->allowlist guard for ~13.7h until dark_factory 43410b3418 widened all four γ copies.
# Kept for the same reason Dockerfile is kept in _EXTLESS: this is a SHARED α/γ vector, and an
# entry with no reify file behind it costs nothing while a missing one costs a rejected charter.
# NOTE: γ LED this one, inverting the normal α-first sequencing — see the seam note in the header.
# Widened 59 -> 60 on 2026-08-27 (#6758): `tombstones` — reify-evidenced, and the first entry
# added because Cycle 10's live-corpus alarm went RED rather than because a human swept:
# docs/reify-language-spec.tombstones (spec-conformance anchor tombstone sidecar; the filename
# is PRD-normative, docs/prds/v0_6/spec-conformance-suite.md D3, so renaming it to dodge the
# allowlist was not available).  γ mirrored it the same day (dark-factory 9b824da7db), so this
# vector is converged again at 60 — see the header status note.
# A LIVE ALARM NOW GUARDS THIS LIST: Cycle 10 of tests/infra/test_lock_charter_guard.sh
# sweeps the tracked corpus and goes RED if a tracked reify extension is missing here —
# the standing signal a pin inside this repo (Cycle 4's CANONICAL_EXTS) cannot give, since
# that pin moves whenever this list moves.  Widening this list is therefore a FOUR-part
# lockstep edit spanning α and γ, NOT an edit to this line alone.  The enumeration of
# those four sites is owned by Cycle 4's comment in tests/infra/test_lock_charter_guard.sh
# and is deliberately not restated here — follow it rather than guessing, because this is
# a SHARED α/γ vector and a unilateral α widening re-opens the seam divergence noted above.
# ---------------------------------------------------------------------------
_EXTS="c cc cjs conf cpp css csv cts cxx diff envrc example example-systemd-config gcode gitattributes gitignore gitkeep gitmodules golden grammar h hh hpp html icns ico jq js json jsonc jsonl jsx lock log manifest md mjs mts npmrc png py python-version ri rs scss service sh step stl svg template timer tombstones toml ts tsx txt typed yaml yml"

# ---------------------------------------------------------------------------
# Canonical extensionless-basename allowlist (C-P1 clause (ii) — PRD §11 Q2;
# mirror of dark_factory:3248).
# The second half of the file-vs-directory evidence: _EXTS answers "does the
# post-dot token name a file?", this answers "is the whole dotless segment a
# known filename?".  Used by _is_file_path() and --list-extensionless.
#
# MATCHED AGAINST THE FULL FINAL SEGMENT ($seg), NEVER THE POST-DOT EXTENSION
# ($ext).  This is the one dangerous detail.  Bash's "${seg##*.}" on a dotted
# segment yields the text after the dot, so .cargo -> cargo: an $ext-based match
# would flip .cargo, .pre-commit, .LICENSE and x.cargo to ACCEPT and blow the
# Cycle-6 anti-dotfile pin, admitting dotted DIRECTORIES as charters.  The match
# is also EXACT, never prefix/substring — pre-commit-hooks and cargo-lib are
# plausible directory names and must stay REJECT.  Both properties are pinned by
# Cycle 8's over-accept block in tests/infra/test_lock_charter_guard.sh.
#
# Sweep of both repos' tracked corpora 2026-07-31, mode-160000 gitlinks excluded.
# This is the canonical command — re-run it verbatim to grow the list:
#   git ls-files -s -z \
#     | awk 'BEGIN { RS = "\0"; FS = "\t" }
#            $1 !~ /^160000 / { n = split($2, s, "/"); print s[n] }' \
#     | grep -v '\.' | LC_ALL=C sort -u
# The -z + FS="\t" shape is load-bearing, not style: `git ls-files -s` emits
# "<mode> SP <sha> SP <stage> TAB <path>", so the whitespace-splitting
# `awk '{print $4}'` form this sweep was first written with truncates any path
# containing a space at that space ("docs/my file.md" -> "docs/my", a dotless
# segment that then looks like a missing allowlist entry).  Neither repo tracks
# such a path today (measured: `git ls-files | grep -c ' '` -> 0 in both), so
# that was latent — but Cycle 9 in tests/infra/test_lock_charter_guard.sh runs
# this sweep as a STANDING alarm over a growing corpus, and it must not be able
# to RED against a basename that does not exist.  All three copies of the
# command (here, Cycle 7's provenance comment, Cycle 9's executable form) are
# kept identical on purpose.
# reify-evidenced (7, all real tracked files):
#   LICENSE cargo cargo-audit-orphans pre-commit pre-merge-commit
#   project-checks reference-transaction
# dark-factory-evidenced only (1): Dockerfile — kept because this is a SHARED
#   α/γ vector, and omitting it would break the cross-source drift comparison
#   dark_factory:3248 enables.
# The gitlink exclusion is dark-factory-relevant only: reify tracks zero
# mode-160000 entries; dark-factory tracks graphiti and mem0, whose extensionless
# submodule mount points must never be admitted as files.
#
# Bounded by measurement, not by heuristic: the same sweep found ZERO
# directory-name collisions for any of the 8 names across ALL path components
# (not just leaves) of either corpus — 177 distinct reify directory names, 97
# dark-factory — so no real directory becomes declarable.
#
# UNIVERSE OF THAT MEASUREMENT: TRACKED path components only.  That is the right
# universe and not merely the convenient one — a lock charter declares paths in
# the tracked corpus, so a tracked directory is the only kind whose name being
# admitted here would let a real directory declaration slip through the gate.
# An untracked scratch directory that happens to be named `cargo` is not
# something a charter names, and C-P3 forbids this predicate from stating the
# filesystem to find out either way.  The distinction is worth stating because
# Cycle 6's anti-dotfile rationale in tests/infra/test_lock_charter_guard.sh
# argues from UNTRACKED directories (.worktrees, .task, .claude, .cargo,
# .taskmaster) — a different set — so the two rationales must not be read as
# resting on the same sweep.  Spot-checked anyway at amendment time: no
# untracked directory named any of the 8 exists in the main checkout
# (find -maxdepth 4 -type d, target/ and node_modules/ excluded) — a check, not
# a guarantee, since the untracked set is per-host and unbounded.  Growing this list
# requires re-running the sweep and updating γ's EXTENSIONLESS_FILENAMES in
# lockstep; Cycle 9's live-corpus alarm goes RED if a new tracked extensionless
# basename lands here without one.
# ---------------------------------------------------------------------------
_EXTLESS="Dockerfile LICENSE cargo cargo-audit-orphans pre-commit pre-merge-commit project-checks reference-transaction"

# ---------------------------------------------------------------------------
# _is_file_path <path>
# Pure-string predicate.  Returns 0 (true = file) or 1 (false = directory).
# No filesystem stat, no model call — C-P3 invariant.
# ---------------------------------------------------------------------------
_is_file_path() {
    local p="$1"
    # Strip all trailing slashes.
    while [ "${p%/}" != "$p" ]; do
        p="${p%/}"
    done
    # Extract the final path segment (everything after the last /).
    local seg="${p##*/}"
    # An empty segment (path was all slashes) → treat as directory.
    [ -z "$seg" ] && return 1
    # Enumerated extensionless filenames → file.  Placed BEFORE the dotless
    # reject below, mirroring γ's documented step order (dark_factory:3248
    # inserts `if seg in EXTENSIONLESS_FILENAMES: return True` immediately ahead
    # of its own no-dot reject).  Compares $seg, never $ext — see the _EXTLESS
    # header.  Distinct loop variable from the _EXTS loop's $e below.
    local x
    for x in $_EXTLESS; do
        [ "$seg" = "$x" ] && return 0
    done
    # Extract extension: everything after the last dot in $seg.
    local ext="${seg##*.}"
    # If there's no dot in the segment (seg == ext), extension-less → REJECT.
    if [ "$ext" = "$seg" ]; then
        return 1
    fi
    # Check ext against _EXTS (space-separated word list).
    local e
    for e in $_EXTS; do
        [ "$ext" = "$e" ] && return 0
    done
    return 1
}

# ---------------------------------------------------------------------------
# Subcommand dispatch
# ---------------------------------------------------------------------------

_subcmd="${1:-}"

case "$_subcmd" in
    classify)
        if [ "${2+set}" != "set" ] || [ -z "${2:-}" ]; then
            printf 'Usage: %s classify <path>\n' "$(basename "$0")" >&2
            exit 2
        fi
        _path="$2"
        if _is_file_path "$_path"; then
            echo "ACCEPT $_path"
            exit 0
        else
            echo "REJECT $_path"
            exit 1
        fi
        ;;

    check)
        # step-6: Cycle 3 GREEN — check list-gate (scaffolded in step-2; Green after step-4 full allowlist).
        shift
        # Collect paths from positional args or stdin.
        if [ "$#" -gt 0 ]; then
            _paths_raw=$(printf '%s\n' "$@")
        else
            _paths_raw=$(cat)
        fi
        # Empty list → ACCEPT ([] defer-to-architect value).
        if [ -z "$_paths_raw" ]; then
            exit 0
        fi
        _rejected=0
        while IFS= read -r _p; do
            # Strip trailing carriage return (CRLF-encoded stdin).
            _p="${_p%$'\r'}"
            # Skip empty/whitespace-only tokens.
            [ -z "${_p// /}" ] && continue
            if ! _is_file_path "$_p"; then
                echo "REJECT $_p"
                _rejected=$((_rejected + 1))
            fi
        done <<< "$_paths_raw"
        if [ "$_rejected" -gt 0 ]; then
            exit 1
        fi
        exit 0
        ;;

    --list-extensions)
        # step-8: Cycle 4 GREEN — print canonical extension allowlist sorted-unique.
        # Shared α/γ test vector (PRD §11 Q2); drift-guarded by test_lock_charter_guard.sh.
        # LC_ALL=C for the same reason as --list-extensionless below: γ's Tier-2
        # comparison is against Python sorted() (code-point order), so byte order
        # is the only ordering the two sides can agree on and the only one that is
        # host-independent as C-P3 requires.  Today's 60 entries (re-measured
        # 2026-08-27 after #6758 added `tombstones`) still sort identically under C and
        # en_US.UTF-8 (md5-identical), so this remains a no-op on the current
        # vector — but the list already carries
        # hyphenated members (example-systemd-config, python-version) and glibc
        # collation ignores punctuation at the primary level, so one future
        # hyphen/underscore entry could silently reorder this output on a
        # non-C-locale host and RED the γ side.  Pinned before that can happen.
        printf '%s\n' $_EXTS | LC_ALL=C sort -u
        exit 0
        ;;

    --list-extensionless)
        # Shared α/γ extensionless-basename vector — the machine-readable
        # counterpart to --list-extensions, and what makes the cross-source
        # Tier-2 drift comparison possible from γ's side (dark_factory:3248).
        # LC_ALL=C is load-bearing, not decoration: this list has uppercase
        # members, so an ambient en_US locale sorts it cargo/…/Dockerfile/LICENSE
        # while γ's Python sorted() is code-point order.  Byte order is the only
        # ordering the two sides can agree on, and the only one that is
        # host-independent as C-P3 requires.
        printf '%s\n' $_EXTLESS | LC_ALL=C sort -u
        exit 0
        ;;

    *)
        printf 'Usage: %s classify <path> | check [path...] | --list-extensions | --list-extensionless\n' "$(basename "$0")" >&2
        printf '  classify <path>       — exit 0=ACCEPT (file), exit 1=REJECT (directory)\n' >&2
        printf '  check [path...]       — exit 0=all-file/empty, exit 1=any-directory; reads stdin if no args\n' >&2
        printf '  --list-extensions     — print canonical extension allowlist (sorted-unique)\n' >&2
        printf '  --list-extensionless  — print canonical extensionless-basename allowlist (sorted-unique)\n' >&2
        exit 2
        ;;
esac
