#!/usr/bin/env bash
# Plan-shape guard for task 5287 (PRD verify-retry-failed-only, task α) —
# scripts/verify.sh must honor a dark-factory-supplied "failed-only" retry
# subset so a merge-gate retry re-runs ONLY the did-not-pass tests against the
# warm _merge-verify target/, instead of a full recompile + full ~20,280-test
# run.
#
# Seam (CLAUDE.md): "reify ships the primitive, dark-factory wires the
# invocation." verify.sh BUILDS the exact-match nextest filterset expression
# and owns the tree-OID guard / loud full-fallback / size ceiling; DF (D2) owns
# the subset CONTENT (the newline-delimited exact test-id file) and sets the
# consumed envs. This test locks in the primitive, at PLAN-BUILD time.
#
# What α delivers (this test's scope):
#   - subset consumption: REIFY_VERIFY_RETRY_SCOPE=failed_only + a matching
#     attempt-0 sidecar tree_oid ⇒ append ` -E 'test(=id1) | test(=id2) | …'`
#     (EXACT `test(=…)` only — never substring — PRD §6.3 soundness) to the
#     NEXTEST=1 cargo line, built once from the DF-written filter file (INV-5).
#   - tree-OID eligibility gate: a mismatched / absent sidecar tree_oid ⇒ no
#     subset fragment (full verify).
#   - byte-identical default: no REIFY_VERIFY_RETRY_* envs ⇒ plan unchanged.
# (The three LOUD full-fallback reasons — tree drift / no subset / subset too
# large — the per-profile filter precedence, and the attempt-0 sidecar stamp
# are added by the later steps of this same test file.)
#
# SCOPE BOUNDARY: α owns this fast hermetic plan-shape unit test. Sibling task δ
# (tests/infra/test_verify_retry_failed_only.sh) owns the @@REIFY_RETRY_SCOPE@@
# honest marker and the B1-B6 e2e boundary test that exercises the REAL nextest
# run — complementary levels, NOT G7 lock-step duplication.
#
# Hermetic: drives ONLY `verify.sh … --print-plan` (verify.sh builds the plan
# and exits 0 — no cargo build, no tests executed). The subset-vs-fallback
# decision is made at build time, so --print-plan is a faithful oracle of
# whether the `-E` subset applied. Command-shape assertions that depend on
# nextest being installed are guarded on a NEXTEST_AVAILABLE probe of the plan
# header's `nextest=` token (sibling idiom, test_verify_test_threads.sh);
# host-independent invariants (byte-identical default; the loud stderr lines,
# added by later steps) are asserted unconditionally. Temp sidecar / filter
# files are pointed at via the REIFY_VERIFY_ATTEMPT_SIDECAR /
# REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE* envs for full hermeticity.
#
# Mirrors:
#   - tests/infra/test_verify_test_threads.sh (task 5264) — the direct
#     structural precedent (env→nextest fragment in emit_nextest_pass,
#     --print-plan oracle + NEXTEST availability probe).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY="$REPO_ROOT/scripts/verify.sh"

echo "=== verify.sh retry-subset plan-shape tests (task 5287, PRD verify-retry-failed-only α) ==="

# --- Hermetic fixtures -------------------------------------------------------
# A temp attempt-0 sidecar (tree_oid=deadbeef) and a 2-line exact-id filter
# file, both outside target/ and pointed at via the testability-knob envs so
# nothing touches the real repo state.
_TMP="$(mktemp -d "${TMPDIR:-/tmp}/reify-retry-subset.XXXXXX")"
trap 'rm -rf "$_TMP"' EXIT

SIDECAR="$_TMP/attempt.json"
printf '{"tree_oid":"deadbeef","profiles":"debug","timestamp":"2026-07-20T00:00:00Z"}\n' > "$SIDECAR"

FILTER="$_TMP/filter.txt"
ID1="reify_core::foo::test_alpha"
ID2="reify_core::bar::test_beta"
printf '%s\n%s\n' "$ID1" "$ID2" > "$FILTER"

# --- nextest availability probe (off the byte-identical default plan) --------
PLAN_DEFAULT="$(bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true
_HEADER="$(printf '%s\n' "$PLAN_DEFAULT" | grep '^# verify.sh plan' || true)"
NEXTEST_AVAILABLE=0
case "$_HEADER" in
    *"nextest=1"*) NEXTEST_AVAILABLE=1 ;;
esac
echo "(nextest available on this host: $NEXTEST_AVAILABLE)"

