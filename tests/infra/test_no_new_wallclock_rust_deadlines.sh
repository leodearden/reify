#!/usr/bin/env bash
# tests/infra/test_no_new_wallclock_rust_deadlines.sh
#
# Regression guard (task #6438):
#   Flags NEW hand-rolled real-clock deadlines and elapsed-time UPPER bounds
#   in gui/src-tauri/src/tests/*.rs, so the flake class de-flaked by tasks
#   #5143, #5422, #5709 and #6438 cannot silently return a FIFTH time THERE.
#   Read that as scoped to that one directory: it is where all four instances
#   happened, and it is all this guard scans. See SCOPE under KNOWN LIMITS.
#
# The guard itself is a LOAD-INDEPENDENT static grep -- it is NOT a wall-clock
# test, and it runs no cargo, no npm and no watcher.
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
# on `_detect_rust_wallclock_deadline` below: it carries the regexes, every
# spelling matched and every spelling deliberately not matched, and the reason
# for each. It sits with the code it describes, so it is the copy to read and
# the copy to keep true -- this summary is deliberately not a second one.
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
# watcher_tests.rs, the single legitimate real-`Instant` offset in the
# directory, argued at the site in its own doc comment. That count is CHECKED,
# not merely asserted here: Section 3 counts escape-annotated lines under the
# live directory and compares them against `_ESC_ALLOWLIST_SIZE`, because the
# detector skips an escaped line without counting it and so returns 0 for one
# escape and for twenty alike. (An earlier draft of
# this guard spelled that site with `checked_add` specifically BECAUSE Rule A
# did not match it. That was a documented bypass masquerading as house style:
# it made the one site invisible AND blessed an undetectable spelling for
# every future one. Rule A now matches both spellings and the site takes the
# escape instead.) A second escape should be argued for on its own merits, in
# a review, not added quietly.
#
# KNOWN LIMITS, stated rather than hidden -- this is a lexical guard, not a
# type-aware one, and it covers ONE directory.
#
# SCOPE, stated first because it bounds every other claim here. `_LIVE_DIR` is
# gui/src-tauri/src/tests and its glob is non-recursive, so what this guard
# ratchets is the file that produced all four flakes -- not the Rust half of
# the tree. The identical Rule A shape exists elsewhere today, unguarded:
#   * crates/reify-audit/tests/jcodemunch_session_live.rs -- 8 matching lines
#   * crates/reify-cli/tests/harness_cli/cli_lsp_protocol.rs -- 4
#   * crates/reify-fdm/src/slice.rs -- 2, and outside tests at that
# i.e. 14 lines across 3 files, measured 2026-08-25 by running this guard's
# own two regexes over crates/ and gui/. They hand-roll a deadline off the raw
# clock and poll against it, which is exactly what Rule A exists to catch.
# Pointing `_LIVE_DIR` at those roots is therefore NOT a one-line change: they
# would fail the gate on day one, so extending the ratchet needs a per-file
# baseline (or an escape argued at each site), and those files sit outside
# task #6438's scope. Filed as follow-up work rather than done here or left
# implied -- until it lands, a reader should assume the Rust half of the tree
# is unguarded except for this one directory.
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
#     `assert` + a `-le`/`-lt <int>` upper bound + a time lexeme. Every rc
#     assertion below uses `-eq`, which fails that guard's operator condition
#     outright and keeps this file un-flaggable by construction.
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

# The directory the live scan in Section 3 guards.
_LIVE_DIR="$REPO_ROOT/gui/src-tauri/src/tests"

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
    # 0.518s, while this whole function over all 35 roots (1330 files, 677k
    # lines) takes 0.108s end to end -- 0.070s of it the grep itself. The loop
    # at that scale would cost ~30s in a gate that is supposed to be instant.
    local _hits _rc=0
    _hits="$(grep -rnE --include='*.rs' -e "$_rule_a" -e "$_rule_b" -- "$@")" || _rc=$?
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

    # comm -23: present LIVE, absent from the BASELINE -- i.e. a violation
    # written today. Multiset semantics come free: comm pairs equal lines one
    # for one, so a 4th copy of a 3x-baselined record surfaces as exactly one
    # unmatched line.
    local _new
    _new="$(LC_ALL=C comm -23 \
        <(_emit_record_stream "$_live") \
        <(_emit_record_stream "$_rows"))"

    [ -n "$_new" ] || return 0
    printf '%s\n' "$_new" | sed 's/^/  + /' >&2
    return 1
}

