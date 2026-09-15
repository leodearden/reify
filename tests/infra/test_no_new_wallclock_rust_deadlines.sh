#!/usr/bin/env bash
# tests/infra/test_no_new_wallclock_rust_deadlines.sh
#
# Regression guard (tasks #6438, #6597):
#   Flags NEW hand-rolled real-clock deadlines and elapsed-time UPPER bounds
#   across EVERY Rust TEST root, so the flake class de-flaked by tasks #5143,
#   #5422, #5709 and #6438 cannot silently return a FIFTH time -- anywhere in
#   the Rust tests, not merely where the first four happened.
#   #6438 shipped this scoped to the one directory gui/src-tauri/src/tests.
#   #6597 widened it to every crates/*/tests, all three gui/src-tauri test
#   roots and tree-sitter-reify/tests, scanned recursively, with the sites
#   that already existed held in a two-directional baseline ratchet rather
#   than blessed.
#   See SCOPE under KNOWN LIMITS for what that does and does not cover.
#
# The guard itself is a LOAD-INDEPENDENT static grep -- it is NOT a wall-clock
# test, and it runs no cargo, no npm and no watcher. It stays instant at the
# widened scope: ~0.13s over 1331 files, re-measured on the 36-root set.
#
# ---------------------------------------------------------------------------
# WHY THIS IS A SIBLING OF test_no_new_wallclock_upper_bounds.sh AND NOT AN
# EXTENSION OF IT.
#
# The obvious question a reviewer should ask is why the existing wall-clock
# guard was not simply pointed at the Rust tests. It cannot be. That guard
# STRUCTURALLY cannot reach gui/src-tauri/src/tests/*.rs, for three
# independent reasons, any ONE of which is disqualifying:
#
#   (1) SCOPE. Its detector iterates `"$dir"/*.sh`, and its Section-3 live
#       scan is hard-scoped to SCRIPT_DIR, i.e. tests/infra. It never sees a
#       .rs file, and never looks outside this directory.
#
#   (2) GRAMMAR. Its match conditions are pure SHELL: the `assert` shell-helper
#       keyword, and the `test`/`[` integer operators -le / -lt against an
#       integer literal. Rust has neither. A Rust upper bound on elapsed time
#       is `<` against a `Duration` value -- a construct that guard's grammar
#       cannot express at all, not merely one it happens not to look for.
#
#   (3) LINE JOINER. Its logical-line reconstruction is a bash quote-state
#       machine (single/double-quote tracking, #-comment handling, backslash
#       continuation). Rust's lexical grammar -- `//`, `/* */`, raw strings,
#       paren-balanced macro invocations -- is a different language. Reusing
#       that awk against Rust would be WRONG, not merely imprecise.
#
# Hence: a sibling with its own two rules, not a widened scope on the old one.
# The two guards share a spelling for their escape comment, and nothing else.
# ---------------------------------------------------------------------------
#
# THE TWO RULES, one line each. The canonical statement is the comment block
# on `_wallclock_fingerprints` below -- the ENGINE, which is where the regexes
# actually live and the only place they appear: it carries every spelling
# matched and every spelling deliberately not matched, and the reason for each.
# It sits with the code it describes, so it is the copy to read and the copy to
# keep true -- this summary is deliberately not a second one. (#6597 moved it
# there from `_detect_rust_wallclock_deadline`, which is now a reporting
# wrapper and states no rules of its own.)
#   Rule A -- the raw clock used as a deadline: `Instant::now()` offset by hand
#             (`+` or `.checked_add`) rather than taken through the WaitClock
#             seam watcher_tests.rs provides, or compared against a deadline,
#             in either operand order.
#   Rule B -- an UPPER bound on elapsed time: against a `Duration`, or against
#             a plain number after a scalar accessor (`.as_millis()` and
#             friends), in either operand order. Upper bounds invert under
#             descheduling; LOWER bounds are monotone-safe and are NOT matched.
# Both rules are single-physical-line by construction.
#
# Escape: a same-line comment carrying the token `wallclock` immediately
# followed by `:allow`. It is written apart HERE on purpose -- see SELF-MATCH
# SAFETY below -- and appears contiguously only on the line it actually
# annotates, so `grep -rn` for it counts real escapes and nothing else. (The
# shipped header spelled it out twice in this paragraph, which quietly
# falsified the no-contiguous-copy claim four lines down and put two
# non-escapes into every such audit; fixed in the #6438 review pass.) The form
# mirrors the sibling guard's `#`-comment escape and the PTODO detector's
# `// ptodo:allow`.
#
# ALLOWLIST: exactly ONE escape exists in tree -- `far_future_stamp()` in
# watcher_tests.rs, argued at the site in its own doc comment. That count is
# CHECKED, not merely asserted here: Section 3 counts escape-annotated lines
# across ALL of `_LIVE_ROOTS`, recursively, and compares them against
# `_ESC_ALLOWLIST_SIZE`, because the detector skips an escaped line without
# counting it and so returns 0 for one escape and for twenty alike. Since #6597
# that claim covers the whole Rust test tree rather than one directory, and it
# survived the widening unchanged: scanning ~44x more files admitted no second
# escape, and the 19 pre-existing sites went to the baseline, NOT to escapes.
# (An earlier draft of
# this guard spelled that site with `checked_add` specifically BECAUSE Rule A
# did not match it. That was a documented bypass masquerading as house style:
# it made the one site invisible AND blessed an undetectable spelling for
# every future one. Rule A now matches both spellings and the site takes the
# escape instead.) A second escape should be argued for on its own merits, in
# a review, not added quietly.
#
# KNOWN LIMITS, stated rather than hidden -- this is a lexical guard, not a
# type-aware one, and it covers every Rust TEST root but no production code.
#
# SCOPE, stated first because it bounds every other claim here. `_LIVE_ROOTS`
# is the glob expansion of crates/*/tests (32 directories today) plus
# gui/src-tauri/src/tests, gui/src-tauri/src/debug_server/tests,
# gui/src-tauri/tests and tree-sitter-reify/tests -- 36 roots, 1331 .rs files,
# scanned RECURSIVELY. #6438 shipped this guard scoped to the single
# non-recursive directory gui/src-tauri/src/tests, where all four flakes
# happened, and said plainly that the rest of the Rust tree was unguarded;
# #6597 closed that. The glob is deliberate: a new crate's tests are ratcheted
# the day they land, so the guard does not need editing to stay honest, and an
# unmatched glob is rejected loudly rather than skipped.
#
# "EVERY TEST ROOT" IS CHECKED, NOT PROMISED. The four enumerated roots are the
# part no glob covers, and an enumeration is exactly where a root goes missing:
# the #6597 review found gui/src-tauri/src/debug_server/tests unscanned -- 31
# test fns, a CHILD of debug_server::tests, so recursing gui/src-tauri/src/tests
# never reaches it -- in the very crate that produced all four flakes. So the
# claim is derived rather than trusted. Section 3 builds ground truth from the
# tree (every directory named `tests` that holds a tracked .rs file) and reds on
# any of them this list does not cover, naming it. A test directory split out
# tomorrow therefore reds the gate instead of quietly becoming a second blind
# spot.
#
# THE BASELINE, AND THE DISTINCTION A READER MUST NOT BLUR. Widening the scan
# could not be a one-line change: 19 violating lines across 6 files already
# existed and would have redded the gate on day one. They are listed in
# tests/infra/wallclock-rust-deadline-baseline.txt. A baseline row and an
# escape comment are DIFFERENT CLAIMS, and conflating them is the one way this
# design fails quietly:
#   * an ESCAPE says "this site is LEGITIMATE, and here is the argument, at the
#     site". There is exactly one, far_future_stamp() in watcher_tests.rs, and
#     adding a second also takes a diff to _ESC_ALLOWLIST_SIZE.
#   * a ROW says "PRE-EXISTING DEBT that MUST NOT GROW". Nothing in that file is
#     blessed. Annotating those 19 sites with escapes instead was considered and
#     rejected: it would have edited 6 files across 5 crates plus
#     tree-sitter-reify, and it would have blessed 19 flakes-in-waiting.
# The ratchet runs in TWO DIRECTIONS, which is what makes it shrink-only. A live
# record absent from the baseline is `+` and reds; a baseline row matching
# nothing live is `-` and ALSO reds, so a fixed site must be drained in the same
# diff. That follows tests/infra/harness-layout-baseline.manifest, which reds on
# orphan rows -- deliberately not ptodo-baseline.txt's subset-only rule, which
# has no forcing function to drain it (a limitation #6859 accepts openly, and
# one this guard need not inherit at 19 hand-auditable rows).
#
# PRODUCTION CODE IS EXCLUDED, and this is an argument rather than an
# oversight. A "root" here is a directory whose CONTENTS are tests -- which is
# why gui/src-tauri/src/tests is IN (a test root that happens to live under
# src/) while crates/*/src is OUT. Re-running the two rules over the whole tree
# finds 5 more matching lines that this guard deliberately drops:
#   * crates/reify-fdm/src/slice.rs:333,340 -- `fn wait_within` polls a real
#     child process for exit within a grace window, then escalates to SIGKILL.
#     (Verified: that file contains no #[cfg(test)] module at all.)
#   * tree-sitter-reify/build.rs:43,58 -- the identical shape in a build script.
# Both rules exist for TEST flake: an upper bound on elapsed time INVERTS under
# load. A subprocess grace period asserts nothing, cannot invert into a failure,
# and has no WaitClock seam to be routed through -- it IS a real-time wait, by
# definition. Scanning it would force escape comments onto correctness code and
# would blur what Rule A means.
#
# THE REMAINING BLIND SPOT, named exactly as #6438 named its own -- and there
# is exactly ONE, which is now a checked claim rather than a hopeful one. An
# inline `#[cfg(test)]` module inside a production src/ file is NOT scanned,
# even though its contents genuinely are tests. Finding one lexically needs
# brace nesting -- i.e. the Rust grammar this guard deliberately refuses to grow
# (see NO LINE JOINER at the engine). ONE instance is known and measured:
# crates/reify-eval/src/compute_targets/fdm_slice.rs:821
# (`elapsed < Duration::from_secs(10)`), inside the #[cfg(test)] mod that starts
# at line 480. It is uncovered. A reader should not assume otherwise, and the
# honest fix is to move such tests to a tests/ root rather than to teach this
# grep to parse Rust.
#
# WHAT MAKES "ONE" TRUE is the other half of the sentence -- that every tests/
# root IS scanned -- and that half is ENFORCED, not asserted: the completeness
# check in Section 3 diffs the root list against the tree. It has to be. An
# earlier draft of this widening left gui/src-tauri/src/debug_server/tests out
# of the list, which made this paragraph false as written: there were TWO
# uncovered locations, not one, and nothing here could see the second. With that
# root added the tree's 36-root ground truth is covered exactly, re-measured,
# and the inline #[cfg(test)] mod above is once again the sole uncovered
# category -- and a future omission reds rather than joining it.
#
# FALSE NEGATIVES, i.e. shapes that get past it by construction:
#   * A named constant: `assert!(elapsed < TIMEOUT_BUDGET)` carries neither a
#     `Duration::` token nor a scalar accessor on the line, so Rule B cannot
#     see it. Chasing it would need type resolution (or a const-name index),
#     which is out of proportion to a grep.
#   * The raw clock bound to a variable BEFORE the comparison:
#     `let now = Instant::now();` ... `if now >= deadline`. Rule A's
#     comparison half sees `Instant::now()` next to an operator, not a
#     variable that once held it -- one hop of dataflow, the same blind spot
#     as the named constant above. Note the hop has to be taken deliberately:
#     the natural spellings of both halves ARE matched, so this is a shape you
#     write around the guard, not one you fall into.
#   * A construct split across physical lines, since neither rule joins lines
#     (see the detector for why that is deliberate). rustfmt keeps every shape
#     above on one line at any realistic width, so this bites a hand-wrapped
#     site.
#
# FALSE POSITIVE -- exactly one, and it is deliberate:
#   * PROSE IS NOT EXEMPT. Both rules scan every physical line, comments
#     included, so a doc comment that QUOTES a forbidden shape (e.g.
#     `/// e.g. assert!(elapsed < Duration::from_secs(2))`) is reported as a
#     violation. Fixture 2z pins that, rather than leaving it as folklore for
#     the next author to discover from a red run. Exempting `//` lines was
#     considered and rejected: it is the first step toward the Rust-grammar
#     joiner this guard deliberately does not have (`/* */`, raw strings, a
#     `//` inside a string literal), and the remedy is already cheap --
#     annotate that one line with the escape, or describe the shape without
#     writing it out, as this header's own rule descriptions do.
#
# Neither gap is silent: the REAL-CLOCK LEDGER in watcher_tests.rs is the prose
# half that covers what a grep cannot, and review is the backstop for both.
#
# SELF-MATCH SAFETY (two directions, do not conflate them):
#   * This guard never scans .sh files, so the Rust fixture strings below can
#     be written as plain literals -- readable, and impossible for this guard
#     to see. Do NOT "helpfully" apply the sibling guard's variable-assembly
#     convention to them; that convention exists because that guard scans its
#     own directory, and this one does not.
#   * The SIBLING guard DOES scan this file (its live scan covers all of
#     tests/infra except its own basename). So no line here may carry
#     `assert` + a `-le`/`-lt <int>` upper bound + a time lexeme. No assertion
#     below uses either operator: rc checks use `-eq`, and the few that are not
#     equalities use `-ne` (a root list is non-empty, a bad root is non-zero) or
#     `-ge` (the file-count floor). `-ge` in particular is not a slip -- a floor
#     is a LOWER bound, and a lower bound is both the only direction that means
#     anything there and the direction that guard is deliberately blind to.
#     Verified: the sibling guard is green on this file.
#   * THE BASELINE FILE, tests/infra/wallclock-rust-deadline-baseline.txt, holds
#     19 verbatim copies of forbidden shapes -- and is invisible to BOTH guards
#     by file type: this one scans *.rs (fixture 4a-4 pins that), the sibling
#     scans *.sh. So it needs no escapes, no assembly convention, and it cannot
#     fingerprint itself into permanent self-reference. Keep it a .txt.
#
# The escape token itself IS assembled from two adjacent single-quoted parts,
# so this file contains no contiguous copy of it -- writing one would silently
# annotate this line for the sibling guard and would pollute any in-tree
# "count the escapes" audit.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; declared
# `pool` in run-all-classification.manifest (hermetic, load-independent).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== Rust real-clock deadline / upper-bound regression guard ==="

# EVERY PATH BELOW IS RESOLVED FROM THE REPO ROOT, pinned once here.
#
# The scan roots in Section 3 are written repo-relative, and that is not
# cosmetic: grep echoes each match's path under the root exactly as it was
# named, so a repo-relative root is what makes a fingerprint repo-relative --
# which is what the committed baseline holds, and what keeps it portable
# across machines and worktrees. Absolute roots would bake this checkout's
# path into every row.
#
# Sections 1, 2 and 4 are unaffected: their roots are absolute mktemp dirs.
cd "$REPO_ROOT" || {
    echo "ERROR: cannot cd to REPO_ROOT ($REPO_ROOT)" >&2
    exit 1
}

# Escape token, assembled from parts (see SELF-MATCH SAFETY above).
_ESC_TOKEN='wallcl''ock:allow'

# Collect all mktemp -d directories for cleanup at EXIT. Individual
# `trap ... EXIT` calls replace each other; a single handler over an array
# ensures every tmpdir is removed regardless of which section runs last.
# (Same idiom as the sibling guard.)
_TMPDIRS=()
trap '[ "${#_TMPDIRS[@]}" -gt 0 ] && rm -rf "${_TMPDIRS[@]}"' EXIT

# _fixture <dir> <basename> <line>...
# Write a fixture source file into a mktemp dir -- NEVER into the real tree.
_fixture() {
    local dir="$1" base="$2"
    shift 2
    : > "$dir/$base"
    local line
    for line in "$@"; do
        printf '%s\n' "$line" >> "$dir/$base"
    done
}