# ---------------------------------------------------------------------------
# Test 1: SUBSET APPLICATION + TREE-OID ELIGIBILITY GATE + DEFAULT-INVARIANT.
#   (a) TREE_OID matches the sidecar (deadbeef) ⇒ the debug `cargo nextest run`
#       line carries the exact-match filterset `test(=id1)` / `test(=id2)`, and
#       the bare-substring form `test(<id1>)` (no `=`) is ABSENT (soundness).
#   (b) TREE_OID mismatches (cafef00d) ⇒ the nextest line has NO `test(=` retry
#       fragment (the eligibility gate blocks the subset → full verify).
#   (c) DEFAULT plan (no REIFY_VERIFY_RETRY_* envs) ⇒ NO `test(=` fragment on
#       any cargo line (byte-identical to today).
# RED at base: emit_nextest_pass has no retry handling, so (a) finds no fragment.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: failed_only subset applies under a matching tree_oid; gate blocks a mismatch; default unchanged ---"

PLAN_MATCH="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true

PLAN_MISMATCH="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=cafef00d \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "match: debug nextest line carries exact-match filterset test(=$ID1)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test(=$2)"' \
        _ "$PLAN_MATCH" "$ID1"

    assert "match: debug nextest line carries exact-match filterset test(=$ID2)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test(=$2)"' \
        _ "$PLAN_MATCH" "$ID2"

    assert "match: bare-substring form test($ID1) (no '='') is ABSENT (exact-match soundness, PRD §6.3)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test($2)"' \
        _ "$PLAN_MATCH" "$ID1"

    assert "mismatch (tree drift): debug nextest line has NO test(= retry fragment (eligibility gate blocks)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test(="' \
        _ "$PLAN_MISMATCH"
fi

assert "default: NO test(= retry fragment on any cargo line (byte-identical default)" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -qF -- "test(="' \
    _ "$PLAN_DEFAULT"

# ---------------------------------------------------------------------------
# Test 2: LOUD tree-drift full-fallback. Under scope=failed_only, when the
# subset is refused because the sidecar tree_oid does not match (or the sidecar
# is absent), verify.sh must say so LOUDLY — never silently narrow-or-widen
# (PRD §4.3 / INV-4 storm escape). The diagnostic is a build-time `echo >&2`,
# so it appears in --print-plan STDERR and is host-independent (asserted
# UNCONDITIONALLY — not NEXTEST-guarded). `2>&1 >/dev/null` captures STDERR
# while dropping the (large) plan STDOUT.
# RED after impl-subset-apply: the eligibility gate already blocks the subset
# on a mismatch/absent sidecar, but emits no loud line.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 2: refused subset emits a LOUD 'retry refused: tree drift' line ---"

STDERR_MISMATCH="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=cafef00d \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    bash "$VERIFY" test --scope all --print-plan 2>&1 >/dev/null)" || true

assert "tree_oid mismatch (deadbeef sidecar vs cafef00d wanted): STDERR carries 'retry refused: tree drift'" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "retry refused: tree drift"' \
    _ "$STDERR_MISMATCH"

# Absent sidecar (nonexistent path) under scope=failed_only — same refusal,
# same loud line (the on-disk sidecar the retry tree-pins against is missing).
MISSING_SIDECAR="$_TMP/nonexistent-sidecar.json"
STDERR_ABSENT="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$MISSING_SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    bash "$VERIFY" test --scope all --print-plan 2>&1 >/dev/null)" || true

assert "absent sidecar under scope=failed_only: STDERR carries 'retry refused: tree drift'" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "retry refused: tree drift"' \
    _ "$STDERR_ABSENT"

# ---------------------------------------------------------------------------
# Test 3: LOUD no-subset full-fallback. Under scope=failed_only with a MATCHING
# sidecar (so the tree-OID gate is satisfied), a filter file that is absent /
# empty / unset must ALSO refuse the subset loudly (distinct 'retry refused: no
# subset' substring) and run FULL — never a silent whole-suite run masquerading
# as a subset. Fragment-absence is NEXTEST-guarded (command shape); the loud
# line is host-independent (unconditional).
# RED after impl-tree-drift-loud: an absent/empty/unset filter already yields no
# fragment, but no loud line.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: absent/empty/unset filter file emits a LOUD 'retry refused: no subset' line ---"

