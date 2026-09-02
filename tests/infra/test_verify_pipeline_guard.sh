#!/usr/bin/env bash
# Infrastructure test for task 4626.
# Drift guard for scripts/verify-pipeline-guard.sh — verifies the classifier's
# decision contract (load-bearing vs fast-path-safe paths).
#
# Auto-discovered by tests/infra/run_all.sh AND auto-pulled into task-scope
# when verify.sh changes (matches the task-4523 'scripts/verify.sh ->
# tests/infra/test_verify_*.sh' row in scripts/verify-pipeline-infra-tests.txt).
#
# HERMETICITY — NARROWED SINCE TASK 6426, do not read the old claim into it.
# Most of this test drives the classifier script directly with no subprocess of
# its own. But the guard's clause 4b now EXECUTES scripts/verify.sh
# (`--print-plan`), and verify.sh runs its cargo-nextest probe unconditionally
# in print mode — see verify.sh, search "Scope note re: --print-plan
# hermeticity". So every case that reaches clause 4b forks verify.sh and,
# through it, cargo. Still no git operations, and no cargo BUILD; the probe is
# a presence/handshake check. Two consequences worth knowing:
#   - Fixture cases set REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 per that scope note,
#     and capture_print_plan retries a truncated capture.
#   - The real-tree NON-VACUITY cases in Pair E (c-bis)(a) depend on the live
#     tree's --print-plan succeeding. A cargo-nextest that is PRESENT but whose
#     probe keeps failing makes verify.sh hard-fail; the guard then correctly
#     fail-softs to an empty clause-4b set, which is a designed-for graceful
#     degradation but WOULD read as a non-vacuity failure. Those cases are
#     therefore gated on an explicit precondition assertion that names that
#     cause directly, rather than being left to fail as "the guard derived
#     nothing".
#
# COST — MEASURED, not estimated (task 6426 review; re-measured at task 6857's
# amendment pass). 143 assertions in 21-53s wall, across three consecutive
# runs on a 32-core host at 1-min loadavg 126-149, plus 15s and 68s measured
# earlier in the same session under lighter and heavier load respectively.
# Quote the RANGE, not a single number, and note WHY the range is this wide:
# this suite runs inside the concurrent tests/infra/run_all.sh pool at the
# merge gate, where the loaded figure is the realistic one, and LOAD dominates
# everything else — the same unchanged suite spanned 15-68s within one
# session, a spread far larger than any change a normal edit makes. No
# idle-tree figure is quoted because none was measured here — do not read the
# 15s low end as one. For reference the pre-6857 suite measured 26-37s for 123
# assertions on the SAME host in the SAME session, so neither the 19
# assertions task 6857 added nor the amendment pass's net +1 moved the wall
# clock out of its own run-to-run noise.
#
# Know where the time goes before adding to it, because clause 4b is LAZY and
# that makes the cost model counter-intuitive: EVERY requires-full-gate exit-1
# assertion falls through all the static clauses and so reaches the fork, while
# exit-0 and exit-2 assertions return without one. The budget is roughly: two
# make_runnable_verify_fixture tree copies, the --print-plan captures (each up
# to 3 capture_print_plan attempts), the >=3s deliberate wait in the BOUNDED
# case, and one fork per remaining forking assertion.
#
# `is-registered` sits OUTSIDE that model entirely and is the cheap way to add
# a membership assertion: it never calls derive_plan_paths (see its arm in the
# guard for why that forfeits no coverage), so it is fork-free on BOTH verdict
# routes — unlike requires-full-gate, whose exit-1 route always forks. That is
# also why switching Pair C clause (d) to it cost nothing: those assertions were
# already exit-0 and so already fork-free.
#
# TO KEEP IT THERE, a new exit-1 assertion that is not specifically about
# clause 4b should use run_guard_nofork rather than run_guard — same derived
# sets, ~0.05s instead of ~0.4-1.4s; see that helper for why it costs no
# assertion strength. Converting the seven real-tree exit-1 assertions to it
# paid for the six added boundary-precision assertions and ~9s besides
# (43.7s/117 -> 34.9s/123).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# plan_capture_lib.sh (task 6426) — `plan_capture_complete` certifies a
# --print-plan capture is not truncated, and `capture_print_plan` retries until
# it is. Pair E (c)'s reachability preflight depends on both: without the
# completeness certification a truncated capture reads as "injected path absent"
# and fires a misleading failure. The lib's fork-free [[ ]] matching also keeps
# that preflight clear of the pipe/EINTR spurious-failure class (esc-4574-42)
# under the concurrent run_all.sh pool.
[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"

# copy_list_preflight_lib.sh (task 6426) — `assert_source_closure_copied` is the
# maintained companion of the verify.sh copy list reused by
# make_runnable_verify_fixture below. It turns a future verify.sh source-line
# addition into a precise "copy-list drift: missing <lib>" error at fixture-build
# time, rather than an opaque downstream preflight failure. Same pairing
# tests/infra/test_verify_throughput.sh uses for the same list.
[ -f "$SCRIPT_DIR/copy_list_preflight_lib.sh" ] || { echo "ERROR: copy_list_preflight_lib.sh not found at $SCRIPT_DIR/copy_list_preflight_lib.sh"; exit 1; }
source "$SCRIPT_DIR/copy_list_preflight_lib.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== verify-pipeline-guard.sh classifier tests ==="

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

GUARD_SH="$REPO_ROOT/scripts/verify-pipeline-guard.sh"

# run_guard <subcommand> [args...] — invoke the classifier under test.
run_guard() {
    bash "$GUARD_SH" "$@"
}

# run_guard_nofork <subcommand> [args...] (task 6426 review) — same classifier,
# same DERIVED SETS, without clause 4b's --print-plan fork. For the exit-1
# (fast-path-safe) assertions, which are the ones that pay it.
#
# WHY THIS COSTS NOTHING IN ASSERTION STRENGTH. The fixture is a PRISTINE
# `cp scripts/verify.sh` with no sibling libs beside it. Clauses 3 (sourced
# libs) and 4a (source-text emitted gates) only READ verify.sh, and they read
# byte-identical content here, so both derive exactly what they derive on the
# real tree. Only clause 4b differs: --print-plan on a lib-less copy hard-fails
# in ~0.03s ("occt-scope-lib.sh not found next to verify.sh") and the documented
# fail-soft route yields an empty set — see the (c-bis) FAIL-SOFT case, which
# pins that degradation directly on this same fixture shape.
#
# WHY THAT IS SAFE FOR THESE ASSERTIONS SPECIFICALLY. Clause 4b is MONOTONE: it
# can only ADD paths, so dropping it can only turn an exit 0 into an exit 1 —
# never the reverse. An assertion that a path is fast-path-safe therefore cannot
# be made to pass spuriously by using this helper; it can only become
# insensitive to 4b. And 4b's contribution is byte-identical to 4a's today, so
# there is nothing to be insensitive to. Use run_guard (the real tree) for every
# exit-0 assertion, where the direction of the monotonicity does matter.
#
# WHAT WOULD MAKE THIS WRONG, and where it would show: if verify.sh ever grows a
# live variable-assembled plan line, 4b starts deriving a path 4a cannot see,
# and an exit-1 assertion on THAT path would go stale here while staying honest
# under run_guard. The live non-vacuity assertions in Pair E (c-bis)(a) compare
# --list-plan-derived against the real tree precisely so that divergence is
# visible somewhere, and Pair E's own cases keep using the runnable fixture.
#
# COST: measured, this takes the seven real-tree exit-1 assertions from a
# ~0.4-1.4s fork each to ~0.05s each.
_NOFORK_DIR="$(mktemp -d)"
_TMPDIRS+=("$_NOFORK_DIR")
_NOFORK_VERIFY="$_NOFORK_DIR/verify.sh"
cp "$REPO_ROOT/scripts/verify.sh" "$_NOFORK_VERIFY"
run_guard_nofork() {
    REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$_NOFORK_VERIFY" bash "$GUARD_SH" "$@"
}

# assert_exit DESC EXPECTED CMD [args...] — assert CMD exits EXPECTED_CODE.
# Increments the global PASS/FAIL counters from test_helpers.sh.
assert_exit() {
    local desc="$1" expected="$2"
    shift 2
    local actual=0
    "$@" >/dev/null 2>&1 || actual=$?
    if [ "$actual" -eq "$expected" ]; then
        echo "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc (expected exit $expected, got $actual)"
        FAIL=$((FAIL + 1))
    fi
}

# make_runnable_verify_fixture <outvar> (task 6426) — materialize a throwaway
# tree whose verify.sh is EXECUTABLE under --print-plan, not merely readable,
# and assign the copied verify.sh path to <outvar>. Registers the tree in
# _TMPDIRS so the existing EXIT trap cleans it up (no second trap).
#
# WHY A WHOLE TREE rather than a bare `cp verify.sh $tmp/verify.sh` (the shape
# Pair B and the pre-6426 Pair E (c) fixture use): verify.sh resolves its libs
# relative to its OWN directory, so a lone copy hard-fails with
# "verify.sh: ERROR — scripts/occt-scope-lib.sh not found next to verify.sh"
# (measured: exit 1 in 0.028s, empty stdout). That is perfectly fine for the
# guard's source-text emitted-gate clause, which only READS verify.sh — but the
# plan-derived clause RUNS it, so any case that drives the plan-derived half
# needs a fixture that actually executes. (The lone-copy shape is still used
# deliberately in (c-bis) to drive the FAIL-SOFT path.)
#
# THE COPY LIST is COPIED from tests/infra/test_verify_throughput.sh
# make_branch_fixture, not shared with it — say it plainly rather than claiming
# a reuse that is not happening. It is the THIRD instance of the same 13
# basenames plus .config/nextest.toml (test_verify_scope.sh carries the other),
# and nothing keeps the three in step automatically.
#
# WHAT IS AND IS NOT PROTECTED. assert_source_closure_copied below is a real
# shared drift guard, but it only sees libs reached by a double-quoted `source`
# statement. The four NON-SOURCED data files this fixture must also copy —
# occt-touching-crates.txt, release-sensitive-crates.txt,
# verify-pipeline-infra-tests.txt and .config/nextest.toml — are exactly the
# ones with no drift protection at all. verify-pipeline-infra-tests.txt is in
# the list specifically because verify.sh READS it (select_infra_tests) without
# sourcing it, so the preflight cannot discover it; if a future verify.sh reads
# a fifth such data file, this list must be updated BY HAND and the failure
# will surface as an opaque --print-plan error, not as a named drift.
#
# The right fix is to extract the sandbox construction (mkdir + cp list + chmod
# + preflight) into a shared helper beside tests/infra/copy_list_preflight_lib.sh
# and have all three callers use it. That requires editing
# test_verify_throughput.sh and test_verify_scope.sh, which are outside task
# 6426's lock scope, so it is filed as follow-up task 7002 rather than done
# here. Note the three fixtures are NOT interchangeable today — throughput's
# make_branch_fixture also `git init`s the tree and returns the DIRECTORY,
# while this one returns the verify.sh PATH and copies a 14th file — so the
# shared helper has to reconcile those contracts, which is why it is a task
# rather than a mechanical lift.
#
# verify.sh `source`s only seven of these DIRECTLY; the remainder are transitive
# under an already-copied lib, which is why the list must not be trimmed to the
# direct-source set.
make_runnable_verify_fixture() {
    local _outvar="$1" _dir _f
    _dir="$(mktemp -d)"
    _TMPDIRS+=("$_dir")
    mkdir -p "$_dir/scripts" "$_dir/.config"
    for _f in \
        verify.sh \
        occt-scope-lib.sh \
        occt-touching-crates.txt \
        release-scope-lib.sh \
        release-sensitive-crates.txt \
        affected-crates-lib.sh \
        lib_test_semaphore.sh \
        lib_slot_acquire.sh \
        lib_clock_stop.sh \
        cpu-admit.sh \
        lib_proc_reaper.sh \
        gen-nextest-config.sh \
        heavy-test-filter-lib.sh \
        verify-pipeline-infra-tests.txt
    do
        cp "$REPO_ROOT/scripts/$_f" "$_dir/scripts/$_f"
    done
    cp "$REPO_ROOT/.config/nextest.toml" "$_dir/.config/nextest.toml"
    chmod +x "$_dir/scripts/verify.sh"
    # Copy-list drift preflight (shared helper, task 5154): a NEW source line in
    # verify.sh — direct, or transitive under an already-copied lib — fails here
    # BY NAME instead of surfacing as an opaque "injected path absent from the
    # plan" failure further down.
    assert_source_closure_copied "$REPO_ROOT/scripts" "$_dir/scripts" verify.sh || return 1
    printf -v "$_outvar" '%s' "$_dir/scripts/verify.sh"
}

# ---------------------------------------------------------------------------
# Pair A — core decision contract
# ---------------------------------------------------------------------------

echo ""
echo "-- Pair A: core decision contract --"

# POSITIVE: anchor — scripts/verify.sh is always load-bearing
assert_exit "POSITIVE: scripts/verify.sh is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate scripts/verify.sh

# POSITIVE: static manifest data deps
assert_exit "POSITIVE: .config/nextest.toml is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate .config/nextest.toml

assert_exit "POSITIVE: scripts/occt-touching-crates.txt is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate scripts/occt-touching-crates.txt

# NEGATIVE: fast-path preserved for genuine config-only paths (exit 1).
# These prove the guard stays surgical and does not break the config-only
# fast-path throughput benefit.
assert_exit "NEGATIVE: docs/note.md is fast-path-safe (exit 1)" 1 \
    run_guard_nofork requires-full-gate docs/note.md

assert_exit "NEGATIVE: dark-factory-orchestrator.yaml is fast-path-safe (exit 1)" 1 \
    run_guard_nofork requires-full-gate dark-factory-orchestrator.yaml

assert_exit "NEGATIVE: README.md is fast-path-safe (exit 1)" 1 \
    run_guard_nofork requires-full-gate README.md

# MIXED: the incident shape — any load-bearing file in the diff forces the full gate.
assert_exit "MIXED: docs/note.md + scripts/verify.sh -> full gate required (exit 0)" 0 \
    run_guard requires-full-gate docs/note.md scripts/verify.sh

# STDIN form: pipe paths to 'requires-full-gate' (no args) — supports large diffs
# that would exceed ARG_MAX if passed as positional args.
assert_exit "STDIN: load-bearing file piped in -> full gate required (exit 0)" 0 \
    bash -c 'printf "docs/x.md\nscripts/verify.sh\n" | bash "$1" requires-full-gate' \
    _ "$GUARD_SH"

# NORMALIZATION: a leading './' is stripped defensively — a caller that passes
# './scripts/verify.sh' instead of the canonical 'scripts/verify.sh' should
# still trigger the full gate (guards against a cross-repo caller prefixing './').
assert_exit "NORMALIZE: ./scripts/verify.sh stripped to scripts/verify.sh (exit 0)" 0 \
    run_guard requires-full-gate ./scripts/verify.sh

# --list contract: output must include scripts/verify.sh (one path per line).
assert "--list output includes scripts/verify.sh" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/verify.sh"' \
    _ "$GUARD_SH"

# Usage error: unknown subcommand/flag -> exit 2.
assert_exit "usage: unknown flag --bogus exits 2" 2 \
    run_guard --bogus

# Usage error: no subcommand -> exit 2.
assert_exit "usage: no subcommand exits 2" 2 \
    run_guard

# ---------------------------------------------------------------------------
# Pair B — live sourced-lib auto-derivation (self-healing coverage)
# ---------------------------------------------------------------------------

echo ""
echo "-- Pair B: sourced-lib auto-derivation --"

# REAL-LIB regression: independently derive the live sourced libs from
# verify.sh using the exact anchored grep idiom from make_branch_fixture in
# test_verify_throughput.sh.  For each derived lib L, assert that
# requires-full-gate scripts/L exits 0.  The test never hardcodes the lib set
# — it derives it dynamically, so future additions are automatically covered.
while IFS= read -r _lib; do
    assert_exit "REAL-LIB: scripts/$_lib is load-bearing (sourced by verify.sh)" 0 \
        run_guard requires-full-gate "scripts/$_lib"
done < <(grep -E '^[[:space:]]*source "\$SCRIPT_DIR/' "$REPO_ROOT/scripts/verify.sh" \
         | sed -n 's|.*source "\$SCRIPT_DIR/\([^"]*\)".*|\1|p')

# Incident-sim: a lib-only diff (the #4618/#4624 class) must NOT fast-path.
# These tasks bumped plan-affecting sourced libs and landed green via the
# config-only fast-path, ambushing the next Rust task (#4288) with a RED
# test_verify_throughput.sh (root-caused in esc-4288-206).
assert_exit "INCIDENT-SIM: scripts/occt-scope-lib.sh (lib diff) -> full gate required (exit 0)" 0 \
    run_guard requires-full-gate scripts/occt-scope-lib.sh

# GROUND-TRUTH: hard-coded assertions for the four libs KNOWN to be sourced by
# verify.sh today.  These are independent of the grep|sed derivation loop
# above — if the production extraction regex had a bug (e.g., mishandled an
# indented or multi-word source line), both the loop and the loop's expectation
# would compute the same wrong result, masking the regression.  This
# independent check catches that divergence.
assert "--list includes scripts/occt-scope-lib.sh (hard-coded ground truth)" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/occt-scope-lib.sh"' \
    _ "$GUARD_SH"
assert "--list includes scripts/release-scope-lib.sh (hard-coded ground truth)" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/release-scope-lib.sh"' \
    _ "$GUARD_SH"
assert "--list includes scripts/affected-crates-lib.sh (hard-coded ground truth)" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/affected-crates-lib.sh"' \
    _ "$GUARD_SH"
assert "--list includes scripts/lib_test_semaphore.sh (hard-coded ground truth)" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/lib_test_semaphore.sh"' \
    _ "$GUARD_SH"

assert "--list includes scripts/lib_proc_reaper.sh (auto-derived: direct source in verify.sh)" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/lib_proc_reaper.sh"' \
    _ "$GUARD_SH"

# scripts/lib_slot_acquire.sh is sourced TRANSITIVELY (lib_test_semaphore.sh
# → lib_slot_acquire.sh) — NOT derivable from verify.sh's direct source lines.
# It must be in the static manifest (verify-pipeline-paths.txt) to prevent the
# merge-worker config fast-path from ambushing a Rust task on an edit to it
# alone (the #4618/#4624→#4288 class).  These two assertions pin it as
# load-bearing (RED until scripts/verify-pipeline-paths.txt is updated; GREEN
# after step-6 adds the manifest row).
assert_exit "POSITIVE: scripts/lib_slot_acquire.sh is load-bearing (transitive; exit 0)" 0 \
    run_guard requires-full-gate scripts/lib_slot_acquire.sh

assert "--list includes scripts/lib_slot_acquire.sh (transitive dep; must be in manifest)" \
    bash -c 'bash "$1" --list | grep -qxF "scripts/lib_slot_acquire.sh"' \
    _ "$GUARD_SH"

# SYNTHETIC self-healing: build a throwaway verify.sh copy with a fake source
# line appended, prove the classifier auto-covers the new lib via
# REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH — no manifest edit needed.
_SYNTH_DIR="$(mktemp -d)"
_TMPDIRS+=("$_SYNTH_DIR")
_SYNTH_VERIFY="$_SYNTH_DIR/verify.sh"
cp "$REPO_ROOT/scripts/verify.sh" "$_SYNTH_VERIFY"
printf '\nsource "$SCRIPT_DIR/zzz-synthetic-lib.sh"\n' >> "$_SYNTH_VERIFY"

assert_exit "SYNTHETIC: zzz-synthetic-lib.sh auto-covered after injection (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-synthetic-lib.sh' \
    _ "$_SYNTH_VERIFY" "$GUARD_SH"

# DERIVATION PRECISION: a sibling that is NOT source'd must remain fast-path-safe.
# Proves the classifier flags ONLY actually-sourced libs, not every script
# under scripts/.
assert_exit "PRECISION: scripts/zzz-not-sourced.sh NOT sourced -> fast-path-safe (exit 1)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-not-sourced.sh' \
    _ "$_SYNTH_VERIFY" "$GUARD_SH"

# ---------------------------------------------------------------------------
# Pair C — doc-sync clause (task 4955)
# ---------------------------------------------------------------------------
#
# Recurrence prevention for the 2026-07-02 W8 incident (esc-4791-35 /
# esc-4906-34): the CLAUDE.md radical trim moved the verify-pipeline
# operational digest into docs/notes/verify-pipeline-knobs.md and landed via
# the merge-worker trivial-pass fast-path (scope=config, non-Rust/non-TS
# diff). That fast-path skipped tests/infra/run_all.sh, so
# test_verify_compile_gate.sh W8 (which greps that doc for compile-gate knob
# strings) never ran at merge, and main went RED on the next task. This is
# the doc-side analogue of the #4618/#4624->#4288 script-side ambush that
# Pair B already guards for verify-pipeline libs.

echo ""
echo "-- Pair C: doc-sync clause --"

DOC_SYNC_MANIFEST="$REPO_ROOT/scripts/doc-sync-paths.txt"

# (a) POSITIVE + ground-truth: hard-coded assertions for the incident doc and
# one other, independent of the self-healing loop in (b) below -- if the
# manifest-reading loop had a bug, both the loop and its expectation would
# compute the same wrong result, masking a regression. This mirrors Pair B's
# REAL-LIB-loop-plus-GROUND-TRUTH split.
assert_exit "POSITIVE: docs/notes/verify-pipeline-knobs.md is load-bearing (W8 incident doc; exit 0)" 0 \
    run_guard requires-full-gate docs/notes/verify-pipeline-knobs.md

assert "--list includes docs/notes/verify-pipeline-knobs.md (hard-coded ground truth)" \
    bash -c 'bash "$1" --list | grep -qxF "docs/notes/verify-pipeline-knobs.md"' \
    _ "$GUARD_SH"

assert_exit "POSITIVE: docs/notes/verify-scope-throughput.md is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate docs/notes/verify-scope-throughput.md

assert "--list includes docs/notes/verify-scope-throughput.md (hard-coded ground truth)" \
    bash -c 'bash "$1" --list | grep -qxF "docs/notes/verify-scope-throughput.md"' \
    _ "$GUARD_SH"

# (b) SELF-HEALING loop: dynamically derive the doc-sync set from
# scripts/doc-sync-paths.txt (guarded so this block is vacuous, not an error,
# before the manifest exists in step-2) and assert EACH entry routes to the
# full gate. This auto-covers every future manifest addition without a test
# edit (mirrors Pair B's sourced-lib loop).
if [ -f "$DOC_SYNC_MANIFEST" ]; then
    while IFS= read -r _doc; do
        assert_exit "SELF-HEALING: $_doc is load-bearing (doc-sync-paths.txt entry; exit 0)" 0 \
            run_guard requires-full-gate "$_doc"
    done < <(grep -v '^\s*#' "$DOC_SYNC_MANIFEST" | grep -v '^\s*$')
fi

# (c) PRECISION negative: a docs/ path NOT registered in doc-sync-paths.txt
# stays fast-path-safe. Proves the clause is surgical (does not blanket-route
# all of docs/), preserving the config-only fast-path throughput benefit.
# (The existing Pair A docs/note.md / README.md negatives already cover the
# generic case.)
assert_exit "PRECISION: docs/notes/unregistered-example.md NOT in doc-sync-paths.txt -> fast-path-safe (exit 1)" 1 \
    run_guard_nofork requires-full-gate docs/notes/unregistered-example.md

# (d) ANTI-DRIFT sweep: independently re-derive every doc a tests/infra check
# cites by grepping tests/infra/*.sh for the $REPO_ROOT/docs/...\.md literal
# form each such check uses to locate its target, and assert EACH one is
# REGISTERED. This is the recurrence guard: a FUTURE doc-sync grep added on a
# new doc that is registered in NEITHER registry goes RED here until it is.
#
# THE PREDICATE IS "REGISTERED", NOT "REQUIRES FULL GATE" (task 6857, filed
# from esc-6758-2). It used to be the latter, which sees only ONE of reify's
# two registration cost points and so called a surgical-only registration
# drift; the only remedy that left a task was to stop matching the grep above,
# which silently shrinks this population and teaches the next author the same
# dodge. Asking `is-registered` removes the false positive without touching the
# population heuristic, whose whole value is that it enrols docs by TEXTUAL
# COINCIDENCE — i.e. without the author's cooperation.
#
# CANONICAL WRITE-UP — the two cost points, the full matched-set list, and the
# caution that exit 0 there means REGISTERED and not FULL GATE REQUIRED: the
# `is-registered` entry in scripts/verify-pipeline-guard.sh's header. It owns
# that rationale; this file deliberately carries no second copy of it.
#
# THE RECURRENCE GUARD IS UNCHANGED IN STRENGTH: membership in either registry
# is always a deliberate registration, so a genuinely unregistered new doc still
# goes RED here. And no full-gate coverage is lost by the switch — sub-block (b)
# SELF-HEALING already asserts requires-full-gate exit 0 for EVERY
# doc-sync-paths.txt entry, independently of this population.
#
# The regex is anchored to the literal "$REPO_ROOT/docs/" prefix, which
# deliberately (i) excludes the bare-path negative fixtures used above and in
# Pair A (docs/note.md, docs/notes/unregistered-example.md are passed WITHOUT
# the $REPO_ROOT/ prefix), and (ii) does not self-match this grep's own
# pattern text below -- the character class [A-Za-z0-9._/-] excludes '[', so
# the match breaks immediately after ".../docs/" at the literal '[' character.
#
# NON-VACUITY FIRST. The population is derived, so a silently-broken derivation
# regex would make this entire clause a no-op that reds NOTHING: the loop body
# would never execute, the suite's assertion count would quietly drop, and the
# recurrence guard would be dead while looking perfectly healthy. Capture the
# population and assert it is non-empty BEFORE looping over it — the same
# anti-vacuity net Pair E (c-bis)(a) gives the plan-derived clause.
_ANTI_DRIFT_DOCS="$(grep -hoE '\$REPO_ROOT/docs/[A-Za-z0-9._/-]*\.md' "$SCRIPT_DIR"/*.sh \
                    | sed 's#^\$REPO_ROOT/##' | sort -u)"

assert "NON-VACUITY: the ANTI-DRIFT sweep derived a NON-EMPTY doc population (a broken regex would silently make every assertion below vanish)" \
    bash -c '[ -n "$1" ]' _ "$_ANTI_DRIFT_DOCS"

while IFS= read -r _doc; do
    [ -n "$_doc" ] || continue
    assert_exit "ANTI-DRIFT: $_doc (grepped from tests/infra/*.sh) is registered — add it to scripts/doc-sync-paths.txt for the full gate, or scripts/verify-pipeline-infra-tests.txt for the citing-test subset (exit 0)" 0 \
        run_guard is-registered "$_doc"
done <<< "$_ANTI_DRIFT_DOCS"

# (e) The `is-registered` membership predicate (task 6857, filed from esc-6758-2)
#
# CONTRACT PINS for the subcommand clause (d) above now asks. The rationale for
# the switch lives in exactly ONE place -- the `is-registered` entry in
# scripts/verify-pipeline-guard.sh's header, which owns the matched sets, the
# two registration cost points, and the caution that exit 0 there means
# REGISTERED and not FULL GATE REQUIRED. What follows PINS that contract rather
# than restating it: each assertion's comment says only what THAT case buys.
# The NO-LEAK pins at the end are what keep the two subcommands' shared exit-0
# spelling from converging into one answer.

# POSITIVE (doc-sync registry): the blunt cost point still answers 0.
assert_exit "IS-REGISTERED: docs/notes/verify-pipeline-knobs.md is registered (doc-sync-paths.txt; exit 0)" 0 \
    run_guard is-registered docs/notes/verify-pipeline-knobs.md

# POSITIVE (surgical registry ONLY) -- THE observed instance from esc-6758-2.
# It is a key in scripts/verify-pipeline-infra-tests.txt (row ->
# tests/infra/test_spec_anchor_lint.sh) and is deliberately NOT in
# doc-sync-paths.txt: a prose note whose only coupling is one link-rot grep
# must not route every edit of itself to a global gate.
assert_exit "IS-REGISTERED: docs/notes/spec-anchor-contract.md is registered SURGICALLY ONLY (verify-pipeline-infra-tests.txt row; exit 0)" 0 \
    run_guard is-registered docs/notes/spec-anchor-contract.md

# POSITIVE (path-kind agnostic): the predicate is not docs/-only. This map key
# is a .py script that requires-full-gate reports 1 for (measured), so it is a
# genuine RED against a docs-only or full-gate-only reading of the predicate.
assert_exit "IS-REGISTERED: scripts/prd-capability-check.py is registered (map key, non-doc; exit 0)" 0 \
    run_guard is-registered scripts/prd-capability-check.py

# NEGATIVE -- the recurrence guard's teeth. A path in NEITHER registry (nor any
# other static clause) is unregistered, so a genuinely new unregistered doc-sync
# grep still reds clause (d). Same fixture path Pair C (c) PRECISION uses.
assert_exit "IS-REGISTERED: docs/notes/unregistered-example.md in NEITHER registry -> not registered (exit 1)" 1 \
    run_guard is-registered docs/notes/unregistered-example.md

# NEGATIVE -- the widened set is still BOUNDED: unioning _SORTED_SET and the
# map keys did not blanket-register docs/.
assert_exit "IS-REGISTERED: docs/note.md is not registered (widened set is still bounded; exit 1)" 1 \
    run_guard is-registered docs/note.md

# ARITY -- a membership query takes EXACTLY one path. requires-full-gate uses
# ANY-semantics over many paths because "does this DIFF need the gate" is
# genuinely a disjunction; "is this path registered" reads as a conjunction, so
# a multi-arg form would silently pick one of two plausible meanings. Refusing
# it makes the surface state the question instead of guessing.
assert_exit "ARITY: is-registered with ZERO args is a usage error (exit 2; no stdin mode)" 2 \
    bash -c 'bash "$1" is-registered < /dev/null' _ "$GUARD_SH"

assert_exit "ARITY: is-registered with TWO paths is a usage error (ANY/ALL ambiguity refused, not guessed; exit 2)" 2 \
    run_guard is-registered docs/notes/verify-pipeline-knobs.md docs/note.md

# STDOUT PIN -- the merge worker parses the guard's stdout as `result=$(...)`.
# is-registered must write NOTHING there on EITHER route, so a future caller
# cannot come to depend on output this subcommand does not promise. stderr is
# left UNREDIRECTED on purpose: a diagnostic written to stderr is permitted,
# one written to stdout is not, and only leaving stderr alone tells them apart.
assert "STDOUT CONTRACT: is-registered prints NOTHING on stdout on the MATCH route (exit 0)" \
    bash -c '_o=$(bash "$1" is-registered docs/notes/spec-anchor-contract.md); [ -z "$_o" ]' \
    _ "$GUARD_SH"

assert "STDOUT CONTRACT: is-registered prints NOTHING on stdout on the NO-MATCH route (exit 1)" \
    bash -c '_o=$(bash "$1" is-registered docs/note.md) || true; [ -z "$_o" ]' \
    _ "$GUARD_SH"

# NO-LEAK PINS — the mechanical encoding of task 6857's RULED-SEPARATELY
# decision: the surgical registry is read LAZILY inside the is-registered
# branch and is never folded into _SET. Folding it in would route every edit of
# every surgically registered artifact to the full global gate -- spending
# exactly the throughput Pair C (c) PRECISION exists to protect, and silently
# rewriting the cross-repo merge-worker contract that consumes exit 0.
assert_exit "NO-LEAK: docs/notes/spec-anchor-contract.md stays fast-path-safe for requires-full-gate (surgical != full gate; exit 1)" 1 \
    run_guard_nofork requires-full-gate docs/notes/spec-anchor-contract.md

assert "NO-LEAK: --list does NOT contain docs/notes/spec-anchor-contract.md (map keys never enter _SET)" \
    bash -c '! bash "$1" --list | grep -qxF "docs/notes/spec-anchor-contract.md"' \
    _ "$GUARD_SH"

# (e-bis) SYNTHETIC / PRECISION for the SURGICAL registry (task 6857).
#
# A direct transposition of the SYNTHETIC / DERIVATION PRECISION pair the
# doc-sync manifest already has (just below), onto the second registry. Both
# halves matter: SYNTHETIC proves the clause is SELF-HEALING — a future
# verify-pipeline-infra-tests.txt row is auto-covered with no edit to this test
# — and PRECISION proves it flags only the rows the map actually carries.
#
# The two ROW-SHAPE cases are the ones that could not be written against the
# real map at all, because it carries no malformed row today: they pin that the
# query point's notion of an ACTIVE ROW matches verify.sh's select_infra_tests(),
# which is the consumer that decides what a row actually buys.
_SYNTH_INFRA_MAP_DIR="$(mktemp -d)"
_TMPDIRS+=("$_SYNTH_INFRA_MAP_DIR")
_SYNTH_INFRA_MAP="$_SYNTH_INFRA_MAP_DIR/verify-pipeline-infra-tests.txt"
cat > "$_SYNTH_INFRA_MAP" <<'SYNTH_MAP_EOF'
# synthetic map (task 6857) — comment rows must be skipped like the real one
docs/zzz-synthetic-surgical.md    tests/infra/test_zzz_synthetic.sh
docs/zzz-no-glob.md
docs/zzz-key.md    scripts/zzz-not-a-key.sh
SYNTH_MAP_EOF

# SYNTHETIC — the SELF-HEALING half: a key present only in the INJECTED map
# answers 0, so a future row in the real map is auto-covered with no edit to
# this test. It doubles as the pin that the knob is honoured AT ALL — without
# it, every case below could be reading the real map and passing for the wrong
# reason.
assert_exit "SYNTHETIC: docs/zzz-synthetic-surgical.md auto-covered after map injection (self-healing; exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_INFRA_TESTS_MAP="$1" bash "$2" is-registered docs/zzz-synthetic-surgical.md' \
    _ "$_SYNTH_INFRA_MAP" "$GUARD_SH"

# The cases below are regression pins on the ROW PARSE — what the guard counts
# as an ACTIVE ROW, and which field of one is a KEY. Each is driven through the
# same injected map, so each states a property of the PARSE rather than of the
# real registry's current contents.

# PRECISION: a sibling not listed in the injected map stays unregistered —
# the clause does not blanket-register every docs/zzz-*.md.
assert_exit "PRECISION: docs/zzz-not-in-map.md absent from the injected map -> not registered (exit 1)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_INFRA_TESTS_MAP="$1" bash "$2" is-registered docs/zzz-not-in-map.md' \
    _ "$_SYNTH_INFRA_MAP" "$GUARD_SH"

# MALFORMED-ROW: a ONE-FIELD row is NOT an active registration. verify.sh's
# select_infra_tests() requires BOTH fields non-empty before it selects
# anything, so such a row selects no test and buys nothing; calling it
# "registered" would let the anti-drift sweep pass on a path that is in truth
# unguarded. The query point must not disagree with the consumer.
assert_exit "MALFORMED-ROW: docs/zzz-no-glob.md has no glob field -> selects no test -> not registered (exit 1)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_INFRA_TESTS_MAP="$1" bash "$2" is-registered docs/zzz-no-glob.md' \
    _ "$_SYNTH_INFRA_MAP" "$GUARD_SH"

# GLOB-FIELD PRECISION: only the FIRST field is a KEY. The second field is a
# test-selection glob, not a registered artifact, and must never be harvested as
# one -- otherwise every guarding test in the map would silently register
# itself and the map's second column would become an unreviewed registry.
# Spelled with a non-infra glob (scripts/zzz-not-a-key.sh) precisely so the
# tests/infra/*.sh clause cannot mask the answer.
assert_exit "GLOB-FIELD PRECISION: scripts/zzz-not-a-key.sh is only a row's SECOND field -> not a key -> not registered (exit 1)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_INFRA_TESTS_MAP="$1" bash "$2" is-registered scripts/zzz-not-a-key.sh' \
    _ "$_SYNTH_INFRA_MAP" "$GUARD_SH"

# ...and the companion that makes the case above legible: an infra-test path IS
# registered, but via the OPEN-ENDED GLOB CLAUSE, never because some row happens
# to name it. TAKEN ALONE this assertion cannot tell those two mechanisms apart
# -- the injected map's first row names exactly this path, so a (hypothetical)
# second-field harvest would answer 0 here too. The MISSING-MAP pair below
# supplies the discriminator, by taking the map away entirely.
assert_exit "GLOB-CLAUSE: tests/infra/test_zzz_synthetic.sh is registered via the tests/infra/*.sh glob, not via the row that names it (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_INFRA_TESTS_MAP="$1" bash "$2" is-registered tests/infra/test_zzz_synthetic.sh' \
    _ "$_SYNTH_INFRA_MAP" "$GUARD_SH"

# MISSING MAP — the graceful degradation registry_keys() promises
# (`[ -f "$_infra_tests_map" ] || return 0`: an absent map yields NO KEYS rather
# than an error). It is the one failure mode that arm explicitly handles, and it
# would regress SILENTLY: drop the -f test and a missing map becomes a `set -e`
# abort or a bare grep error, both of which surface as a non-zero exit that
# every caller reads as an ordinary NOT-REGISTERED verdict. The exit code alone
# is therefore NOT a sufficient pin, so the first assertion additionally
# requires the arm to REACH its own documented no-match diagnostic, and to do so
# without grep ever having been handed the missing file.
_MISSING_INFRA_MAP="$_SYNTH_INFRA_MAP_DIR/does-not-exist.txt"

assert "MISSING MAP: is-registered REACHES its documented no-match route (exit 1 + the guard's own diagnostic, no grep error) instead of aborting en route" \
    bash -c '
        _err=$(REIFY_VERIFY_PIPELINE_GUARD_INFRA_TESTS_MAP="$1" \
               bash "$2" is-registered docs/notes/spec-anchor-contract.md 2>&1 >/dev/null) \
            && _rc=0 || _rc=$?
        [ "$_rc" -eq 1 ]                                  || exit 1
        case "$_err" in *"NOT registered"*) ;; *) exit 1 ;; esac
        case "$_err" in *"No such file"*) exit 1 ;; esac
    ' _ "$_MISSING_INFRA_MAP" "$GUARD_SH"

