#!/usr/bin/env bash
# Infrastructure test for task 4626.
# Drift guard for scripts/verify-pipeline-guard.sh — verifies the classifier's
# decision contract (load-bearing vs fast-path-safe paths).
#
# Auto-discovered by tests/infra/run_all.sh AND auto-pulled into task-scope
# when verify.sh changes (matches the task-4523 'scripts/verify.sh ->
# tests/infra/test_verify_*.sh' row in scripts/verify-pipeline-infra-tests.txt).
#
# This test is hermetic: it drives the classifier script directly with no
# cargo or git operations.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

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
    run_guard requires-full-gate docs/note.md

assert_exit "NEGATIVE: dark-factory-orchestrator.yaml is fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate dark-factory-orchestrator.yaml

assert_exit "NEGATIVE: README.md is fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate README.md

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
    run_guard requires-full-gate docs/notes/unregistered-example.md

# (d) ANTI-DRIFT sweep: independently re-derive every doc-sync doc by
# grepping tests/infra/*.sh for the $REPO_ROOT/docs/...\.md literal form each
# doc-sync check uses to locate its target, and assert EACH one routes to the
# full gate. This is the recurrence guard: a FUTURE doc-sync grep added on a
# new doc that is not registered in doc-sync-paths.txt goes RED here until it
# is registered.
#
# The regex is anchored to the literal "$REPO_ROOT/docs/" prefix, which
# deliberately (i) excludes the bare-path negative fixtures used above and in
# Pair A (docs/note.md, docs/notes/unregistered-example.md are passed WITHOUT
# the $REPO_ROOT/ prefix), and (ii) does not self-match this grep's own
# pattern text below -- the character class [A-Za-z0-9._/-] excludes '[', so
# the match breaks immediately after ".../docs/" at the literal '[' character.
while IFS= read -r _doc; do
    assert_exit "ANTI-DRIFT: $_doc (grepped from tests/infra/*.sh) is load-bearing (exit 0)" 0 \
        run_guard requires-full-gate "$_doc"
done < <(grep -hoE '\$REPO_ROOT/docs/[A-Za-z0-9._/-]*\.md' "$SCRIPT_DIR"/*.sh \
         | sed 's#^\$REPO_ROOT/##' | sort -u)

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
    run_guard requires-full-gate tests/infra/zzz-not-a-script.txt

# (i) PRECISION: a .sh file OUTSIDE tests/infra is not caught by this clause
# (proves the tests/infra/ prefix requirement).
assert_exit "PRECISION: scripts/zzz-not-infra.sh OUTSIDE tests/infra -> fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate scripts/zzz-not-infra.sh

# (j) PRECISION: an unanchored substring path is not caught (proves ^
# anchoring to the repo-relative form, not a 'contains tests/infra/'
# substring match).
assert_exit "PRECISION: other/tests/infra/test_z.sh unanchored -> fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate other/tests/infra/test_z.sh

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
# lines -- add() / add_tool() (scripts/verify.sh:1507 / :1571, the only two
# PLAN+= sites) -- and never `source`s them. The guard's live sourced-lib
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
_SYNTH_DIR_E="$(mktemp -d)"
_TMPDIRS+=("$_SYNTH_DIR_E")
_SYNTH_VERIFY_E="$_SYNTH_DIR_E/verify.sh"
cp "$REPO_ROOT/scripts/verify.sh" "$_SYNTH_VERIFY_E"
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

# PIN (green on arrival): the bare './scripts/<x>.sh' emission shape derives.
assert_exit "SELF-HEALING: zzz-synthetic-gate.sh auto-covered after plan-line injection (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-synthetic-gate.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# PIN (green on arrival): the guarded 'if test -f …; then bash scripts/<x>.sh; fi'
# shape derives too -- that is the real shape of check_event_inventory.sh and
# test_pm_standardization.sh at verify.sh:2630-2631, so this pins the exact
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
# actually reaches the plan (scripts/verify.sh:2350, :2603), so without this
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
# today (scripts/verify.sh:2610, `add 'wait "$_VERIFY_NODE_BG_PID"'`), so this
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
# (scripts/verify.sh:2627), covered by (a)'s ground truth; this synthetic case
# pins the prefix-agnostic property itself, so a future gate under any repo
# directory is covered without another amendment.
assert_exit "SELF-HEALING: non-scripts/ emitted gate tests/zzz-nonscripts-gate.sh derives (exit 0)" 0 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate tests/zzz-nonscripts-gate.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# PIN (green on arrival) — DOCUMENTED LIMITATION, deliberately asserting the
# gap rather than closing it. The clause reads verify.sh's SOURCE TEXT, so it
# only sees paths written LITERALLY in a plan line. A plan line assembled
# through a variable -- `_cmd="./scripts/x.sh"; add_tool "$_cmd"`, the shape
# verify.sh already uses for _gui_cmd / _sidecar_cmd / _ts_cmd at
# scripts/verify.sh:2615-2617 -- derives nothing. This assertion exists so the
# limitation cannot drift away from the wording in
# scripts/verify-pipeline-paths.txt's EMITTED GATE SCRIPTS note (which is what
# tells a future author to hand-register such a gate or rewrite it to a
# literal): if someone later makes the derivation variable-aware, this case
# goes RED and the doc gets updated in the same change.
assert_exit "LIMITATION: variable-assembled add_tool \"\$_cmd\" is NOT derived (exit 1; documented, pinned)" 1 \
    bash -c 'REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH="$1" bash "$2" requires-full-gate scripts/zzz-variable-assembled.sh' \
    _ "$_SYNTH_VERIFY_E" "$GUARD_SH"

# (d) MAP-WIRING: this guard's own oracle and static manifest must select this
# test as a per-task fail-fast pole.
#
# MEASURED GAP this closes: select_infra_tests() (scripts/verify.sh:1254-1280)
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