# ---------------------------------------------------------------------------
# _count_rust_wallclock_escapes <dir>
#
# Prints (stdout) the number of PHYSICAL lines carrying the escape token
# across all *.rs files in <dir>, and lists each one (stderr) as
# "file:lineno: <content>". Always returns 0 -- the COUNT is the result, and
# what to do with it is the caller's assertion, not this function's.
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
    local dir="$1"

    # Split across two adjacent single-quoted strings, as everywhere else in
    # this file: a contiguous copy here would annotate this very line and
    # would make the counter count itself if it were ever pointed at a .rs
    # copy of its own logic.
    local _esc_re; _esc_re='wallcl''ock:allow'

    local _n=0
    local f
    for f in "$dir"/*.rs; do
        [ -f "$f" ] || continue

        local _lineno=0
        local _line
        while IFS= read -r _line || [ -n "$_line" ]; do
            _lineno=$((_lineno + 1))
            if [[ "$_line" =~ $_esc_re ]]; then
                echo "$f:$_lineno: $_line" >&2
                _n=$((_n + 1))
            fi
        done < "$f"
    done

    echo "$_n"
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

# ===========================================================================
# Section 3: LIVE guard -- scan the real gui/src-tauri/src/tests for
#             un-escaped hand-rolled deadlines and elapsed upper bounds.
#
# This lands GREEN. Before task #6438 the live scan would have reported
# exactly three violations, all in watcher_tests.rs: one Rule A (the 5s
# hand-rolled poll deadline) and two Rule B (the 1s and 2s elapsed upper
# bounds). That task deleted all three, so every remaining Rule A/B match in
# the directory is the single escaped site, `far_future_stamp()`. If the
# violation assertion ever fails, read the reported lines before reaching for
# an escape: the three sanctioned fixes in the detector's remediation hint
# come first, in that order.
#
# TWO ASSERTIONS, NOT ONE (#6438 review). The violation scan alone does not
# say what the paragraph above wants it to say. The detector skips an escaped
# line without counting it, so its rc is 0 for one escape and for twenty
# alike -- a second escape could be added and BOTH the header's allowlist and
# this comment would go on claiming there was one. So the escape count is
# asserted separately, against a number stated here. Adding an escape now
# takes a diff to _ESC_ALLOWLIST_SIZE as well as to the annotated line, which
# is exactly the "deliberate, reviewable act" the allowlist asks for -- and
# the reviewer sees the count change rather than having to grep for it.
# ===========================================================================
echo ""
echo "--- Section 3: live scan of gui/src-tauri/src/tests ---"

assert "live scan target directory exists" test -d "$_LIVE_DIR"

_s3_rc=0
_detect_rust_wallclock_deadline "$_LIVE_DIR" 2>&1 || _s3_rc=$?
assert "live scan: no un-escaped hand-rolled deadlines or elapsed upper bounds in gui/src-tauri/src/tests (returns 0)" \
    test "$_s3_rc" -eq 0

# The allowlist as a NUMBER rather than as prose. One escape:
# `far_future_stamp()` in watcher_tests.rs, argued in its own doc comment.
# Changing this line is the reviewable act; see the header's ALLOWLIST
# paragraph before you do.
_ESC_ALLOWLIST_SIZE=1

_s3_esc_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s3_esc_tmpdir")
_s3_esc_count="$(_count_rust_wallclock_escapes "$_LIVE_DIR" 2>"$_s3_esc_tmpdir/escapes.txt")"

if [ "$_s3_esc_count" != "$_ESC_ALLOWLIST_SIZE" ]; then
    echo "" >&2
    echo "Escape-annotated lines under $_LIVE_DIR: $_s3_esc_count; the allowlist says $_ESC_ALLOWLIST_SIZE." >&2
    echo "The annotated lines are:" >&2
    cat "$_s3_esc_tmpdir/escapes.txt" >&2
    echo "" >&2
    echo "An escape suppresses BOTH rules on its line, so each one is a hole in this guard." >&2
    echo "If the new site is genuinely legitimate, argue it where the next reader will find" >&2
    echo "it -- in a doc comment at the site, as far_future_stamp() does -- and raise" >&2
    echo "_ESC_ALLOWLIST_SIZE in this file so the change is visible in review. If a site was" >&2
    echo "REMOVED, lower it. Do not delete this assertion: it is the only thing standing" >&2
    echo "between one argued escape and an allowlist nobody reads." >&2
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

test_summary