# ---------------------------------------------------------------------------
# _wallclock_assert_roots <root>...
#
# THE ROOT VALIDATOR, shared by every scanning function below so a bad root
# cannot enter through one door after being refused at another. Returns 0 if
# every <root> is an existing directory; otherwise names the offender on
# stderr and returns 2.
#
# 2 SPECIFICALLY, because that is grep's "an input was unusable" code, and
# every caller here already distinguishes rc 0 (clean) / rc 1 (a real finding)
# / rc >= 2 (the scan itself is broken). Collapsing a bad root into rc 0 is the
# vacuity hole Section 4e exists to close; collapsing it into rc 1 would be
# only marginally better, since it would be read as "there are violations".
#
# THE FAILURE THIS IS REALLY FOR is an UNMATCHED GLOB. `$REPO_ROOT/crates/*/tests`
# expanding to nothing leaves bash's default behaviour: the LITERAL pattern
# string, `.../crates/*/tests`, as a single array element. grep would report
# that as one missing path and, with the baseline drained to zero, everything
# downstream would be consistent and empty. So the check is on the root list,
# before any scanning, and it is loud.
#
# IT ALSO REJECTS AN OVERLAPPING PAIR -- one root equal to, or nested inside,
# another. That is a DIFFERENT failure with the same remedy. Every root is
# scanned RECURSIVELY, so grep visits a nested root's files once per root and
# the engine emits each violating line twice; because the baseline comparison
# is a deliberate MULTISET, those second copies arrive as `+` records that
# read exactly like new violations. Nothing downstream can tell them apart,
# and the completeness check cannot warn about them either -- it treats a
# descendant as COVERED by design (4f-2), which is correct for its own
# question and blind to this one. So the list is checked for overlap here,
# once, before any scanning (fixtures 4e-10, 4e-11).
# ---------------------------------------------------------------------------
_wallclock_assert_roots() {
    local _r
    for _r in "$@"; do
        [ -d "$_r" ] && continue
        echo "ERROR: wallclock scan root is not a directory: $_r" >&2
        if [ -e "$_r" ]; then
            echo "  It exists but is not a directory. A root names a TREE to scan; scanning" >&2
            echo "  a single file where a tree was intended is the same vacuity hole." >&2
        else
            echo "  It does not exist. If it looks like a glob pattern rather than a path," >&2
            echo "  that glob matched nothing and bash left the pattern string behind." >&2
        fi
        return 2
    done

    # OVERLAP, by POSITION rather than by value: the duplicate case is `$i` and
    # `$j` holding the SAME string, so a `[ "$_a" = "$_b" ] && continue` guard
    # over a value pair would skip the very case it most needs to catch.
    #
    # The pattern is quoted except for the trailing `/*`, exactly as in
    # _wallclock_uncovered_test_roots: a root is a PATH, not a pattern, and the
    # separator is what makes one root a parent of another. An unquoted
    # `${_roots[_i]}*` would call `src2` a child of `src` and reject a list that
    # is fine (fixture 4e-11).
    #
    # Lexical, deliberately: it compares the strings as written, so `a/tests`
    # and `./a/tests` read as distinct. The lists this guards are written by
    # hand in one style, and resolving paths here would trade a cheap exact
    # check for a symlink-following one with its own surprises.
    local -a _roots=("$@")
    local _i _j _n="${#_roots[@]}"
    for (( _i = 0; _i < _n; _i++ )); do
        for (( _j = 0; _j < _n; _j++ )); do
            [ "$_i" -eq "$_j" ] && continue
            case "${_roots[_j]}" in
                "${_roots[_i]}")
                    echo "ERROR: wallclock scan root listed twice: ${_roots[_i]}" >&2
                    ;;
                "${_roots[_i]}"/*)
                    echo "ERROR: wallclock scan root ${_roots[_j]} is nested inside ${_roots[_i]}" >&2
                    ;;
                *) continue ;;
            esac
            echo "  Roots are scanned RECURSIVELY, so the inner one is already covered and" >&2
            echo "  every violating line beneath it would be fingerprinted TWICE. The ratchet" >&2
            echo "  compares MULTISETS, so those second copies would be reported as new" >&2
            echo "  violations that no edit can fix. Drop the redundant entry." >&2
            return 2
        done
    done
    return 0
}

# ---------------------------------------------------------------------------
# _wallclock_files_scanned <root>...
#
# THE NON-VACUITY FLOOR. Prints (stdout) the number of *.rs files under
# <root>..., recursively. Returns 0, or the validator's 2 for a bad root.
#
# WHAT IT IS FOR: the ratchet is a subset oracle, and a subset oracle is
# trivially satisfied by the empty set. Asserting the count is what makes
# "no new violations" mean "we looked, and there were none" rather than
# "we looked at nothing". The stale direction covers this only while the
# baseline is non-empty -- i.e. only until this ratchet succeeds.
#
# `find` rather than `grep -rl -e ''`: an EMPTY .rs file matches no line, so
# grep would not list it, and the count would silently disagree with the
# number of files a reader would count by hand. The floor's whole job is to be
# a number you can trust without re-deriving it.
# ---------------------------------------------------------------------------
_wallclock_files_scanned() {
    _wallclock_assert_roots "$@" || return $?

    local _n _rc=0
    _n="$(find "$@" -type f -name '*.rs' -print | wc -l)" || _rc=$?
    [ "$_rc" -eq 0 ] || return "$_rc"

    echo "$_n"
    return 0
}

# ---------------------------------------------------------------------------
# _wallclock_fingerprints <root>...
#
# THE ENGINE. Every rule-matching path in this file goes through here: the
# detector below is a reporting wrapper over it, and the baseline ratchet
# compares its output against a committed file. Rule A and Rule B therefore
# exist in exactly ONE place (SPOT) -- a second copy for the baseline path
# would be a straight duplication of the one thing this guard actually knows.
#
# Prints, to stdout, one FINGERPRINT per violating line across all <root>s,
# recursively, `*.rs` only, escape-annotated lines excluded:
#
#     <path> :: <whitespace-trimmed-line-text>
#
# LINE NUMBERS ARE ERASED on purpose. A fingerprint names a SITE, and an edit
# above a site does not move the site -- baselining `file:216:` instead would
# red the gate on every unrelated insertion in the same file. Same rationale
# as ptodo.rs::fingerprint.
#
# PATHS ARE ECHOED AS THE ROOT WAS NAMED -- grep resolves each <root> against
# the CURRENT DIRECTORY and prints paths under it verbatim. So a repo-relative
# root yields repo-relative records (what the baseline file holds) and an
# absolute mktemp root yields absolute ones (what the fixtures assert). The
# function itself knows nothing about the repo; fixing the cwd is the caller's
# job, and Section 3 does it once for the whole script.
#
# OUTPUT IS SORTED, GLOBALLY and under LC_ALL=C. Both halves matter. The
# ratchet compares two streams with `comm`, which is only correct if they were
# sorted the SAME way, so the collation is pinned rather than inherited: the
# 19 live records sort DIFFERENTLY under en_US.UTF-8 than under C (measured --
# the `Ok(None) if ...` row moves), and a baseline generated on one machine
# would otherwise report phantom +/- records on another.
#
# OUTPUT IS A MULTISET, NOT A SET -- see Section 4b and the `sort` below.
#
# EXIT CODES, following grep's: 0 with records, 0 with none (grep's rc 1 means
# "clean", not "broken"), and grep's rc >= 2 PROPAGATED, because an unreadable
# or non-existent root must never reach the comparison looking clean.
#
# THE TWO RULES, canonically -- this is the ONE copy. Both are single-physical-line
# by construction, and both scan every line including comments.
#
#   Rule A  (Instant::now\(\)[[:space:]]*(\+|\.checked_add|[<>]=?))
#           |([<>]=?[[:space:]]*Instant::now\(\))
#           A real-clock deadline built by hand: reading the raw clock and
#           offsetting from it, instead of going through the WaitClock seam --
#           or, in the trailing alternatives, CHECKING such a deadline by
#           comparing the raw clock against it. Those comparison alternatives
#           were added in the #6438 review pass, and they close the one gap
#           that mattered: the offset half is defeated by a line break alone.
#           `let start = Instant::now();` on one line and
#           `let deadline = start + Duration::from_secs(5);` on the next puts
#           `Instant::now()` adjacent to nothing, and no Rule B token appears
#           anywhere either -- so the entire construct #6438 deleted could
#           have been rewritten straight past the guard without one
#           deliberate evasion. A deadline is inert until it is compared
#           against the clock, so the comparison line is where an otherwise
#           invisible one resurfaces. BOTH operand orders are matched
#           (`Instant::now() < deadline` and `deadline > Instant::now()`), and
#           BOTH directions: unlike Rule B, direction carries no safety
#           meaning here -- comparing the raw clock against a deadline is the
#           hand-rolled poll loop whichever way round it is written.
#           BOTH offset spellings are matched too. `checked_add` was originally left out
#           on the theory that it signals deliberate intent -- but intent is
#           not the property being guarded, and leaving it out meant
#           `Instant::now().checked_add(Duration::from_secs(5)).unwrap()` was
#           a completely invisible way to write the very deadline this rule
#           exists to catch. It is now matched, and the one legitimate site in
#           tree (far_future_stamp in watcher_tests.rs) carries an escape that
#           argues its case at the site -- which is the reviewable outcome, and
#           strictly better than a spelling nobody can see.
#           The rule deliberately spares:
#             * `let t0 = Instant::now();`          -- a synthetic-clock seed
#             * `VirtualClock::new(Instant::now())` -- likewise
#             * `clock.now() + timeout`             -- the blessed seam itself
#             * `t0 + Duration::from_millis(150)`   -- synthetic arithmetic
#
#   Rule B  (<=?[[:space:]]*Duration::)|(Duration::[a-z_]*\([^)]*\)[[:space:]]*>)
#           |(\.as_<scalar>\(\)[[:space:]]*<=?)
#           |(>=?[[:space:]]*<expr>\.as_<scalar>\(\))
#           An UPPER bound against a Duration -- what a Rust upper bound on
#           elapsed time looks like, whether the left operand is `x.elapsed()`
#           or a bound variable holding it. That is exactly why the rule does
#           not key on `.elapsed()`: the bound deleted from what is now
#           watcher_drop_wakes_and_joins_a_worker_parked_indefinitely
#           compared a bound variable, and an .elapsed()-keyed rule would have
#           missed it.
#           THREE spellings, because an upper bound has three natural ones and
#           a guard that caught only the first would pass while the flake
#           landed: `elapsed < Duration::..`, `elapsed <= Duration::..`, and the
#           reversed `Duration::.. > elapsed` / `Duration::.. >= elapsed`.
#           `elapsed >= Duration::..` / `elapsed > Duration::..` are NOT matched:
#           per watcher_tests.rs's own invariant note, LOWER bounds are monotone
#           under descheduling and are the safe form. Note the two halves of the
#           rule are mirror images on purpose -- the operand order decides which
#           direction a comparison bounds, so `Duration::` on the left with `>`
#           means the same thing as `Duration::` on the right with `<`.
#           THE SCALAR FAMILY (the two trailing alternatives, added in the
#           #6438 review pass) is that same bound with the TYPE ERASED:
#           `assert!(start.elapsed().as_millis() < 500)` and
#           `assert!(elapsed.as_secs_f64() <= 2.0)` state exactly the
#           starvation-invertible claim the Duration alternatives exist to
#           catch, while carrying no `Duration::` token anywhere on the line
#           -- so those alternatives could not see them at all. The accessor
#           list is explicit (as_millis, as_micros, as_nanos, as_secs_f32,
#           as_secs_f64, as_secs) rather than a blanket `as_[a-z_]*`, which
#           would flag ordinary comparisons like `a.as_str() < b`. Direction
#           is preserved exactly as above: the forward alternative takes only
#           `<` / `<=` AFTER the accessor and the reversed one only `>` / `>=`
#           BEFORE it, so `elapsed.as_millis() >= 150` and
#           `150 < elapsed.as_millis()` -- both LOWER bounds -- stay clean.
#
# NO LINE JOINER, deliberately -- and this is the sharpest difference from the
# sibling guard, which needs a bash quote-state machine. Both rules here are
# single-physical-line constructs: `Instant::now() +` is one token pair, and a
# comparison operator sits on the same line as the `Duration::` it compares
# against in every formatting rustfmt produces. So a physical-line escape
# comment is EXACT, and a Rust-grammar joiner (`//`, `/* */`, raw strings,
# paren-balanced macros) would be substantial complexity for zero detection
# gain -- and would be a fresh source of bugs in a guard whose whole value is
# being trivially auditable.
# ---------------------------------------------------------------------------
_wallclock_fingerprints() {
    # SAME validation as the floor, so a bad root cannot slip in through the
    # engine and contribute zero records to a comparison that then reports
    # "clean" for it (fixture 4e-5).
    _wallclock_assert_roots "$@" || return $?

    # The escape token is split across two adjacent single-quoted strings so
    # this source file holds no contiguous copy of it (see SELF-MATCH SAFETY
    # in the header).
    local _esc_re; _esc_re='wallcl''ock:allow'
    local _rule_a
    _rule_a='(Instant::now\(\)[[:space:]]*(\+|\.checked_add|[<>]=?))'
    _rule_a="${_rule_a}"'|([<>]=?[[:space:]]*Instant::now\(\))'
    local _rule_b
    _rule_b='(<=?[[:space:]]*Duration::)|(Duration::[a-z_]*\([^)]*\)[[:space:]]*>)'
    # Scalar-accessor family, composed in rather than spelled inline so the
    # accessor list appears once instead of twice (the same composition idiom
    # the sibling guard uses for its `_wc_var_sfx`). The `<expr>` before the
    # accessor in the reversed alternative is deliberately narrow -- an
    # identifier with dots -- because a blanket `.*` there would let any `>`
    # earlier on the line (a `->` return arrow, a generic close) drag an
    # innocent line in.
    local _scal; _scal='(millis|micros|nanos|secs_f32|secs_f64|secs)'
    _rule_b="${_rule_b}"'|(\.as_'"${_scal}"'\(\)[[:space:]]*<=?)'
    _rule_b="${_rule_b}"'|(>=?[[:space:]]*[A-Za-z_][A-Za-z0-9_.]*\.as_'"${_scal}"'\(\))'

    # ONE grep for the whole scan, not one [[ =~ ]] per line. The detector
    # this replaced justified its per-line bash loop against `echo | grep` per
    # line -- true, but it never weighed one grep for the ENTIRE scan.
    # Measured on this tree: the bash loop over ONE ~30-file directory takes
    # 0.518s, while this whole function over all 36 roots (1331 files, 679k
    # lines) takes ~0.13s end to end -- about 0.10s of it the grep itself, over
    # a 0.10-0.44s spread across eight runs on a busy host. The loop at that
    # scale would cost ~30s in a gate that is supposed to be instant.
    # `-a` IS NOT OPTIONAL. Without it GNU grep stops at the first NUL byte in
    # a file, prints "binary file matches" to STDERR and contributes NOTHING to
    # the captured stdout -- so a violating line in such a file would vanish
    # from the record stream while rc stayed 0, and the guard would report "we
    # looked, and there were none" about a file it never read. The floor cannot
    # notice, because it counts with `find` and still counts the skipped file.
    # A silent skip is the one failure mode this guard is built not to have
    # (fixture 4e-8).
    local _hits _rc=0
    _hits="$(grep -arnE --include='*.rs' -e "$_rule_a" -e "$_rule_b" -- "$@")" || _rc=$?
    [ "$_rc" -le 1 ] || return "$_rc"

    # Escape filtering is its own pass for the same reason: it is one fork,
    # not one per line. rc 1 here means "every hit was escaped", which is a
    # clean result, so only rc >= 2 propagates.
    local _kept _krc=0
    _kept="$(printf '%s' "$_hits" | grep -vE "$_esc_re")" || _krc=$?
    [ "$_krc" -le 1 ] || return "$_krc"
    [ -n "$_kept" ] || return 0

    local _m _p _rest _text
    while IFS= read -r _m; do
        # NEVER a colon field-split. `awk -F:`/`cut -d: -f3` would cut inside
        # Rust's `::` and emit `Instant  now()` for a line containing
        # `Instant::now()` -- a fingerprint that can never match its own
        # source line again (fixture 4a-1; found by dry-running the generator).
        # Two anchored prefix strips take exactly the path and the line
        # number, leaving every later colon untouched by construction.
        _p="${_m%%:*}"       # path: up to the FIRST colon
        _rest="${_m#*:}"     # drop the path
        _text="${_rest#*:}"  # drop the line number; all later colons survive

        # Trim in bash rather than forking sed per line. A single stream `sed`
        # is not an option: it would have to tell the ` :: ` separator apart
        # from a `::` in the source text, and the separator does not exist yet
        # at that point.
        _text="${_text#"${_text%%[![:space:]]*}"}"
        _text="${_text%"${_text##*[![:space:]]}"}"

        printf '%s :: %s\n' "$_p" "$_text"

    # PLAIN `sort`, NEVER `sort -u`, HERE OR ANYWHERE DOWNSTREAM -- and this is
    # a deliberate divergence from the closest precedent, crates/reify-audit's
    # ptodo.rs::fingerprint, which collapses identical markers by design.
    # The two baselines are different KINDS of oracle:
    #   * ptodo's is a SUBSET oracle over deduped fingerprints (the #6859
    #     ruling), so a second copy of an already-baselined marker is
    #     intentionally not news.
    #   * this one is a MULTISET oracle, so a second copy IS news. Erasing line
    #     numbers makes the 3 copies of `while Instant::now() < deadline {` in
    #     jcodemunch_session_live.rs byte-equal records; deduping here would let
    #     a 4th land unseen, which is precisely a new hand-rolled deadline
    #     arriving under cover of an old one. Section 4b pins all three cases.
    done <<< "$_kept" | LC_ALL=C sort
}

# ---------------------------------------------------------------------------
# _detect_rust_wallclock_deadline <root>...
#
# REPORTING WRAPPER over _wallclock_fingerprints. It adds the human half --
# what to DO about a violation -- and nothing else. Rule A and Rule B are
# deliberately NOT restated here; they live with the engine above, in one copy.
#
# Prints each violating record to stderr, then the three sanctioned fixes in
# the order they should be tried. Returns 1 if any violation was found, 0 if
# none. An engine error (a missing or unreadable root) propagates as its own
# rc >= 2 rather than being flattened into "clean".
#
# This function USED to be the whole guard, carrying its own copy of both
# regexes and a per-line bash read loop over `"$dir"/*.rs`. Both are gone. The
# regex copy went because two statements of the only thing this file knows is
# exactly how the ratchet and the detector drift apart without either looking
# wrong; the loop went because it was NON-RECURSIVE -- which silently misses
# the 5 baselined sites that live in a subdirectory -- and ~400x slower than
# one grep at the widened scope.
# ---------------------------------------------------------------------------
_detect_rust_wallclock_deadline() {
    # Split across two adjacent single-quoted strings, as everywhere else in
    # this file: a contiguous copy would annotate this very line for the
    # sibling guard. Used only to SPELL the escape in the hint below.
    local _esc_re; _esc_re='wallcl''ock:allow'

    local _records _rc=0
    _records="$(_wallclock_fingerprints "$@")" || _rc=$?
    [ "$_rc" -eq 0 ] || return "$_rc"
    [ -n "$_records" ] || return 0

    printf '%s\n' "$_records" >&2
    echo "" >&2
    echo "Each line above builds a real-clock deadline by hand, or bounds elapsed time from ABOVE." >&2
    echo "An upper bound on elapsed time INVERTS under load: a saturated host that deschedules the" >&2
    echo "test thread fails code that behaved perfectly. That is the flake class tasks #5143, #5422," >&2
    echo "#5709 and #6438 each had to clean up. Try these three fixes, in this order:" >&2
    echo "  1. Drive the budget through the WaitClock seam in watcher_tests.rs (clock.now(), " >&2
    echo "     VirtualClock) so the assertion consumes no real time and the claim becomes exact." >&2
    echo "  2. Delete the upper bound outright and let nextest's slow-timeout / terminate-after" >&2
    echo "     catch a genuine hang -- that is what the two tombstones in watcher_tests.rs do." >&2
    echo "  3. Only if the site is genuinely legitimate, annotate it on the same line with" >&2
    echo "     '// ${_esc_re} -- <reason>'. Exactly ONE escape exists in tree today" >&2
    echo "     (far_future_stamp in watcher_tests.rs, argued at the site); yours would be" >&2
    echo "     the second, so state the argument where the next reader will find it." >&2
    return 1
}

# ---------------------------------------------------------------------------
# _emit_record_stream <blob>
#
# Print <blob> as newline-terminated lines, or NOTHING AT ALL when it is empty.
# `printf '%s\n' ""` would emit a single blank line, which `comm` would then
# treat as a record that is present on one side and absent on the other -- a
# phantom +/- on every drained-baseline or clean-tree comparison. Small, but it
# is the difference between the empty case working and the empty case lying.
# ---------------------------------------------------------------------------
_emit_record_stream() {
    [ -n "$1" ] || return 0
    printf '%s\n' "$1"
}

# ---------------------------------------------------------------------------
# _wallclock_baseline_check <baseline-file> <root>...
#
# THE RATCHET. Compares the live fingerprint multiset under <root>... against
# the multiset committed in <baseline-file>, and reds on any difference.
#
# Returns 0 and prints NOTHING when the two are equal -- silence on success is
# load-bearing (see Section 4c). Returns 1 on any difference, naming every
# offending record on stderr. Propagates the engine's rc >= 2 unchanged: a
# missing or unreadable root must never be reported as "no new violations".
#
# BASELINE ROWS are every line that is neither blank nor a `#` comment
# (indented or not), so the committed file can carry the header that explains
# what a row MEANS. That follows tests/infra/harness-layout-baseline.manifest,
# not ptodo-baseline.txt, which forbids comments. The stripping is ONE `grep -v`
# with two anchored alternatives, borrowed verbatim from
# harness-layout-lib.sh's _harness_layout_baseline_load, for its exit-status
# property: a single rc, so grep's error rc 2 ("cannot read the baseline")
# stays distinguishable from its rc 1 ("the baseline has no rows"). Two piped
# `grep -v`s would hide that behind PIPESTATUS, and a blanket `|| true` would
# collapse an unreadable baseline into a clean pass -- which is the vacuous
# green that file's own comment records as a real bug.
#
# COLLATION IS PINNED ON BOTH SIDES AND ON `comm` ITSELF. comm is only correct
# when its two inputs are sorted the way comm compares them; the engine sorts
# under LC_ALL=C, so this must too, rather than inheriting whatever locale the
# runner happens to have.
# ---------------------------------------------------------------------------
_wallclock_baseline_check() {
    local _baseline="$1"; shift

    local _live _lrc=0
    _live="$(_wallclock_fingerprints "$@")" || _lrc=$?
    [ "$_lrc" -eq 0 ] || return "$_lrc"

    local _rows _brc=0
    _rows="$(grep -vE '^[[:space:]]*#|^[[:space:]]*$' -- "$_baseline")" || _brc=$?
    [ "$_brc" -le 1 ] || return "$_brc"
    _rows="$(_emit_record_stream "$_rows" | LC_ALL=C sort)"

    # TWO DIRECTIONS over the SAME pair of sorted multisets.
    #   comm -23: present LIVE, absent from the BASELINE -- a violation written
    #             today.
    #   comm -13: present in the BASELINE, absent LIVE -- a site that was fixed
    #             or moved, whose row is now dead weight.
    # Multiset semantics come free either way: comm pairs equal lines one for
    # one, so a 4th copy of a 3x-baselined record surfaces as exactly one
    # unmatched line, and deleting one row of a duplicate pair leaves exactly
    # one stale row.
    local _new _stale
    _new="$(LC_ALL=C comm -23 \
        <(_emit_record_stream "$_live") \
        <(_emit_record_stream "$_rows"))"
    _stale="$(LC_ALL=C comm -13 \
        <(_emit_record_stream "$_live") \
        <(_emit_record_stream "$_rows"))"

    [ -n "$_new" ] || [ -n "$_stale" ] || return 0

    # BOTH directions are reported, always. They never net out against each
    # other: a run that fixed one site and added another has one of each, and
    # the reader needs to see both (fixture 4d-5).
    [ -z "$_new" ] || printf '%s\n' "$_new" | sed 's/^/  + /' >&2
    [ -z "$_stale" ] || printf '%s\n' "$_stale" | sed 's/^/  - /' >&2

    echo "" >&2
    echo "The live scan and $_baseline disagree." >&2
    echo "The two directions mean OPPOSITE things and take opposite fixes:" >&2
    # The legend deliberately does NOT begin a line with the record prefixes it
    # describes. A reported record and the prose about it must be tellable
    # apart, by a reader skimming and by anything counting them -- fixtures
    # 4d-3/4d-4/4d-5 count `^  + ` and `^  - ` lines, and an earlier draft that
    # opened these paragraphs with `  + <record>` was scored as three extra
    # records by its own report.
    if [ -n "$_new" ]; then
        echo "" >&2
        echo "A '+' line is a NEW hand-rolled real-clock deadline, or a NEW upper bound on" >&2
        echo "elapsed time. An upper bound INVERTS under load: a saturated host that" >&2
        echo "deschedules the test thread fails code that behaved perfectly." >&2
        echo "DO NOT simply append a baseline row for it -- a row means 'pre-existing debt" >&2
        echo "that must not grow', NOT 'blessed'. Apply the three sanctioned fixes in order:" >&2
        echo "  1. Drive the budget through the WaitClock seam (clock.now(), VirtualClock)" >&2
        echo "     so the assertion consumes no real time and the claim becomes exact." >&2
        echo "  2. Delete the upper bound and let nextest's slow-timeout / terminate-after" >&2
        echo "     catch a genuine hang." >&2
        echo "  3. Only if the site is genuinely legitimate, take the same-line escape AND" >&2
        echo "     raise _ESC_ALLOWLIST_SIZE in this file, so the argument lands in review" >&2
        echo "     rather than in a baseline row." >&2
    fi
    if [ -n "$_stale" ]; then
        echo "" >&2
        echo "A '-' line is a baselined site that was FIXED, MOVED or DELETED -- good news." >&2
        echo "Delete that row from $_baseline in this same diff." >&2
        echo "This direction is what makes the ratchet shrink instead of accreting dead" >&2
        echo "rows, which is why it is a red rather than a shrug." >&2
    fi
    echo "" >&2
    echo "Regenerate-and-DIFF if you need to; never regenerate-and-replace, which would" >&2
    echo "launder every + record above into the baseline unread." >&2
    return 1
}

# ---------------------------------------------------------------------------
# _count_rust_wallclock_escapes <root>...
#
# Prints (stdout) the number of PHYSICAL lines carrying the escape token
# across all *.rs files under <root>..., RECURSIVELY, and lists each one
# (stderr) as "file:lineno: <content>". Returns 0 -- the COUNT is the result,
# and what to do with it is the caller's assertion, not this function's. The
# one exception is a bad root, which returns the validator's 2: it is not an
# answer of any kind, and a zero there could match the allowlist by luck.
#
# WHY THIS EXISTS AT ALL (#6438 review). The detector above `continue`s on an
# escaped line without counting it, so its rc is 0 whether the tree holds one
# escape or twenty. The header's ALLOWLIST paragraph and Section 3's prose
# both claimed exactly ONE escape exists; nothing checked it, so a second
# could be added silently -- precisely the failure mode the allowlist says a
# second escape must not have ("a deliberate, reviewable act rather than
# pre-existing noise"). Counting turns that prose into an assertion: adding an
# escape now also requires editing _ESC_ALLOWLIST_SIZE below, which is a
# one-line diff a reviewer cannot miss.
#
# It counts LINES, not sites, which is the same unit the detector skips on --
# so the two can never disagree about what an escape is.
# ---------------------------------------------------------------------------
_count_rust_wallclock_escapes() {
    # SAME validator as the engine and the floor: a root that does not exist
    # must be a hard error, never a zero contribution that could make the
    # total match the allowlist by luck (fixture 2af).
    _wallclock_assert_roots "$@" || return $?

    # Split across two adjacent single-quoted strings, as everywhere else in
    # this file: a contiguous copy here would annotate this very line and
    # would make the counter count itself if it were ever pointed at a .rs
    # copy of its own logic.
    local _esc_re; _esc_re='wallcl''ock:allow'

    # Same single-grep-over-all-roots shape as _wallclock_fingerprints, for
    # the same two reasons: it recurses, and it is one fork rather than one
    # per line. `-n` keeps the "file:lineno: <content>" listing the contract
    # below promises. rc 1 means zero escapes -- a legitimate answer, not a
    # failure -- so only rc >= 2 propagates.
    # `-a` for the same reason as the engine, in the more dangerous direction:
    # a skipped file lowers the COUNT, and Section 3 compares that count against
    # _ESC_ALLOWLIST_SIZE for equality (fixture 4e-9).
    local _hits _rc=0
    _hits="$(grep -arnE --include='*.rs' -e "$_esc_re" -- "$@")" || _rc=$?
    [ "$_rc" -le 1 ] || return "$_rc"

    if [ -z "$_hits" ]; then
        echo "0"
        return 0
    fi

    printf '%s\n' "$_hits" >&2
    printf '%s\n' "$_hits" | grep -c .
    return 0
}

# ---------------------------------------------------------------------------
# _wallclock_uncovered_test_roots <root>...
#
# Reads candidate test roots on STDIN, one per line, and prints (stdout) those
# NOT COVERED by <root>.... Always returns 0.
#
# SAME CONTRACT AS _count_rust_wallclock_escapes, and for the same reason: the
# LIST is the result, and the verdict is the caller's. Section 3 turns an empty
# list into a green and a non-empty one into a named red; this function has no
# opinion about either.
#
# WHAT IT IS FOR: `_LIVE_ROOTS` is the one input every other assertion in this
# file trusts. Naming members one at a time can only pin the roots someone
# thought of, and the root the #6597 review found missing was one nobody had
# (gui/src-tauri/src/debug_server/tests). Deriving ground truth from the tree
# and diffing it against the list is what makes the header's "EVERY Rust TEST
# root" a checked claim rather than a promise.
#
# COVERED means EQUAL TO an argument, or a DESCENDANT of one. Prefix coverage
# rather than equality, because every root is scanned RECURSIVELY: a nested
# `a/tests/b/tests` is genuinely reached from `a/tests`, and reporting it would
# be a false red that invites someone to enumerate subdirectories.
#
# THE QUOTING IS THE WHOLE IMPLEMENTATION. `"$_arg"` is quoted, so an argument
# matches LITERALLY -- a root is a path, not a pattern -- and only the trailing
# `/*` is a wildcard. That is exactly what makes `x/tests2` NOT covered by
# `x/tests`. An unquoted `$_arg*` would call it covered and silently drop a real
# test root from the report this function exists to produce; fixture 4f-4 pins
# the difference, because that failure direction is the dangerous one.
#
# NO ROOT VALIDATION HERE, deliberately. The candidates come from `git
# ls-files` and the question is whether a NAME is covered by the list, which is
# independent of what exists on disk. Whether the list's OWN entries exist is
# _wallclock_assert_roots's job (Section 4e), asserted separately in Section 3.
# ---------------------------------------------------------------------------
_wallclock_uncovered_test_roots() {
    local _cand _arg _covered
    while IFS= read -r _cand; do
        # A blank line is not a candidate. Skipped rather than reported, so an
        # empty input cannot manufacture a phantom uncovered root.
        [ -n "$_cand" ] || continue
        _covered=0
        for _arg in "$@"; do
            case "$_cand" in
                "$_arg"|"$_arg"/*) _covered=1; break ;;
            esac
        done
        [ "$_covered" -eq 1 ] || printf '%s\n' "$_cand"
    done
    return 0
}
# ===========================================================================
# Section 1: Hermetic positive-detection -- the detector must flag a planted
#             hand-rolled real-clock deadline (Rule A).
# ===========================================================================
echo ""
echo "--- Section 1: hermetic positive-detection fixture ---"

_s1_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s1_tmpdir")
_fixture "$_s1_tmpdir" "fixture_pos.rs" \
    'fn poll_until_pending() {' \
    '    let deadline = Instant::now() + Duration::from_secs(5);' \
    '}'

# RED: _detect_rust_wallclock_deadline is not yet defined in this file. Run
# without the implementation, bash reports "command not found" (rc 127) and
# this assertion fails. The next step defines the function and turns it green.
# Same two-step shape the sibling guard documents at its own Section 1.
_s1_rc=0
_detect_rust_wallclock_deadline "$_s1_tmpdir" 2>/dev/null || _s1_rc=$?
assert "Rule A: planted hand-rolled real-clock deadline is flagged (returns 1, not 127/cmd-not-found)" \
    test "$_s1_rc" -eq 1

# ===========================================================================
# Section 2: Hermetic precision fixtures -- each case must either stay CLEAN
#             (rc 0) or confirm a true positive still fires (rc 1).
#
# The negatives are the point. A rule that flagged every `Instant::now()` or
# every `Duration` comparison would be useless: watcher_tests.rs is FULL of
# legitimate synthetic-clock arithmetic, and a guard that cried wolf there
# would be escaped into irrelevance within a week.
# ===========================================================================
echo ""
echo "--- Section 2: hermetic precision fixtures ---"

# ---------------------------------------------------------------------------
# 2a: VirtualClock seed -- NOT flagged. `let t0 = Instant::now();` binds a
#     reference point for synthetic arithmetic; no deadline is constructed.
# ---------------------------------------------------------------------------
_s2a_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2a_tmpdir")
_fixture "$_s2a_tmpdir" "fixture.rs" \
    '    let t0 = Instant::now();'

_s2a_rc=0
_detect_rust_wallclock_deadline "$_s2a_tmpdir" 2>/dev/null || _s2a_rc=$?
assert "2a: bare Instant::now() binding (synthetic-clock seed) NOT flagged (returns 0)" \
    test "$_s2a_rc" -eq 0

# ---------------------------------------------------------------------------
# 2b: VirtualClock construction -- NOT flagged. Seeding a virtual clock from
#     the real one is the SANCTIONED way to get determinism, not a violation.
# ---------------------------------------------------------------------------
_s2b_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2b_tmpdir")
_fixture "$_s2b_tmpdir" "fixture.rs" \
    '    let mut clock = VirtualClock::new(Instant::now());'

_s2b_rc=0
_detect_rust_wallclock_deadline "$_s2b_tmpdir" 2>/dev/null || _s2b_rc=$?
assert "2b: VirtualClock::new(Instant::now()) seed NOT flagged (returns 0)" \
    test "$_s2b_rc" -eq 0

# ---------------------------------------------------------------------------
# 2c: The blessed WaitClock seam -- NOT flagged. This is the exact line inside
#     wait_until_on that every de-flaked test is supposed to route through.
#     If the guard flagged it, the guard would be forbidding its own remedy.
# ---------------------------------------------------------------------------
_s2c_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2c_tmpdir")
_fixture "$_s2c_tmpdir" "fixture.rs" \
    '    let deadline = clock.now() + timeout;'

_s2c_rc=0
_detect_rust_wallclock_deadline "$_s2c_tmpdir" 2>/dev/null || _s2c_rc=$?
assert "2c: clock.now() + timeout (the WaitClock seam) NOT flagged (returns 0)" \
    test "$_s2c_rc" -eq 0

# ---------------------------------------------------------------------------
# 2d: Synthetic offset off a bound variable -- NOT flagged. Every debouncer_*
#     test is built from these; they consume no real time.
# ---------------------------------------------------------------------------
_s2d_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2d_tmpdir")
_fixture "$_s2d_tmpdir" "fixture.rs" \
    '    assert_eq!(deb.drain_ready(t0 + Duration::from_millis(150)), vec![]);'

_s2d_rc=0
_detect_rust_wallclock_deadline "$_s2d_tmpdir" 2>/dev/null || _s2d_rc=$?
assert "2d: synthetic offset off a bound instant NOT flagged (returns 0)" \
    test "$_s2d_rc" -eq 0

# ---------------------------------------------------------------------------
# 2e: Rule B positive -- an upper bound on `.elapsed()`. This is the literal
#     shape deleted from wait_for_returns_true_promptly_when_condition_
#     already_satisfied by this task.
# ---------------------------------------------------------------------------
_s2e_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2e_tmpdir")
_fixture "$_s2e_tmpdir" "fixture.rs" \
    '    assert!(' \
    '        start.elapsed() < Duration::from_secs(1),' \
    '        "should return promptly when already satisfied"' \
    '    );'

_s2e_rc=0
_detect_rust_wallclock_deadline "$_s2e_tmpdir" 2>/dev/null || _s2e_rc=$?
assert "2e: upper bound on .elapsed() against a Duration is flagged (returns 1)" \
    test "$_s2e_rc" -eq 1

# ---------------------------------------------------------------------------
# 2f: Rule B positive, BOUND-VARIABLE form -- also flagged. This fixture is
#     why Rule B keys on the comparison against a `Duration` rather than on
#     `.elapsed()`: the deleted bound in watcher_drop_joins_worker_without_
#     hanging_even_with_a_pending_event compared a `let elapsed =
#     start.elapsed();` variable, and an `.elapsed()`-keyed rule would have
#     missed it entirely.
# ---------------------------------------------------------------------------
_s2f_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2f_tmpdir")
_fixture "$_s2f_tmpdir" "fixture.rs" \
    '    assert!(' \
    '        elapsed < Duration::from_secs(2),' \
    '        "Drop should join the worker thread promptly"' \
    '    );'

_s2f_rc=0
_detect_rust_wallclock_deadline "$_s2f_tmpdir" 2>/dev/null || _s2f_rc=$?
assert "2f: upper bound on a bound elapsed VARIABLE is flagged too (returns 1)" \
    test "$_s2f_rc" -eq 1

# ---------------------------------------------------------------------------
# 2g: Rule B negative -- LOWER bounds are monotone under descheduling and are
#     the SAFE form. watcher_tests.rs states this invariant itself and relies
#     on two such bounds to prove WallClock::sleep really blocks; flagging
#     them would push the file toward deleting its own load-bearing checks.
# ---------------------------------------------------------------------------
_s2g_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2g_tmpdir")
_fixture "$_s2g_tmpdir" "fixture.rs" \
    '    assert!(' \
    '        start.elapsed() >= Duration::from_millis(150),' \
    '        "should wait out the full timeout"' \
    '    );'

_s2g_rc=0
_detect_rust_wallclock_deadline "$_s2g_tmpdir" 2>/dev/null || _s2g_rc=$?
assert "2g: LOWER bound on .elapsed() NOT flagged (monotone-safe) (returns 0)" \
    test "$_s2g_rc" -eq 0

# ---------------------------------------------------------------------------
# 2h: Rule B negative -- a Duration used as a VALUE, not as a bound. The `<`
#     in Rule B must key on comparison; a method call that merely mentions
#     Duration (here wait_until_on's poll-interval clamp) is not an assertion.
# ---------------------------------------------------------------------------
_s2h_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2h_tmpdir")
_fixture "$_s2h_tmpdir" "fixture.rs" \
    '        clock.sleep(Duration::from_millis(20).min(remaining));'

_s2h_rc=0
_detect_rust_wallclock_deadline "$_s2h_tmpdir" 2>/dev/null || _s2h_rc=$?
assert "2h: Duration used as a value (not a bound) NOT flagged (returns 0)" \
    test "$_s2h_rc" -eq 0

# ---------------------------------------------------------------------------
# 2i: Escape on a Rule A positive -- NOT flagged.
# ---------------------------------------------------------------------------
_s2i_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2i_tmpdir")
_fixture "$_s2i_tmpdir" "fixture.rs" \
    "    let deadline = Instant::now() + Duration::from_secs(5); // $_ESC_TOKEN -- reason"

_s2i_rc=0
_detect_rust_wallclock_deadline "$_s2i_tmpdir" 2>/dev/null || _s2i_rc=$?
assert "2i: same-line escape comment opts a Rule A site out (returns 0)" \
    test "$_s2i_rc" -eq 0

# ---------------------------------------------------------------------------
# 2j: Escape on a Rule B positive -- NOT flagged. Asserted separately from 2i
#     because the escape is checked once per line but the rules are separate
#     alternatives; an implementation that wired the escape into only one of
#     them would pass 2i and fail here.
# ---------------------------------------------------------------------------
_s2j_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2j_tmpdir")
_fixture "$_s2j_tmpdir" "fixture.rs" \
    "        elapsed < Duration::from_secs(2), // $_ESC_TOKEN -- reason"

_s2j_rc=0
_detect_rust_wallclock_deadline "$_s2j_tmpdir" 2>/dev/null || _s2j_rc=$?
assert "2j: same-line escape comment opts a Rule B site out (returns 0)" \
    test "$_s2j_rc" -eq 0

# ---------------------------------------------------------------------------
# 2k: SCOPE -- only .rs files are scanned. A violation planted in a non-Rust
#     file in the same directory must not be reported.
# ---------------------------------------------------------------------------
_s2k_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2k_tmpdir")
_fixture "$_s2k_tmpdir" "fixture.rs.txt" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'

_s2k_rc=0
_detect_rust_wallclock_deadline "$_s2k_tmpdir" 2>/dev/null || _s2k_rc=$?
assert "2k: a violation in a non-.rs file is out of scope (returns 0)" \
    test "$_s2k_rc" -eq 0

# ---------------------------------------------------------------------------
# 2l: MIXED -- a clean file and a violating file in the same directory. Pins
#     that the scan is per-directory rather than first-file-wins.
# ---------------------------------------------------------------------------
_s2l_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2l_tmpdir")
_fixture "$_s2l_tmpdir" "aaa_clean.rs" \
    '    let t0 = Instant::now();'
_fixture "$_s2l_tmpdir" "zzz_dirty.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'

_s2l_rc=0
_detect_rust_wallclock_deadline "$_s2l_tmpdir" 2>/dev/null || _s2l_rc=$?
assert "2l: a violation in a later file is still found (returns 1)" \
    test "$_s2l_rc" -eq 1

# ---------------------------------------------------------------------------
# 2m: EMPTY directory -- no .rs files at all. Must be clean, not an error:
#     under `set -euo pipefail` an unmatched glob is a classic way for a
#     detector to abort instead of returning 0.
# ---------------------------------------------------------------------------
_s2m_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2m_tmpdir")

_s2m_rc=0
_detect_rust_wallclock_deadline "$_s2m_tmpdir" 2>/dev/null || _s2m_rc=$?
assert "2m: a directory with no .rs files is clean, not an error (returns 0)" \
    test "$_s2m_rc" -eq 0

# ---------------------------------------------------------------------------
# 2n: Rule A positive, CHECKED_ADD form -- also flagged. `checked_add` is an
#     equally natural spelling of "build a deadline off the raw clock", and it
#     was a false negative until this fixture existed: the reviewer of #6438
#     verified `Instant::now().checked_add(Duration::from_secs(5)).unwrap()`
#     returned rc 0 from the shipped detector, i.e. the guard's headline claim
#     ("the fifth instance cannot land silently") was false for anyone who
#     happened to write it this way.
# ---------------------------------------------------------------------------
_s2n_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2n_tmpdir")
_fixture "$_s2n_tmpdir" "fixture.rs" \
    '    let deadline = Instant::now().checked_add(Duration::from_secs(5)).unwrap();'

_s2n_rc=0
_detect_rust_wallclock_deadline "$_s2n_tmpdir" 2>/dev/null || _s2n_rc=$?
assert "2n: Instant::now().checked_add(..) deadline is flagged too (returns 1)" \
    test "$_s2n_rc" -eq 1

# ---------------------------------------------------------------------------
# 2o: Rule A escape on the CHECKED_ADD form -- NOT flagged. Asserted alongside
#     2i/2j for the same reason 2j exists: the escape must apply uniformly to
#     every alternative, not just the one it was first written against. This
#     is the shape the single in-tree escape (far_future_stamp) uses.
# ---------------------------------------------------------------------------
_s2o_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2o_tmpdir")
_fixture "$_s2o_tmpdir" "fixture.rs" \
    "    let stamp = Instant::now().checked_add(Duration::from_secs(3600)); // $_ESC_TOKEN -- reason"

_s2o_rc=0
_detect_rust_wallclock_deadline "$_s2o_tmpdir" 2>/dev/null || _s2o_rc=$?
assert "2o: same-line escape opts a Rule A checked_add site out (returns 0)" \
    test "$_s2o_rc" -eq 0

# ---------------------------------------------------------------------------
# 2p: Rule B positive, `<=` form -- also flagged. Same false-negative story as
#     2n: `<=` is an equally natural spelling of an elapsed-time upper bound,
#     and a rule matching only a bare `<` passes exactly when it should fail.
# ---------------------------------------------------------------------------
_s2p_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2p_tmpdir")
_fixture "$_s2p_tmpdir" "fixture.rs" \
    '    assert!(elapsed <= Duration::from_secs(2), "should join promptly");'

_s2p_rc=0
_detect_rust_wallclock_deadline "$_s2p_tmpdir" 2>/dev/null || _s2p_rc=$?
assert "2p: <= upper bound against a Duration is flagged (returns 1)" \
    test "$_s2p_rc" -eq 1

# ---------------------------------------------------------------------------
# 2q: Rule B positive, REVERSED-OPERAND form -- also flagged. `Duration > x`
#     bounds x from ABOVE just as surely as `x < Duration` does; operand order
#     is a style choice, not a semantic one, so the guard must be symmetric or
#     it merely dictates which way to write the flake.
# ---------------------------------------------------------------------------
_s2q_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2q_tmpdir")
_fixture "$_s2q_tmpdir" "fixture.rs" \
    '    assert!(Duration::from_secs(2) > elapsed, "should join promptly");'

_s2q_rc=0
_detect_rust_wallclock_deadline "$_s2q_tmpdir" 2>/dev/null || _s2q_rc=$?
assert "2q: reversed-operand upper bound (Duration > elapsed) is flagged (returns 1)" \
    test "$_s2q_rc" -eq 1

# ---------------------------------------------------------------------------
# 2r: Rule B negative, the MIRROR of 2q -- a `Duration::` to the RIGHT of `>`
#     is a LOWER bound and must stay clean. This is the false positive the
#     reversed-operand alternative could most easily introduce, and the pair
#     2q/2r is what pins that the rule keys on operand ORDER rather than on
#     "a Duration appears near a comparison".
# ---------------------------------------------------------------------------
_s2r_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2r_tmpdir")
_fixture "$_s2r_tmpdir" "fixture.rs" \
    '    assert!(start.elapsed() > Duration::from_millis(150), "should block");' \
    '    fn next_wait(&self) -> Option<Duration> { self.remaining }'

_s2r_rc=0
_detect_rust_wallclock_deadline "$_s2r_tmpdir" 2>/dev/null || _s2r_rc=$?
assert "2r: lower bound with Duration on the right, and a -> return type, stay clean (returns 0)" \
    test "$_s2r_rc" -eq 0

# ---------------------------------------------------------------------------
# 2s: Rule A positive, SPLIT-DEADLINE form -- the construct #6438 deleted,
#     minimally rewritten so no line carries `Instant::now()` next to `+`.
#     The guard shipped by #6438 returned rc 0 for this whole fixture: the
#     seed line is a sanctioned synthetic-clock binding (2a), the offset line
#     never mentions `Instant::now()`, and neither line compares against a
#     `Duration`. The comparison line is the one that must fire -- a deadline
#     nobody checks does nothing, so checking it is where it resurfaces.
# ---------------------------------------------------------------------------
_s2s_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2s_tmpdir")
_fixture "$_s2s_tmpdir" "fixture.rs" \
    '    let start = Instant::now();' \
    '    let deadline = start + Duration::from_secs(5);' \
    '    while Instant::now() < deadline {' \
    '        std::thread::sleep(Duration::from_millis(2));' \
    '    }'

_s2s_rc=0
_detect_rust_wallclock_deadline "$_s2s_tmpdir" 2>/dev/null || _s2s_rc=$?
assert "2s: split-deadline poll loop is flagged at its comparison (returns 1)" \
    test "$_s2s_rc" -eq 1

# ---------------------------------------------------------------------------
# 2t: Rule A positive, REVERSED-OPERAND comparison -- `deadline <= Instant::
#     now()` is the same expiry check with the operands swapped, and operand
#     order is a style choice. Pairs with 2s exactly as 2q pairs with 2e.
# ---------------------------------------------------------------------------
_s2t_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2t_tmpdir")
_fixture "$_s2t_tmpdir" "fixture.rs" \
    '        if deadline <= Instant::now() {'

_s2t_rc=0
_detect_rust_wallclock_deadline "$_s2t_tmpdir" 2>/dev/null || _s2t_rc=$?
assert "2t: reversed-operand expiry check (deadline <= Instant::now()) is flagged (returns 1)" \
    test "$_s2t_rc" -eq 1

# ---------------------------------------------------------------------------
# 2u: Rule A negative, the MIRROR of 2s/2t -- the SAME poll loop taken through
#     the `WaitClock` seam stays clean. This is the remedy the detector's own
#     hint names first, so flagging it would leave an author with no legal
#     way to wait at all. Only the RAW clock is a violation.
# ---------------------------------------------------------------------------
_s2u_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2u_tmpdir")
_fixture "$_s2u_tmpdir" "fixture.rs" \
    '    let deadline = clock.now() + timeout;' \
    '    while clock.now() < deadline {' \
    '        clock.sleep(Duration::from_millis(20));' \
    '    }'

_s2u_rc=0
_detect_rust_wallclock_deadline "$_s2u_tmpdir" 2>/dev/null || _s2u_rc=$?
assert "2u: the same poll loop on the WaitClock seam stays clean (returns 0)" \
    test "$_s2u_rc" -eq 0

# ---------------------------------------------------------------------------
# 2v: Rule B positive, SCALAR-ACCESSOR form -- 2e's bound with the type
#     erased. No `Duration::` token appears on the line, so the Duration
#     alternatives cannot see it; it is nonetheless the identical
#     starvation-invertible claim, and the most natural way to write it once
#     someone reaches for a millisecond count.
# ---------------------------------------------------------------------------
_s2v_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2v_tmpdir")
_fixture "$_s2v_tmpdir" "fixture.rs" \
    '    assert!(start.elapsed().as_millis() < 500, "should return promptly");'

_s2v_rc=0
_detect_rust_wallclock_deadline "$_s2v_tmpdir" 2>/dev/null || _s2v_rc=$?
assert "2v: scalar upper bound (as_millis() < N) is flagged (returns 1)" \
    test "$_s2v_rc" -eq 1

# ---------------------------------------------------------------------------
# 2w: Rule B positive, FLOAT-SECONDS scalar with `<=` -- asserted separately
#     from 2v because the accessor list and the operator are independent
#     halves of the alternative, and an implementation that hard-coded
#     `as_millis` or a bare `<` would pass 2v and fail here.
# ---------------------------------------------------------------------------
_s2w_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2w_tmpdir")
_fixture "$_s2w_tmpdir" "fixture.rs" \
    '    assert!(elapsed.as_secs_f64() <= 2.0, "Drop should join promptly");'

_s2w_rc=0
_detect_rust_wallclock_deadline "$_s2w_tmpdir" 2>/dev/null || _s2w_rc=$?
assert "2w: scalar upper bound (as_secs_f64() <= 2.0) is flagged (returns 1)" \
    test "$_s2w_rc" -eq 1

# ---------------------------------------------------------------------------
# 2x: Rule B positive, REVERSED scalar form -- `500 > elapsed.as_millis()`
#     bounds elapsed from above just as `elapsed.as_millis() < 500` does.
#     Same symmetry argument as 2q, applied to the scalar family.
# ---------------------------------------------------------------------------
_s2x_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2x_tmpdir")
_fixture "$_s2x_tmpdir" "fixture.rs" \
    '    assert!(500 > elapsed.as_millis(), "should return promptly");'

_s2x_rc=0
_detect_rust_wallclock_deadline "$_s2x_tmpdir" 2>/dev/null || _s2x_rc=$?
assert "2x: reversed scalar upper bound (N > as_millis()) is flagged (returns 1)" \
    test "$_s2x_rc" -eq 1

# ---------------------------------------------------------------------------
# 2y: Rule B negative, the MIRROR of 2v/2x and the false positive the scalar
#     family could most easily introduce. LOWER bounds in BOTH operand orders
#     must stay clean -- they are the monotone-safe form this file relies on
#     to prove `WallClock::sleep` really blocks -- and so must an unrelated
#     `<` comparison on a non-time accessor, which is why the accessor list is
#     explicit rather than a blanket `as_*()`. The `-> Duration` line pins
#     that a return arrow cannot stand in for the reversed alternative's `>`.
# ---------------------------------------------------------------------------
_s2y_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2y_tmpdir")
_fixture "$_s2y_tmpdir" "fixture.rs" \
    '    assert!(start.elapsed().as_millis() >= 150, "should block");' \
    '    assert!(150 < elapsed.as_millis(), "should block");' \
    '    assert!(a.as_str() < b.as_str(), "ordering is unrelated to time");' \
    '    fn budget(&self) -> Duration { self.remaining }'

_s2y_rc=0
_detect_rust_wallclock_deadline "$_s2y_tmpdir" 2>/dev/null || _s2y_rc=$?
assert "2y: scalar LOWER bounds in both orders, and non-time accessors, stay clean (returns 0)" \
    test "$_s2y_rc" -eq 0

# ---------------------------------------------------------------------------
# 2z: THE ONE DELIBERATE FALSE POSITIVE, pinned rather than left as folklore.
#     Both rules scan every physical line, comments included, so a doc comment
#     that QUOTES a forbidden shape is reported like the real thing. See KNOWN
#     LIMITS in the header for why exempting `//` lines was rejected: this
#     assertion is the checked half of that argument, and it also means a
#     future decision to exempt them turns THIS fixture red -- a deliberate
#     choice -- instead of silently widening the guard's blind spot.
#     The remedy for a legitimate case is the same same-line escape as
#     anywhere else, which 2i/2j/2o already pin.
# ---------------------------------------------------------------------------
_s2z_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2z_tmpdir")
_fixture "$_s2z_tmpdir" "fixture.rs" \
    '/// Never write this: assert!(elapsed < Duration::from_secs(2))'

_s2z_rc=0
_detect_rust_wallclock_deadline "$_s2z_tmpdir" 2>/dev/null || _s2z_rc=$?
assert "2z: a comment QUOTING a forbidden shape is flagged too (documented, returns 1)" \
    test "$_s2z_rc" -eq 1

# ---------------------------------------------------------------------------
# 2aa: escape COUNTER, zero case. A clean file (and a violating one that
#      carries no escape) must count zero -- the counter keys on the escape
#      token, not on whether a line would otherwise be flagged.
# ---------------------------------------------------------------------------
_s2aa_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2aa_tmpdir")
_fixture "$_s2aa_tmpdir" "fixture.rs" \
    '    let t0 = Instant::now();' \
    '    let deadline = Instant::now() + Duration::from_secs(5);'

_s2aa_count="$(_count_rust_wallclock_escapes "$_s2aa_tmpdir" 2>/dev/null)"
assert "2aa: a directory with no escape annotations counts 0" \
    test "$_s2aa_count" -eq 0

# ---------------------------------------------------------------------------
# 2ab: escape COUNTER, one case -- the shape of the single in-tree escape.
# ---------------------------------------------------------------------------
_s2ab_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2ab_tmpdir")
_fixture "$_s2ab_tmpdir" "fixture.rs" \
    "    let stamp = Instant::now().checked_add(Duration::from_secs(3600)); // $_ESC_TOKEN -- reason" \
    '    let t0 = Instant::now();'

_s2ab_count="$(_count_rust_wallclock_escapes "$_s2ab_tmpdir" 2>/dev/null)"
assert "2ab: a single escape annotation counts 1" \
    test "$_s2ab_count" -eq 1

# ---------------------------------------------------------------------------
# 2ac: escape COUNTER, MANY case, across two files. This is the assertion
#      that matters: a counter that saturated at one -- or that stopped at
#      the first file, as a `grep -l`-shaped implementation would -- would
#      pass 2ab and still let a second escape land silently, which is the
#      whole reason the counter exists.
# ---------------------------------------------------------------------------
_s2ac_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2ac_tmpdir")
_fixture "$_s2ac_tmpdir" "aaa.rs" \
    "    let a = Instant::now() + Duration::from_secs(1); // $_ESC_TOKEN -- reason" \
    "    let b = Instant::now() + Duration::from_secs(2); // $_ESC_TOKEN -- reason"
_fixture "$_s2ac_tmpdir" "zzz.rs" \
    "    let c = Instant::now() + Duration::from_secs(3); // $_ESC_TOKEN -- reason"

_s2ac_count="$(_count_rust_wallclock_escapes "$_s2ac_tmpdir" 2>/dev/null)"
assert "2ac: three escapes across two files count 3, not 1 and not 2" \
    test "$_s2ac_count" -eq 3

# ---------------------------------------------------------------------------
# 2ad: escape COUNTER, RECURSION. An escape buried in a subdirectory must be
#      counted. The counter globbed `"$dir"/*.rs`, which is non-recursive --
#      harmless while it only ever saw one flat directory, and a silent hole
#      the moment Section 3 points it at 36 roots full of subdirectories. An
#      uncounted escape is strictly worse than an uncounted violation: the
#      detector cannot see an escaped line AT ALL, so the escape count is the
#      only thing that knows the line exists.
# ---------------------------------------------------------------------------
_s2ad_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2ad_tmpdir")
mkdir -p "$_s2ad_tmpdir/nested/deeper"
_fixture "$_s2ad_tmpdir" "top.rs" \
    "    let a = Instant::now() + Duration::from_secs(1); // $_ESC_TOKEN -- reason"
_fixture "$_s2ad_tmpdir/nested" "mid.rs" \
    "    let b = Instant::now() + Duration::from_secs(2); // $_ESC_TOKEN -- reason"
_fixture "$_s2ad_tmpdir/nested/deeper" "low.rs" \
    "    let c = Instant::now() + Duration::from_secs(3); // $_ESC_TOKEN -- reason"

_s2ad_count="$(_count_rust_wallclock_escapes "$_s2ad_tmpdir" 2>/dev/null)"
assert "2ad: escapes nested two levels deep are counted (3, not 1)" \
    test "$_s2ad_count" -eq 3

# ---------------------------------------------------------------------------
# 2ae: escape COUNTER, MULTIPLE ROOTS in one call -- the shape Section 3 now
#      uses. A single-root implementation would silently count only `$1`,
#      which for the live list is crates/reify-ast/tests: zero escapes, and a
#      guard that reports "0" while the tree holds one.
# ---------------------------------------------------------------------------
_s2ae_r1="$(mktemp -d)"; _TMPDIRS+=("$_s2ae_r1")
_s2ae_r2="$(mktemp -d)"; _TMPDIRS+=("$_s2ae_r2")
_s2ae_r3="$(mktemp -d)"; _TMPDIRS+=("$_s2ae_r3")
_fixture "$_s2ae_r1" "clean.rs" \
    '    let t0 = Instant::now();'
_fixture "$_s2ae_r2" "one.rs" \
    "    let a = Instant::now() + Duration::from_secs(1); // $_ESC_TOKEN -- reason"
mkdir -p "$_s2ae_r3/sub"
_fixture "$_s2ae_r3/sub" "two.rs" \
    "    let b = Instant::now() + Duration::from_secs(2); // $_ESC_TOKEN -- reason"

_s2ae_count="$(_count_rust_wallclock_escapes "$_s2ae_r1" "$_s2ae_r2" "$_s2ae_r3" 2>/dev/null)"
assert "2ae: escapes across three roots, one of them nested, count 2" \
    test "$_s2ae_count" -eq 2

# ---------------------------------------------------------------------------
# 2af: escape COUNTER, NON-.rs and BAD ROOT. The count feeds an equality
#      assertion, so both of its failure directions matter: an escape in a
#      .txt must not inflate it, and a root that does not exist must be a hard
#      error rather than a zero contribution that could make the total match
#      the allowlist by luck.
# ---------------------------------------------------------------------------
_s2af_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2af_tmpdir")
_fixture "$_s2af_tmpdir" "notrust.txt" \
    "    let a = Instant::now() + Duration::from_secs(1); // $_ESC_TOKEN -- reason"

_s2af_count="$(_count_rust_wallclock_escapes "$_s2af_tmpdir" 2>/dev/null)"
assert "2af: an escape in a non-.rs file is not counted" \
    test "$_s2af_count" -eq 0

_s2af_rc=0
_count_rust_wallclock_escapes "$_s2af_tmpdir/no-such-root" >/dev/null 2>&1 || _s2af_rc=$?
assert "2af: a root that does not exist is a hard error, not a zero count" \
    test "$_s2af_rc" -ne 0


# ===========================================================================
# Section 3: LIVE GUARD -- the ratchet, over EVERY Rust test root.
#
# Scope, since this is what task #6597 changed. #6438 shipped this guard
# scoped to the single non-recursive directory gui/src-tauri/src/tests -- the
# file where all four flakes happened, and, as its header said plainly, all it
# scanned. It now covers every crates/*/tests (32 today, glob-expanded so a new
# crate is covered the day it lands), all three gui/src-tauri test roots, and
# tree-sitter-reify/tests, recursively -- 36 roots, 1331 .rs files today.
#
# That widening cannot be a one-line change, because 19 violating lines across
# 6 files already exist and would red the gate on day one. They are BASELINED
# in tests/infra/wallclock-rust-deadline-baseline.txt -- pre-existing debt that
# must not grow, which is a different and weaker claim than the escape
# comment's "legitimate, argued at the site". Read that file's header before
# touching a row.
#
# FOUR ASSERTIONS, and each one covers a way the other three can lie:
#   (1) THE ROOT LIST IS REAL, AND COMPLETE. Every entry exists; the list names
#       the roots known to hold baselined sites; and it is diffed against ground
#       truth derived from the tree, so a test root NOBODY thought of cannot sit
#       unscanned -- the failure the #6597 review found. A glob that expanded to
#       nothing would otherwise leave a shorter list that still passes
#       everything else.
#   (2) THE RATCHET HOLDS, in both directions -- no new violation, and no
#       stale row.
#   (3) THE FLOOR. Files were actually scanned. A subset oracle is trivially
#       satisfied by the empty set, and once the baseline is drained to zero
#       -- the goal -- nothing else here would notice a scan of nothing.
#   (4) THE ESCAPE COUNT still matches the allowlist. The ratchet cannot see
#       an escaped line at all, so widening the scan must not quietly admit an
#       unargued escape from some other crate.
# ===========================================================================
echo ""
echo "--- Section 3: live scan of every Rust test root ---"

# Every Rust TEST ROOT, repo-relative (see the cd at the top of this file).
# `crates/*/tests` rather than an enumerated list so a new crate's tests are
# ratcheted the day they land -- the guard must not need editing to stay
# honest. An unmatched glob leaves the literal pattern string as an array
# element, which _wallclock_assert_roots rejects loudly rather than skipping.
#
# PRODUCTION CODE IS DELIBERATELY EXCLUDED, and roots are directories whose
# CONTENTS are tests -- which is why gui/src-tauri/src/tests is in (a test root
# that happens to live under src/) while crates/*/src is out. See KNOWN LIMITS
# in the header for the argument and the sites it drops.
_LIVE_ROOTS=(
    crates/*/tests
    gui/src-tauri/src/tests
    # A test root under src/, and not reachable from the one above it. Its
    # CONTENTS are tests -- 31 #[test]/#[tokio::test] fns in write_tools.rs --
    # split out of debug_server.rs's `mod tests` for SIZE alone, and deliberately
    # a CHILD of debug_server::tests rather than a sibling under src/tests/, so
    # `use super::*` still reaches the private production items. That parentage is
    # why recursing gui/src-tauri/src/tests never reaches it, and why the #6597
    # review found it unscanned -- in the crate that produced all four flakes.
    #
    # ENUMERATED rather than globbed as `gui/src-tauri/src/*/tests`: a glob with
    # one member today collapses to its literal pattern string the day that member
    # moves, which _wallclock_assert_roots would then report as a missing root --
    # a hard error for a tree that is actually fine. The completeness check below
    # is what keeps the enumeration honest instead of a glob.
    gui/src-tauri/src/debug_server/tests
    gui/src-tauri/tests
    tree-sitter-reify/tests
)

_BASELINE_FILE="$SCRIPT_DIR/wallclock-rust-deadline-baseline.txt"

# --- (1) the root list is real ---------------------------------------------
assert "live scan: _LIVE_ROOTS is non-empty" \
    test "${#_LIVE_ROOTS[@]}" -ne 0

_s3_missing=0
for _s3_root in "${_LIVE_ROOTS[@]}"; do
    [ -d "$_s3_root" ] && continue
    echo "_LIVE_ROOTS entry is not a directory: $_s3_root" >&2
    _s3_missing=$((_s3_missing + 1))
done
assert "live scan: every _LIVE_ROOTS entry exists on disk" \
    test "$_s3_missing" -eq 0

# The named members are the ones that would make a collapsed glob obvious: a
# `crates/*/tests` that matched nothing still leaves the enumerated roots
# behind, and the list would look plausible. Naming two crates known to hold
# baselined sites means the list cannot shrink silently.
# gui/src-tauri/src/debug_server/tests is named for a different reason: it is
# the root the #6597 review found MISSING, so it is pinned by name as well as
# by the general completeness check below -- a specific regression and a
# general property, which are not the same assertion.
for _s3_want in gui/src-tauri/src/tests gui/src-tauri/src/debug_server/tests \
                gui/src-tauri/tests \
                crates/reify-audit/tests crates/reify-fdm/tests \
                tree-sitter-reify/tests; do
    _s3_found=0
    for _s3_root in "${_LIVE_ROOTS[@]}"; do
        [ "$_s3_root" = "$_s3_want" ] && { _s3_found=1; break; }
    done
    assert "live scan: _LIVE_ROOTS contains $_s3_want" \
        test "$_s3_found" -eq 1
done

# THE LIST IS COMPLETE -- checked against the tree, not asserted in the header.
# Naming members one at a time can only pin the roots someone thought of; the
# failure the #6597 review actually found was a root NOBODY thought of
# (gui/src-tauri/src/debug_server/tests). So ground truth is DERIVED: every
# directory that holds a tracked .rs file and is named `tests`. `git ls-files`
# is the right source precisely because it is TRACKED-only -- an untracked
# scratch directory must not red the gate.
#
# Coverage is by PREFIX, not equality: the roots scan recursively, so a nested
# `a/tests/b/tests` is genuinely reached from `a/tests`. See Section 4f.
_s3_gt_rc=0
_s3_gt="$(git ls-files '*.rs' \
    | sed -E 's#(.*/tests)/.*#\1#' \
    | LC_ALL=C sort -u \
    | grep '/tests$')" || _s3_gt_rc=$?
assert "live scan: the ground-truth test-root derivation succeeds" \
    test "$_s3_gt_rc" -eq 0

# The derivation itself must not be vacuous: a `sed` or `git` that silently
# stopped producing roots would make the completeness check below trivially
# true, which is the same empty-set hole Section 4e exists to close one layer
# down. 36 roots today; the floor is deliberately loose, and it is a LOWER
# bound because that is the only direction that means anything for a tree that
# grows.
_s3_gt_n="$(_emit_record_stream "$_s3_gt" | grep -c . || true)"
assert "live scan: the ground-truth derivation found a plausible number of test roots" \
    test "$_s3_gt_n" -ge 20

_s3_unc_rc=0
_s3_uncovered="$(_emit_record_stream "$_s3_gt" \
    | _wallclock_uncovered_test_roots "${_LIVE_ROOTS[@]}")" || _s3_unc_rc=$?
assert "live scan: the completeness check runs (returns 0 -- the LIST is the result)" \
    test "$_s3_unc_rc" -eq 0

if [ -n "$_s3_uncovered" ]; then
    echo "" >&2
    echo "Tracked Rust TEST ROOTS that _LIVE_ROOTS does not cover:" >&2
    printf '%s\n' "$_s3_uncovered" | sed 's/^/  /' >&2
    echo "" >&2
    echo "Each directory above holds tracked Rust tests that this guard is NOT scanning," >&2
    echo "so a hand-rolled deadline or an elapsed upper bound can land there unseen." >&2
    echo "Add it to _LIVE_ROOTS above. If it is genuinely not a test root, say why in the" >&2
    echo "header's SCOPE paragraph rather than quietly narrowing the derivation -- an" >&2
    echo "unexplained exclusion is how the gap this check exists to catch got here." >&2
fi

assert "live scan: _LIVE_ROOTS covers EVERY tracked Rust test root" \
    test -z "$_s3_uncovered"

# --- (2) the ratchet holds, in both directions -----------------------------
assert "live scan: the baseline file exists" test -f "$_BASELINE_FILE"

_s3_rc=0
_wallclock_baseline_check "$_BASELINE_FILE" "${_LIVE_ROOTS[@]}" || _s3_rc=$?
assert "live scan: no NEW hand-rolled deadlines or elapsed upper bounds, and no STALE baseline rows (returns 0)" \
    test "$_s3_rc" -eq 0

# --- (3) the non-vacuity floor ---------------------------------------------
# 1331 .rs files today. The floor is deliberately loose: it must catch a root
# list that collapsed, without churning every time a test file is added or
# deleted. It is a LOWER bound, which is the only direction that means
# anything here -- an upper bound would red on a growing tree.
#
# RC IS CAPTURED, NOT INHERITED. A bad root returns the validator's 2, and an
# unguarded command substitution under `set -euo pipefail` would abort the
# whole script on it -- taking Sections 4a-4f and the "Results:" summary with
# it, so the run that found the problem could not report anything, including
# the problem. Verified before this was fixed: breaking the glob to
# `crates/*/testsXX` exited 2 here and printed no summary line at all. The two
# live calls above already capture rc this way; these two did not.
_s3_files_rc=0
_s3_files="$(_wallclock_files_scanned "${_LIVE_ROOTS[@]}")" || _s3_files_rc=$?
assert "live scan: the floor runs -- every root was scannable (returns 0)" \
    test "$_s3_files_rc" -eq 0
# The default keeps a broken scan reporting a second named FAIL rather than a
# bare "integer expression expected" from an empty capture.
assert "live scan: the floor -- at least 1000 .rs files were actually scanned" \
    test "${_s3_files:-0}" -ge 1000

# --- (4) the escape allowlist ----------------------------------------------
# The allowlist as a NUMBER rather than as prose. One escape:
# `far_future_stamp()` in watcher_tests.rs, argued in its own doc comment.
# Changing this line is the reviewable act; see the header's ALLOWLIST
# paragraph before you do. Widening the scan did NOT raise it: the 19
# pre-existing sites went into the baseline, not into escapes.
_ESC_ALLOWLIST_SIZE=1

_s3_esc_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s3_esc_tmpdir")

# Same rc capture as the floor, and for the same reason.
_s3_esc_rc=0
_s3_esc_count="$(_count_rust_wallclock_escapes "${_LIVE_ROOTS[@]}" \
    2>"$_s3_esc_tmpdir/escapes.txt")" || _s3_esc_rc=$?
assert "live scan: the escape counter runs -- every root was scannable (returns 0)" \
    test "$_s3_esc_rc" -eq 0
# -1 rather than 0 as the broken-scan default: 0 is a legitimate answer this
# assertion could one day be asked to accept, and a sentinel that could never
# be right is the honest stand-in for "there is no answer".
_s3_esc_count="${_s3_esc_count:--1}"

if [ "$_s3_esc_count" != "$_ESC_ALLOWLIST_SIZE" ]; then
    echo "" >&2
    echo "Escape-annotated lines across all Rust test roots: $_s3_esc_count; the allowlist says $_ESC_ALLOWLIST_SIZE." >&2
    echo "The annotated lines are:" >&2
    cat "$_s3_esc_tmpdir/escapes.txt" >&2
    echo "" >&2
    echo "An escape suppresses BOTH rules on its line, so each one is a hole in this guard." >&2
    echo "If the new site is genuinely legitimate, argue it where the next reader will find" >&2
    echo "it -- in a doc comment at the site, as far_future_stamp() does -- and raise" >&2
    echo "_ESC_ALLOWLIST_SIZE in this file so the change is visible in review. If a site was" >&2
    echo "REMOVED, lower it. Do not delete this assertion: it is the only thing standing" >&2
    echo "between one argued escape and an allowlist nobody reads." >&2
    echo "NOTE an escape is NOT the remedy for a baselined site -- a baseline row says" >&2
    echo "'debt, must not grow', an escape says 'legitimate'. Do not convert one to the" >&2
    echo "other to quiet a red." >&2
fi

assert "live scan: escape-annotated line count matches the allowlist size" \
    test "$_s3_esc_count" -eq "$_ESC_ALLOWLIST_SIZE"

# ===========================================================================
# Section 4a: FINGERPRINT EMISSION -- hermetic fixtures for
#             `_wallclock_fingerprints <root>...`, the engine the detector
#             above and the baseline ratchet below are both built on.
#
# A fingerprint is `<path> :: <whitespace-trimmed-line-text>`, one record per
# violating LINE, sorted. Line numbers are deliberately erased: a record must
# survive an edit ABOVE its site, or the baseline would red on every unrelated
# insertion. Same rationale as ptodo.rs::fingerprint.
#
# Every fixture here is a mktemp dir, never the real tree.
# ===========================================================================
echo ""
echo "--- Section 4a: fingerprint emission ---"

# ---------------------------------------------------------------------------
# 4a-1: THE COLON TRAP, and the reason this section leads with it. Rust source
#       is full of `::`, so parsing grep's `file:line:text` with `awk -F:` or
#       `cut -d: -f3` MANGLES it -- a dry run of that design emitted
#       `Instant  now()` for this very fixture, i.e. a baseline row that could
#       never match its own source line again. The record must carry the line
#       BYTE-EXACT after trimming, `::` and all.
# ---------------------------------------------------------------------------
_s4a1_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4a1_tmpdir")
_fixture "$_s4a1_tmpdir" "fixture.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'

_s4a1_out="$(_wallclock_fingerprints "$_s4a1_tmpdir" 2>/dev/null || true)"
assert "4a-1: one violating line emits exactly its own record, path and text intact" \
    test "$_s4a1_out" = "$_s4a1_tmpdir/fixture.rs :: let deadline = Instant::now() + Duration::from_secs(5);"

_s4a1_keeps_colons=0
case "$_s4a1_out" in *'Instant::now()'*) _s4a1_keeps_colons=1 ;; esac
assert "4a-1: the emitted record preserves the literal Instant::now() (no colon field-split)" \
    test "$_s4a1_keeps_colons" -eq 1