# ...and the discriminator the GLOB-CLAUSE case above needs: with NO map at all
# the same infra-test path STILL answers 0, which can only be the glob clause
# talking. Together the two MISSING-MAP assertions cover both verdict routes
# through an absent map, so the degradation cannot regress on one of them only.
assert_exit "MISSING MAP: the glob clause alone still answers 0 with no map whatsoever (the verdict is not row-derived; exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_INFRA_TESTS_MAP="$1" bash "$2" is-registered tests/infra/test_zzz_synthetic.sh' \
    _ "$_MISSING_INFRA_MAP" "$GUARD_SH"

# SYNTHETIC self-healing: build a throwaway doc-sync manifest containing only
# a synthetic path, prove the classifier auto-covers it via
# REIFY_VERIFY_PIPELINE_GUARD_DOC_SYNC_PATHS — no real-manifest edit needed.
# Mirrors Pair B's SYNTHETIC/PRECISION pair for the sourced-lib override.
_SYNTH_DOC_SYNC_DIR="$(mktemp -d)"
_TMPDIRS+=("$_SYNTH_DOC_SYNC_DIR")
_SYNTH_DOC_SYNC_MANIFEST="$_SYNTH_DOC_SYNC_DIR/doc-sync-paths.txt"
printf 'docs/zzz-synthetic-doc-sync.md\n' > "$_SYNTH_DOC_SYNC_MANIFEST"