# DRY assertion helper: a refused (eligible-but-no-usable-subset) run must have
# no fragment on the debug nextest line AND a loud 'no subset' STDERR line.
_assert_no_subset_loud() {
    local _label="$1" _plan="$2" _err="$3"
    if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
        assert "no-subset ($_label): debug nextest line has NO test(= fragment (full pass)" \
            bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test(="' \
            _ "$_plan"
    fi
    assert "no-subset ($_label): STDERR carries 'retry refused: no subset'" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "retry refused: no subset"' \
        _ "$_err"
}

_ERR="$_TMP/err.txt"

# (i) filter env → a nonexistent path.
PLAN_NF="$(REIFY_VERIFY_RETRY_SCOPE=failed_only REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$_TMP/nonexistent-filter.txt" \
    bash "$VERIFY" test --scope all --print-plan 2>"$_ERR")" || true
_assert_no_subset_loud "nonexistent filter file" "$PLAN_NF" "$(cat "$_ERR")"

# (ii) filter env → an existing but EMPTY file (no non-blank lines).
EMPTY_FILTER="$_TMP/empty-filter.txt"
: > "$EMPTY_FILTER"
PLAN_EF="$(REIFY_VERIFY_RETRY_SCOPE=failed_only REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$EMPTY_FILTER" \
    bash "$VERIFY" test --scope all --print-plan 2>"$_ERR")" || true
_assert_no_subset_loud "empty filter file" "$PLAN_EF" "$(cat "$_ERR")"

# (iii) filter env unset entirely (base + both per-profile variants) — matching
# sidecar, so eligibility holds, but there is no subset to run.
PLAN_UF="$(env -u REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE \
    -u REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_DEBUG \
    -u REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_RELEASE \
    REIFY_VERIFY_RETRY_SCOPE=failed_only REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    bash "$VERIFY" test --scope all --print-plan 2>"$_ERR")" || true
_assert_no_subset_loud "filter env unset" "$PLAN_UF" "$(cat "$_ERR")"

# ---------------------------------------------------------------------------
# Test 4: SUBSET-SIZE CEILING (a construction-bug backstop: a subset ≈ the whole
# suite means DF built a bad subset — INV-4 storm escape, PRD §4.3/§11). The
# ceiling is a tunable heuristic, NOT a first-principles number, so this test
# INJECTS a small ceiling (REIFY_VERIFY_RETRY_MAX_SUBSET=3) and asserts the
# RELATION — n>ceiling ⇒ loud fallback, n<=ceiling ⇒ subset applies — never a
# magic production value.
# RED after impl-no-subset-loud: no ceiling exists, so 4 IDs still apply.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 4: subset larger than REIFY_VERIFY_RETRY_MAX_SUBSET falls back loudly ---"

# (a) 4 IDs with ceiling=3 ⇒ 4 > 3 ⇒ refuse loudly, run FULL.
FILTER4="$_TMP/filter4.txt"
printf 'c1::m::t1\nc1::m::t2\nc1::m::t3\nc1::m::t4\n' > "$FILTER4"
PLAN_BIG="$(REIFY_VERIFY_RETRY_SCOPE=failed_only REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER4" \
    REIFY_VERIFY_RETRY_MAX_SUBSET=3 \
    bash "$VERIFY" test --scope all --print-plan 2>"$_ERR")" || true
ERR_BIG="$(cat "$_ERR")"

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "ceiling (4 IDs > 3): debug nextest line has NO test(= fragment (full pass)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test(="' \
        _ "$PLAN_BIG"
fi
assert "ceiling (4 IDs > 3): STDERR carries 'retry refused: subset too large'" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "retry refused: subset too large"' \
    _ "$ERR_BIG"

# (b) 2 IDs with ceiling=3 ⇒ 2 <= 3 ⇒ subset applies, no 'too large' line.
PLAN_SMALL="$(REIFY_VERIFY_RETRY_SCOPE=failed_only REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    REIFY_VERIFY_RETRY_MAX_SUBSET=3 \
    bash "$VERIFY" test --scope all --print-plan 2>"$_ERR")" || true
ERR_SMALL="$(cat "$_ERR")"

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "ceiling (2 IDs <= 3): debug nextest line carries the subset test(=$ID1)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test(=$2)"' \
        _ "$PLAN_SMALL" "$ID1"
    assert "ceiling (2 IDs <= 3): debug nextest line carries the subset test(=$ID2)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "test(=$2)"' \
        _ "$PLAN_SMALL" "$ID2"
fi
assert "ceiling (2 IDs <= 3): STDERR has NO 'retry refused: subset too large' line" \
    bash -c '! printf "%s\n" "$1" | grep -qF -- "retry refused: subset too large"' \
    _ "$ERR_SMALL"