# ---------------------------------------------------------------------------
# 4a-2: WHITESPACE IS TRIMMED, leading and trailing. Indentation is not part
#       of what a site IS, so a reindentation (an added `if` block, a rustfmt
#       width change) must not churn the baseline. Two fixtures indented
#       differently must produce the SAME text half.
# ---------------------------------------------------------------------------
_s4a2_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4a2_tmpdir")
_fixture "$_s4a2_tmpdir" "a.rs" \
    '            elapsed < Duration::from_secs(2),   '
_fixture "$_s4a2_tmpdir" "b.rs" \
    'elapsed < Duration::from_secs(2),'

_s4a2_out="$(_wallclock_fingerprints "$_s4a2_tmpdir" 2>/dev/null || true)"
assert "4a-2: leading and trailing whitespace are trimmed, so indentation does not churn the baseline" \
    test "$_s4a2_out" = "$(printf '%s\n%s' \
        "$_s4a2_tmpdir/a.rs :: elapsed < Duration::from_secs(2)," \
        "$_s4a2_tmpdir/b.rs :: elapsed < Duration::from_secs(2),")"

# ---------------------------------------------------------------------------
# 4a-3: AN ESCAPE-ANNOTATED LINE EMITS NO RECORD. The escape and the baseline
#       are different claims -- "legitimate, argued at the site" versus
#       "pre-existing debt that must not grow" -- and an escaped site belongs
#       to the FIRST. If escaped lines reached the fingerprint stream they
#       would need baseline rows too, which would blur exactly that
#       distinction and double-count the one argued site.
# ---------------------------------------------------------------------------
_s4a3_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4a3_tmpdir")
_fixture "$_s4a3_tmpdir" "fixture.rs" \
    "    let stamp = Instant::now().checked_add(Duration::from_secs(3600)); // $_ESC_TOKEN -- reason"