assert_exit "SYNTHETIC: docs/zzz-synthetic-doc-sync.md auto-covered after injection (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_DOC_SYNC_PATHS="$1" bash "$2" requires-full-gate docs/zzz-synthetic-doc-sync.md' \
    _ "$_SYNTH_DOC_SYNC_MANIFEST" "$GUARD_SH"

# DERIVATION PRECISION: a sibling NOT listed in the temp manifest must remain
# fast-path-safe. Proves the clause flags ONLY the docs the override manifest
# actually lists, not every docs/zzz-*.md path.
assert_exit "PRECISION: docs/zzz-not-registered.md NOT in synthetic manifest -> fast-path-safe (exit 1)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_DOC_SYNC_PATHS="$1" bash "$2" requires-full-gate docs/zzz-not-registered.md' \
    _ "$_SYNTH_DOC_SYNC_MANIFEST" "$GUARD_SH"

# ---------------------------------------------------------------------------
# Pair D — infra-test glob clause (task 5256)
# ---------------------------------------------------------------------------
#
# Recurrence prevention for the 2026-07-19 incident (tasks 5247/5249, PRD
# docs/prds/merge-gate-health.md W3a): unregistered NEW tests/infra/*.sh files
# landed via the dark-factory merge-worker trivial-pass fast-path (only the
# two explicitly-listed infra entries in verify-pipeline-paths.txt --
# tests/infra/run_all.sh and tests/infra/run-all-ambient-vars.manifest -- were
# load-bearing), skipping the full gate and deterministically redding main. A
# literal-per-file manifest line cannot cover not-yet-existent infra tests, so
# ANY tests/infra/*.sh path must be treated as load-bearing via an open-ended
# glob clause in the guard itself. Neither manifest entry is used as a
# positive below -- every positive here is a genuine RED against the
# pre-task-5256 guard.

