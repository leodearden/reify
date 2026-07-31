#!/usr/bin/env bash
# tests/infra/test_verify_nextest_absent_suites.sh — regression guard for
# host-independence of NINE NAMED plan-oracle infra suites on a nextest-LESS
# host (tasks 5599 + 5604 + 5587):
#
#     tests/infra/test_verify_compile_gate.sh              (S1, task 5599)
#     tests/infra/test_verify_semaphore_wiring.sh          (S2, task 5599)
#     tests/infra/test_verify_offline_partition.sh         (S3, task 5599)
#     tests/infra/test_verify_semaphore_e2e.sh             (S4, task 5599)
#     tests/infra/test_verify_scope.sh                     (S5, task 5604)
#     tests/infra/test_verify_failfast_order.sh            (S6, task 5604)
#     tests/infra/test_occt_gated_scope.sh                 (S7, task 5604)
#     tests/infra/test_release_mode_in_test_command.sh     (S8, task 5604)
#     tests/infra/test_verify_retry_subset.sh              (S9, task 5587)
#
# Those nine, and ONLY those nine. This file does NOT claim that
# `tests/infra/run_all.sh` as a whole is green without cargo-nextest.
#
# PROBLEM. scripts/verify.sh gracefully falls back to emitting `cargo test`
# instead of `cargo nextest run` when cargo-nextest is genuinely absent from
# PATH (plan header `nextest=0`). The covered suites used to hard-code the
# literal string `cargo nextest run` inside their `bash -c` assert bodies, so
# they FAILed spuriously on such a host — the assert is checking an
# ordering/precedence property of the emitted plan that holds identically on
# the cargo-test fallback path, not anything nextest-specific. Worse, several
# other asserts passed VACUOUSLY there (their grep matched nothing), silently
# testing nothing.
#
# WHY THE FALLBACK IS SHAPE-IDENTICAL. This block is the CANONICAL statement of
# the rationale; the covered suites carry a one-line pointer back here rather
# than their own copy, so it can be corrected in one place (task 5604).
#
# scripts/verify.sh builds the test pass in ONE function, emit_nextest_pass(),
# which ends in a two-armed if/else: the nextest arm emits
# `cargo nextest run ${selector}${rel}... --config-file <path>`, and the else
# arm (taken when the NEXTEST probe finds cargo-nextest absent) emits
# `cargo test ${selector}${rel} -- --test-threads=${TEST_THREADS:-1}`. Both arms
# interpolate the SAME `${selector}${rel}` fragment and both are wrapped by the
# same `timeout --kill-after=60 <n>m` prefix, so --workspace, -p <crate>,
# --release and the timeout wrapper are byte-identical across runners; only
# --config-file / -E / `-- --test-threads=N` differ.
#
# (Cited by enclosing FUNCTION deliberately. Earlier revisions of this note
# named `scripts/verify.sh:1659` / `:1685` in four separate files at once —
# unanchored line numbers in a 1700-line script that any insertion above them
# would silently invalidate, with nothing here detecting the drift.)
#
# That is why nearly every host-dependence in the covered suites is fixed by
# WIDENING a grep to `cargo (test|nextest run)` rather than by guarding the
# assert away — and why most floors below equal their suite's ambient count
# exactly. Where a property really IS runner-specific (cargo test has no
# --config-file), the correct fix is a guard, and it must be written in the
# skip-outside-assert form — see the S7/S8 note below for why the in-body
# `exit 0` form defeats the floor that is supposed to police it.
#
# RUNTIME COST (measured on this lane, task 5604). Ambient wall times of the
# four newly-nested suites: scope 59s, failfast_order 5s, occt_gated_scope 8s,
# release_mode 2s; each runs somewhat slower under the harness (cold plan
# capture — scope was 73s there). End to end this suite measured 155s with all
# eight S-rows green, against ~130s for the original four; run-to-run variance
# of roughly ±30s comes from plan-capture cache warmth, so treat ~3 min as the
# working figure. tests/infra/run_all.sh applies NO per-member timeout — only
# verify.sh's `timeout --kill-after=60 30m` envelope around the whole run — so
# this sits comfortably inside budget for the intra-run-serial bucket. Weighed
# deliberately: the alternative is a prose audit verdict that rots silently.
# The 9th member (retry_subset, task 5587) adds ~3s (measured 2.5s ambient on
# this lane; hermetic --print-plan only, no cargo build), so the ~3 min working
# figure above is unchanged.
#
# This suite turns the previously-manual acceptance ritual into a mechanical
# check: it builds a nextest-absent environment ONCE and runs each covered
# suite under it, asserting each reaches test_summary with rc=0, reports
# "0 failed", AND still runs at least its pinned floor of asserts (so a future
# change that guards coverage away instead of fixing it fails loudly rather
# than reporting a vacuous green — see _suite_is_clean_without_nextest).
#
# WHY NOT THE NAIVE `PATH="$STUB:/usr/bin:/bin"` RECIPE. The obvious harness
# (stub `cargo` + fresh HOME + PATH cut down to /usr/bin:/bin) does yield
# nextest=0, but it strips ~/.cargo/bin WHOLESALE — and the `tree-sitter` CLI
# lives there too. That makes test_verify_semaphore_e2e.sh's suite-start
# ensure_tree_sitter_ready gate fail, so its Sections A/B/C/F1/H FAIL loudly
# ("tree-sitter artifacts not ready") for reasons that have nothing to do with
# nextest — 5 extra failures that all PASS on the normal host. Under that
# harness the acceptance target ("0 FAIL") is unreachable, and the confound is
# invisible unless you already know where tree-sitter lives.
#
# HARNESS ACTUALLY USED — tests/infra/nextest_absent_lib.sh (task 5602). Its
# five load-bearing elements, and the measurements that justify each one, were
# lifted VERBATIM out of this file into that lib's header, which is now the
# single source of truth for the mechanism — restating them here would leave two
# prose sites to keep in sync and no gate that notices when they drift. This file
# is the lib's ORIGIN, not a client that adopted it: the simulation it runs under
# is unchanged, it is merely now shared with the two other suites that had each
# hand-rolled the same thing.
#
# NON-VACUITY SELF-CHECKS. Before covering any suite, the harness is checked
# against itself: nextest_absent_assert_real emits the same H1-H7 this file used
# to open-code, in the same order (see the lib). Without these, a broken harness
# — one that no longer hides cargo-nextest, or that "works" only by breaking
# something other than nextest — would let this whole suite pass while simulating
# nothing at all.
#
# Compile-free with respect to THIS file's own harness (verify.sh --print-plan
# is pure bash string-building); the nested suites do whatever they already do.
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh); registered in
# tests/infra/run-all-classification.manifest as `intra-run-serial`, because it
# nests test_verify_semaphore_e2e.sh which is itself intra-run-serial (it
# mutates lane-shared state: working-tree parser.c, CoW target/). Running this
# file from a `pool` member would let that mutation race other pool members.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
VERIFY="$REPO_ROOT/scripts/verify.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/nextest_absent_lib.sh" ] || {
    echo "ERROR: nextest_absent_lib.sh not found at $SCRIPT_DIR/nextest_absent_lib.sh"
    exit 1
}
# shellcheck source=tests/infra/nextest_absent_lib.sh
source "$SCRIPT_DIR/nextest_absent_lib.sh"