_s4a3_out="$(_wallclock_fingerprints "$_s4a3_tmpdir" 2>/dev/null || true)"
assert "4a-3: an escape-annotated violation emits no record" \
    test "$_s4a3_out" = ""

# ---------------------------------------------------------------------------
# 4a-4: NON-.rs FILES ARE OUT OF SCOPE, the fingerprint-stream twin of
#       fixture 2k. This also protects the baseline FILE itself, which holds
#       verbatim copies of every forbidden shape below and would otherwise
#       fingerprint itself into permanent self-reference.
# ---------------------------------------------------------------------------
_s4a4_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4a4_tmpdir")
_fixture "$_s4a4_tmpdir" "fixture.rs.txt" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'

_s4a4_out="$(_wallclock_fingerprints "$_s4a4_tmpdir" 2>/dev/null || true)"
assert "4a-4: a violation in a non-.rs file emits no record" \
    test "$_s4a4_out" = ""

# ---------------------------------------------------------------------------
# 4a-5: RECURSION INTO SUBDIRECTORIES -- the single most load-bearing fixture
#       in this section. The shipped detector globbed `"$dir"/*.rs`, which is
#       NON-recursive, and the real roots this guard now covers are full of
#       subdirectories: 5 of the 19 baselined sites live in one
#       (harness_cli_surface/, harness_traits/, harness_stress_scenarios/).
#       A non-recursive engine would report those as clean AND turn their
#       baseline rows stale -- red for the wrong reason, then green the moment
#       someone "fixed" it by deleting the rows.
# ---------------------------------------------------------------------------
_s4a5_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4a5_tmpdir")
mkdir -p "$_s4a5_tmpdir/nested/deeper"
_fixture "$_s4a5_tmpdir/nested/deeper" "buried.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'