echo ""
echo "-- Pair D: infra-test glob clause --"

# (a) POSITIVE: an existing infra test file (this file itself).
assert_exit "POSITIVE: tests/infra/test_verify_pipeline_guard.sh is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate tests/infra/test_verify_pipeline_guard.sh

# (b) INCIDENT SIGNAL: the 5247/5249 incident shape -- a brand-new infra test
# path that need not exist on disk (this is a pure path classifier).
assert_exit "INCIDENT SIGNAL: tests/infra/test_anything.sh is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate tests/infra/test_anything.sh

# (c) PRD SIGNAL: the exact example path from merge-gate-health.md W3a.
assert_exit "PRD SIGNAL: tests/infra/test_x.sh is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate tests/infra/test_x.sh

# (d) BREADTH: a non-test_-prefixed .sh proves the rule is *.sh, not
# test_*.sh (e.g. test_helpers.sh, sourced by every test, must be covered
# too).
assert_exit "BREADTH: tests/infra/zzz_helper.sh (non-test_-prefixed) is load-bearing (exit 0)" 0 \
    run_guard requires-full-gate tests/infra/zzz_helper.sh

# (e) MIXED: the incident shape -- a config-only file alongside a new infra
# test in the same diff must still force the full gate.
assert_exit "MIXED: docs/note.md + tests/infra/test_new.sh -> full gate required (exit 0)" 0 \
    run_guard requires-full-gate docs/note.md tests/infra/test_new.sh