echo "=== the nine covered plan-oracle infra suites are host-independent on a nextest-less host (tasks 5599 + 5604 + 5587) ==="

# ---------------------------------------------------------------------------
# Harness construction (once, at suite start) — delegated to
# tests/infra/nextest_absent_lib.sh, which also supplies nx_run below.
# ---------------------------------------------------------------------------

# Guarded: nextest_absent_init (task 5645) now fails loudly and returns
# non-zero when the constructed env is not genuinely nextest-absent (e.g. a
# second, non-mirror-source PATH directory still exposes cargo-nextest). This
# suite already treats that exact condition as a HOST PRECONDITION SKIP via
# nextest_absent_available immediately below, not a suite failure, so a
# non-zero return here must not abort the suite via `set -e` before that skip
# check runs. NX_WORKDIR/NX_FARM/NX_PATH are left fully constructed either
# way, so the nextest_absent_available call below is valid regardless.
nextest_absent_init || true

# HOST PRECONDITION (skip, do NOT fail). Where the constructed env is not a
# genuine simulation — cargo-nextest still reachable, or cargo not executable
# under it — that is a property of the HOST, not a defect in the code under
# test, so it must not surface as a red suite. It especially must not `exit 1`
# BEFORE test_summary, which would leave run_all.sh (or any suite nesting this
# one the way this one nests others) with a non-zero rc and no "Results:" line
# at all, i.e. indistinguishable from a genuine mid-suite abort. Emit an
# explicit SKIP and a clean summary instead.
#
# The predicate is nextest_absent_available, keyed on that OBSERVABLE pair
# rather than on the old "is there a ~/.cargo/bin to mirror?" test. The change
# is load-bearing for THIS file specifically: S2 below runs
# test_verify_semaphore_wiring.sh — which now builds its own env from the same
# lib — INSIDE this env, where there is no cargo bin dir left to mirror. Under
# the directory-existence rule that nested suite would SKIP, emit
# "0 passed, 0 failed", and blow S2's floor of 22.
if ! nextest_absent_available; then
    echo ""
    echo "SKIP: harness unavailable on this host — $(nextest_absent_reason)"
    echo "      Reporting a clean summary: this is a host limitation, not a"
    echo "      defect in the suites under test."
    test_summary
    exit 0