# ---------------------------------------------------------------------------
# Test 5: PER-PROFILE filter precedence (_DEBUG / _RELEASE) with base fallback.
# Each profile's nextest pass resolves its own subset, so a --profile both retry
# must apply the debug-specific IDs to the debug pass and the release-specific
# IDs to the release pass — with the base var as the fallback when a per-profile
# var is unset. The debug pass is the `cargo nextest run` line WITHOUT the
# ` --release` token; the release pass is the one WITH it. NEXTEST-guarded
# (command shape).
# RED after impl-ceiling: only the base var is honored, so the _DEBUG/_RELEASE
# files are ignored (debug line lacks test(=alpha::a)).
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 5: per-profile _DEBUG/_RELEASE filter precedence (+ base-var fallback) ---"

DBGID="alpha::a"
RELID="beta::b"
FDEBUG="$_TMP/fdebug.txt";   printf '%s\n' "$DBGID" > "$FDEBUG"
FRELEASE="$_TMP/frelease.txt"; printf '%s\n' "$RELID" > "$FRELEASE"

# (a) per-profile vars set (no base): debug pass ⇒ alpha::a, release ⇒ beta::b.
PLAN_PP="$(REIFY_VERIFY_RETRY_SCOPE=failed_only REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_DEBUG="$FDEBUG" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_RELEASE="$FRELEASE" \
    bash "$VERIFY" test --profile both --scope all --print-plan 2>/dev/null)" || true

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "per-profile: DEBUG nextest line (no --release) carries test(=$DBGID)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -v -- " --release" | grep -qF -- "test(=$2)"' \
        _ "$PLAN_PP" "$DBGID"
    assert "per-profile: DEBUG nextest line does NOT carry the release id test(=$RELID)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -v -- " --release" | grep -qF -- "test(=$2)"' \
        _ "$PLAN_PP" "$RELID"
    assert "per-profile: RELEASE nextest line (--release) carries test(=$RELID)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -- " --release" | grep -qF -- "test(=$2)"' \
        _ "$PLAN_PP" "$RELID"
    assert "per-profile: RELEASE nextest line does NOT carry the debug id test(=$DBGID)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -- " --release" | grep -qF -- "test(=$2)"' \
        _ "$PLAN_PP" "$DBGID"
fi

# (b) base var only (no per-profile): BOTH passes use the base file's IDs.
PLAN_BASE_BOTH="$(env -u REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_DEBUG \
    -u REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_RELEASE \
    REIFY_VERIFY_RETRY_SCOPE=failed_only REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    bash "$VERIFY" test --profile both --scope all --print-plan 2>/dev/null)" || true

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "base fallback: DEBUG nextest line carries base test(=$ID1)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -v -- " --release" | grep -qF -- "test(=$2)"' \
        _ "$PLAN_BASE_BOTH" "$ID1"
    assert "base fallback: RELEASE nextest line carries base test(=$ID1)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -- " --release" | grep -qF -- "test(=$2)"' \
        _ "$PLAN_BASE_BOTH" "$ID1"
fi

# ---------------------------------------------------------------------------
# Test 6: attempt-0 sidecar STAMP (plan-shape, mirroring test_reify_bin_freshness
# Check 18). On a full attempt-0 merge run the plan must emit a line writing the
# reify-verify-attempt.json sidecar (tree_oid via `git rev-parse HEAD:`,
# planned_profiles, timestamp), ordered BEFORE the first nextest pass (but still after
# the pre-build poles: lint/compile wave, gui block, run_all.sh) so the pin is
# written on a RED attempt-0 too — task 5548, PRD verify-retry-failed-only α2.
# The sidecar asserts "the warm target/ was built FROM this tree", NOT "this
# tree passed the gate". A retry (scope=failed_only) must NOT re-stamp; a
# per-task plan must be unchanged. Comment lines are stripped (grep -v '^#')
# so indices count command leaves only, per the Check-18 idiom.
# The ordering asserts below are plan SHAPE only; Test 6b further down EXECUTES
# the extracted stamp line hermetically (one plan line, no cargo) to prove the
# pin is actually reachable on a red pole — sibling task δ's e2e boundary test
# still owns the real-nextest end-to-end write, so the level split stays
# explicit and this is not read as G7 duplication.
# RED at base (task 5548 not yet applied): the stamp is the LAST line of
# add_test_passes, so the inverted ordering assert below and Test 6b both fail.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 6: attempt-0 merge plan stamps reify-verify-attempt.json before the first pass (survives red); retry & task do not ---"

MERGE_PLAN="$(DF_VERIFY_ROLE=merge bash "$VERIFY" all --scope all --print-plan 2>/dev/null | grep -v '^#')" || true