# (f) STDIN form: pipe paths to 'requires-full-gate' (no args) -- supports
# large diffs that would exceed ARG_MAX if passed as positional args.
assert_exit "STDIN: infra test piped in -> full gate required (exit 0)" 0 \
    bash -c 'printf "docs/x.md\ntests/infra/test_new.sh\n" | bash "$1" requires-full-gate' \
    _ "$GUARD_SH"

# (g) NORMALIZE: a leading './' is stripped defensively before the glob
# match runs, so a caller-prefixed './tests/infra/test_new.sh' still routes
# to the full gate.
assert_exit "NORMALIZE: ./tests/infra/test_new.sh stripped then glob-matched (exit 0)" 0 \
    run_guard requires-full-gate ./tests/infra/test_new.sh

# (h) PRECISION: a non-.sh file under tests/infra stays fast-path-safe -- the
# glob is surgical to .sh, not a blanket route for all of tests/infra/.
assert_exit "PRECISION: tests/infra/zzz-not-a-script.txt NOT .sh -> fast-path-safe (exit 1)" 1 \
    run_guard_nofork requires-full-gate tests/infra/zzz-not-a-script.txt

# (i) PRECISION: a .sh file OUTSIDE tests/infra is not caught by this clause
# (proves the tests/infra/ prefix requirement).
assert_exit "PRECISION: scripts/zzz-not-infra.sh OUTSIDE tests/infra -> fast-path-safe (exit 1)" 1 \
    run_guard_nofork requires-full-gate scripts/zzz-not-infra.sh

# (j) PRECISION: an unanchored substring path is not caught (proves ^
# anchoring to the repo-relative form, not a 'contains tests/infra/'
# substring match).
assert_exit "PRECISION: other/tests/infra/test_z.sh unanchored -> fast-path-safe (exit 1)" 1 \
    run_guard_nofork requires-full-gate other/tests/infra/test_z.sh

# ---------------------------------------------------------------------------
# Pair E — emitted-gate plan-line derivation (task 6320)
# ---------------------------------------------------------------------------
#
# ORIGIN: task 6243's reviewer_comprehensive completeness comment — 6243
# registered two emitted gate scripts by hand
# (check-nan-safe-ordering.sh, check-compute-trampoline-registration.sh) and
# the reviewer observed that its siblings share the identical defect.
#
# DEFECT: verify.sh's lint plan invokes these gate scripts via EMITTED plan
# lines -- add() / add_tool() (grep `^add() {` and `^add_tool() {` in
# scripts/verify.sh; the only two PLAN+= sites) -- and never `source`s them. The guard's live sourced-lib
# clause therefore cannot see them, so a gate-script-only diff is classified
# config-only and takes the dark-factory merge-worker trivial-pass fast-path,
# skipping the full gate. That is exactly the #4618/#4624 -> #4288 ambush
# class Pair B already guards for sourced libs: the gate script lands green
# without ever having been run, and the NEXT task eats the RED.
#
# Fix shape: a LIVE derivation clause (like Pair B's, not like a manifest
# row), so a future emitted gate is covered with no manifest edit at all.

echo ""
echo "-- Pair E: emitted-gate plan-line derivation --"

# (a) GROUND-TRUTH: a hard-coded, literal list of every gate script emitted by
# verify.sh's plan today. Deliberately NOT derived from verify.sh: a
# derivation-driven loop alone would go silently VACUOUS (not red) if a future
# plan-emission refactor broke the extraction, so this hard-coded tier is the
# anti-drift net. Mirrors Pair B's REAL-LIB-loop + GROUND-TRUTH split and Pair
# C's (a)/(b) split for the same reason.
#
# RED until step-2 adds the emitted-gate derivation clause for the first SEVEN
# entries (measured exit 1 at HEAD fee75336ca); the last two are already GREEN
# via their task-6243 rows in scripts/verify-pipeline-paths.txt.
#
# AMENDMENT (reviewer_comprehensive completeness): tests/sync_comments_test.sh
# is the TENTH emitted gate and shares the identical ambush class -- verify.sh
# :2627 emits `add_tool "if test -f tests/sync_comments_test.sh; then ... bash
# tests/sync_comments_test.sh; ..."`. It was missed purely because the first
# cut of the derivation hard-coded a `scripts/` prefix; measured exit 1 (fast-
# path safe) with no row in verify-pipeline-paths.txt or doc-sync-paths.txt.
# Listing it HERE, in the prefix-agnostic ground truth, is what keeps the
# clause honest about covering the whole emitted-gate class rather than one
# directory of it.
for _gate in \
    scripts/check-manifold-deps.sh \
    scripts/check-infra-classification-manifest.sh \
    scripts/check-harness-baseline-registration.sh \
    scripts/tree-sitter-generate.sh \
    scripts/ensure-gui-sidecar-placeholder.sh \
    scripts/check_event_inventory.sh \
    scripts/test_pm_standardization.sh \
    scripts/check-nan-safe-ordering.sh \
    scripts/check-compute-trampoline-registration.sh \
    tests/sync_comments_test.sh
do
    assert_exit "GROUND-TRUTH: $_gate is load-bearing (emitted by verify.sh's plan; exit 0)" 0 \
        run_guard requires-full-gate "$_gate"
    assert "--list includes $_gate (emitted gate; hard-coded ground truth)" \
        bash -c 'bash "$1" --list | grep -qxF "$2"' \
        _ "$GUARD_SH" "$_gate"
done

# (b) DIFF-SHAPE coverage, mirroring Pair A / Pair D, driven through
# scripts/check-manifold-deps.sh -- an emitted gate that is NOT in any
# manifest today, so each of these is a genuine RED against the pre-step-2
# guard rather than a restatement of an already-covered manifest row.

# INCIDENT-SIM: the ambush shape -- a gate-script-only diff must NOT fast-path.
assert_exit "INCIDENT-SIM: scripts/check-manifold-deps.sh (gate-only diff) -> full gate required (exit 0)" 0 \
    run_guard requires-full-gate scripts/check-manifold-deps.sh

# MIXED: a config-only file alongside an emitted gate still forces the full gate.
assert_exit "MIXED: docs/note.md + scripts/check-manifold-deps.sh -> full gate required (exit 0)" 0 \
    run_guard requires-full-gate docs/note.md scripts/check-manifold-deps.sh

# STDIN form: piped paths (large diffs that would exceed ARG_MAX as argv).
assert_exit "STDIN: emitted gate piped in -> full gate required (exit 0)" 0 \
    bash -c 'printf "docs/x.md\nscripts/check-manifold-deps.sh\n" | bash "$1" requires-full-gate' \
    _ "$GUARD_SH"

# NORMALIZE: a caller-prefixed './' is stripped before the match runs.
assert_exit "NORMALIZE: ./scripts/check-manifold-deps.sh stripped then matched (exit 0)" 0 \
    run_guard requires-full-gate ./scripts/check-manifold-deps.sh

# (c) SELF-HEALING + PRECISION, driven through a throwaway verify.sh copy via
# REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH -- the same synthetic-injection idiom
# Pair B uses for the sourced-lib clause. Proves the emitted-gate clause is a
# LIVE derivation (a new plan-emitted gate is covered with no manifest edit)
# and that it is surgical (only actually-emitted top-level scripts/ paths).
#
# The fixture is built RUNNABLE (make_runnable_verify_fixture, above): the nine
# EOF-appended emission-shape cases below only need verify.sh to be READABLE,
# but the variable-assembled cases added by task 6426 need --print-plan to
# actually EXECUTE on it. One fixture serves both injection mechanisms.
make_runnable_verify_fixture _SYNTH_VERIFY_E
cat >> "$_SYNTH_VERIFY_E" <<'SYNTH_PLAN_LINES_EOF'

add_tool "./scripts/zzz-synthetic-gate.sh"
add_tool "if test -f scripts/zzz-synthetic-guarded.sh; then bash scripts/zzz-synthetic-guarded.sh; fi"
#   add_tool "./scripts/zzz-comment-only-gate.sh"
add_tool "./other/scripts/zzz-nested.sh"
add "./scripts/zzz-bare-add-gate.sh"
add './scripts/zzz-singlequote-gate.sh'
add_tool "./scripts/zzz-right.sha256sums"
add_tool "if test -f tests/zzz-nonscripts-gate.sh; then bash tests/zzz-nonscripts-gate.sh; fi"
_zzz_variable_gate="./scripts/zzz-variable-assembled.sh"
add_tool "$_zzz_variable_gate"
SYNTH_PLAN_LINES_EOF

# --- task 6426: a REACHABLE variable-assembled plan line --------------------
# The heredoc above appends at EOF. That is enough for the SOURCE-TEXT half of
# the emitted-gate clause, which only reads the file — but verify.sh `exit 0`s
# at the end of its --print-plan block, so an
# EOF-appended statement is NEVER EXECUTED and can never reach the printed plan
# (measured: 0 occurrences). To drive the PLAN-DERIVED half, the
# variable-assembled line has to be injected INSIDE build_plan, where it runs.
#
# ANCHOR: the `add_tool "./scripts/tree-sitter-generate.sh"` statement
# in scripts/verify.sh, inserted immediately before it at the same
# indentation — an unconditionally-reached RUN_RUST branch of build_plan under
# the canonical widest invocation. Measured on this fixture: exactly one
# occurrence in --print-plan output, and ZERO hits from the source-text grep
# `^[[:space:]]*add(_tool)?[[:space:]]+` (the statement begins with the
# assignment, not with `add_tool`) — i.e. exactly the residual gap task 6426
# exists to close, reproduced end to end.
#
# If a future refactor moves, renames or deletes that anchor, the REACHABILITY
# PREFLIGHT further down is what turns the resulting vacuity into a loud
# failure rather than a silently-passing assertion.
awk '
    /^[[:space:]]*add_tool "\.\/scripts\/tree-sitter-generate\.sh"$/ && !_injected {
        match($0, /^[[:space:]]*/)
        print substr($0, 1, RLENGTH) "_zzz_pp_gate=\"./scripts/zzz-print-plan-variable.sh\"; add_tool \"$_zzz_pp_gate\""
        print substr($0, 1, RLENGTH) "_zzz_pd_bait=\"other/scripts/zzz-pd-nested.sh scripts/zzz-pd-right.sha256sums\"; add_tool \"true $_zzz_pd_bait\""
        _injected = 1
    }
    { print }