_s4a5_out="$(_wallclock_fingerprints "$_s4a5_tmpdir" 2>/dev/null || true)"
assert "4a-5: a violation in a SUBDIRECTORY of the root is emitted (the scan recurses)" \
    test "$_s4a5_out" = "$_s4a5_tmpdir/nested/deeper/buried.rs :: let deadline = Instant::now() + Duration::from_secs(5);"

# ---------------------------------------------------------------------------
# 4a-6: A CLEAN ROOT EMITS NOTHING AND SUCCEEDS. grep's rc 1 ("no matches")
#       must not be allowed to abort the engine under `set -euo pipefail`, nor
#       to be confused with its rc 2 ("error"), which 4e pins as fatal.
# ---------------------------------------------------------------------------
_s4a6_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4a6_tmpdir")
_fixture "$_s4a6_tmpdir" "clean.rs" \
    '    let t0 = Instant::now();' \
    '    assert!(start.elapsed() >= Duration::from_millis(150), "should block");'

_s4a6_rc=0
_s4a6_out="$(_wallclock_fingerprints "$_s4a6_tmpdir" 2>/dev/null)" || _s4a6_rc=$?
assert "4a-6: a clean root emits nothing and returns 0 (grep rc 1 is not an error)" \
    test "$_s4a6_rc" -eq 0