assert "merge attempt-0: a plan line writes reify-verify-attempt.json via 'git rev-parse HEAD:'" \
    bash -c 'printf "%s\n" "$1" | grep -F "reify-verify-attempt.json" | grep -qF "git rev-parse HEAD:"' \
    _ "$MERGE_PLAN"

assert "merge attempt-0: sidecar stamp line references tree_oid, planned_profiles, timestamp" \
    bash -c '
        L=$(printf "%s\n" "$1" | grep -F "reify-verify-attempt.json" | head -1)
        printf "%s\n" "$L" | grep -qF "tree_oid" && \
        printf "%s\n" "$L" | grep -qF "planned_profiles" && \
        printf "%s\n" "$L" | grep -qF "timestamp"
    ' _ "$MERGE_PLAN"

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "merge attempt-0: sidecar stamp index < first cargo nextest run index (stamp BEFORE the first pass, survives a red test pole)" \
        bash -c '
            STAMP_IDX=$(printf "%s\n" "$1" | grep -nF "reify-verify-attempt.json" | head -1 | cut -d: -f1)
            FIRST_NEXTEST_IDX=$(printf "%s\n" "$1" | grep -nE "(^| )cargo nextest run " | head -1 | cut -d: -f1)
            [ -n "$STAMP_IDX" ] && [ -n "$FIRST_NEXTEST_IDX" ] && [ "$STAMP_IDX" -lt "$FIRST_NEXTEST_IDX" ]
        ' _ "$MERGE_PLAN"
fi

# Companion: the stamp must still land AFTER the pre-build poles (lint/compile
# wave, gui block, run_all.sh) — it only moved above the FIRST nextest pass,
# not to the very top of the plan. Vacuously PASSES when RUNALL_IDX is empty
# (the run_all.sh plan line is suppressed under REIFY_INFRA_SUITE_ACTIVE —
# verify.sh ~:2110 — so a nested-suite caller must not spuriously FAIL this
# secondary sanity check).
assert "merge attempt-0: sidecar stamp index > run_all.sh index (still after the pre-build poles), or vacuous if absent" \
    bash -c '
        RUNALL_IDX=$(printf "%s\n" "$1" | grep -nF "run_all.sh" | head -1 | cut -d: -f1)
        STAMP_IDX=$(printf "%s\n" "$1" | grep -nF "reify-verify-attempt.json" | head -1 | cut -d: -f1)
        [ -z "$RUNALL_IDX" ] && exit 0
        [ -n "$STAMP_IDX" ] && [ "$STAMP_IDX" -gt "$RUNALL_IDX" ]
    ' _ "$MERGE_PLAN"

# ---------------------------------------------------------------------------
# Test 6b: RED-ATTEMPT-0 SURVIVAL (executed, not just positional). Rebuilds the
# real merge plan with REIFY_VERIFY_ATTEMPT_SIDECAR pointed at a sandbox (named
# distinctly from the real default filename — see the RED_SIDECAR comment
# below — so this actually exercises the override) so the build-time-baked
# path in the emitted stamp line writes there; extracts, IN PLAN ORDER (never
# hardcoded — derived from the plan's own grep -n indices), the {stamp line,
# first `cargo nextest run` line} sub-sequence; replays them in a sandbox CWD
# through a minimal first-failure-exit loop mirroring the real executor's
# per-command dispatch (verify.sh:2442-2480): the real loop runs most plan
# lines — including this stamp line — through `reaper_run_in_pgroup "$_cmd"`
# (bare `eval` there is reserved for `_VERIFY_NODE_BG_PID` lines only). This
# replay uses bare `eval` as an acceptable stand-in: the stamp line is a
# side-effect-only file write with no process-group semantics to exercise.
# The nextest line is substituted with `false` as a simulated RED test pole.
# Proves the pin is USABLE after a red pole, not merely early: at base the
# plan order is [nextest, stamp], so this replay hits the substituted `false`
# first and never reaches the stamp — RED for a structural reason, not a
# cosmetic one. GIT_DIR is pointed at this worktree's real gitdir so `git
# rev-parse HEAD:` in the replayed stamp line returns the REAL tree OID (not
# the `|| echo unknown` sentinel) from a sandbox CWD with no .git of its own —
# that is what makes the written pin actually usable by the drift guard, which
# the last two asserts confirm by feeding it back through the real
# (unmodified) guard at scripts/verify.sh:~1684-1714.
# The stamp-line fixture itself (RED_STAMP_IDX) is asserted UNCONDITIONALLY:
# the stamp is emitted independently of nextest availability, so a missing
# stamp line is a loud FAIL, not a silent skip. Only the replay and its
# downstream assertions are guarded on NEXTEST_AVAILABLE/RED_NEXTEST_IDX,
# since a nextest-less host legitimately emits `cargo test` instead of `cargo
# nextest run ` — that branch echoes an explicit skip note rather than
# asserting nothing.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 6b: a RED attempt-0 (simulated failing test pole) still leaves a usable tree-OID pin ---"