' "$_SYNTH_VERIFY_E" > "$_SYNTH_VERIFY_E.injected"
mv "$_SYNTH_VERIFY_E.injected" "$_SYNTH_VERIFY_E"
chmod +x "$_SYNTH_VERIFY_E"

# SOURCE-LEVEL INJECTION PREFLIGHT (anti-vacuity, cheap half). The awk above is
# anchored on a verify.sh statement; if that statement is moved, reindented,
# renamed or deleted the awk silently no-ops. The --print-plan REACHABILITY
# preflight further down catches that too, but only after a fork — this one
# fails immediately and says which pass broke, distinguishing "the awk did not
# fire" from "it fired but the line is unreachable".
assert "PREFLIGHT: awk injected the variable-assembled line into the fixture exactly once (anchor still exists)" \
    bash -c '[ "$(grep -c "_zzz_pp_gate=" "$1")" -eq 1 ]' _ "$_SYNTH_VERIFY_E"

# Same preflight for the boundary-precision bait line injected alongside it
# (see the (c) BOUNDARY PRECISION assertions further down for what it drives).
assert "PREFLIGHT: awk injected the boundary-precision bait line exactly once" \
    bash -c '[ "$(grep -c "_zzz_pd_bait=" "$1")" -eq 1 ]' _ "$_SYNTH_VERIFY_E"

# The bait paths must be PLAN-DERIVED-ONLY for the boundary assertions to be
# testing clause 4b rather than 4a: assert the source-text half cannot see them,
# i.e. no line of the fixture both starts with an add()/add_tool() statement and
# names a bait path literally. (It is the ASSIGNMENT that names them; the
# add_tool statement names only "$_zzz_pd_bait".)
assert "PREFLIGHT: bait paths are invisible to the source-text half (4a derives zero of them)" \
    bash -c '! grep -E "^[[:space:]]*add(_tool)?[[:space:]]+" "$1" | grep -qE "zzz-pd-(nested|right)"' \
    _ "$_SYNTH_VERIFY_E"

# PIN (green on arrival): the bare './scripts/<x>.sh' emission shape derives.
assert_exit "SELF-HEALING: zzz-synthetic-gate.sh auto-covered after plan-line injection (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-synthetic-gate.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# PIN (green on arrival): the guarded 'if test -f …; then bash scripts/<x>.sh; fi'
# shape derives too -- that is the real shape of check_event_inventory.sh and
# test_pm_standardization.sh (grep `test_pm_standardization.sh` in
# scripts/verify.sh), so this pins the exact
# emission form sub-block (a)'s ground truth depends on.
assert_exit "SELF-HEALING: zzz-synthetic-guarded.sh ('if test -f …; then bash …' shape) auto-covered (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-synthetic-guarded.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# PIN (green on arrival): a sibling never mentioned in ANY plan line stays
# fast-path-safe. Proves the clause flags only actually-emitted gates, not
# every script under scripts/ (mirrors Pair B's PRECISION negative).
assert_exit "PRECISION: scripts/zzz-not-emitted.sh never emitted -> fast-path-safe (exit 1)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-not-emitted.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# PIN (green on arrival): a COMMENTED-OUT add_tool line must not make its path
# load-bearing. Pins the '^[[:space:]]*add(_tool)?' statement anchor -- the same
# hardening clause 3's source-statement grep carries.
assert_exit "PRECISION: scripts/zzz-comment-only-gate.sh in a '#' comment line -> fast-path-safe (exit 1)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-comment-only-gate.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# RED DRIVER (the genuine failure this sub-block exists for): an emitted
# 'other/scripts/zzz-nested.sh' must NOT be mis-derived as the top-level
# 'scripts/zzz-nested.sh'. The extraction has no LEFT path boundary yet, so
# grep -o matches the 'scripts/…' tail of the nested path and wrongly promotes
# an unrelated top-level script to load-bearing. Step-4 adds the boundary.
# (Pair D's (j) unanchored-substring negative is this same property for the
# infra-test glob clause.)
assert_exit "PRECISION: other/scripts/zzz-nested.sh must NOT promote scripts/zzz-nested.sh (exit 1)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-nested.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# --- (c) AMENDMENT cases (reviewer_comprehensive) -------------------------
# Five further EMISSION SHAPES the clause's regex either claims to handle or
# must exclude, none of which the four lines above exercised. Each is injected
# through the same synthetic verify.sh, so none depends on the live tree
# happening to contain the shape today (which is exactly why sub-block (a)'s
# hard-coded ground truth, pinned to today's ten gates, cannot cover them).

# PIN (green on arrival): the BARE `add "..."` emission shape derives, not just
# `add_tool "..."`. This is how scripts/ensure-gui-sidecar-placeholder.sh
# actually reaches the plan (grep `ensure-gui-sidecar-placeholder.sh` in
# scripts/verify.sh), so without this
# case the '(_tool)?' optional group has no direct pin -- only the indirect
# ground-truth entry in (a), which a future refactor of that one gate would
# silently take with it.
assert_exit "SELF-HEALING: bare add \"...\" shape (not add_tool) derives zzz-bare-add-gate.sh (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-bare-add-gate.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# RED DRIVER: a SINGLE-QUOTED plan line must derive too. The first cut of the
# statement anchor required a literal double quote
# ('^[[:space:]]*add(_tool)?[[:space:]]+"'), so `add './scripts/x.sh'` was
# invisible -- and single quotes are the natural spelling for a literal gate
# invocation that needs no interpolation. verify.sh already emits that shape
# today (grep -E "^\s*add(_tool)? '" scripts/verify.sh — `add 'wait
# "$_VERIFY_NODE_BG_PID"'`), so this
# is a live idiom, not a hypothetical one. The '#'-comment exclusion the clause
# relies on comes from the '^[[:space:]]*add' anchor itself, NOT from the quote
# character, so accepting either quote costs no precision (the comment-only
# case below still passes).
assert_exit "SELF-HEALING: single-quoted add '...' plan line derives zzz-singlequote-gate.sh (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-singlequote-gate.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# RED DRIVER: the RIGHT path boundary, the mirror of the left one above. The
# character class includes '.', so with no right anchor an emitted
# 'scripts/zzz-right.sha256sums' backtracks to a 'scripts/zzz-right.sh' match
# and promotes an unrelated same-stem script to load-bearing. Conservative in
# polarity (a spurious full gate, never a skipped one) but it is precisely the
# over-match asymmetry the left-boundary comment claims to have closed.
assert_exit "PRECISION: emitted scripts/zzz-right.sha256sums must NOT promote scripts/zzz-right.sh (exit 1)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-right.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# RED DRIVER: an emitted gate OUTSIDE scripts/ is the same ambush class and
# must derive as well. Live instance: tests/sync_comments_test.sh
# (grep `sync_comments_test.sh` in scripts/verify.sh), covered by (a)'s ground
# truth; this synthetic case
# pins the prefix-agnostic property itself, so a future gate under any repo
# directory is covered without another amendment.
assert_exit "SELF-HEALING: non-scripts/ emitted gate tests/zzz-nonscripts-gate.sh derives (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate tests/zzz-nonscripts-gate.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# PIN (green on arrival) — RESIDUAL LIMITATION, deliberately asserting what the
# clause still does NOT cover rather than closing it.
#
# TASK 6426 RESCOPED THIS CASE; do not read it as the old claim. The clause is
# now a UNION of (4a) a grep of verify.sh's SOURCE TEXT and (4b) one canonical
# widest `--print-plan` invocation reading the RESOLVED plan. 4b closes the
# variable-assembled shape for every plan line that invocation REACHES; the
# SELF-HEALING case below pins that closure.
#
# WHAT IS LEFT is the UNREACHED-BRANCH case, and this fixture is a faithful
# instance of it: `_zzz_variable_gate` is appended at EOF, AFTER verify.sh's
# --print-plan block `exit 0`s, so build_plan never executes the statement and
# NO invocation of any shape can emit it (measured: 0 occurrences in this
# fixture's own printed plan). 4a cannot see it either, because the path is
# behind a variable. Underived by BOTH halves of the union — the honest
# successor to the old "source text only" limitation, not its absence.
#
# >>> CANONICAL WRITE-UP: clause 4b's block comment in
# scripts/verify-pipeline-guard.sh. It owns the rationale, the current
# measurements and — importantly — which live verify.sh plan lines are and are
# NOT covered examples (the `_gui_cmd` triple is NOT one; an earlier draft of
# this comment cited it and was wrong twice over). Read it there rather than
# growing a second copy here.
#
# READ THIS BEFORE "FIXING" IT. If someone later makes the derivation cover
# unreached branches too (symbolic evaluation of build_plan, an N-way invocation
# matrix), this case goes RED — and the residual-limitation wording in
# scripts/verify-pipeline-guard.sh, scripts/verify-pipeline-paths.txt and
# docs/notes/verify-pipeline-knobs.md must all be updated in the same change.
assert_exit "RESIDUAL LIMITATION: variable-assembled plan line in a branch the canonical invocation never reaches derives nothing (exit 1; documented, pinned)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-variable-assembled.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# --- task 6426: the REACHABLE variable-assembled case -----------------------
#
# REACHABILITY PREFLIGHT (anti-vacuity; NOT optional). Assert the fixture's OWN
# --print-plan output really does contain the injected path. Without it, a
# future verify.sh refactor that moved or renamed the insertion anchor would
# leave the RED DRIVER below asserting a path that is simply never emitted: it
# would go silently VACUOUS — still "passing", because the clause would then
# legitimately derive nothing — instead of red. That is the same anti-drift
# role sub-block (a)'s hard-coded ground truth plays for the source-text half.
#
# plan_capture_complete certifies the capture is not truncated; an interrupted
# capture reads as "path absent" and would fire the preflight for the wrong
# reason. REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 follows verify.sh's own
# --print-plan hermeticity scope note (search "Scope note re: --print-plan
# hermeticity" in scripts/verify.sh): the
# nextest probe runs unconditionally in print mode and can otherwise fork cargo
# and sleep before hard-failing.
_PP_FIXTURE_DUMP=""
capture_print_plan _PP_FIXTURE_DUMP 3 \
    env DF_VERIFY_ROLE=merge REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 \
        bash "$_SYNTH_VERIFY_E" all --scope all --profile both --include-infra --print-plan \
    || true