fi

# ---------------------------------------------------------------------------
# Non-vacuity self-checks — these are what stop this suite from passing by
# simulating nothing.
# ---------------------------------------------------------------------------

echo ""
echo "--- H: the nextest-absent harness genuinely simulates a nextest-less host ---"

# The realness checks themselves live in the lib, as
# nextest_absent_assert_real — see the asserts at the bottom of this section.
# They are emitted as assert() calls against THIS suite's PASS/FAIL counters
# (not returned as one boolean), so the H-section pass count is what it always
# was and the S1-S8 floors below stay calibrated against the same total.

# ---------------------------------------------------------------------------
# Covered-suite checker
# ---------------------------------------------------------------------------

# _suite_is_clean_without_nextest <basename> <pass-floor> — run
# tests/infra/<basename> under the nextest-absent env and succeed ONLY if ALL
# THREE hold: the exit rc is 0, the final "Results:" line reports 0 failures,
# and it reports at least <pass-floor> passing asserts.
#
# WHY THE FLOOR. rc=0 + "0 failed" alone says nothing about how many asserts
# actually RAN. A covered suite fixes its host-dependence either by widening a
# grep (coverage preserved) or by wrapping the assert in a NEXTEST_AVAILABLE
# guard (coverage deliberately dropped on this path) — and nothing stops a
# future edit from widening a guard until it wraps the whole suite. That suite
# would still report "rc=0, 0 failed" while checking nothing, which is exactly
# the vacuity failure mode this file exists to prevent. The floor is the
# measured nextest-less pass count at the time of writing; a drop below it
# fails loudly. It is a FLOOR, not equality, so legitimately ADDING asserts to
# a covered suite does not require touching this file.
#
# WHY THE COUNTS ARE PARSED RATHER THAN grep'd. `grep -q '0 failed'` is an
# unanchored substring match: "Results: 5 passed, 10 failed" contains the
# substring "0 failed" and would pass. Today the rc=0 conjunct masks that
# (test_summary exits 1 whenever FAIL>0), but the failure-count check is
# precisely the check that must still hold in the case where a suite reports
# failures yet exits 0 anyway — a suite that stops calling test_summary, or
# whose rc gets swallowed. So both numbers are extracted from the anchored,
# whole-line shape that test_helpers.sh:63 emits and compared numerically. A
# Results line that does not match that shape leaves both empty, which the
# -n guards below reject rather than silently reading as 0.
#
# On failure, echo the captured `FAIL:` lines (and the Results line) so
# assert()'s tail-50 dump names the offending asserts rather than just
# reporting a bare non-zero rc.
_suite_is_clean_without_nextest() {
    local basename="$1"
    local floor="$2"
    local suite="$SCRIPT_DIR/$basename"
    local out rc results passed failed

    [ -f "$suite" ] || {
        echo "ERROR: covered suite not found at $suite"
        return 1
    }

    set +e
    out="$(nx_run bash "$suite" 2>&1)"
    rc=$?
    set -e

    results="$(printf '%s\n' "$out" | grep -E '^Results:' | tail -1)"
    passed="$(printf '%s\n' "$results" \
        | sed -n 's/^Results: \([0-9]\{1,\}\) passed, \([0-9]\{1,\}\) failed$/\1/p')"
    failed="$(printf '%s\n' "$results" \
        | sed -n 's/^Results: \([0-9]\{1,\}\) passed, \([0-9]\{1,\}\) failed$/\2/p')"

    if [ "$rc" -eq 0 ] && [ -n "$passed" ] && [ -n "$failed" ] \
       && [ "$failed" -eq 0 ] && [ "$passed" -ge "$floor" ]; then
        echo "$basename: rc=$rc  $results  (>= floor $floor)"
        return 0
    fi

    echo "$basename FAILED under the nextest-absent harness: rc=$rc (pass floor $floor)"
    echo "  ${results:-(no Results: line — suite aborted before test_summary)}"
    if [ -z "$passed" ] || [ -z "$failed" ]; then
        echo "  -> the Results line does not match the canonical"
        echo "     'Results: <N> passed, <M> failed' shape from test_helpers.sh"
    elif [ "$failed" -ne 0 ]; then
        echo "  -> $failed assert(s) failed on the nextest-less path"
    elif [ "$passed" -lt "$floor" ]; then
        echo "  -> COVERAGE SHRANK: only $passed assert(s) ran, floor is $floor."
        echo "     The suite is green but is checking LESS than it used to on a"
        echo "     nextest-less host. Two causes, and they need opposite fixes:"
        echo "       (a) a guard was widened instead of a grep — fix the suite;"
        echo "       (b) the suite's assert count is DATA-DRIVEN and its input"
        echo "           list legitimately shrank (see the PASS FLOORS table"
        echo "           above: it annotates which floors have this dependency"
        echo "           and on what file) — re-pin the floor here with the"
        echo "           reason and the new fixed/data-driven split."
        echo "     Check the floor table before re-pinning; do not assume (a)."
    fi
    printf '%s\n' "$out" | grep -E '^\s*FAIL:' || true
    return 1
}

