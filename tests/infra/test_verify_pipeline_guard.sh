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

assert_exit "NEGATIVE: orchestrator.yaml is fast-path-safe (exit 1)" 1 \
    run_guard requires-full-gate orchestrator.yaml

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

test_summary