assert "PREFLIGHT: fixture's own --print-plan capture is complete (not truncated)" \
    plan_capture_complete "$_PP_FIXTURE_DUMP"

assert "PREFLIGHT: fixture's own --print-plan emits scripts/zzz-print-plan-variable.sh (injection is REACHABLE)" \
    plan_match "$_PP_FIXTURE_DUMP" 'zzz-print-plan-variable\.sh'

# RED DRIVER (the gap task 6426 exists to close): a variable-assembled plan
# line in a REACHED branch of build_plan. The source-text half cannot see it —
# the statement begins with the assignment, not with `add_tool`, so the
# '^[[ space ]]*add(_tool)?[[ space ]]+' statement anchor yields zero hits —
# but the RESOLVED plan names it outright (exactly one occurrence, certified by
# the preflight immediately above). Deriving from the EMITTED plan rather than
# from the source text is what closes it, and closing it is what retires the
# hand-registration advice in scripts/verify-pipeline-paths.txt.
assert_exit "SELF-HEALING: variable-assembled add_tool \"\$_cmd\" in a REACHED plan branch IS derived via --print-plan (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-print-plan-variable.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

assert "--list includes scripts/zzz-print-plan-variable.sh (plan-derived, not source-text-derived)" \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" --list | grep -qxF scripts/zzz-print-plan-variable.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# --- (c) BOUNDARY PRECISION THROUGH THE PLAN-DERIVED HALF (task 6426 review) -
#
# Pair E's other PRECISION cases pin the over-match boundaries against clause
# 4a: their bait is a LITERAL path in an add_tool statement, so the plan-derived
# half never decides those verdicts. Since the two halves now share ONE
# extraction filter (_extract_sh_paths in scripts/verify-pipeline-guard.sh),
# these assertions pin that the shared filter is genuinely what 4b runs — a
# future edit that gave 4b its own private regex again would show up here.
#
# The bait is VARIABLE-ASSEMBLED (certified plan-derived-only by the preflight
# above), so every verdict below is clause 4b's alone:
#   left  boundary — an emitted 'other/scripts/zzz-pd-nested.sh' must derive AS
#                    ITSELF and must NOT collapse to top-level
#                    'scripts/zzz-pd-nested.sh'
#   right boundary — an emitted 'scripts/zzz-pd-right.sha256sums' must not
#                    backtrack to a 'scripts/zzz-pd-right.sh' match
#
# ONE FORK FOR ALL FOUR: --list-plan-derived is captured once and the assertions
# are pure string checks over that capture, rather than four more
# requires-full-gate processes each paying their own --print-plan.
_PD_SET_E=""
_PD_SET_E="$(REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$_SYNTH_VERIFY_E" \
             bash "$GUARD_SH" --list-plan-derived 2>/dev/null)" || true

# Non-vacuity: everything below is a "must NOT contain" except the first, so an
# empty capture (the fail-soft route) would pass three of them for free.
assert "NON-VACUITY: fixture --list-plan-derived capture is non-empty" \
    bash -c '[ -n "$1" ]' _ "$_PD_SET_E"

assert "BOUNDARY (4b): emitted other/scripts/zzz-pd-nested.sh derives as itself (prefix-agnostic)" \
    bash -c 'printf "%s\n" "$1" | grep -qxF other/scripts/zzz-pd-nested.sh' _ "$_PD_SET_E"

assert "BOUNDARY (4b): other/scripts/zzz-pd-nested.sh must NOT promote scripts/zzz-pd-nested.sh (left boundary)" \
    bash -c '! printf "%s\n" "$1" | grep -qxF scripts/zzz-pd-nested.sh' _ "$_PD_SET_E"

assert "BOUNDARY (4b): emitted scripts/zzz-pd-right.sha256sums must NOT promote scripts/zzz-pd-right.sh (right boundary)" \
    bash -c '! printf "%s\n" "$1" | grep -qxF scripts/zzz-pd-right.sh' _ "$_PD_SET_E"

# --- (c-bis) ROBUSTNESS of the plan-derived half (task 6426) ---------------
# Four properties the plan-derived half needs before it is safe in the
# merge-worker hot path, where the guard runs on EVERY classification. Each is
# a separate failure mode of "the guard now FORKS verify.sh instead of only
# reading it", and none is covered by the emission-shape cases above.

# (a) LIVE NON-VACUITY — the anti-drift net for the plan-derived half itself.
#
# Under a UNION, a silently-broken plan-derived clause is INVISIBLE: the
# source-text clause still covers every literal gate, so the classifier keeps
# answering correctly for today's inputs, nothing goes red, and the residual
# gap reopens unnoticed. That is exactly the failure mode sub-block (a)'s
# hard-coded ground truth exists to prevent for the source-text half — and the
# plan-derived half needs the same net.
#
# Isolating the set is the only way a test can observe that half ALONE, hence
# the `--list-plan-derived` diagnostic subcommand (which doubles as an operator
# affordance for debugging a surprising classification, mirroring `--list`).
# Asserted against the REAL tree, not a fixture: a fixture would only prove the
# plumbing runs, not that it still derives anything from the verify.sh that
# actually ships.
#
# RED today: the subcommand does not exist, so the `*)` usage branch fires
# exit 2 and both assertions below fail.
assert_exit "NON-VACUITY: --list-plan-derived is a real subcommand, exits 0 (diagnostic, not a diff verdict)" 0 \
    run_guard --list-plan-derived

# PRECONDITION, asserted separately and FIRST so a flaky environment is
# diagnosable rather than mysterious. The two non-vacuity assertions below
# depend on the LIVE tree's --print-plan succeeding, and the guard deliberately
# fail-softs to an EMPTY clause-4b set when it does not. That degradation is
# correct behaviour — but it would surface below as "the guard derived nothing",
# which reads as a derivation bug rather than as its actual cause. The most
# likely cause is a cargo-nextest that is PRESENT but whose probe keeps failing:
# verify.sh then hard-fails after up to 4 cargo forks (search "Scope note re:
# --print-plan hermeticity" in scripts/verify.sh). capture_print_plan's retry
# gives the same resilience the fixture preflights already get.
_PP_REAL_DUMP=""
capture_print_plan _PP_REAL_DUMP 3 \
    env DF_VERIFY_ROLE=merge REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 \
        bash "$REPO_ROOT/scripts/verify.sh" all --scope all --profile both --include-infra --print-plan \
    || true

assert "PRECONDITION: the real tree's own --print-plan succeeds and is complete (if THIS fails, the two NON-VACUITY assertions below are expected to fail too — fix the probe, not the guard)" \
    plan_capture_complete "$_PP_REAL_DUMP"

assert "NON-VACUITY: --list-plan-derived prints a NON-EMPTY set against the real tree" \
    bash -c '_o=$(bash "$1" --list-plan-derived) && [ -n "$_o" ]' \
    _ "$GUARD_SH"

assert "NON-VACUITY: --list-plan-derived contains scripts/check-manifold-deps.sh (derived from the EMITTED plan)" \
    bash -c 'bash "$1" --list-plan-derived | grep -qxF scripts/check-manifold-deps.sh' \
    _ "$GUARD_SH"

# PIN — the 0/1/2 exit contract is CLOSED to new subcommands: whatever joins
# the dispatch (--list-plan-derived at task 6426, is-registered at task 6857,
# whatever comes next), a genuinely unknown flag must still be a usage error.
# This is the SINGLE home for that pin — do not add a per-subcommand copy of it
# alongside each new arm; the code path under test is the same `*)` arm.
assert_exit "CONTRACT: an unknown flag exits 2 — the 0/1/2 contract is closed to new subcommands" 2 \
    run_guard --list-bogus

# (b) FAIL-SOFT / NEVER-FAIL-OPEN — the property that makes the hybrid safe.
#
# A deliberately LIB-LESS fixture: a bare `cp verify.sh $tmp/verify.sh` with no
# sibling libs beside it, i.e. exactly the pre-6426 Pair E (c) fixture shape.
# Measured: --print-plan on such a copy exits 1 in 0.028s with EMPTY stdout and
# "verify.sh: ERROR — scripts/occt-scope-lib.sh not found next to verify.sh" on
# stderr. An empty derivation means "nothing extra is load-bearing", so a
# plan-derived clause that REPLACED the source-text one would fail OPEN here —
# the #4618/#4624 -> #4288 ambush class all over again.
#
# The union degrades instead: the source-text floor still sees the appended
# literal, so the gate stays load-bearing. THIS IS THE ASSERTION THAT STOPS A
# FUTURE AUTHOR FROM "SIMPLIFYING" THE UNION INTO A REPLACEMENT. The classifier
# can never call something LESS load-bearing than it did before task 6426.
_LIBLESS_DIR_E="$(mktemp -d)"
_TMPDIRS+=("$_LIBLESS_DIR_E")
_LIBLESS_VERIFY_E="$_LIBLESS_DIR_E/verify.sh"
cp "$REPO_ROOT/scripts/verify.sh" "$_LIBLESS_VERIFY_E"
printf '\nadd_tool "./scripts/zzz-failsoft-gate.sh"\n' >> "$_LIBLESS_VERIFY_E"

assert_exit "FAIL-SOFT: a failing --print-plan degrades to the source-text floor — zzz-failsoft-gate.sh stays load-bearing (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-failsoft-gate.sh' \
    _ "$_LIBLESS_VERIFY_E" "$GUARD_SH"

# The assertion above is answered by the source-text floor, which since clause
# 4b became LAZY means it returns without ever attempting the failing fork —
# correct, and the point of the union, but it no longer exercises the fail-soft
# route itself. --list-plan-derived always forks, so it is the assertion that
# still does: a failing --print-plan must yield an EMPTY set at exit 0, never a
# non-zero abort under the guard's `set -euo pipefail` and never a partial set.
assert "FAIL-SOFT: --list-plan-derived on a lib-less verify.sh exits 0 with an EMPTY set (the documented degradation, not an abort)" \
    bash -c '_o=$(REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" --list-plan-derived 2>/dev/null) && [ -z "$_o" ]' \
    _ "$_LIBLESS_VERIFY_E" "$GUARD_SH"