# H1-H7, in that order, emitted by the lib against this suite's counters:
#
#   H1  cargo-nextest is NOT resolvable under the harness env (ABSENCE, as
#       non-resolvability)
#   H2  cargo still RUNS under it, H3 tree-sitter still RUNS under it —
#       PRESENCE as executability, because `command -v` is too weak: a harness
#       that perturbs more than intended can leave a tool
#       resolvable-but-unrunnable, and the suite would then be simulating "the
#       toolchain is broken" instead of the single intended variable
#   H4  the plan header reads nextest=0 UNDER the harness
#   H5  the plan header reads nextest=1 WITHOUT it — conditional on the host
#       actually having cargo-nextest, since on a nextest-less host there is
#       nothing for the farm to hide. test_helpers.sh has no SKIP counter, so a
#       skipped assert simply does not increment PASS (same convention as the
#       guarded regions in test_verify_offline_partition.sh)
#   H6  no rustup toolchain sync into the temp HOME, H7 the temp HOME is still
#       small — asserted LAST, so every check that actually exercises the
#       harness has already had its chance to provoke a sync
#
# ORDER AND COUNT ARE PART OF THE CONTRACT, not incidental: the S1-S8 floors
# below were measured against a run in which this section contributed exactly
# these asserts. nextest_absent_assert_real reproduces them one-for-one.
nextest_absent_assert_real "$VERIFY"

