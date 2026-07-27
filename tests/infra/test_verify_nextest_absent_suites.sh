#!/usr/bin/env bash
# tests/infra/test_verify_nextest_absent_suites.sh — regression guard for
# host-independence of FOUR NAMED plan-oracle infra suites on a nextest-LESS
# host (task 5599):
#
#     tests/infra/test_verify_compile_gate.sh
#     tests/infra/test_verify_semaphore_wiring.sh
#     tests/infra/test_verify_offline_partition.sh
#     tests/infra/test_verify_semaphore_e2e.sh
#
# Those four, and ONLY those four — see "NOT COVERED HERE" below. This file
# does NOT claim that `tests/infra/run_all.sh` as a whole is green without
# cargo-nextest.
#
# PROBLEM. scripts/verify.sh gracefully falls back to emitting `cargo test`
# instead of `cargo nextest run` when cargo-nextest is genuinely absent from
# PATH (plan header `nextest=0`). The four suites above used to hard-code the
# literal string `cargo nextest run` inside their `bash -c` assert bodies, so
# they FAILed spuriously on such a host — the assert is checking an
# ordering/precedence property of the emitted plan that holds identically on
# the cargo-test fallback path, not anything nextest-specific. Worse, several
# other asserts passed VACUOUSLY there (their grep matched nothing), silently
# testing nothing.
#
# This suite turns the previously-manual acceptance ritual into a mechanical
# check: it builds a nextest-absent environment ONCE and runs each covered
# suite under it, asserting each reaches test_summary with rc=0, reports
# "0 failed", AND still runs at least its pinned floor of asserts (so a future
# change that guards coverage away instead of fixing it fails loudly rather
# than reporting a vacuous green — see _suite_is_clean_without_nextest).
#
# NOT COVERED HERE (measured under this exact harness at task/5599
# HEAD=7adf5995f2; both files are outside task 5599's module locks, so they
# could not be fixed there — follow-up filed as ticket
# tkt_0RRQHFMP224R3KNR2572TKCG3J):
#   tests/infra/test_verify_scope.sh          rc=1, 133 passed, 10 failed
#   tests/infra/test_verify_failfast_order.sh rc=1,  38 passed,  2 failed
# Two more plan-oracle suites are GREEN under the harness but were not audited
# for the vacuity class above, because at least some of their nextest-string
# assertions are NEGATIVE greps: tests/infra/test_occt_gated_scope.sh (49/0)
# and tests/infra/test_release_mode_in_test_command.sh (9/0). All four are in
# the follow-up ticket's scope; when one is fixed, add it as a new S-row here.
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
# HARNESS ACTUALLY USED — tests/infra/nextest_absent_lib.sh (task 5602).
#
# The five load-bearing elements first established HERE (symlink farm mirroring
# the cargo bin dir MINUS cargo-nextest; PATH = farm : real-PATH-with-that-dir-
# filtered-out; temp HOME so verify.sh's apply_env() finds no $HOME/.cargo/env;
# CARGO_HOME deliberately unset because cargo resolves `cargo-<subcmd>` from
# $CARGO_HOME/bin in ADDITION to PATH; RUSTUP_HOME carried across, resolved
# while HOME is still real, or the stranded rustup shim downloads a whole fresh
# toolchain) were lifted VERBATIM into that lib, together with the measurements
# that justify each one. This file is the lib's origin rather than a client that
# adopted it, so the simulation it runs under is unchanged — it is now shared
# with the two other suites that had each hand-rolled the same thing.
#
# NON-VACUITY SELF-CHECKS. Before covering any suite, the harness is checked
# against itself — nextest_absent_assert_real emits the same H1-H7 this file
# used to open-code, in the same order: cargo-nextest must be genuinely
# unreachable under it, `cargo` and `tree-sitter` must both still RUN under it
# (executability, not merely `command -v` resolvability — a harness where cargo
# resolves but cannot actually execute would be simulating "the toolchain is
# broken" rather than the intended single variable), the plan header under it
# must read nextest=0, the plan header WITHOUT it must read nextest=1, and the
# harness must not have perturbed the toolchain enough to provoke a rustup
# toolchain sync into its temp HOME. Without these a broken harness (e.g. one
# that no longer hides cargo-nextest) would let this whole suite pass while
# simulating nothing at all — or would "work" only by breaking something other
# than nextest.
#
# The harness idiom itself came from tests/infra/test_verify_nextest_probe.sh
# (temp HOME + PATH shim dir + cleanup trap), substituting the symlink farm for
# the bare stub dir per the tree-sitter confound above; that suite now sources
# the same lib, which is what task 5602 consolidated.
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

echo "=== the four covered plan-oracle infra suites are host-independent on a nextest-less host (task 5599) ==="

# ---------------------------------------------------------------------------
# Harness construction (once, at suite start) — delegated to
# tests/infra/nextest_absent_lib.sh, which also supplies nx_run below.
# ---------------------------------------------------------------------------

nextest_absent_init

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
# was and the S1-S4 floors below stay calibrated against the same total.

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
        echo "     nextest-less host — a guard was widened instead of a grep."
        echo "     Fix the suite, or re-pin the floor here with the reason."
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
# ORDER AND COUNT ARE PART OF THE CONTRACT, not incidental: the S1-S4 floors
# below were measured against a run in which this section contributed exactly
# these asserts. nextest_absent_assert_real reproduces them one-for-one.
nextest_absent_assert_real "$VERIFY"

# ---------------------------------------------------------------------------
# S: the covered plan-oracle suites are clean on a nextest-less host
# ---------------------------------------------------------------------------

echo ""
echo "--- S: covered suites reach test_summary with rc=0 / 0 FAIL / >= pass floor without cargo-nextest ---"

# PASS FLOORS — the nextest-less pass count measured for each suite at
# task/5599 HEAD=7adf5995f2 (see _suite_is_clean_without_nextest for why a
# bare "0 failed" is not sufficient). Where the floor is BELOW the suite's
# ambient nextest-ful count, the difference is the asserts deliberately
# guarded away as nextest-only, and the delta is recorded here so a further
# shrink is visible as a diff to this table rather than as silence:
#   compile_gate      35 nextest-less / 35 ambient  (all recovered by widening)
#   semaphore_wiring  22 nextest-less / 22 ambient  (all recovered by widening)
#   offline_partition 30 nextest-less / 35 ambient  (5 guarded: -E heavy-filter
#                                                    asserts, no fallback shape)
#   semaphore_e2e     65 nextest-less / 65 ambient  (1 guarded --config-file
#                                                    assert, replaced 1:1 by a
#                                                    fallback-shape else arm)
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
assert "S4: test_verify_semaphore_e2e.sh reaches test_summary with rc=0 / 0 FAIL / >= 65 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_semaphore_e2e.sh 65

test_summary