RED_DIR="$_TMP/red"
mkdir -p "$RED_DIR/target"
# Deliberately NOT named reify-verify-attempt.json: that is verify.sh's own
# DEFAULT relative path, and $RED_DIR is also the replay's CWD below, so a
# same-named override would resolve to the exact same file the default would
# use — making this fixture unable to distinguish "the override was honored"
# from "the override was silently ignored and the writer fell back to its
# default". A distinctly-named sidecar under target/ (still satisfying the
# `test -d target` guard) closes that gap.
RED_SIDECAR="$RED_DIR/target/red-pin.json"

RED_PLAN="$(REIFY_VERIFY_ATTEMPT_SIDECAR="$RED_SIDECAR" \
    DF_VERIFY_ROLE=merge bash "$VERIFY" all --scope all --print-plan 2>/dev/null | grep -v '^#')" || true

# Anchor on "tree_oid" (part of the printf's own format string) rather than the
# sidecar filename: RED_SIDECAR is deliberately renamed off the default above,
# so the plan's redirect target no longer contains the literal
# "reify-verify-attempt.json" substring that MERGE_PLAN's extraction (above)
# relies on. "tree_oid" uniquely identifies the stamp line regardless of the
# configured sidecar path (verified: exactly one match in both the raw and
# comment-stripped plan).
RED_STAMP_IDX="$(printf '%s\n' "$RED_PLAN" | grep -nF "tree_oid" | head -1 | cut -d: -f1)" || true
RED_STAMP_LINE="$(printf '%s\n' "$RED_PLAN" | grep -F "tree_oid" | head -1)" || true
RED_NEXTEST_IDX="$(printf '%s\n' "$RED_PLAN" | grep -nE "(^| )cargo nextest run " | head -1 | cut -d: -f1)" || true

# The stamp-line fixture is unconditional: it is emitted independently of
# nextest availability, so its absence is a real FAIL, not a silent skip
# (contrast the nextest-gated block below, which legitimately skips on a
# nextest-less host).
assert "red-attempt-0 fixture: RED_PLAN carries a stamp line" \
    bash -c '[ -n "$1" ]' _ "$RED_STAMP_IDX"

if [ "$NEXTEST_AVAILABLE" -eq 1 ] && [ -n "$RED_NEXTEST_IDX" ]; then
    # Order the replay sequence from the plan's OWN indices — never hardcode
    # which comes first, since that is exactly the property under test.
    if [ "$RED_STAMP_IDX" -lt "$RED_NEXTEST_IDX" ]; then
        REPLAY_SEQ=("$RED_STAMP_LINE" "false")
    else
        REPLAY_SEQ=("false" "$RED_STAMP_LINE")
    fi

    REPO_GIT_DIR="$(git -C "$REPO_ROOT" rev-parse --absolute-git-dir)"
    EXPECT_TREE="$(git -C "$REPO_ROOT" rev-parse HEAD:)"

    RED_RC=0
    (
        cd "$RED_DIR"
        export GIT_DIR="$REPO_GIT_DIR"
        for _c in "${REPLAY_SEQ[@]}"; do
            eval "$_c" || { _rc=$?; exit "$_rc"; }
        done
    ) || RED_RC=$?

    assert "red-attempt-0: the simulated failing test pole really failed (nonzero replay exit)" \
        bash -c '[ "$1" -ne 0 ]' _ "$RED_RC"

    assert "red-attempt-0: the tree-OID pin file EXISTS despite the failure" \
        bash -c 'test -f "$1"' _ "$RED_SIDECAR"

    assert "red-attempt-0: the pin holds the REAL tree_oid (not the || echo unknown sentinel)" \
        bash -c '
            grep -qF "\"tree_oid\":\"$2\"" "$1" && ! grep -qF "\"tree_oid\":\"unknown\"" "$1"
        ' _ "$RED_SIDECAR" "$EXPECT_TREE"

    # Close the loop on the user-observable signal: feed the red-written
    # sidecar back into a FRESH scope=failed_only merge plan and assert the
    # real (unmodified) drift guard now takes the eligible branch. Reuses the
    # existing FILTER/ID1 fixture (:69-72).
    LOOP_ERR="$RED_DIR/loop-err.txt"
    LOOP_PLAN="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
        REIFY_VERIFY_RETRY_TREE_OID="$EXPECT_TREE" \
        REIFY_VERIFY_ATTEMPT_SIDECAR="$RED_SIDECAR" \
        REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
        DF_VERIFY_ROLE=merge bash "$VERIFY" test --scope all --print-plan 2>"$LOOP_ERR")" || true

    assert "red-attempt-0: retry against the red-written pin does NOT log 'retry refused: tree drift'" \
        bash -c '! grep -qF "retry refused: tree drift" "$1"' \
        _ "$LOOP_ERR"

    assert "red-attempt-0: retry against the red-written pin applies the subset (debug nextest line carries test(=$ID1))" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -v -- " --release" | grep -qF -- "test(=$2)"' \
        _ "$LOOP_PLAN" "$ID1"