# ---------------------------------------------------------------------------
# S: the covered plan-oracle suites are clean on a nextest-less host
# ---------------------------------------------------------------------------

echo ""
echo "--- S: covered suites reach test_summary with rc=0 / 0 FAIL / >= pass floor without cargo-nextest ---"

# PASS FLOORS — the nextest-less pass count measured for each suite (S1-S4 at
# task/5599 HEAD=7adf5995f2; S5-S8 at task/5604 HEAD=375ae351e4, ambient and
# under this harness; the semaphore_e2e row RE-MEASURED at task/5839
# HEAD=1c039a8232, 65 → 85, after that task added 20 asserts covering the
# causal PSI-fixture updater). See _suite_is_clean_without_nextest for why a bare
# "0 failed" is not sufficient. Where the floor is BELOW the suite's ambient
# nextest-ful count, the difference is the asserts deliberately guarded away
# as nextest-only, and the delta is recorded here so a further shrink is
# visible as a diff to this table rather than as silence:
#   compile_gate      35 nextest-less / 35 ambient  (all recovered by widening)
#   semaphore_wiring  22 nextest-less / 22 ambient  (all recovered by widening)
#   offline_partition 30 nextest-less / 35 ambient  (5 guarded: -E heavy-filter
#                                                    asserts, no fallback shape)
#   semaphore_e2e     85 nextest-less / 85 ambient  (1 guarded --config-file
#                                                    assert, replaced 1:1 by a
#                                                    fallback-shape else arm;
#                                                    task 5839's 20 added
#                                                    asserts all exercise the
#                                                    PSI/compile-gate path and
#                                                    never touch cargo-nextest,
#                                                    so the two counts stay
#                                                    equal and the 1:1
#                                                    accounting is unchanged)
#   scope            153 nextest-less / 153 ambient (10 RED positives recovered
#                                                    by widening; 9 further
#                                                    NEGATIVES widened too —
#                                                    they were passing
#                                                    vacuously. 0 guarded away)
#   failfast_order    40 nextest-less /  40 ambient (2 recovered by widening,
#                                                    both halves of each
#                                                    compound assert)
#   occt_gated_scope  48 nextest-less /  49 ambient (1 guarded: Test 9's
#                                                    --config-file assert is
#                                                    genuinely nextest-only.
#                                                    Otherwise already clean —
#                                                    extract, assert-non-empty,
#                                                    THEN negative-grep)
#   release_mode       9 nextest-less /   9 ambient (already clean — the
#                                                    alternation was already
#                                                    used throughout)
#   retry_subset      16 nextest-less /  48 ambient (32-assert delta is the
#                                                    suite's PRE-EXISTING
#                                                    NEXTEST_AVAILABLE-guarded
#                                                    plan-shape blocks — Tests
#                                                    1, 4, 5, 6b, 7 — genuinely
#                                                    nextest-only command-shape
#                                                    asserts, NOT coverage this
#                                                    task guards away. Fixed
#                                                    count: no data-driven loop
#                                                    in this suite)
#
# DATA-DRIVEN FLOORS. Not every floor is a fixed constant, and a drop below one
# is not automatically a defect. occt_gated_scope's 48 = 33 fixed asserts + 15
# per-crate asserts driven by scripts/occt-touching-crates.txt (5 declared
# crates x 3 loops: workspace-membership, nextest.toml package() filter, and
# the no---exclude check). Legitimately REMOVING a crate from that manifest
# drops this suite's count by 3 and trips S7 with a "COVERAGE SHRANK" message
# whose (a) branch does not apply — re-pin the floor here with the new split
# rather than hunting for a nextest guard that does not exist. The other eight
# floors are fixed counts: no assert in those suites sits inside a data-driven
# loop, so a drop there really does mean an assert was guarded away or deleted
# (retry_subset included — it has no data-driven assert loop either, unlike
# occt_gated_scope's per-crate loops).
#
# Of the four task-5604 rows, three are 1:1 nextest-less/ambient — nothing
# guarded away, every failure recovered by widening a grep. The fourth
# (occt_gated_scope) carries a 1-assert delta whose reason is recorded above,
# the same shape S3/S4 use. Any future row whose two numbers differ must carry
# its reason here too.
assert "S1: test_verify_compile_gate.sh reaches test_summary with rc=0 / 0 FAIL / >= 35 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_compile_gate.sh 35

