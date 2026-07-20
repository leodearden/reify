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

test_summary