else
    echo "(skipped Test 6b: nextest unavailable)"
fi

# (b) A retry (scope=failed_only) merge run must NOT re-stamp (DF re-runs a full
# attempt-0 for a new tree; a full-fallback retry deliberately does not stamp).
MERGE_RETRY_PLAN="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    DF_VERIFY_ROLE=merge bash "$VERIFY" all --scope all --print-plan 2>/dev/null | grep -v '^#')" || true

assert "merge retry (scope=failed_only): NO reify-verify-attempt.json stamp line" \
    bash -c '! printf "%s\n" "$1" | grep -qF "reify-verify-attempt.json"' \
    _ "$MERGE_RETRY_PLAN"

# (c) A per-task plan (role=task) must be byte-unchanged — no stamp line.
TASK_PLAN="$(bash "$VERIFY" test --scope all --print-plan 2>/dev/null | grep -v '^#')" || true

assert "task plan: NO reify-verify-attempt.json stamp line (per-task plans unchanged)" \
    bash -c '! printf "%s\n" "$1" | grep -qF "reify-verify-attempt.json"' \
    _ "$TASK_PLAN"

# ---------------------------------------------------------------------------
# Test 7: retry-subset × heavy-exclude COMPOSITION (reviewer_comprehensive).
# The retry fragment ` -E 'test(=…)'` is emitted next to the gate fragment
# `_GATE_HEAVY_EXCLUDE` ` -E "not (heavy)"`. nextest 0.9.136 combines multiple
# `-E`/--filterset expressions with a set UNION (OR), NOT an intersection — so
# on merge+REIFY_GATE_EXCLUDE_HEAVY=1 the emitted `(not heavy) OR {subset}`
# degrades to the whole non-heavy suite, silently defeating the "re-run ONLY
# the did-not-pass tests" contract in exactly its target configuration (the
# gate-exclude knob and the failed-only retry co-occur on merge by design).
# The fix folds both filtersets into a SINGLE `-E '(not (heavy)) & {subset}'`
# term; a single-`-E` invariant (count==1) structurally prevents the union
# from recurring. The real heavy atom is drawn from the single source of truth
# (scripts/heavy-test-filter-lib.sh, mirroring test_verify_gate_exclude_heavy)
# so the fixture cannot silently drift.
# RED at current HEAD (after impl-doc-envs): emit_nextest_pass appends
# `${_GATE_HEAVY_EXCLUDE}` and `${_retry_filter_frag}` as TWO separate `-E`
# fragments, so (a) finds 2 and (c) still sees the standalone `-E "not (`.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 7: retry subset folds into a SINGLE -E with the heavy filter (never a 2nd -E union) ---"

# Real heavy atom from the single source of truth — the fixture cannot drift.
HEAVY_LIB="$REPO_ROOT/scripts/heavy-test-filter-lib.sh"
[ -f "$HEAVY_LIB" ] || { echo "ERROR: $HEAVY_LIB not found (task 4912/A1 not landed?)"; exit 1; }
# shellcheck source=scripts/heavy-test-filter-lib.sh
source "$HEAVY_LIB"
HEAVY_ATOM="binary(determinism)"
case "${REIFY_HEAVY_NEXTEST_FILTER:-}" in
    *"$HEAVY_ATOM"*) ;;
    *) echo "ERROR: fixture atom '$HEAVY_ATOM' not in REIFY_HEAVY_NEXTEST_FILTER — drifted from $HEAVY_LIB"; exit 1 ;;