assert "4a-6: a clean root's output is empty" \
    test "$_s4a6_out" = ""

# ---------------------------------------------------------------------------
# 4a-7: MULTIPLE ROOTS in one call, and the output is SORTED across all of
#       them -- not concatenated per root. `comm` below compares two sorted
#       streams, so an engine that emitted root-major order would desynchronise
#       the comparison and report phantom `+`/`-` records.
# ---------------------------------------------------------------------------
_s4a7_r1="$(mktemp -d)"; _TMPDIRS+=("$_s4a7_r1")
_s4a7_r2="$(mktemp -d)"; _TMPDIRS+=("$_s4a7_r2")
_fixture "$_s4a7_r1" "z.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(9);'
_fixture "$_s4a7_r2" "a.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(1);'

_s4a7_out="$(_wallclock_fingerprints "$_s4a7_r1" "$_s4a7_r2" 2>/dev/null || true)"
assert "4a-7: two roots in one call emit both records, globally sorted" \
    test "$_s4a7_out" = "$(printf '%s\n%s' \
        "$_s4a7_r1/z.rs :: let deadline = Instant::now() + Duration::from_secs(9);" \
        "$_s4a7_r2/a.rs :: let deadline = Instant::now() + Duration::from_secs(1);" | LC_ALL=C sort)"

# ===========================================================================
# Section 4b: FINGERPRINTS ARE A MULTISET, NOT A SET.
#
# This is the section that stops a whole class of silent regression, and it is
# NOT hypothetical -- the live tree has duplicates today:
#   * `while Instant::now() < deadline {` appears 3x in
#     crates/reify-audit/tests/jcodemunch_session_live.rs
#   * `elapsed < Duration::from_secs(10),` appears 2x in
#     crates/reify-fdm/tests/slice.rs
# Line numbers are erased from a fingerprint, so those copies are BYTE-EQUAL
# records. Under a `sort -u` emission path, or any set-membership comparison,
# the baseline would carry one row for each and a FOURTH and THIRD copy could
# land fully green. The ratchet would then be counting distinct SPELLINGS
# rather than sites, which is not what it claims to guard.
#
# Note this is a deliberate divergence from ptodo.rs::fingerprint, which
# collapses identical markers on purpose; see the comment at the `sort`.
# ===========================================================================
echo ""
echo "--- Section 4b: fingerprints are a multiset ---"

# ---------------------------------------------------------------------------
# 4b-1: the live shape -- the SAME line twice in one file and a third time in
#       a sibling. Three occurrences, three records.
# ---------------------------------------------------------------------------
_s4b1_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4b1_tmpdir")
_fixture "$_s4b1_tmpdir" "a.rs" \
    '    while Instant::now() < deadline {' \
    '        std::thread::sleep(RUN_POLL_INTERVAL);' \
    '    while Instant::now() < deadline {'
_fixture "$_s4b1_tmpdir" "b.rs" \
    '    while Instant::now() < deadline {'

_s4b1_out="$(_wallclock_fingerprints "$_s4b1_tmpdir" 2>/dev/null || true)"
_s4b1_n="$(printf '%s\n' "$_s4b1_out" | grep -c . || true)"
assert "4b-1: three occurrences of one line across two files emit THREE records, not one" \
    test "$_s4b1_n" -eq 3

# ---------------------------------------------------------------------------
# 4b-2: two byte-identical lines inside a SINGLE file. Asserted separately
#       from 4b-1 because the two collapse differently: a per-file `sort -u`
#       would pass 4b-1 (the paths differ) and fail here, while a global one
#       fails both.
# ---------------------------------------------------------------------------
_s4b2_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4b2_tmpdir")
_fixture "$_s4b2_tmpdir" "slice.rs" \
    '        elapsed < Duration::from_secs(10),' \
    '        "first bound"' \
    '        elapsed < Duration::from_secs(10),'

_s4b2_out="$(_wallclock_fingerprints "$_s4b2_tmpdir" 2>/dev/null || true)"
_s4b2_n="$(printf '%s\n' "$_s4b2_out" | grep -c . || true)"
assert "4b-2: two identical lines within one file emit TWO records, not one" \
    test "$_s4b2_n" -eq 2

# ---------------------------------------------------------------------------
# 4b-3: duplicates that differ only by INDENTATION collapse to the same
#       record and are still counted twice. Trimming (4a-2) and multiset
#       counting interact here: trimming makes them byte-equal, so this is
#       exactly the pair a set would eat.
# ---------------------------------------------------------------------------
_s4b3_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4b3_tmpdir")
_fixture "$_s4b3_tmpdir" "c.rs" \
    '    elapsed < Duration::from_secs(10),' \
    '                elapsed < Duration::from_secs(10),'

_s4b3_out="$(_wallclock_fingerprints "$_s4b3_tmpdir" 2>/dev/null || true)"
_s4b3_n="$(printf '%s\n' "$_s4b3_out" | grep -c . || true)"
assert "4b-3: two occurrences equal only after trimming are still counted twice" \
    test "$_s4b3_n" -eq 2

# ===========================================================================
# Section 4c: BASELINE CHECK -- the NEW direction.
#
# `_wallclock_baseline_check <baseline-file> <root>...` compares the live
# fingerprint multiset against a committed one. A record present LIVE but
# absent from the baseline is a NEW violation: someone hand-rolled a deadline
# or an elapsed upper bound today, and the gate must red.
#
# SILENCE ON SUCCESS IS AN ASSERTION HERE, NOT A STYLE POINT. test_helpers.sh's
# assert dumps a checker's captured output only on FAIL, so a passing run of
# this suite is byte-stable -- and a function that chattered on success would
# put 19 baselined records into every green run's log, training every reader
# to ignore exactly the lines that matter when it eventually reds.
#
# The STALE direction (a baseline row matching nothing live) is Section 4d.
# ===========================================================================
echo ""
echo "--- Section 4c: baseline check, NEW direction ---"

# ---------------------------------------------------------------------------
# 4c-1: EXACT MATCH -- live multiset equals the baseline. rc 0, and not one
#       byte on stdout or stderr.
# ---------------------------------------------------------------------------
_s4c1_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4c1_tmpdir")
mkdir -p "$_s4c1_tmpdir/src"
_fixture "$_s4c1_tmpdir/src" "a.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'
_fixture "$_s4c1_tmpdir" "baseline.txt" \
    "$_s4c1_tmpdir/src/a.rs :: let deadline = Instant::now() + Duration::from_secs(5);"

_s4c1_rc=0
_wallclock_baseline_check "$_s4c1_tmpdir/baseline.txt" "$_s4c1_tmpdir/src" \
    > "$_s4c1_tmpdir/out.txt" 2>&1 || _s4c1_rc=$?
assert "4c-1: live multiset equal to the baseline returns 0" \
    test "$_s4c1_rc" -eq 0
assert "4c-1: an exact match is SILENT on stdout and stderr" \
    test "$(cat "$_s4c1_tmpdir/out.txt")" = ""

# ---------------------------------------------------------------------------
# 4c-2: A NEW VIOLATION -- live carries a record the baseline does not. rc 1,
#       and the offending record is named, prefixed `+`, so the reader is told
#       WHICH line to fix rather than being handed a bare red.
# ---------------------------------------------------------------------------
_s4c2_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4c2_tmpdir")
mkdir -p "$_s4c2_tmpdir/src"
_fixture "$_s4c2_tmpdir/src" "a.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(5);' \
    '    assert!(elapsed < Duration::from_secs(2), "brand new");'
_fixture "$_s4c2_tmpdir" "baseline.txt" \
    "$_s4c2_tmpdir/src/a.rs :: let deadline = Instant::now() + Duration::from_secs(5);"

_s4c2_rc=0
_wallclock_baseline_check "$_s4c2_tmpdir/baseline.txt" "$_s4c2_tmpdir/src" \
    > "$_s4c2_tmpdir/out.txt" 2>&1 || _s4c2_rc=$?
assert "4c-2: a live record absent from the baseline returns 1" \
    test "$_s4c2_rc" -eq 1

_s4c2_named=0
case "$(cat "$_s4c2_tmpdir/out.txt")" in
    *'+ '"$_s4c2_tmpdir"'/src/a.rs :: assert!(elapsed < Duration::from_secs(2), "brand new");'*)
        _s4c2_named=1 ;;