# (c) BOUNDED / NO-HANG — the merge-worker hot-path requirement.
#
# A RUNNABLE fixture with a `sleep 600` injected at the same in-build_plan
# anchor the reachable variable-assembled case uses, so the block happens
# during PLAN CONSTRUCTION, before anything is printed — the shape a wedged
# gate script or a stuck probe would produce in the field.
#
# RED originally (measured): with no bound inside the plan-derived clause the
# guard inherits the block, and an outer `timeout 8` killed it at exit 124 in
# 8.01s. The fix is a bound INSIDE the clause. Its ceiling is
# REIFY_VERIFY_PIPELINE_GUARD_PRINT_PLAN_TIMEOUT (default in the tens of
# seconds — three orders of magnitude over the ~0.4s measured cost of the real
# invocation); this case pins it low so the assertion stays cheap, and keeps
# the outer `timeout 30` as the thing that must NEVER fire.
#
# PROBE-PATH CHOICE IS LOAD-BEARING — read before "simplifying" it. This case
# originally probed with scripts/check-manifold-deps.sh, and once clause 4b
# became LAZY (task 6426 review) that made it VACUOUS: check-manifold-deps.sh
# matches in the static source-text pass, so the guard returned in 0.11s
# without ever forking the wedged fixture, and the assertion passed while
# testing nothing. The probe must therefore be a path that NO static clause
# matches, so every clause misses and 4b is genuinely consulted — docs/note.md,
# the same fast-path-safe path Pair A uses. Expected exit is 1 for that reason:
# the wedged plan derives nothing, and nothing else matches either. Any 124
# means the bound is gone.
make_runnable_verify_fixture _BLOCKING_VERIFY_E
awk '
    /^[[:space:]]*add_tool "\.\/scripts\/tree-sitter-generate\.sh"$/ && !_injected {
        match($0, /^[[:space:]]*/)
        print substr($0, 1, RLENGTH) "sleep 600"
        _injected = 1
    }
    { print }
' "$_BLOCKING_VERIFY_E" > "$_BLOCKING_VERIFY_E.blocking"
mv "$_BLOCKING_VERIFY_E.blocking" "$_BLOCKING_VERIFY_E"
chmod +x "$_BLOCKING_VERIFY_E"

# FORK MARKER — makes "did the guard fork this fixture at all?" a DETERMINISTIC
# observation rather than a timing inference. Written unconditionally at the top
# of the fixture, so its existence after a call means clause 4b ran the fixture
# and its absence means it did not. Timing alone cannot answer that without
# being hostage to load under the concurrent run_all.sh pool, and the LAZY
# assertion below is exactly a "this should be FAST" claim — the direction where
# a timing threshold flakes.
_BLOCKING_MARKER="$(dirname "$_BLOCKING_VERIFY_E")/pp-forked.marker"
sed -i "2i : > \"$_BLOCKING_MARKER\"" "$_BLOCKING_VERIFY_E"

# INJECTION PREFLIGHTS (anti-vacuity; NOT optional — same role as the
# REACHABILITY PREFLIGHT above, which this case lacked until the task-6426
# review). The awk is anchored on the SAME verify.sh statement the reachable
# case uses, so a single refactor that moves, reindents, renames or deletes it
# silently guts this assertion — the awk no-ops, --print-plan completes
# normally in ~0.3s, and the assertion still passes with the timeout bound
# entirely untested — while the other case goes loudly red. Pin both injections.
assert "PREFLIGHT: awk injected 'sleep 600' into the blocking fixture exactly once (anchor still exists)" \
    bash -c '[ "$(grep -c "sleep 600" "$1")" -eq 1 ]' _ "$_BLOCKING_VERIFY_E"

assert "PREFLIGHT: the fork marker line was injected into the blocking fixture" \
    bash -c 'grep -qF "pp-forked.marker" "$1"' _ "$_BLOCKING_VERIFY_E"

rm -f "$_BLOCKING_MARKER"
_BOUNDED_T0=$SECONDS
assert_exit "BOUNDED: a wedged --print-plan cannot hang the guard — the 4b-consulting route returns under an outer timeout 30, never 124" 1 \
    env REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$_BLOCKING_VERIFY_E" \
        REIFY_VERIFY_PIPELINE_GUARD_PRINT_PLAN_TIMEOUT=3 \
        timeout 30 bash "$GUARD_SH" requires-full-gate docs/note.md
_BOUNDED_ELAPSED=$(( SECONDS - _BOUNDED_T0 ))

# Marker present == the fork genuinely happened. This also certifies the marker
# MECHANISM is live, which is what makes the LAZY assertion's marker-absent
# check meaningful rather than trivially true.
assert "BOUNDED: the 4b-consulting route really did fork the wedged fixture (marker written)" \
    test -e "$_BLOCKING_MARKER"

# ...and elapsed >= the configured bound is what proves the fork was cut short
# by the clause's OWN timeout rather than completing on its own (the unwedged
# invocation costs ~0.4s, well under 3s). Cannot flake: a `sleep 600` behind a
# `timeout 3` takes at least 3s by construction, so this only goes red when the
# bound is actually gone. Upper end is the outer timeout 30, asserted by the
# exit code above.
assert "BOUNDED: the wedged fork was cut by the clause's own timeout (elapsed ${_BOUNDED_ELAPSED}s >= 3s)" \
    test "$_BOUNDED_ELAPSED" -ge 3

# COMPLEMENT — the LAZY property itself (task 6426 review): the same wedged
# fixture, probed with a path the STATIC clauses DO match, must return without
# forking at all. That is the optimisation's whole point (the guard runs on
# every dark-factory merge-worker classification), and it is also the guard
# against silently reverting to eager evaluation — which would look green
# everywhere else, since eager and lazy agree on every verdict.
rm -f "$_BLOCKING_MARKER"
assert_exit "LAZY: a statically-matched path returns via the source-text floor on the SAME wedged fixture (exit 0)" 0 \
    env REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$_BLOCKING_VERIFY_E" \
        REIFY_VERIFY_PIPELINE_GUARD_PRINT_PLAN_TIMEOUT=3 \
        timeout 30 bash "$GUARD_SH" requires-full-gate scripts/check-manifold-deps.sh

assert "LAZY: that call did NOT fork --print-plan at all (marker absent — clause 4b stayed unevaluated)" \
    bash -c '[ ! -e "$1" ]' _ "$_BLOCKING_MARKER"

# (d) STDOUT CONTRACT — the merge worker parses the guard's stdout as
# `result=$(...)`, the first matched load-bearing path (see the header's usage
# block). ANY --print-plan output or clause diagnostic leaking to stdout
# corrupts that parse, and the leak would be invisible to every exit-code
# assertion above.
#
# Driven on the FAIL-SOFT fixture specifically, because that is the route that
# actually produces output to leak: verify.sh writes its lib-resolution error
# to stderr there. stderr is left UNREDIRECTED by these cases on purpose — a
# clause diagnostic written to stderr is permitted, one written to stdout is
# not, and only leaving stderr alone can tell the two apart.
assert "STDOUT CONTRACT: fail-soft route prints EXACTLY the matched path and nothing else" \
    bash -c '_o=$(REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-failsoft-gate.sh); [ "$_o" = "scripts/zzz-failsoft-gate.sh" ]' \
    _ "$_LIBLESS_VERIFY_E" "$GUARD_SH"

assert "STDOUT CONTRACT: an exit-1 (fast-path-safe) classification prints NOTHING on stdout" \
    bash -c '_o=$(bash "$1" requires-full-gate docs/note.md) || true; [ -z "$_o" ]' \
    _ "$GUARD_SH"

# (d) MAP-WIRING: this guard's own oracle and static manifest must select this
# test as a per-task fail-fast pole.
#
# MEASURED GAP this closes: select_infra_tests() (grep `^select_infra_tests() {`
# in scripts/verify.sh)
# matches artifact fields by EXACT repo-relative path, and at HEAD fee75336ca
# scripts/verify-pipeline-infra-tests.txt had NO row for either
# scripts/verify-pipeline-guard.sh or scripts/verify-pipeline-paths.txt. The
# nearest row, 'scripts/verify.sh -> tests/infra/test_verify_*.sh', does not
# fire on them. So a guard-only or manifest-only task-scope (--scope
# branch/staged) verify selected ZERO infra poles -- including this task's own
# diff -- and the new emitted-gate derivation clause could regress with no
# fail-fast signal at all until the merge-tier tests/infra/run_all.sh pool.
#
# This is the CHEAP per-task complement to the full-gate route, not a
# substitute for it -- the same two-lever pairing the task-5252 block in
# scripts/verify-pipeline-infra-tests.txt already describes, and the same
# "citing-test subset" cost point as the task-4955 doc-sync rows. This test is
# hermetic (no cargo, no git), so the pole is nearly free.
#
# Matched by GLOB EXPANSION rather than by literal string, so a future
# broadening to e.g. tests/infra/test_verify_pipeline_*.sh still satisfies it.
# Precedent shape: tests/infra/test_target_per_lane_independence.sh:293
# ("verify-pipeline-infra-tests.txt maps <artifact> -> this test") and
# test_warm_lane_gc_sweep.sh block F.

VP_INFRA_MAP="$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt"
_SELF_TEST_PATH="$SCRIPT_DIR/test_verify_pipeline_guard.sh"

# _map_selects_this_test <artifact-path> — mirror select_infra_tests()'s parse
# exactly (same active-row filter, same two-field `read`), then expand each
# matching row's glob under $REPO_ROOT and report success if the expansion
# contains THIS file.
_map_selects_this_test() {
    local _want="$1" _artifact _glob _line _expanded
    [ -f "$VP_INFRA_MAP" ] || return 1
    while IFS= read -r _line; do
        read -r _artifact _glob <<< "$_line"
        [ -n "$_artifact" ] || continue
        [ -n "$_glob" ]     || continue
        [ "$_artifact" = "$_want" ] || continue
        for _expanded in "$REPO_ROOT"/$_glob; do
            [ "$_expanded" = "$_SELF_TEST_PATH" ] && return 0
        done
    done < <(grep -v '^\s*#' "$VP_INFRA_MAP" | grep -v '^\s*$')
    return 1
}

# RED until step-6 adds the two rows.
assert "MAP-WIRING: verify-pipeline-infra-tests.txt maps scripts/verify-pipeline-guard.sh -> this test" \
    _map_selects_this_test scripts/verify-pipeline-guard.sh

assert "MAP-WIRING: verify-pipeline-infra-tests.txt maps scripts/verify-pipeline-paths.txt -> this test" \
    _map_selects_this_test scripts/verify-pipeline-paths.txt

test_summary
