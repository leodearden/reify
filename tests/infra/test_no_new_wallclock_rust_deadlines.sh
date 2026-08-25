#!/usr/bin/env bash
# tests/infra/test_no_new_wallclock_rust_deadlines.sh
#
# Regression guard (task #6438):
#   Flags NEW hand-rolled real-clock deadlines and elapsed-time UPPER bounds
#   in gui/src-tauri/src/tests/*.rs, so the flake class de-flaked by tasks
#   #5143, #5422, #5709 and #6438 cannot silently return a FIFTH time.
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
# type-aware one.
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
# _detect_rust_wallclock_deadline <dir>
#
# Scans all *.rs files in <dir> for hand-rolled real-clock deadlines and
# elapsed-time upper bounds. A PHYSICAL line is a violation iff it matches
# Rule A or Rule B and does NOT carry the escape comment.
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
#           not key on `.elapsed()`: the bound deleted from
#           watcher_drop_joins_worker_without_hanging_even_with_a_pending_event
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
#
# Prints each violation as "file:lineno: <content>" to stderr.
# Returns 1 if any violations found, 0 if none.
#
# Uses [[ =~ ]] per line rather than `echo | grep`, avoiding two subprocess
# spawns per line -- the same performance rationale the sibling guard records.
# ---------------------------------------------------------------------------
_detect_rust_wallclock_deadline() {
    local dir="$1"

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

    local _found=0
    local f
    for f in "$dir"/*.rs; do
        # An unmatched glob expands to the literal pattern under default
        # shell options, so a directory with no .rs files lands here with
        # `f` set to `<dir>/*.rs`, which is not a file. Skipping keeps an
        # empty directory CLEAN rather than aborting under `set -euo
        # pipefail` (fixture 2m).
        [ -f "$f" ] || continue

        local _lineno=0
        local _line
        # `IFS=` and `-r` keep each line byte-exact (leading whitespace,
        # backslashes). The `|| [ -n "$_line" ]` guard processes a final
        # line lacking a trailing newline instead of silently dropping it.
        while IFS= read -r _line || [ -n "$_line" ]; do
            _lineno=$((_lineno + 1))
            [[ "$_line" =~ $_esc_re ]] && continue
            if [[ "$_line" =~ $_rule_a ]] || [[ "$_line" =~ $_rule_b ]]; then
                echo "$f:$_lineno: $_line" >&2
                _found=1
            fi
        done < "$f"
    done

    if [ "$_found" = "1" ]; then
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
    fi
    return 0
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

test_summary