esac
assert "4c-2: the new record is reported by name, prefixed +" \
    test "$_s4c2_named" -eq 1

_s4c2_quiet=0
case "$(cat "$_s4c2_tmpdir/out.txt")" in
    *'+ '"$_s4c2_tmpdir"'/src/a.rs :: let deadline'*) ;;
    *) _s4c2_quiet=1 ;;
esac
assert "4c-2: the already-baselined record is NOT reported as new" \
    test "$_s4c2_quiet" -eq 1

# ---------------------------------------------------------------------------
# 4c-3: BASELINE COMMENTS AND BLANK LINES ARE IGNORED. The committed baseline
#       opens with a ~55-line `#` header explaining what a row means; if the
#       loader counted those as rows, every one of them would be a permanent
#       stale record and the gate could never be green. Blank lines likewise.
#       Same stripping rule as tests/infra/harness-layout-baseline.manifest
#       (and deliberately NOT ptodo-baseline.txt, which forbids comments).
# ---------------------------------------------------------------------------
_s4c3_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4c3_tmpdir")
mkdir -p "$_s4c3_tmpdir/src"
_fixture "$_s4c3_tmpdir/src" "a.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'
_fixture "$_s4c3_tmpdir" "baseline.txt" \
    '# a header comment' \
    '' \
    '    # an INDENTED comment' \
    '   ' \
    "$_s4c3_tmpdir/src/a.rs :: let deadline = Instant::now() + Duration::from_secs(5);" \
    ''

_s4c3_rc=0
_wallclock_baseline_check "$_s4c3_tmpdir/baseline.txt" "$_s4c3_tmpdir/src" \
    > "$_s4c3_tmpdir/out.txt" 2>&1 || _s4c3_rc=$?
assert "4c-3: comment and blank lines in the baseline are not rows (returns 0)" \
    test "$_s4c3_rc" -eq 0
assert "4c-3: a baseline with a comment header stays silent on an exact match" \
    test "$(cat "$_s4c3_tmpdir/out.txt")" = ""

# ---------------------------------------------------------------------------
# 4c-4: AN EMPTY BASELINE against a violating tree. The end state this ratchet
#       is aimed at is a DRAINED baseline, so the zero-row case must behave --
#       every live record is new, and none of them may be swallowed by an
#       "empty baseline means nothing to compare" short circuit.
# ---------------------------------------------------------------------------
_s4c4_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4c4_tmpdir")
mkdir -p "$_s4c4_tmpdir/src"
_fixture "$_s4c4_tmpdir/src" "a.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'
_fixture "$_s4c4_tmpdir" "baseline.txt" \
    '# nothing baselined'

_s4c4_rc=0
_wallclock_baseline_check "$_s4c4_tmpdir/baseline.txt" "$_s4c4_tmpdir/src" \
    > "$_s4c4_tmpdir/out.txt" 2>&1 || _s4c4_rc=$?
assert "4c-4: an empty baseline reports every live record as new (returns 1)" \
    test "$_s4c4_rc" -eq 1

# ===========================================================================
# Section 4d: BASELINE CHECK -- the STALE direction, and the multiset
#             property end to end.
#
# A baseline row matching nothing live is ALSO a red. That is what makes this
# ratchet SHRINK-ONLY rather than a pile that accretes dead rows: fix a site
# and you must delete its row in the same diff, so the file can only get
# smaller. It is the harness-layout-baseline.manifest precedent (which reds on
# orphan rows), deliberately NOT ptodo-baseline.txt's subset-only rule -- that
# one has no forcing function to drain it, a limitation #6859 accepts openly
# and this guard need not inherit at 19 hand-auditable rows.
#
# The stale direction also happens to be what stops the SUBSET-ORACLE VACUITY
# hole today: a scan that silently matched nothing would turn all 19 rows
# stale and red. That cover EVAPORATES once the baseline is drained to zero,
# which is the goal state -- hence the explicit floor in Section 4e.
# ===========================================================================
echo ""
echo "--- Section 4d: baseline check, STALE direction ---"

# ---------------------------------------------------------------------------
# 4d-1: A FIXED SITE. The baseline names a record the live scan no longer
#       produces. rc 1, and the row is named prefixed `-`, so the reader is
#       told to delete it rather than left guessing why a clean tree is red.
# ---------------------------------------------------------------------------
_s4d1_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4d1_tmpdir")
mkdir -p "$_s4d1_tmpdir/src"
_fixture "$_s4d1_tmpdir/src" "a.rs" \
    '    let t0 = Instant::now();'
_fixture "$_s4d1_tmpdir" "baseline.txt" \
    "$_s4d1_tmpdir/src/a.rs :: let deadline = Instant::now() + Duration::from_secs(5);"

_s4d1_rc=0
_wallclock_baseline_check "$_s4d1_tmpdir/baseline.txt" "$_s4d1_tmpdir/src" \
    > "$_s4d1_tmpdir/out.txt" 2>&1 || _s4d1_rc=$?
assert "4d-1: a baseline row matching nothing live returns 1 (the ratchet shrinks)" \
    test "$_s4d1_rc" -eq 1

_s4d1_named=0
case "$(cat "$_s4d1_tmpdir/out.txt")" in
    *'- '"$_s4d1_tmpdir"'/src/a.rs :: let deadline = Instant::now() + Duration::from_secs(5);'*)
        _s4d1_named=1 ;;
esac
assert "4d-1: the stale row is reported by name, prefixed -" \
    test "$_s4d1_named" -eq 1

# ---------------------------------------------------------------------------
# 4d-2: A DELETED FILE is the same shape as a fixed site. Pinned separately
#       because a file that no longer exists is the likelier way a row goes
#       stale (a rename, a test moved between crates -- #7365 moved
#       cli_lsp_protocol.rs and 4 of the 19 rows with it), and an
#       implementation keyed on per-file comparison rather than on the whole
#       multiset could pass 4d-1 and miss this.
# ---------------------------------------------------------------------------
_s4d2_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4d2_tmpdir")
mkdir -p "$_s4d2_tmpdir/src"
_fixture "$_s4d2_tmpdir/src" "still_here.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'
_fixture "$_s4d2_tmpdir" "baseline.txt" \
    "$_s4d2_tmpdir/src/still_here.rs :: let deadline = Instant::now() + Duration::from_secs(5);" \
    "$_s4d2_tmpdir/src/renamed_away.rs :: let deadline = Instant::now() + Duration::from_secs(9);"

_s4d2_rc=0
_wallclock_baseline_check "$_s4d2_tmpdir/baseline.txt" "$_s4d2_tmpdir/src" \
    > "$_s4d2_tmpdir/out.txt" 2>&1 || _s4d2_rc=$?
assert "4d-2: a row naming a file that no longer exists is stale (returns 1)" \
    test "$_s4d2_rc" -eq 1

# ---------------------------------------------------------------------------
# 4d-3: MULTISET, END TO END, NEW direction. The baseline carries a record
#       TWICE; two live copies are clean, and a THIRD is reported as new. This
#       is the whole reason Section 4b exists, asserted through the ratchet
#       rather than through the engine alone -- a set-based comparison here
#       would pass 4b (the engine emits three records) and still swallow the
#       third copy at the comparison.
# ---------------------------------------------------------------------------
_s4d3_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4d3_tmpdir")
mkdir -p "$_s4d3_tmpdir/src"
_fixture "$_s4d3_tmpdir/src" "a.rs" \
    '    while Instant::now() < deadline {' \
    '    while Instant::now() < deadline {'
_fixture "$_s4d3_tmpdir" "baseline.txt" \
    "$_s4d3_tmpdir/src/a.rs :: while Instant::now() < deadline {" \
    "$_s4d3_tmpdir/src/a.rs :: while Instant::now() < deadline {"

_s4d3_rc=0
_wallclock_baseline_check "$_s4d3_tmpdir/baseline.txt" "$_s4d3_tmpdir/src" \
    > "$_s4d3_tmpdir/out2.txt" 2>&1 || _s4d3_rc=$?
assert "4d-3: two baselined copies against two live copies is clean (returns 0)" \
    test "$_s4d3_rc" -eq 0

_fixture "$_s4d3_tmpdir/src" "a.rs" \
    '    while Instant::now() < deadline {' \
    '    while Instant::now() < deadline {' \
    '    while Instant::now() < deadline {'

_s4d3b_rc=0
_wallclock_baseline_check "$_s4d3_tmpdir/baseline.txt" "$_s4d3_tmpdir/src" \
    > "$_s4d3_tmpdir/out3.txt" 2>&1 || _s4d3b_rc=$?
assert "4d-3: a THIRD copy of a 2x-baselined record is reported as new (returns 1)" \
    test "$_s4d3b_rc" -eq 1

_s4d3_once=0
_s4d3_plus="$(grep -c '^  + ' "$_s4d3_tmpdir/out3.txt" || true)"
[ "$_s4d3_plus" = "1" ] && _s4d3_once=1
assert "4d-3: exactly ONE record is reported new, not all three copies" \
    test "$_s4d3_once" -eq 1

# ---------------------------------------------------------------------------
# 4d-4: MULTISET, END TO END, STALE direction -- the mirror of 4d-3. Two
#       baselined copies against ONE live copy leaves exactly one stale row.
#       Without this, draining a duplicate pair by deleting only one of its
#       two rows would go unnoticed.
# ---------------------------------------------------------------------------
_s4d4_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4d4_tmpdir")
mkdir -p "$_s4d4_tmpdir/src"
_fixture "$_s4d4_tmpdir/src" "a.rs" \
    '        elapsed < Duration::from_secs(10),'
_fixture "$_s4d4_tmpdir" "baseline.txt" \
    "$_s4d4_tmpdir/src/a.rs :: elapsed < Duration::from_secs(10)," \
    "$_s4d4_tmpdir/src/a.rs :: elapsed < Duration::from_secs(10),"

_s4d4_rc=0
_wallclock_baseline_check "$_s4d4_tmpdir/baseline.txt" "$_s4d4_tmpdir/src" \
    > "$_s4d4_tmpdir/out.txt" 2>&1 || _s4d4_rc=$?
assert "4d-4: two baselined copies against one live copy returns 1" \
    test "$_s4d4_rc" -eq 1

_s4d4_once=0
_s4d4_minus="$(grep -c '^  - ' "$_s4d4_tmpdir/out.txt" || true)"
[ "$_s4d4_minus" = "1" ] && _s4d4_once=1
assert "4d-4: exactly ONE row is reported stale, not both copies" \
    test "$_s4d4_once" -eq 1

# ---------------------------------------------------------------------------
# 4d-5: BOTH DIRECTIONS AT ONCE. A tree that fixed one site and added another
#       must report a `-` AND a `+` and red -- not net out to "one row in, one
#       row out, nothing to see". A count-based implementation would pass every
#       fixture above and fail exactly here.
# ---------------------------------------------------------------------------
_s4d5_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4d5_tmpdir")
mkdir -p "$_s4d5_tmpdir/src"
_fixture "$_s4d5_tmpdir/src" "a.rs" \
    '    assert!(elapsed < Duration::from_secs(2), "brand new");'
_fixture "$_s4d5_tmpdir" "baseline.txt" \
    "$_s4d5_tmpdir/src/a.rs :: let deadline = Instant::now() + Duration::from_secs(5);"

_s4d5_rc=0
_wallclock_baseline_check "$_s4d5_tmpdir/baseline.txt" "$_s4d5_tmpdir/src" \
    > "$_s4d5_tmpdir/out.txt" 2>&1 || _s4d5_rc=$?
assert "4d-5: one site fixed and one added returns 1 (the two do not net out)" \
    test "$_s4d5_rc" -eq 1

_s4d5_both=0
_s4d5_np="$(grep -c '^  + ' "$_s4d5_tmpdir/out.txt" || true)"
_s4d5_nm="$(grep -c '^  - ' "$_s4d5_tmpdir/out.txt" || true)"
[ "$_s4d5_np" = "1" ] && [ "$_s4d5_nm" = "1" ] && _s4d5_both=1
assert "4d-5: both directions are reported -- one + record and one - row" \
    test "$_s4d5_both" -eq 1

# ===========================================================================
# Section 4e: THE VACUITY FLOOR, and roots that do not exist.
#
# WHY THIS SECTION IS MANDATORY, not defensive padding. A subset oracle is
# TRIVIALLY SATISFIED BY THE EMPTY SET: a scan that silently visited no files
# at all reports "no new violations" and passes. Today the stale direction
# covers that by accident -- scanning nothing turns all 19 baseline rows stale
# and reds -- but that cover EVAPORATES the moment the baseline is drained to
# zero, which is precisely the state this ratchet exists to reach. At that
# point a typo in the root list would leave the guard permanently, silently
# green while guarding nothing.
#
# So the floor is explicit and independent of the baseline: COUNT the .rs
# files actually visited, and treat a root that does not exist as a HARD
# ERROR rather than as an empty contribution. grep's own exit codes already
# draw that line -- an empty directory is rc 1 (clean) and a missing one is
# rc 2 (error) -- and this section pins that the distinction survives all the
# way out to the caller.
# ===========================================================================
echo ""
echo "--- Section 4e: vacuity floor and root validation ---"

# ---------------------------------------------------------------------------
# 4e-1: a populated root counts the .rs files it holds.
# ---------------------------------------------------------------------------
_s4e1_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e1_tmpdir")
_fixture "$_s4e1_tmpdir" "a.rs" '    let t0 = Instant::now();'
_fixture "$_s4e1_tmpdir" "b.rs" '    fn main() {}'

_s4e1_n="$(_wallclock_files_scanned "$_s4e1_tmpdir" 2>/dev/null || echo "ERR")"
assert "4e-1: a root holding two .rs files counts 2" \
    test "$_s4e1_n" -eq 2

# ---------------------------------------------------------------------------
# 4e-2: an EMPTY root counts 0 and is NOT an error. An existing-but-empty
#       directory is a real state (a crate whose tests were all consolidated
#       away), and conflating it with a missing one would make the guard red
#       on a legitimate tree.
# ---------------------------------------------------------------------------
_s4e2_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e2_tmpdir")

_s4e2_rc=0
_s4e2_n="$(_wallclock_files_scanned "$_s4e2_tmpdir" 2>/dev/null)" || _s4e2_rc=$?
assert "4e-2: an empty root counts 0" \
    test "$_s4e2_n" -eq 0
assert "4e-2: an empty root is not an error (returns 0)" \
    test "$_s4e2_rc" -eq 0

# ---------------------------------------------------------------------------
# 4e-3: counts RECURSIVELY and ACROSS ROOTS, and counts only .rs. The live
#       floor is asserted over 36 roots full of subdirectories, so a
#       non-recursive or first-root-only count would report a number far
#       below the real one and could satisfy a floor it should not.
# ---------------------------------------------------------------------------
_s4e3_r1="$(mktemp -d)"; _TMPDIRS+=("$_s4e3_r1")
_s4e3_r2="$(mktemp -d)"; _TMPDIRS+=("$_s4e3_r2")
mkdir -p "$_s4e3_r1/nested/deeper"
_fixture "$_s4e3_r1" "top.rs" '    fn main() {}'
_fixture "$_s4e3_r1/nested" "mid.rs" '    fn main() {}'
_fixture "$_s4e3_r1/nested/deeper" "low.rs" '    fn main() {}'
_fixture "$_s4e3_r1" "notrust.txt" '    fn main() {}'
_fixture "$_s4e3_r2" "other.rs" '    fn main() {}'

_s4e3_n="$(_wallclock_files_scanned "$_s4e3_r1" "$_s4e3_r2" 2>/dev/null || echo "ERR")"
assert "4e-3: three nested .rs plus one in a second root count 4, ignoring the .txt" \
    test "$_s4e3_n" -eq 4

# ---------------------------------------------------------------------------
# 4e-4: A ROOT THAT DOES NOT EXIST IS A HARD ERROR. This is the assertion the
#       whole section is for. It must never be a silent zero contribution --
#       that is exactly how a typo'd root list plus a drained baseline goes
#       green while scanning nothing.
# ---------------------------------------------------------------------------
_s4e4_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e4_tmpdir")

_s4e4_rc=0
_wallclock_files_scanned "$_s4e4_tmpdir/no-such-root" \
    > "$_s4e4_tmpdir/out.txt" 2>&1 || _s4e4_rc=$?
assert "4e-4: a root that does not exist is a hard error (returns non-zero)" \
    test "$_s4e4_rc" -ne 0

_s4e4_named=0
case "$(cat "$_s4e4_tmpdir/out.txt")" in
    *"$_s4e4_tmpdir/no-such-root"*) _s4e4_named=1 ;;
esac
assert "4e-4: the error names the missing root" \
    test "$_s4e4_named" -eq 1

# ---------------------------------------------------------------------------
# 4e-5: THE SAME VALIDATION IN THE FINGERPRINT ENGINE. The floor is only half
#       the protection: if a bad root reached the comparison it would
#       contribute no records, and the NEW direction would report "clean" for
#       it. Asserted on the engine directly so the guarantee cannot be lost by
#       someone calling it without the counter.
# ---------------------------------------------------------------------------
_s4e5_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e5_tmpdir")

_s4e5_rc=0
_wallclock_fingerprints "$_s4e5_tmpdir/no-such-root" \
    > "$_s4e5_tmpdir/out.txt" 2>&1 || _s4e5_rc=$?
