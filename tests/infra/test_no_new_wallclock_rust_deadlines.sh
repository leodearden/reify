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
# THE TWO RULES (both single-physical-line; see the detector for rationale):
#   Rule A -- `Instant::now()` immediately followed by `+` or by
#             `.checked_add`: a real-clock deadline built by hand instead of
#             taken through the WaitClock seam that watcher_tests.rs provides.
#   Rule B -- an UPPER bound compared against a `Duration`, in any of its
#             three natural spellings: `x < Duration::..`, `x <= Duration::..`,
#             and the reversed `Duration::.. > x` / `>= x`. An upper bound on
#             elapsed time inverts under descheduling. LOWER bounds
#             (`x >= Duration::..`, `x > Duration::..`) are deliberately NOT
#             matched -- they are monotone-safe.
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
# directory, argued at the site in its own doc comment. (An earlier draft of
# this guard spelled that site with `checked_add` specifically BECAUSE Rule A
# did not match it. That was a documented bypass masquerading as house style:
# it made the one site invisible AND blessed an undetectable spelling for
# every future one. Rule A now matches both spellings and the site takes the
# escape instead.) A second escape should be argued for on its own merits, in
# a review, not added quietly.
#
# KNOWN LIMITS, stated rather than hidden -- this is a lexical guard, not a
# type-aware one, so two shapes get past it by construction:
#   * A named constant: `assert!(elapsed < TIMEOUT_BUDGET)` has no `Duration::`
#     token on the line, so Rule B cannot see it. Chasing it would need type
#     resolution (or a const-name index), which is out of proportion to a grep.
#   * A construct split across physical lines, since neither rule joins lines
#     (see the detector for why that is deliberate). rustfmt keeps both shapes
#     on one line at any realistic width, so this bites a hand-wrapped site.
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
#   Rule A  Instant::now\(\)[[:space:]]*(\+|\.checked_add)
#           A real-clock deadline built by hand: reading the raw clock and
#           offsetting from it, instead of going through the WaitClock seam.
#           BOTH spellings are matched. `checked_add` was originally left out
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
    local _rule_a; _rule_a='Instant::now\(\)[[:space:]]*(\+|\.checked_add)'
    local _rule_b; _rule_b='(<=?[[:space:]]*Duration::)|(Duration::[a-z_]*\([^)]*\)[[:space:]]*>)'

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

# ===========================================================================
# Section 3: LIVE guard -- scan the real gui/src-tauri/src/tests for
#             un-escaped hand-rolled deadlines and elapsed upper bounds.
#
# This lands GREEN. Before task #6438 the live scan would have reported
# exactly three violations, all in watcher_tests.rs: one Rule A (the 5s
# hand-rolled poll deadline) and two Rule B (the 1s and 2s elapsed upper
# bounds). That task deleted all three, so every remaining Rule A/B match in
# the directory is the single escaped site, `far_future_stamp()` -- which this
# scan therefore proves is still the ONLY one. If this assertion ever fails,
# read the reported lines before reaching for an escape: the three sanctioned
# fixes in the detector's remediation hint come first, in that order.
# ===========================================================================
echo ""
echo "--- Section 3: live scan of gui/src-tauri/src/tests ---"

assert "live scan target directory exists" test -d "$_LIVE_DIR"

_s3_rc=0
_detect_rust_wallclock_deadline "$_LIVE_DIR" 2>&1 || _s3_rc=$?
assert "live scan: no un-escaped hand-rolled deadlines or elapsed upper bounds in gui/src-tauri/src/tests (returns 0)" \
    test "$_s3_rc" -eq 0

test_summary