assert "S2: test_verify_semaphore_wiring.sh reaches test_summary with rc=0 / 0 FAIL / >= 22 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_semaphore_wiring.sh 22

assert "S3: test_verify_offline_partition.sh reaches test_summary with rc=0 / 0 FAIL / >= 30 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_offline_partition.sh 30

# S4 is the reason the harness uses a symlink farm rather than the naive
# PATH="$STUB:/usr/bin:/bin" recipe (see the header). test_verify_semaphore_e2e.sh
# gates Sections A/B/C/F1/H behind ensure_tree_sitter_ready, and the tree-sitter
# CLI lives in ~/.cargo/bin alongside cargo-nextest — stripping that directory
# wholesale would add 5 "tree-sitter artifacts not ready" failures that have
# nothing to do with nextest, making "0 FAIL" unreachable. H3 above pins that
# tree-sitter still resolves under the harness, so a regression in the farm
# surfaces there rather than as a confusing failure here.
assert "S4: test_verify_semaphore_e2e.sh reaches test_summary with rc=0 / 0 FAIL / >= 85 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_semaphore_e2e.sh 85

assert "S5: test_verify_scope.sh reaches test_summary with rc=0 / 0 FAIL / >= 153 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_scope.sh 153

assert "S6: test_verify_failfast_order.sh reaches test_summary with rc=0 / 0 FAIL / >= 40 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_failfast_order.sh 40

# S7/S8 were GREEN ON ARRIVAL — task 5604 audited both and found no vacuity,
# and the verdict is recorded in each file. They are listed here anyway, and
# the reason is the FLOOR, not the rc: the audit's finding is worth nothing as
# prose, because it silently rots the moment someone edits either suite. As an
# S-row with a measured floor it becomes mechanical.
#
# WHAT THE FLOOR DOES AND DOES NOT CATCH. Be precise here, because the two
# guard styles are NOT equivalent:
#
#   DETECTED — the skip-outside-assert form, `if cond; then assert ...; else
#   echo "  SKIP: ..."; fi` (used by H5 above, by test_verify_offline_partition.sh,
#   and now by test_occt_gated_scope.sh's Test 9). The skipped assert never
#   runs, so PASS does not increment, the nextest-less count drops below the
#   floor, and this suite fails loudly.
#
#   NOT DETECTED — an early `exit 0` INSIDE the assert body. test_helpers.sh:42
#   counts any zero-exit checker as a PASS, so such an assert still increments
#   PASS while checking nothing. The floor is structurally blind to it: the
#   count is unchanged. No pass-count check can catch this shape.
#
# That blind spot had a live in-file precedent — test_occt_gated_scope.sh's
# Test 9 used the in-body `exit 0` form, so a future editor copying the nearest
# example would have produced exactly the silent coverage shrink this row
# claims to prevent. Task 5604's amendment converted Test 9 to the detected
# form (hence S7's floor of 48 rather than 49, and the 1-assert delta recorded
# in the table above), so the in-file precedent now teaches the right shape.
# The residual limitation stands, though: if you must guard an assert, guard it
# OUTSIDE the assert call. Same reasoning as _suite_is_clean_without_nextest's
# own "WHY THE FLOOR" note.
assert "S7: test_occt_gated_scope.sh reaches test_summary with rc=0 / 0 FAIL / >= 48 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_occt_gated_scope.sh 48

assert "S8: test_release_mode_in_test_command.sh reaches test_summary with rc=0 / 0 FAIL / >= 9 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_release_mode_in_test_command.sh 9

assert "S9: test_verify_retry_subset.sh reaches test_summary with rc=0 / 0 FAIL / >= 16 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_retry_subset.sh 16

test_summary