assert "4e-5: the fingerprint engine rejects a root that does not exist (non-zero)" \
    test "$_s4e5_rc" -ne 0

_s4e5_named=0
case "$(cat "$_s4e5_tmpdir/out.txt")" in
    *"$_s4e5_tmpdir/no-such-root"*) _s4e5_named=1 ;;
esac
assert "4e-5: the engine's error names the missing root" \
    test "$_s4e5_named" -eq 1

# ---------------------------------------------------------------------------
# 4e-6: AND OUT THROUGH THE RATCHET. The end-to-end statement: a bad root can
#       never reach `_wallclock_baseline_check`'s caller looking like "no new
#       violations". rc must be neither 0 (clean) nor 1 (a real difference),
#       so a caller can tell a broken scan from a failing one.
# ---------------------------------------------------------------------------
_s4e6_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e6_tmpdir")
_fixture "$_s4e6_tmpdir" "baseline.txt" '# empty'

_s4e6_rc=0
_wallclock_baseline_check "$_s4e6_tmpdir/baseline.txt" "$_s4e6_tmpdir/no-such-root" \
    > "$_s4e6_tmpdir/out.txt" 2>&1 || _s4e6_rc=$?
assert "4e-6: a bad root propagates out of the ratchet as rc 2, not 0 and not 1" \
    test "$_s4e6_rc" -eq 2

# ---------------------------------------------------------------------------
# 4e-7: A ROOT THAT IS A FILE, not a directory. The realistic way the glob in
#       Section 3 goes wrong is an UNMATCHED glob, which bash leaves as the
#       literal pattern string -- a path that does not exist, covered by 4e-4.
#       A plain file is the other near-miss (a root list that lost its `/tests`
#       suffix), and it must be rejected rather than quietly scanned as one
#       file, because scanning one file where a whole tree was intended is the
#       same vacuity hole wearing a different hat.
# ---------------------------------------------------------------------------
_s4e7_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e7_tmpdir")
_fixture "$_s4e7_tmpdir" "notadir.rs" '    let t0 = Instant::now();'

_s4e7_rc=0
_wallclock_files_scanned "$_s4e7_tmpdir/notadir.rs" \
    > "$_s4e7_tmpdir/out.txt" 2>&1 || _s4e7_rc=$?
assert "4e-7: a root that is a file, not a directory, is a hard error (non-zero)" \
    test "$_s4e7_rc" -ne 0

# ---------------------------------------------------------------------------
# 4e-8: A FILE GREP WOULD CLASSIFY AS BINARY IS STILL SCANNED. The floor above
#       counts a root's .rs files with `find`, so it counts a file whether or
#       not grep can read it -- which leaves room for a skip the floor cannot
#       see. GNU grep stops at the first NUL byte in a file and reports
#       "binary file matches" on STDERR, contributing NOTHING to the captured
#       record stream: the guard would then say "we looked, and there were
#       none" about a file it never read. That is the same vacuity hole this
#       section exists to close, one layer down -- and a SILENT one, which is
#       against the whole design. `-a` is what makes the scan total.
#
#       A NUL in a .rs file is not a realistic thing to write by hand. It is
#       a realistic thing to arrive by accident (a bad merge, a truncated
#       checkout, a generated file), and the failure direction is the bad one:
#       the violation disappears rather than announcing itself.
# ---------------------------------------------------------------------------
_s4e8_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e8_tmpdir")
printf 'fn stray() {}\n\0\nlet deadline = Instant::now() + Duration::from_secs(5);\n' \
    > "$_s4e8_tmpdir/nul.rs"

_s4e8_out="$(_wallclock_fingerprints "$_s4e8_tmpdir" 2>/dev/null || true)"
assert "4e-8: a violation after a NUL byte is still fingerprinted, not silently skipped" \
    test "$_s4e8_out" = "$_s4e8_tmpdir/nul.rs :: let deadline = Instant::now() + Duration::from_secs(5);"

# ---------------------------------------------------------------------------
# 4e-9: THE SAME FOR THE ESCAPE COUNTER, where the skip is worse. A missed
#       violation is a missed red; a missed ESCAPE moves the count AWAY from
#       _ESC_ALLOWLIST_SIZE, and two cancelling skips would move it back
#       toward the allowlist -- a number Section 3 trusts to be exact.
# ---------------------------------------------------------------------------
_s4e9_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e9_tmpdir")
printf 'fn stray() {}\n\0\nlet a = Instant::now() + Duration::from_secs(1); // %s -- reason\n' \
    "$_ESC_TOKEN" > "$_s4e9_tmpdir/nul.rs"

_s4e9_count="$(_count_rust_wallclock_escapes "$_s4e9_tmpdir" 2>/dev/null)"
assert "4e-9: an escape after a NUL byte is still counted" \
    test "$_s4e9_count" -eq 1

# ---------------------------------------------------------------------------
# 4e-10: A ROOT NESTED INSIDE ANOTHER IS A HARD ERROR. Every root is scanned
#        RECURSIVELY, so a nested entry is not merely redundant -- grep visits
#        its files once per root, and the engine emits each violating line
#        TWICE. The baseline comparison is a deliberate MULTISET, so those
#        second copies surface as `+` records indistinguishable from a real new
#        violation, and the reader is sent hunting for a line that was already
#        baselined.
#
#        Nothing else can catch this. `_wallclock_assert_roots` used to check
#        only that each root was a directory, and the completeness check in
#        Section 3 treats a descendant as COVERED by design (4f-2) -- so it
#        will never report a nested root and can never warn against adding one.
#        Meanwhile Section 3's own remediation text says "Add it to _LIVE_ROOTS
#        above", which is exactly the hand-edit that would introduce one.
# ---------------------------------------------------------------------------
_s4e10_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e10_tmpdir")
mkdir -p "$_s4e10_tmpdir/src/sub"
_fixture "$_s4e10_tmpdir/src/sub" "a.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'

_s4e10_rc=0
_wallclock_fingerprints "$_s4e10_tmpdir/src" "$_s4e10_tmpdir/src/sub" \
    > "$_s4e10_tmpdir/out.txt" 2>&1 || _s4e10_rc=$?
assert "4e-10: a root nested inside another is a hard error, not a double count" \
    test "$_s4e10_rc" -eq 2

_s4e10_named=0
case "$(cat "$_s4e10_tmpdir/out.txt")" in
    *"$_s4e10_tmpdir/src/sub"*) _s4e10_named=1 ;;
esac
assert "4e-10: the error names the nested root" \
    test "$_s4e10_named" -eq 1

# A REFUSED SCAN EMITS NO RECORDS. Without this the assertion above passes in
# the RED state by accident -- the doubled records themselves contain the
# nested path. ` :: ` is the fingerprint separator, so its absence says the
# scan was refused rather than performed and reported.
_s4e10_records=0
case "$(cat "$_s4e10_tmpdir/out.txt")" in
    *" :: "*) _s4e10_records=1 ;;
esac
assert "4e-10: the refused scan emits no records at all, doubled or otherwise" \
    test "$_s4e10_records" -eq 0

# ---------------------------------------------------------------------------
# 4e-11: THE SAME ROOT TWICE, which is the degenerate case of 4e-10 and the
#        likelier typo -- a copy-paste into the list, or a glob whose expansion
#        already contains an enumerated entry. It doubles every record in the
#        subtree, so it is rejected on the same terms.
#
#        THE PREFIX TRAP is pinned here too, in the safe direction: `src2` is
#        NOT nested in `src`, and rejecting it would red a list that is fine.
#        Same quoting as _wallclock_uncovered_test_roots -- a root is a path,
#        not a pattern, so only the trailing `/` is what makes one a parent.
# ---------------------------------------------------------------------------
_s4e11_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e11_tmpdir")
mkdir -p "$_s4e11_tmpdir/src" "$_s4e11_tmpdir/src2"
_fixture "$_s4e11_tmpdir/src" "a.rs" '    fn main() {}'
_fixture "$_s4e11_tmpdir/src2" "b.rs" '    fn main() {}'

_s4e11_rc=0
_wallclock_files_scanned "$_s4e11_tmpdir/src" "$_s4e11_tmpdir/src" \
    > /dev/null 2>&1 || _s4e11_rc=$?
assert "4e-11: the same root listed twice is a hard error" \
    test "$_s4e11_rc" -eq 2

_s4e11_sib="$(_wallclock_files_scanned "$_s4e11_tmpdir/src" "$_s4e11_tmpdir/src2" \
    2>/dev/null || echo "ERR")"
assert "4e-11: src2 is NOT nested in src -- a string prefix is not a parent" \
    test "$_s4e11_sib" -eq 2

# ---------------------------------------------------------------------------
# 4e-12: AN UNREADABLE BASELINE IS A HARD ERROR. The other input to the ratchet
#        is the baseline FILE, and it has the same vacuity failure as a bad
#        root: if it cannot be read, "no rows" and "no baseline" become the
#        same state.
#
#        `_wallclock_baseline_check` loads rows with a single `grep -v` for
#        exactly this reason -- one rc, so grep's error 2 ("cannot read the
#        baseline") stays distinguishable from its 1 ("the baseline has no
#        rows") -- and its doc comment argues at length that a blanket
#        `|| true` there would collapse the first into a clean pass. Nothing
#        pinned it. 4e-6 covers a bad ROOT through the ratchet; 4c-3/4c-4 only
#        ever hand it a readable file.
#
#        Verified against the regression it exists to catch, rather than
#        assumed: replacing the loader's rc handling with a blanket `|| true`
#        turns this fixture and 4e-13 RED and leaves the other 108 assertions
#        in this file green -- so the pair is the only thing standing between
#        that refactor and a guard that passes while reading nothing.
# ---------------------------------------------------------------------------
_s4e12_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e12_tmpdir")
mkdir -p "$_s4e12_tmpdir/src"
_fixture "$_s4e12_tmpdir/src" "a.rs" \
    '    let deadline = Instant::now() + Duration::from_secs(5);'

_s4e12_rc=0
_wallclock_baseline_check "$_s4e12_tmpdir/no-such-baseline.txt" "$_s4e12_tmpdir/src" \
    > /dev/null 2>&1 || _s4e12_rc=$?
assert "4e-12: a baseline that cannot be read is rc 2 -- not 0, and not 1 either" \
    test "$_s4e12_rc" -eq 2

# ---------------------------------------------------------------------------
# 4e-13: THE SAME, OVER A CLEAN TREE -- which is where the vacuous green
#        actually lands, and so the sharper half of the pair. With a violating
#        tree above, a `|| true` loader still reports the live record as `+`
#        and reds for the wrong reason; with NOTHING to report, both sides are
#        empty and it returns a confident 0.
#
#        That is the goal state of this ratchet, which is what makes it worth
#        an assertion: once the baseline is drained, "the file is missing" and
#        "the file is empty" differ by nothing except this rc.
# ---------------------------------------------------------------------------
_s4e13_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s4e13_tmpdir")
mkdir -p "$_s4e13_tmpdir/src"
_fixture "$_s4e13_tmpdir/src" "clean.rs" '    let t0 = Instant::now();'

_s4e13_rc=0
_wallclock_baseline_check "$_s4e13_tmpdir/no-such-baseline.txt" "$_s4e13_tmpdir/src" \
    > /dev/null 2>&1 || _s4e13_rc=$?
assert "4e-13: a missing baseline over a CLEAN tree is rc 2, never a vacuous 0" \
    test "$_s4e13_rc" -eq 2

# ===========================================================================
# Section 4f: ROOT-SET COMPLETENESS -- is the root LIST itself right?
#
# WHY THIS SECTION EXISTS (#6597 review). Every other assertion in this file
# takes `_LIVE_ROOTS` as given and checks what happens INSIDE it. That left one
# failure nothing here could see: a genuine Rust test root simply MISSING from
# the list. The review found exactly that -- gui/src-tauri/src/debug_server/tests,
# 31 #[test]/#[tokio::test] fns split out of debug_server.rs's `mod tests` for
# size alone, and a CHILD of debug_server::tests, so recursing
# gui/src-tauri/src/tests never reaches it -- in the very crate that produced
# all four historical flakes. Adding one path by hand is how that gap got here,
# so the fix is the move this guard already made for _ESC_ALLOWLIST_SIZE: the
# header's "EVERY Rust TEST root" claim becomes CHECKED against the tree
# (Section 3), and this section pins the primitive that checks it.
#
# `_wallclock_uncovered_test_roots <root>...` reads candidate roots on stdin and
# prints the UNCOVERED ones on stdout. COVERAGE IS BY PREFIX, not string
# equality, because the roots scan recursively: a nested `a/tests/b/tests` is
# genuinely reached from `a/tests` and reporting it would be a false red. The
# trap in prefix matching is `x/tests2`, which a naive `case $_cand in $_arg*)`
# swallows -- 4f-4 pins it, because that failure direction is the dangerous one
# (a root wrongly called covered is a root nobody scans).
#
# House contract, same as _count_rust_wallclock_escapes: the LIST is the result
# and the function always returns 0. The verdict belongs to the caller, which
# is why every fixture below asserts rc 0 alongside the output.
#
# PURE STRING LOGIC -- synthetic stdin against synthetic args, no filesystem,
# no mktemp. These roots need not exist, and deliberately do not: the question
# is whether the LIST covers a name, which is independent of what is on disk
# (that is _wallclock_assert_roots's job, Section 4e).
# ===========================================================================
echo ""
echo "--- Section 4f: root-set completeness ---"

# ---------------------------------------------------------------------------
# 4f-1: an EXACTLY EQUAL root is covered.
# ---------------------------------------------------------------------------
_s4f1_rc=0
_s4f1_out="$(printf '%s\n' 'x/tests' \
    | _wallclock_uncovered_test_roots 'x/tests' 2>/dev/null)" || _s4f1_rc=$?
assert "4f-1: the primitive returns 0 -- the list is the result, not a verdict" \
    test "$_s4f1_rc" -eq 0
assert "4f-1: a root equal to an argument is covered (nothing printed)" \
    test -z "$_s4f1_out"

# ---------------------------------------------------------------------------
# 4f-2: a DESCENDANT is covered. The roots scan recursively, so `x/tests/sub`
#       is genuinely reached from `x/tests`; reporting it would be a false red
#       that invites someone to "fix" it by enumerating subdirectories.
# ---------------------------------------------------------------------------
_s4f2_rc=0
_s4f2_out="$(printf '%s\n' 'x/tests/sub' 'x/tests/sub/deeper/tests' \
    | _wallclock_uncovered_test_roots 'x/tests' 2>/dev/null)" || _s4f2_rc=$?
assert "4f-2: the primitive returns 0 for descendants" \
    test "$_s4f2_rc" -eq 0
assert "4f-2: a descendant of an argument is covered, at any depth (nothing printed)" \
    test -z "$_s4f2_out"

# ---------------------------------------------------------------------------
# 4f-3: a SIBLING is NOT covered, and is printed. This is the review's actual
#       failure shape: a real test root beside the ones in the list.
# ---------------------------------------------------------------------------
_s4f3_rc=0
_s4f3_out="$(printf '%s\n' 'y/tests' \
    | _wallclock_uncovered_test_roots 'x/tests' 2>/dev/null)" || _s4f3_rc=$?
assert "4f-3: an uncovered root still returns 0 -- it is a finding, not an error" \
    test "$_s4f3_rc" -eq 0
assert "4f-3: a sibling root is NOT covered and is printed by name" \
    test "$_s4f3_out" = "y/tests"

# ---------------------------------------------------------------------------
# 4f-4: THE PREFIX TRAP. `x/tests2` shares a string prefix with `x/tests` but
#       is a different directory, and `x/tests` does not scan it. A naive
#       `case $_cand in $_arg*)` calls it covered -- silently dropping a real
#       test root from the completeness report, which is precisely the failure
#       this whole section exists to catch. The quoted `"$_arg"/*` form is what
#       makes the difference, so it is pinned rather than trusted.
# ---------------------------------------------------------------------------
_s4f4_rc=0
_s4f4_out="$(printf '%s\n' 'x/tests2' \
    | _wallclock_uncovered_test_roots 'x/tests' 2>/dev/null)" || _s4f4_rc=$?
assert "4f-4: the prefix trap returns 0" \
    test "$_s4f4_rc" -eq 0
assert "4f-4: x/tests2 is NOT covered by x/tests (a string prefix is not a parent)" \
    test "$_s4f4_out" = "x/tests2"

# ---------------------------------------------------------------------------
# 4f-5: EVERY uncovered root is printed, not just the first, and coverage is
#       tested against ALL the arguments rather than only `$1`. Section 3
#       reports the list to a human who then edits _LIVE_ROOTS; stopping at
#       the first would turn one edit into as many red runs as there are gaps.
# ---------------------------------------------------------------------------
_s4f5_rc=0
_s4f5_out="$(printf '%s\n' 'a/tests' 'b/tests' 'c/tests' \
    | _wallclock_uncovered_test_roots 'b/tests' 'd/tests' 2>/dev/null)" || _s4f5_rc=$?
_s4f5_n="$(_emit_record_stream "$_s4f5_out" | grep -c . || true)"
assert "4f-5: multiple uncovered roots return 0" \
    test "$_s4f5_rc" -eq 0
assert "4f-5: BOTH uncovered roots are printed, not just the first" \
    test "$_s4f5_n" -eq 2
assert "4f-5: coverage is tested against every argument, not only the first" \
    test "$_s4f5_out" = "a/tests
c/tests"

test_summary