esac
# The OLD (pre-fold) standalone gate fragment: a double-quoted `-E "not (`.
# After the fold the heavy negation lives INSIDE a single-quoted combined term,
# so this exact literal must be ABSENT on the folded line (but PRESENT on the
# retry-inactive regression line). Defined as a literal (real double-quote) and
# passed as an arg, mirroring test_verify_gate_exclude_heavy.sh's NOT_PATTERN.
NOT_DQ='-E "not ('

# The single config where the union bug bites: merge + heavy-exclude + an
# eligible retry (matching sidecar tree_oid + within-ceiling 2-line subset).
PLAN_FOLD="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    DF_VERIFY_ROLE=merge REIFY_GATE_EXCLUDE_HEAVY=1 \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true
NLINE_FOLD="$(printf '%s\n' "$PLAN_FOLD" | grep -E "(^| )cargo nextest run " | grep -v -- " --release" | head -1)" || true

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    # (a) ANTI-UNION CRUX: exactly ONE ` -E ` on the folded line, never two.
    assert "fold: merge+heavy+retry debug nextest line has EXACTLY ONE ' -E ' (no 2nd -E union)" \
        bash -c '[ "$(printf "%s\n" "$1" | grep -oF -- " -E " | wc -l)" -eq 1 ]' \
        _ "$NLINE_FOLD"

    # (b) the single -E term intersects BOTH groups (gate `not (heavy)` & subset).
    assert "fold: single -E term carries the heavy negation 'not (' AND the real heavy atom ($HEAVY_ATOM)" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "not (" && printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$NLINE_FOLD" "$HEAVY_ATOM"
    assert "fold: single -E term carries BOTH exact retry ids test(=$ID1) and test(=$ID2)" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "test(=$2)" && printf "%s\n" "$1" | grep -qF -- "test(=$3)"' \
        _ "$NLINE_FOLD" "$ID1" "$ID2"
    assert "fold: gate and retry are INTERSECTED (a '& (test(=' join is present, not a union)" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "& (test(="' \
        _ "$NLINE_FOLD"

    # (c) OLD broken shape absent: the heavy negation must NOT survive as its own
    #     double-quoted `-E "not (` fragment (that WAS the union-bug second -E).
    assert "fold: OLD standalone double-quote fragment ($NOT_DQ) is ABSENT (heavy negation folded in)" \
        bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$NLINE_FOLD" "$NOT_DQ"
fi

# (d) REGRESSION / byte-identical: merge+heavy with retry INACTIVE (no
# REIFY_VERIFY_RETRY_SCOPE) must be untouched — the standalone double-quoted
# `-E "not (` fragment stays, and there is exactly one ` -E ` (the non-retry
# gate path is not folded).
PLAN_INACTIVE="$(DF_VERIFY_ROLE=merge REIFY_GATE_EXCLUDE_HEAVY=1 \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true
NLINE_INACTIVE="$(printf '%s\n' "$PLAN_INACTIVE" | grep -E "(^| )cargo nextest run " | grep -v -- " --release" | head -1)" || true

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "regression: merge+heavy, retry INACTIVE — line keeps standalone $NOT_DQ fragment" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$NLINE_INACTIVE" "$NOT_DQ"
    assert "regression: merge+heavy, retry INACTIVE — line has EXACTLY ONE ' -E ' (untouched)" \
        bash -c '[ "$(printf "%s\n" "$1" | grep -oF -- " -E " | wc -l)" -eq 1 ]' \
        _ "$NLINE_INACTIVE"
fi

# Secondary robustness: offline role + eligible retry folds the POSITIVE heavy
# select `(heavy)` with the subset while PRESERVING the non-filterset
# `--run-ignored all` flag. Offline emits a single (release) nextest line.
PLAN_OFFLINE="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    DF_VERIFY_ROLE=offline \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true
NLINE_OFFLINE="$(printf '%s\n' "$PLAN_OFFLINE" | grep -E "(^| )cargo nextest run " | head -1)" || true

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "offline fold: nextest line has EXACTLY ONE ' -E ' (positive heavy select folded with subset)" \
        bash -c '[ "$(printf "%s\n" "$1" | grep -oF -- " -E " | wc -l)" -eq 1 ]' \
        _ "$NLINE_OFFLINE"
    assert "offline fold: gate & retry intersected ('& (test(=' present)" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "& (test(="' \
        _ "$NLINE_OFFLINE"
    assert "offline fold: non-filterset flag --run-ignored all PRESERVED" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "--run-ignored all"' \
        _ "$NLINE_OFFLINE"
fi

test_summary
