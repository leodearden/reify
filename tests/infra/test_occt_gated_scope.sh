#!/usr/bin/env bash
# Infrastructure test for task 2000.
# Validates that the OCCT-touching crate list is correct and that
# dark-factory-orchestrator.yaml routes exactly those crates through the flock gate.
#
# Assertions:
#   1. scripts/occt-touching-crates.txt exists and is non-empty (after stripping comments/blanks).
#   2. Every declared entry is a real workspace member.
#   3. Declared set EQUALS the cargo-tree-derived OCCT-touching set (drift catcher).
#   4. Each declared crate has -p <crate> in the gated debug AND release invocations.
#   5. The gated invocations do NOT contain --workspace.
#   6. Each declared crate has --exclude <crate> in the ungated debug AND release invocations.
#   7. Each ungated invocation is wrapped with timeout --kill-after=60 [0-9]+m.
#   8. Gated debug invocation appears before ungated debug invocation (ordering).
#   9. Gated release invocation appears before ungated release invocation (ordering).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# Source the shared OCCT-scope library: occt_declared_set + occt_touching_set are
# the SINGLE implementations of the declared and cargo-metadata-derived sets,
# shared with scripts/verify.sh so the two cannot drift apart (Test 3 below is the
# drift catcher that proves they agree).
[ -f "$REPO_ROOT/scripts/occt-scope-lib.sh" ] || { echo "ERROR: occt-scope-lib.sh not found at $REPO_ROOT/scripts/occt-scope-lib.sh"; exit 1; }
source "$REPO_ROOT/scripts/occt-scope-lib.sh"

CRATE_LIST="$REPO_ROOT/scripts/occt-touching-crates.txt"

echo "=== OCCT gated scope tests ==="

# ---------------------------------------------------------------------------
# Test 1: declared list file exists and is non-empty
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: scripts/occt-touching-crates.txt exists and is non-empty ---"

assert "scripts/occt-touching-crates.txt exists" \
    test -f "$CRATE_LIST"

assert "scripts/occt-touching-crates.txt is non-empty after stripping comments/blanks" \
    bash -c "[ -f '$CRATE_LIST' ] && [ -n \"\$(grep -v '^\s*#' '$CRATE_LIST' | grep -v '^\s*\$')\" ]"

# ---------------------------------------------------------------------------
# Test 2: every declared entry is a real workspace member
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 2: every declared crate is a real workspace member ---"

# Collect workspace members via cargo metadata.
WORKSPACE_MEMBERS="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | python3 -c "import sys,json; m=json.load(sys.stdin); [print(p['name']) for p in m['packages']]")"

# Declared set comes from the shared library (single source of truth).
DECLARED_CRATES="$(occt_declared_set)"

while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "declared crate '$crate' is a real workspace member" \
        grep -qxF "$crate" <<< "$WORKSPACE_MEMBERS"
done <<< "$DECLARED_CRATES"

# ---------------------------------------------------------------------------
# Test 3: declared set equals cargo-metadata-derived OCCT-touching set
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: declared set equals cargo-metadata-derived OCCT-touching set ---"

# Actual OCCT-touching set comes from the shared library (single source of
# truth): a single `cargo metadata` invocation over the workspace-unified
# resolve graph. The full rationale for that approach lives in the
# occt_touching_set doc comment in scripts/occt-scope-lib.sh.
ACTUAL_TOUCHING="$(occt_touching_set)"

# Write both sets to temp files and diff for actionable failure output.
# On mismatch the diff is printed so the reader can see exactly which crate
# drifted without re-running locally.
_DECLARED_TMP="$(mktemp)"
_ACTUAL_TMP="$(mktemp)"
echo "$DECLARED_CRATES" | sort > "$_DECLARED_TMP"
echo "$ACTUAL_TOUCHING" | sort > "$_ACTUAL_TMP"
_DIFF_OUT="$(diff "$_DECLARED_TMP" "$_ACTUAL_TMP" 2>&1 || true)"
rm -f "$_DECLARED_TMP" "$_ACTUAL_TMP"
if [ -n "$_DIFF_OUT" ]; then
    echo "  OCCT-touching set drift detected (< declared, > cargo-metadata-derived):"
    echo "$_DIFF_OUT" | sed 's/^/    /'
fi
assert "declared OCCT-touching set equals cargo-metadata-derived set (no missing or extra entries)" \
    test -z "$_DIFF_OUT"

# ---------------------------------------------------------------------------
# Nextest occt-group assertions (task 4451, task 4503/γ):
# (a) [test-groups] occt max-threads = 24 (env-driven, default 24).
#     task 4451 raised it from inert 1 to 4; task 4503/γ raises 4→24 with the
#     held-slot semaphore (task β/4502) as the cross-run safety bound.
# (b) [[profile.default.overrides]] filter for test-group 'occt' contains
#     package(<crate>) for every declared OCCT crate (drift catch: a missing
#     crate would escape the max-threads cap and run unbounded in the pool).
# ---------------------------------------------------------------------------
NEXTEST_TOML="$REPO_ROOT/.config/nextest.toml"

echo ""
echo "--- Nextest occt-group (task 4503/γ): max-threads = 24 (env-driven, default 24) ---"
assert "nextest.toml: [test-groups] occt has max-threads = 24 (env-driven, default 24)" \
    grep -qF 'occt = { max-threads = 24 }' "$NEXTEST_TOML"

echo ""
echo "--- Nextest occt-group (task 4451): filter drift check (every declared crate is package()-filtered) ---"
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "nextest.toml occt-group filter contains package($crate)" \
        grep -qF "package($crate)" "$NEXTEST_TOML"
done <<< "$DECLARED_CRATES"

# ---------------------------------------------------------------------------
# Tests 4–8: folded-contract plan-shape assertions (task 4451)
# Source of truth: scripts/verify.sh --print-plan (the oracle the orchestrator
# calls). --profile both --scope all forces the full plan; env lines stripped.
#
# Folded contract: (1) no cargo-test-occt-gated.sh invocation; (2) full-workspace
# debug pass is `cargo nextest run --workspace` with NO --exclude; (3) release pass
# includes -p reify-eval (OCCT∩release-sensitive, folded); (4) workspace pass is
# wrapped in the standard outer timeout.
# RED against current verify.sh (which still emits the gated pass).
# ---------------------------------------------------------------------------
TEST_PLAN_SEGS="$(env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --print-plan | grep -v '^#')"
export TEST_PLAN_SEGS

echo ""
echo "--- Test 4 (task 4451): plan has NO cargo-test-occt-gated.sh invocation (fold) ---"
assert "plan contains NO cargo-test-occt-gated.sh (gated pass dropped, OCCT in nextest pool)" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'cargo-test-occt-gated\.sh'"

# NEXTEST-LESS HOST AUDIT (task 5604): Tests 5-8 need NO change — they are
# host-independent BY CONSTRUCTION, and this is the shape to copy. Why the
# alternation is sound at all (verify.sh's two emission branches share their
# selector fragment): see the canonical "WHY THE FALLBACK IS SHAPE-IDENTICAL"
# block in tests/infra/test_verify_nextest_absent_suites.sh.
#
# Each one EXTRACTS with the `cargo (test|nextest run)` alternation into
# FULL_WS_DEBUG / NEXTEST_RELEASE, asserts the extract is NON-EMPTY
# (`test -n`), and only THEN negative-greps the extract. That ordering is what
# makes them non-vacuous: a negative grep run directly against the whole plan
# passes for free on a nextest-less host (its needle matches nothing), whereas
# here the `test -n` assert fails first if the pass is missing. Sibling suites
# that instead negative-grepped a hard-coded `cargo nextest run` against the
# raw plan were silently asserting nothing on that path — see task 5604's fix
# to test_verify_scope.sh / test_verify_failfast_order.sh.
#
# Two deliberate exceptions further down, neither of them a defect:
#   - Test 9 (--config-file / reify-nextest-occt) is GENUINELY nextest-only:
#     cargo test has no --config-file. It carries an explicit PLAN_HAS_NEXTEST
#     guard with a written reason — the correct form when a property really is
#     runner-specific, as opposed to widening the grep. The guard is written
#     OUTSIDE the assert (skip-and-echo), not as an early `exit 0` inside the
#     assert body; see the "GUARD FORM MATTERS" note at Test 9 for why that
#     distinction is what keeps S7's floor load-bearing.
#   - Tests 9b/9c are negative regression guards whose subject is a
#     `cargo nextest run` line. When NEXTEST=0 no such line exists, so they are
#     INERT by construction rather than vacuous by accident: the thing they
#     forbid (a broken --config form on a nextest line) cannot occur where
#     there are no nextest lines at all.
#
# Pinned mechanically by S7 in test_verify_nextest_absent_suites.sh, floor 57
# = 58 ambient minus Test 9's runner-specific skip, so this verdict fails
# loudly if a future edit guards a further assert away here.  (Floor raised
# from 48/49 by task 5984; the 1-assert delta is unchanged.)
echo ""
echo "--- Test 5 (task 4451): full-workspace nextest pass has --workspace with NO --exclude ---"
FULL_WS_DEBUG="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -E 'cargo (test|nextest run) --workspace' | grep -v -- '--release' || true)"
export FULL_WS_DEBUG

assert "full-workspace debug pass exists (cargo (test|nextest run) --workspace)" \
    test -n "$FULL_WS_DEBUG"
assert "full-workspace debug nextest pass has NO --exclude (OCCT folded into nextest pool)" \
    bash -c "! printf '%s' \"\$FULL_WS_DEBUG\" | grep -q -- '--exclude'"

echo ""
echo "--- Test 6 (task 4451): no OCCT crate is --exclude'd from the workspace nextest pass ---"
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "workspace nextest pass does NOT have '--exclude $crate' (OCCT folded in)" \
        bash -c "! printf '%s' \"\$FULL_WS_DEBUG\" | grep -qF ' --exclude $crate'"
done <<< "$DECLARED_CRATES"

echo ""
echo "--- Test 7 (task 4451): release nextest pass includes -p reify-eval (folded) ---"
NEXTEST_RELEASE="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -v 'cargo-test-occt-gated\.sh' \
    | grep -E 'cargo (test|nextest run)' \
    | grep -- '--release' || true)"
export NEXTEST_RELEASE

assert "release pass exists (cargo (test|nextest run) ... --release, no gated wrapper)" \
    test -n "$NEXTEST_RELEASE"
assert "release nextest pass has '-p reify-eval' (OCCT∩release-sensitive, folded)" \
    bash -c "printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' -p reify-eval'"
assert "release nextest pass has '--release'" \
    bash -c "printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' --release'"
assert "release nextest pass does NOT have '--workspace' (sensitivity-scoped)" \
    bash -c "! printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' --workspace'"

echo ""
echo "--- Test 8 (task 4451): workspace nextest pass is wrapped in outer timeout ---"
assert "workspace nextest pass is wrapped in 'timeout --kill-after=60 [0-9]+m'" \
    bash -c "printf '%s' \"\$FULL_WS_DEBUG\" | grep -qE 'timeout[[:space:]]+--kill-after=60[[:space:]]+[0-9]+m[[:space:]]'"

# ---------------------------------------------------------------------------
# Tests 9–12 (task 4503/γ): --config-file plan assertions for the env-driven
# occt nextest group cap (REIFY_OCCT_NEXTEST_MAX_THREADS, default 24).
#
# Mechanism: scripts/gen-nextest-config.sh generates a full copy of
# .config/nextest.toml with the occt literal rewritten to the resolved cap,
# and prints the temp path to stdout.  scripts/verify.sh passes that path as
# `cargo nextest run ... --config-file <real-tmp>` in EXECUTE mode.
#
# --print-plan mode (hermetic oracle): verify.sh emits a static placeholder
# path (`…/reify-nextest-occt.<print-plan-placeholder>`) instead of calling
# gen-nextest-config.sh.  No subprocess is spawned and no temp file is created
# during plan inspection.  The placeholder path is NOT re-runnable; only the
# execute path produces a real config file.  Test 9 checks the 'reify-nextest-occt'
# prefix (present in both the real path and the placeholder), not a real file.
#
# Tests 10-11 (compile-free parse, task 4613): gen-nextest-config.sh output is
# parsed directly for the [test-groups] occt max-threads integer — no
# cargo/nextest invocation, no workspace compile.  The old behavioral check via
# show-config was dropped: it forced a full workspace build on cold cache and
# blew run_all.sh's 20-min budget (esc-4607-213).  The --config-file mechanism
# was verified once on nextest 0.9.136 (documented in gen-nextest-config.sh).
# The TOML cap contract is covered by the parse plus Tests 12a/12b.
#
# NOTE: the broken cargo-config form `--config 'test-groups.occt.max-threads=N'`
# (the step-3 mechanism) is a NO-OP for nextest test-groups — it overrides CARGO
# config, not nextest's own test-groups (verified on nextest 0.9.136).  That form
# must not be re-shipped; see regression guard below.
#
# Guard: plan-shape assertions are only meaningful when the plan actually uses
# cargo nextest run.  When NEXTEST=0 the plan uses cargo test (no --config-file
# support), so skip the plan-shape checks (vacuous pass).
#
# GUARD FORM MATTERS (task 5604 amendment).  The skip is expressed OUTSIDE the
# assert (`if [ "$PLAN_HAS_NEXTEST" -gt 0 ]; then assert ...; else echo SKIP; fi`)
# rather than as an early `exit 0` INSIDE the assert body.  test_helpers.sh:42
# counts any zero-exit checker as a PASS, so an in-body `exit 0` guard still
# increments PASS while checking nothing — invisible to S7's pass floor in
# test_verify_nextest_absent_suites.sh, which is the mechanism meant to catch
# exactly this.  The outside form makes the skip VISIBLE as a count of 57 rather
# than 58 on a nextest-less host, so a future guard that silently swallows
# coverage trips the floor.  Copy this shape, not an in-body `exit 0`.
# ---------------------------------------------------------------------------
PLAN_HAS_NEXTEST="$(printf '%s\n' "$TEST_PLAN_SEGS" | grep -c 'cargo nextest run' || true)"
GEN_CFG="$REPO_ROOT/scripts/gen-nextest-config.sh"

echo ""
echo "--- Tests 9–12 (task 4503/γ): --config-file plan assertions for env-driven occt cap ---"

# Test 9 (plan-shape): every cargo nextest run line carries --config-file <path>
# where the path contains the 'reify-nextest-occt' prefix.  In --print-plan mode
# (used here) this is the static placeholder; in execute mode it is the real temp path.
#
# Genuinely nextest-only: `cargo test` has no --config-file, so this cannot be
# fixed by widening the grep the way task 5604 fixed its siblings.  Guarded in
# the skip-outside-assert form per the note above — this is the ONE assert
# behind S7's 57-nextest-less / 58-ambient delta.
if [ "$PLAN_HAS_NEXTEST" -gt 0 ]; then
    assert "every 'cargo nextest run' plan line carries '--config-file' with 'reify-nextest-occt' path" \
        bash -c "
            bad=\$(printf '%s\n' \"\$TEST_PLAN_SEGS\" \
                | grep 'cargo nextest run' \
                | grep -v -- '--config-file.*reify-nextest-occt' || true)
            [ -z \"\$bad\" ]
        "
else
    echo "  SKIP: Test 9 (--config-file / reify-nextest-occt) — the plan has no"
    echo "        'cargo nextest run' line on this host (nextest=0) and cargo test"
    echo "        has no --config-file, so the property is genuinely runner-specific."
    echo "        S7's floor in test_verify_nextest_absent_suites.sh is pinned at 57"
    echo "        (= 58 ambient minus this assert) to account for exactly this skip."
fi

# Regression guard: NO cargo nextest run line may carry the broken Cargo-config form.
# cargo --config overrides CARGO configuration only; test-groups is a nextest config
# key and --config is a silent no-op for it (verified empirically on nextest 0.9.136).
assert "NO 'cargo nextest run' line carries the broken cargo-config form --config test-groups.occt.max-threads" \
    bash -c "
        bad=\$(printf '%s\n' \"\$TEST_PLAN_SEGS\" \
            | grep 'cargo nextest run' \
            | grep -F -- \"--config 'test-groups.occt.max-threads\" || true)
        [ -z \"\$bad\" ]
    "

# Regression guard: no live (non-comment) invocation of the workspace-compiling
# nextest show-config command in this script — that command enumerates test
# binaries and forces a full workspace compile on cold cache, blowing
# run_all.sh's 20-min budget (esc-4607-213). Tests 10-11 parse the generated
# config file directly (compile-free). The needle is split to prevent self-match.
assert "no live nextest show-config invocation in this script (compile-free Tests 10-11)" \
    bash -c "
        _SELF='${BASH_SOURCE[0]}'
        _NEEDLE=\"cargo nextest show\"\"-config\"
        bad=\$(grep -F \"\$_NEEDLE\" \"\$_SELF\" | grep -v '^[[:space:]]*#' || true)
        [ -z \"\$bad\" ]
    "

# Test 10 (compile-free parse, default): gen-nextest-config.sh with
# REIFY_OCCT_NEXTEST_MAX_THREADS unset and REIFY_OCCT_NPROC=32/MEMTOTAL_GIB=128
# injected (host-independent: workstation profile min(24,32,64)=24, task 4621)
# produces a config file whose [test-groups] section has occt max-threads = 24.
# Section-scoped awk extraction; no cargo/nextest invocation, no workspace compile
# (task 4613, esc-4607-213).
assert "gen-nextest-config.sh NPROC=32/MEM=128 (workstation profile): [test-groups] occt max-threads resolves to 24" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=32 REIFY_OCCT_MEMTOTAL_GIB=128 env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '/^\[test-groups\]/{f=1;next}/^\[/{f=0}f&&/occt.*max-threads/{match(\$0,/[0-9]+/);print substr(\$0,RSTART,RLENGTH);exit}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"24\" ]
    "

# Test 11 (compile-free parse, override): REIFY_OCCT_NEXTEST_MAX_THREADS=7
# produces a config whose [test-groups] occt max-threads resolves to 7.
assert "gen-nextest-config.sh REIFY_OCCT_NEXTEST_MAX_THREADS=7: [test-groups] occt max-threads resolves to 7" \
    bash -c "
        cfg=\$(REIFY_OCCT_NEXTEST_MAX_THREADS=7 bash \"${GEN_CFG}\")
        val=\$(awk '/^\[test-groups\]/{f=1;next}/^\[/{f=0}f&&/occt.*max-threads/{match(\$0,/[0-9]+/);print substr(\$0,RSTART,RLENGTH);exit}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"7\" ]
    "

# Test 12a (fallback, no nextest required): gen-nextest-config.sh with
# REIFY_OCCT_NPROC=32/MEMTOTAL_GIB=128 (host-independent workstation profile,
# task 4621) default output contains the TOML literal 'occt = { max-threads = 24 }'
# so the mechanism has coverage even when nextest is absent from PATH.
assert "gen-nextest-config.sh NPROC=32/MEM=128 (workstation profile): output file contains TOML 'occt = { max-threads = 24 }'" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=32 REIFY_OCCT_MEMTOTAL_GIB=128 env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash \"${GEN_CFG}\")
        rc=0
        grep -qF 'occt = { max-threads = 24 }' \"\$cfg\" || rc=1
        rm -f \"\$cfg\"
        exit \$rc
    "

# Test 12b: override REIFY_OCCT_NEXTEST_MAX_THREADS=7 produces 'occt = { max-threads = 7 }'.
assert "gen-nextest-config.sh REIFY_OCCT_NEXTEST_MAX_THREADS=7: output file contains TOML 'occt = { max-threads = 7 }'" \
    bash -c "
        cfg=\$(REIFY_OCCT_NEXTEST_MAX_THREADS=7 bash \"${GEN_CFG}\")
        rc=0
        grep -qF 'occt = { max-threads = 7 }' \"\$cfg\" || rc=1
        rm -f \"\$cfg\"
        exit \$rc
    "

# ---------------------------------------------------------------------------
# Tests 13a–13c (task 4621): host-relative nproc bound (GREEN from step-2).
#
# REIFY_OCCT_NPROC injects the CPU count so the derivation is deterministically
# testable on ANY host (workstation 32t or laptop 16t).  REIFY_OCCT_MEMTOTAL_GIB
# is set high (999) so the RAM term (added in step-4) does not bind and confound
# these nproc-focused assertions.
#
# Derivation: cap = min(HARD_CAP=24, nproc, [ram_bound])
#   HARD_CAP from REIFY_OCCT_NEXTEST_HARD_CAP (default 24).
#   nproc from REIFY_OCCT_NPROC if valid, else system nproc.
# ---------------------------------------------------------------------------
echo ""
echo "--- Tests 13a–13c (task 4621): host-relative nproc bound for OCCT cap ---"

# Test 13a: REIFY_OCCT_NPROC=16 → cap=16 (nproc < HARD_CAP=24).
# RED against current code (always emits 24 regardless of nproc).
assert "gen-nextest-config.sh REIFY_OCCT_NPROC=16: [test-groups] occt max-threads resolves to 16 (nproc binds)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=16 REIFY_OCCT_MEMTOTAL_GIB=999 env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '/^\[test-groups\]/{f=1;next}/^\[/{f=0}f&&/occt.*max-threads/{match(\$0,/[0-9]+/);print substr(\$0,RSTART,RLENGTH);exit}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"16\" ]
    "

# Test 13b: REIFY_OCCT_NPROC=40 → cap=24 (nproc > HARD_CAP; ceiling holds).
# Guard: ensures nproc > HARD_CAP does not break the hard ceiling.
assert "gen-nextest-config.sh REIFY_OCCT_NPROC=40: [test-groups] occt max-threads resolves to 24 (HARD_CAP binds)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=40 REIFY_OCCT_MEMTOTAL_GIB=999 env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '/^\[test-groups\]/{f=1;next}/^\[/{f=0}f&&/occt.*max-threads/{match(\$0,/[0-9]+/);print substr(\$0,RSTART,RLENGTH);exit}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"24\" ]
    "

# Test 13c: explicit REIFY_OCCT_NEXTEST_MAX_THREADS=7 wins verbatim even with
# REIFY_OCCT_NPROC=16 present (explicit override escape hatch is preserved).
assert "gen-nextest-config.sh REIFY_OCCT_NEXTEST_MAX_THREADS=7 wins over REIFY_OCCT_NPROC=16: resolves to 7" \
    bash -c "
        cfg=\$(REIFY_OCCT_NEXTEST_MAX_THREADS=7 REIFY_OCCT_NPROC=16 REIFY_OCCT_MEMTOTAL_GIB=999 bash \"${GEN_CFG}\")
        val=\$(awk '/^\[test-groups\]/{f=1;next}/^\[/{f=0}f&&/occt.*max-threads/{match(\$0,/[0-9]+/);print substr(\$0,RSTART,RLENGTH);exit}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"7\" ]
    "

# ---------------------------------------------------------------------------
# Tests 14a–14d (task 4621): host-relative RAM bound (RED until step-4 impl).
#
# Derivation: cap = min(HARD_CAP=24, nproc, floor(MemTotalGiB / GIB_PER_THREAD=2))
#   GIB_PER_THREAD from REIFY_OCCT_GIB_PER_THREAD (default 2, strict digits-only).
#   MemTotalGiB from REIFY_OCCT_MEMTOTAL_GIB (testability knob); fallback /proc/meminfo.
#
# Each test injects both REIFY_OCCT_NPROC and REIFY_OCCT_MEMTOTAL_GIB so the
# result is an exact integer min() deterministic on any host.
# RED against step-2 code (which has no RAM term: min(24, nproc) only).
# ---------------------------------------------------------------------------
echo ""
echo "--- Tests 14a–14d (task 4621): host-relative RAM bound for OCCT cap ---"

# Helper awk extractor (same pattern used in Tests 10–13).
_OCCT_AWK='/^\[test-groups\]/{f=1;next}/^\[/{f=0}f&&/occt.*max-threads/{match($0,/[0-9]+/);print substr($0,RSTART,RLENGTH);exit}'

# Test 14a: NPROC=16, MEMTOTAL_GIB=16, GIB_PER_THREAD=2
#   ram_bound = floor(16/2) = 8 → min(24,16,8) = 8 (RAM binds).
#   RED: step-2 returns min(24,16)=16.
assert "gen-nextest-config.sh NPROC=16/MEM=16/GPT=2: [test-groups] occt max-threads = 8 (RAM binds)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=16 REIFY_OCCT_MEMTOTAL_GIB=16 REIFY_OCCT_GIB_PER_THREAD=2 \
              env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '${_OCCT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"8\" ]
    "

# Test 14b: NPROC=16, MEMTOTAL_GIB=64, GIB_PER_THREAD=2 (default)
#   ram_bound = floor(64/2) = 32 → min(24,16,32) = 16 (nproc binds, RAM slack).
#   RED: step-2 returns min(24,16)=16 — happens to be correct, but for wrong reasons;
#   step-4 must handle this correctly via the full three-term min().
assert "gen-nextest-config.sh NPROC=16/MEM=64/GPT=2: [test-groups] occt max-threads = 16 (nproc binds, RAM slack)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=16 REIFY_OCCT_MEMTOTAL_GIB=64 REIFY_OCCT_GIB_PER_THREAD=2 \
              env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '${_OCCT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"16\" ]
    "

# Test 14c: NPROC=32, MEMTOTAL_GIB=128, GIB_PER_THREAD=2 (workstation profile)
#   ram_bound = floor(128/2) = 64 → min(24,32,64) = 24 (HARD_CAP binds).
assert "gen-nextest-config.sh NPROC=32/MEM=128/GPT=2 (workstation profile): [test-groups] occt max-threads = 24 (HARD_CAP binds)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=32 REIFY_OCCT_MEMTOTAL_GIB=128 REIFY_OCCT_GIB_PER_THREAD=2 \
              env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '${_OCCT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"24\" ]
    "

# Test 14d: REIFY_OCCT_GIB_PER_THREAD=4 (wider-margin knob), two sub-cases.
#   NPROC=16, MEMTOTAL_GIB=64: ram_bound=floor(64/4)=16 → min(24,16,16)=16 (nproc=RAM).
#   NPROC=16, MEMTOTAL_GIB=16: ram_bound=floor(16/4)=4  → min(24,16,4)=4  (RAM binds).
#   RED (second sub-case): step-2 returns min(24,16)=16, not 4.
assert "gen-nextest-config.sh NPROC=16/MEM=64/GPT=4: [test-groups] occt max-threads = 16 (nproc=RAM=16)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=16 REIFY_OCCT_MEMTOTAL_GIB=64 REIFY_OCCT_GIB_PER_THREAD=4 \
              env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '${_OCCT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"16\" ]
    "

assert "gen-nextest-config.sh NPROC=16/MEM=16/GPT=4: [test-groups] occt max-threads = 4 (RAM binds, GPT=4 knob honored)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=16 REIFY_OCCT_MEMTOTAL_GIB=16 REIFY_OCCT_GIB_PER_THREAD=4 \
              env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '${_OCCT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"4\" ]
    "

# ---------------------------------------------------------------------------
# Tests 15a–15b (task 4621 amendment): HARD_CAP knob and /proc/meminfo branch.
#
# 15a exercises the REIFY_OCCT_NEXTEST_HARD_CAP parse path (gen-nextest-config.sh:59-62).
#    With NPROC=32 and MEM=999 the CPU/RAM terms don't bind, so the HARD_CAP is
#    the sole active ceiling.  A regression in that parse would silently produce 24.
# 15b omits REIFY_OCCT_MEMTOTAL_GIB so the /proc/meminfo MemTotal parse runs
#    (gen-nextest-config.sh:97-104).  The assertion is a range check [1..nproc=32]
#    rather than an exact value so it stays deterministic across hosts:
#    — if /proc/meminfo is readable, the RAM term participates (cap ≤ 32).
#    — if /proc/meminfo is absent (non-Linux), the RAM term is skipped → cap = 24.
#    Both outcomes satisfy the range assertion; the key correctness property is
#    that the result is at least 1 (the clamp added for suggestion 1) and at most
#    nproc=32.
# ---------------------------------------------------------------------------
echo ""
echo "--- Tests 15a–15b (task 4621 amendment): HARD_CAP override and /proc/meminfo branch ---"

# Test 15a: REIFY_OCCT_NEXTEST_HARD_CAP=12 with NPROC=32/MEM=999
#   min(12, 32, floor(999/2)=499) = 12 (custom HARD_CAP binds).
assert "gen-nextest-config.sh REIFY_OCCT_NEXTEST_HARD_CAP=12/NPROC=32/MEM=999: [test-groups] occt max-threads = 12 (HARD_CAP override)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NEXTEST_HARD_CAP=12 REIFY_OCCT_NPROC=32 REIFY_OCCT_MEMTOTAL_GIB=999 \
              env -u REIFY_OCCT_NEXTEST_MAX_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '${_OCCT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"12\" ]
    "

# Test 15b: omit REIFY_OCCT_MEMTOTAL_GIB → /proc/meminfo branch or RAM-term-skip.
#   cap must be a positive integer ≤ nproc=32 on any host.
assert "gen-nextest-config.sh NPROC=32, no REIFY_OCCT_MEMTOTAL_GIB: cap in [1..32] via /proc/meminfo (or RAM term skipped)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=32 \
              env -u REIFY_OCCT_NEXTEST_MAX_THREADS env -u REIFY_OCCT_MEMTOTAL_GIB \
              env -u REIFY_OCCT_NEXTEST_HARD_CAP bash \"${GEN_CFG}\")
        val=\$(awk '${_OCCT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ -n \"\$val\" ] && [ \"\$val\" -ge 1 ] && [ \"\$val\" -le 32 ]
    "

# ---------------------------------------------------------------------------
# Tests 16a-16d (task 5141): per-test slow-timeout/terminate-after ceiling —
# DEFAULT TIER. A single hung test must be SIGTERM/SIGKILLed BY NEXTEST — with
# by-name attribution in its output — well before the pass-level
# `timeout --kill-after=60 60m` wall (verify.sh, 3600s budget) fires as exit
# 124 with ZERO test-level attribution (the task 4877/4878 failure shape).
#
# [profile.default] slow-timeout = { period = "120s", terminate-after = 10 }
# = 1200s (20 min) ceiling applied to every test that has no more specific
# override. The 5 known-slow "heavy" binaries get a separate, larger ceiling
# via their own [[profile.default.overrides]] blocks (task 5141 step-3/4) —
# nextest resolves each override SETTING by first-match independently, so the
# heavy tier does not fall through to this default.
# ---------------------------------------------------------------------------
echo ""
echo "--- Tests 16a-16d (task 5141): default-tier slow-timeout/terminate-after ceiling ---"

# Section-scoped awk: bind the match to the [profile.default] TABLE only (the
# `/^\[/` reset excludes the [[profile.default.overrides]] blocks that follow),
# mirroring the occt max-threads section-scoped extractor (_OCCT_AWK above).
_DEFAULT_ST_AWK='/^\[profile\.default\]/{f=1;next}/^\[/{f=0}f&&/slow-timeout/{print;exit}'

# Test 16a: [profile.default] carries the exact default-tier slow-timeout.
assert "nextest.toml: [profile.default] has slow-timeout = { period = \"120s\", terminate-after = 10 } (section-scoped to the [profile.default] table, not an overrides block)" \
    bash -c "awk '${_DEFAULT_ST_AWK}' '$NEXTEST_TOML' | grep -qF 'slow-timeout = { period = \"120s\", terminate-after = 10 }'"

# Test 16b (compile-free, task 4613 convention): gen-nextest-config.sh's
# full-copy + anchored occt-only sed preserves the default-tier slow-timeout
# verbatim in the generated temp config (no cargo/nextest invocation).
assert "gen-nextest-config.sh: default-tier slow-timeout preserved verbatim in generated config" \
    bash -c "
        cfg=\$(REIFY_OCCT_NEXTEST_MAX_THREADS=24 bash \"${GEN_CFG}\")
        rc=0
        awk '${_DEFAULT_ST_AWK}' \"\$cfg\" | grep -qF 'slow-timeout = { period = \"120s\", terminate-after = 10 }' || rc=1
        rm -f \"\$cfg\"
        exit \$rc
    "

# Test 16c (amended — reviewer test-quality finding): numeric basis — the
# default ceiling (period-seconds * terminate-after, BOTH FACTORS EXTRACTED
# FROM nextest.toml below, not hardcoded literals) is strictly less than the
# 3600s (60m) pass-level wall (verify.sh timeout --kill-after=60 60m), so
# nextest attributes-and-kills a hang BEFORE the outer timeout fires exit 124
# with zero attribution.
#
# The original form (`test $((120*10)) -lt 3600`) was pure arithmetic on
# literals that never read nextest.toml — it could never fail regardless of
# the file's contents. Extracting period and terminate-after from the actual
# [profile.default] table ties the check to real state, so an edit that
# pushed the product past the wall would now fail here.
_DEFAULT_ST_PERIOD_AWK='/^\[profile\.default\]/{f=1;next}/^\[/{f=0}f&&/slow-timeout/{match($0,/period[[:space:]]*=[[:space:]]*"[0-9]+s"/);seg=substr($0,RSTART,RLENGTH);match(seg,/[0-9]+/);print substr(seg,RSTART,RLENGTH);exit}'
_DEFAULT_ST_TERM_AWK='/^\[profile\.default\]/{f=1;next}/^\[/{f=0}f&&/slow-timeout/{match($0,/terminate-after[[:space:]]*=[[:space:]]*[0-9]+/);seg=substr($0,RSTART,RLENGTH);match(seg,/[0-9]+$/);print substr(seg,RSTART,RLENGTH);exit}'
_DEFAULT_ST_PERIOD="$(awk "$_DEFAULT_ST_PERIOD_AWK" "$NEXTEST_TOML")"
_DEFAULT_ST_TERM="$(awk "$_DEFAULT_ST_TERM_AWK" "$NEXTEST_TOML")"

assert "default slow-timeout ceiling (period=${_DEFAULT_ST_PERIOD:-unset}s * terminate-after=${_DEFAULT_ST_TERM:-unset}, both extracted from nextest.toml) is strictly less than the 3600s (60m) pass-level wall" \
    bash -c "[ -n '${_DEFAULT_ST_PERIOD:-}' ] && [ -n '${_DEFAULT_ST_TERM:-}' ] && [ \$(( ${_DEFAULT_ST_PERIOD:-0} * ${_DEFAULT_ST_TERM:-0} )) -lt 3600 ]"

# Test 16d: REGRESSION GUARD (green-on-arrival) — no `retries` key anywhere in
# nextest.toml or the generated config. A retry would re-run and mask the very
# hangs/flakes this ceiling exists to attribute-and-kill (task invariant: no
# retries, ever). Comment lines are excluded so prose mentioning the word
# "retries" (e.g. documenting this very invariant) cannot false-positive.
assert "no 'retries' key in .config/nextest.toml (no-retries invariant)" \
    bash -c "! grep -v '^[[:space:]]*#' '$NEXTEST_TOML' | grep -q 'retries'"

assert "no 'retries' key in gen-nextest-config.sh generated config (no-retries invariant preserved end-to-end)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NEXTEST_MAX_THREADS=24 bash \"${GEN_CFG}\")
        rc=0
        grep -v '^[[:space:]]*#' \"\$cfg\" | grep -q 'retries' && rc=1
        rm -f \"\$cfg\"
        exit \$rc
    "

# ---------------------------------------------------------------------------
# Tests 17a-17b (task 5984): global [profile.default] test-threads pool cap.
#
# Before this task the ONLY concurrency bound in nextest.toml was the `occt`
# test-group's max-threads.  Every crate OUTSIDE that group (reify_lsp,
# reify-syntax, reify-compiler, the GUI Rust tests, ...) ran in nextest's
# DEFAULT global pool, which is the host's logical CPU count (32 here) —
# i.e. uncapped in practice.  `[profile.default] test-threads` is the global
# pool bound; these tests pin that it exists and actually bounds.
#
# The extractor below is section-scoped to the [profile.default] TABLE: the
# `/^\[/{f=0}` reset is what stops a match inside a following
# [[profile.default.overrides]] block from satisfying a [profile.default]
# assertion (same reason Test 16a is section-scoped).
# ---------------------------------------------------------------------------
echo ""
echo "--- Tests 17a-17b (task 5984): global [profile.default] test-threads pool cap ---"

_DEFAULT_TT_AWK='/^\[profile\.default\]/{f=1;next}/^\[/{f=0}f&&/^test-threads[[:space:]]*=/{match($0,/[0-9]+/);print substr($0,RSTART,RLENGTH);exit}'

# Test 17a: the in-file literal is the SED-TEMPLATE FALLBACK — the reference-host
# nproc placeholder, deliberately NOT a narrowing ceiling.  RE-POINTED in place
# by task 6018 (was 16, framed as "the HARD_CAP literal"): HARD_CAP no longer
# defaults to a narrowing constant, so the literal now stands in for host nproc
# on the 32-core reference host, exactly as the in-file '24' stands in for the
# resolved occt cap.  scripts/gen-nextest-config.sh rewrites this line to the
# host-resolved value for every verify pass, so the literal is only ever seen by
# a bare `cargo nextest run` that bypasses verify.sh.
# Asserted on the EXTRACTED value (not a raw grep) so an occurrence inside an
# overrides block cannot satisfy it — that section scoping is the whole point of
# _DEFAULT_TT_AWK's `/^\[/{f=0}` reset.
# Deliberately a BARE INTEGER, not nextest's `"num-cpus"`: 17b extracts with
# match($0,/[0-9]+/) (empty on a quoted string) and 17k pins the
# `^test-threads = [0-9][0-9]*$` sed anchor.  Both must stay green here.
assert "nextest.toml: [profile.default] has test-threads = 32 (sed-template fallback — the reference-host nproc placeholder, NOT a narrowing ceiling; section-scoped to the [profile.default] table)" \
    bash -c "[ \"\$(awk '${_DEFAULT_TT_AWK}' '$NEXTEST_TOML')\" = '32' ]"

# Test 17b: that value is a non-empty positive integer >= 1.  nextest treats an
# absent key as unbounded (= logical CPU count), so this pins that the in-file
# fallback actually BOUNDS the pool rather than merely being present.
# NOTE the two zero-ish cases are NOT the same failure and must not be conflated:
# an ABSENT key silently defaults to the CPU count (fail-open), whereas an
# explicit `test-threads = 0` is REJECTED by nextest 0.9.136 as an invalid config
# (fail-closed — see Test 17g).  This assert excludes both.
assert "nextest.toml: [profile.default] test-threads is a non-empty positive integer >= 1 (fallback actually bounds the pool)" \
    bash -c "
        val=\$(awk '${_DEFAULT_TT_AWK}' '$NEXTEST_TOML')
        [ -n \"\$val\" ] && [ \"\$val\" -ge 1 ]
    "

# ---------------------------------------------------------------------------
# Tests 17c-17e (task 5984): host-relative derivation of the global pool cap.
#
# Derivation: tt = min(HARD_CAP, nproc)   [nproc term skipped if unavailable]
#   HARD_CAP — REIFY_NEXTEST_TEST_THREADS_HARD_CAP, whose DEFAULT is the resolved
#     host CPU count (task 6018; it was the literal 16 under task 5984).  So by
#     default the min() collapses to nproc and imposes NO ceiling below what
#     nextest would itself pick; the knob remains an escape hatch for a host that
#     genuinely needs tightening (Test 17f).
#   REIFY_NEXTEST_TEST_THREADS — explicit override, wins verbatim.
#   nproc — REIFY_OCCT_NPROC if valid, else system nproc/getconf.  That knob is
#     deliberately REUSED rather than aliased: the value is the host's logical
#     CPU count, nothing about it is OCCT-specific (the prefix is historical,
#     from task 4621's occt-only derivation).
#
# NO RAM term here, unlike the occt cap — deliberate, see the rationale block in
# .config/nextest.toml: this key exists to bound runqueue depth, not RSS.  OCCT
# threads carry ~2 GiB anon each and ordinary Rust test binaries do not, which is
# why the RAM term belongs on the occt cap and not on this one.
#
# Why host-relative at all (the task 4621 lesson): a bare literal OVERSUBSCRIBES
# any host smaller than the literal — an 8-core host or a CPU-quota'd container
# would get 16 runnable test threads on a 16-literal, making the "cap" actively
# harmful.  Host-relativity is what these tests pin; the DEFAULT ceiling being
# nproc itself (task 6018) is what stops the derivation narrowing below it.  REIFY_OCCT_NPROC is injected in every case
# below so each expectation is an exact integer on ANY host (Tests 13a-15b form).
# Compile-free: no cargo, no nextest, no workspace compile (task 4613).
#
# HERMETICITY (required form — copy this, not a shorter one): every case that
# asserts an EXACT derived value must clear BOTH global-pool knobs, i.e.
#   env -u REIFY_NEXTEST_TEST_THREADS -u REIFY_NEXTEST_TEST_THREADS_HARD_CAP
# unless it is itself setting that knob.  HARD_CAP is a supported tuning point,
# so an operator or CI job may legitimately have it exported; clearing only
# REIFY_NEXTEST_TEST_THREADS leaves 17d/17e resolving to the ambient ceiling
# (e.g. 4) and failing spuriously in that shell.  17h/17i already used the full
# form; 17d/17e were widened to match in the task-5984 review pass.
# ---------------------------------------------------------------------------
echo ""
echo "--- Tests 17c-17e (task 5984): host-relative derivation of the global test-threads cap ---"

# Test 17c: explicit REIFY_NEXTEST_TEST_THREADS=5 wins verbatim.  Value-agnostic
# and untouched by task 6018 — the explicit-override branch bypasses the
# derivation entirely, so un-narrowing the default cannot move it.
assert "gen-nextest-config.sh REIFY_NEXTEST_TEST_THREADS=5: [profile.default] test-threads resolves to 5 (explicit override wins verbatim)" \
    bash -c "
        cfg=\$(REIFY_NEXTEST_TEST_THREADS=5 bash \"${GEN_CFG}\")
        val=\$(awk '${_DEFAULT_TT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"5\" ]
    "

# Test 17d: REIFY_OCCT_NPROC=8 -> 8 (nproc binds; HARD_CAP defaults to nproc, so
# min(8, 8) = 8).  This is the assertion a bare in-file literal cannot satisfy:
# on an 8-core host a fixed 32 would oversubscribe 4x.
assert "gen-nextest-config.sh REIFY_OCCT_NPROC=8: [profile.default] test-threads resolves to 8 (nproc binds, no 2x oversubscription)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=8 \
              env -u REIFY_NEXTEST_TEST_THREADS -u REIFY_NEXTEST_TEST_THREADS_HARD_CAP \
              bash \"${GEN_CFG}\")
        val=\$(awk '${_DEFAULT_TT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"8\" ]
    "

# Test 17e: REIFY_OCCT_NPROC=32 -> 32 (workstation profile).  RE-POINTED in place
# by task 6018 (was `-> 16 (HARD_CAP binds)`), not retired: the property under
# test is unchanged — what the workstation profile resolves to — only its value
# moved.  HARD_CAP now defaults to the resolved host CPU count, so the derivation
# applies NO ceiling below nproc and min(32, 32) = 32.
#
# READ 17d AND 17e TOGETHER — THIS ONE PINS NO CEILING (stated explicitly, task
# 6018 review-amendment pass, so a future reader does not mistake it for one).
# Before 6018 the pair was asymmetric: 17d showed nproc binding DOWNWARD on a
# small host while 17e showed the fixed HARD_CAP of 16 binding downward on a
# large one.  With HARD_CAP defaulting to nproc, BOTH now assert the SAME
# property — `tt == the injected nproc` — at two host sizes, and neither
# constrains the pool from above any more.  Test 17f is the ONLY remaining
# ceiling assert in this block.
#
# Kept as two asserts rather than folded into one parameterised case: they are
# the small-host and workstation ANCHORS respectively, 17e's 32 is the value the
# rest of this task's artifacts quote (the in-file literal, Test 17i's ordering,
# docs/notes/nextest-global-pool-concurrency-observation.md), and folding would
# drop an assert from the S7 pass floor in
# tests/infra/test_verify_nextest_absent_suites.sh for no coverage gain.
assert "gen-nextest-config.sh REIFY_OCCT_NPROC=32 (workstation profile): [profile.default] test-threads resolves to 32 (the injected host CPU count — the derivation applies NO ceiling below nproc by default)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=32 \
              env -u REIFY_NEXTEST_TEST_THREADS -u REIFY_NEXTEST_TEST_THREADS_HARD_CAP \
              bash \"${GEN_CFG}\")
        val=\$(awk '${_DEFAULT_TT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"32\" ]
    "

# ---------------------------------------------------------------------------
# Tests 17f-17g (task 5984): the HARD_CAP knob and the >=1 clamp.
#
# 17f exercises the REIFY_NEXTEST_TEST_THREADS_HARD_CAP parse path — mirroring
#   Test 15a's role for REIFY_OCCT_NEXTEST_HARD_CAP.  With NPROC=32 the CPU term
#   does not bind and the default ceiling IS that CPU term, so the custom
#   ceiling 4 is the SOLE active bound; without this assert a regression in that
#   parse silently produces 32 (the unnarrowed default) and nothing notices.
#   This is the assert that keeps the tightening escape hatch alive now that the
#   default no longer narrows anything (task 6018).
# 17g pins the zero-clamp.  `0` is digits-only-VALID, so it passes the parse
#   guard and would otherwise reach the config as `test-threads = 0`.
#   VERIFIED on cargo-nextest 0.9.136 (scratch crate, task-5984 review pass):
#   nextest REJECTS that value rather than treating it as unbounded —
#     error: failed to parse nextest config at `.../nextest.toml`
#     Caused by: profile.default.test-threads: invalid value: integer `0`,
#                expected an integer or the string "num-cpus"
#   and exits 96.  So the unclamped failure mode is fail-CLOSED: every nextest
#   pass on the host aborts before running a test — a verify OUTAGE, not the
#   silently-uncapped pool an earlier draft of this comment claimed.  (Reason
#   from the verified behaviour, not that inverted claim: an unclamped 0 is not
#   merely a perf problem.)  Same "arithmetically valid, semantically
#   nonsensical" defence the occt ram_bound already carries, and the identical
#   nextest rejection it cites for `max-threads = 0`.
# ---------------------------------------------------------------------------
echo ""
echo "--- Tests 17f-17g (task 5984): global-cap HARD_CAP knob and >=1 clamp ---"

# Test 17f: REIFY_NEXTEST_TEST_THREADS_HARD_CAP=4 with NPROC=32
#   min(4, 32) = 4 (custom ceiling binds).
#   RED against step-4, which hardcodes 16 with no knob.
assert "gen-nextest-config.sh REIFY_NEXTEST_TEST_THREADS_HARD_CAP=4/NPROC=32: [profile.default] test-threads = 4 (custom HARD_CAP binds)" \
    bash -c "
        cfg=\$(REIFY_NEXTEST_TEST_THREADS_HARD_CAP=4 REIFY_OCCT_NPROC=32 \
              env -u REIFY_NEXTEST_TEST_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '${_DEFAULT_TT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"4\" ]
    "

# Test 17g: REIFY_NEXTEST_TEST_THREADS_HARD_CAP=0 with NPROC=32 -> 1, never 0.
#   Drives the clamp via the DERIVE branch; Test 17j drives the same clamp via
#   the explicit-override branch.
#   RED against step-4, which has no clamp.
assert "gen-nextest-config.sh REIFY_NEXTEST_TEST_THREADS_HARD_CAP=0/NPROC=32: [profile.default] test-threads = 1 (clamped, never 0 = invalid config nextest rejects)" \
    bash -c "
        cfg=\$(REIFY_NEXTEST_TEST_THREADS_HARD_CAP=0 REIFY_OCCT_NPROC=32 \
              env -u REIFY_NEXTEST_TEST_THREADS bash \"${GEN_CFG}\")
        val=\$(awk '${_DEFAULT_TT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"1\" ]
    "

# ---------------------------------------------------------------------------
# Tests 17h-17i (task 5984): REGRESSION GUARDS (green-on-arrival).
#
# Labeled as such following Test 16d's precedent, so a reviewer does not read
# them as a failed RED step: they pin properties the derivation already
# satisfies, so that a future edit cannot quietly violate them.
#
# Both derive BOTH operands from real parsed state rather than repeating
# literals — the test-quality standard Test 16c and Assertion H were amended to
# meet.  An assertion whose two sides are both hardcoded can never fail no
# matter what the config says.
# ---------------------------------------------------------------------------
echo ""
echo "--- Tests 17h-17i (task 5984, re-pointed 6018): REGRESSION GUARDS — no oversubscription; the occt group cap is a genuine backstop below the global ---"

# Test 17h (no oversubscription): the generated global pool never exceeds the
# host's CPU count.  This is the invariant the whole task exists to establish,
# and precisely what a bare in-file literal violates on a small host.  The
# comparison operand is the INJECTED cpu count (8), not a repeated expectation
# of the derivation's output.
assert "gen-nextest-config.sh REIFY_OCCT_NPROC=8 (no explicit overrides): generated [profile.default] test-threads is a positive integer <= the injected CPU count 8 (never oversubscribes)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=8 \
              env -u REIFY_NEXTEST_TEST_THREADS -u REIFY_NEXTEST_TEST_THREADS_HARD_CAP \
              bash \"${GEN_CFG}\")
        val=\$(awk '${_DEFAULT_TT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ -n \"\$val\" ] && [ \"\$val\" -ge 1 ] && [ \"\$val\" -le 8 ]
    "

# Test 17i (the occt group cap is a GENUINE BACKSTOP, not subsumed config):
# under the host-independent workstation profile, extract BOTH caps from the SAME
# generated file and assert the global is strictly GREATER than the occt group
# cap (32 > 24).  RE-POINTED in place by task 6018, not retired: the property
# under test — the ORDERING of the two caps on this host class — is the same one
# 17h/17i were written to pin; task 5984's narrowing had inverted it, and
# un-narrowing the global inverts it back.
#
# Why the new direction is the load-bearing one.  While the global resolved to 16
# it subsumed the group entirely and a group cap of 24 could never bind — the
# group was dead config kept only against a future raise.  With the global back
# at nproc (32) the group cap binds again at 24, so the group's effective memory
# bound is 24 x ~2 GiB = 48 GiB per run, consistent with the headroom basis at
# the top of .config/nextest.toml.  That is exactly the property the AMENDMENT
# block in that file now asserts, and this assert is what keeps the two honest.
#
# DO NOT generalise this to "global >= occt cap on any host".  That is FALSE in
# BOTH directions, and now in the other one: the occt cap carries a RAM term the
# global deliberately does not, so on e.g. NPROC=8 with a small MemTotal the occt
# cap resolves BELOW the global — and on a host where an operator tightens
# REIFY_NEXTEST_TEST_THREADS_HARD_CAP the global drops back below the group.
# This assertion is deliberately scoped to the injected workstation profile.
assert "gen-nextest-config.sh NPROC=32/MEM=128 (workstation profile): generated [profile.default] test-threads is strictly GREATER than the generated [test-groups] occt max-threads, BOTH extracted from the same file (the occt group cap is a genuine backstop below the global, not subsumed by it)" \
    bash -c "
        cfg=\$(REIFY_OCCT_NPROC=32 REIFY_OCCT_MEMTOTAL_GIB=128 \
              env -u REIFY_NEXTEST_TEST_THREADS -u REIFY_NEXTEST_TEST_THREADS_HARD_CAP \
              -u REIFY_OCCT_NEXTEST_MAX_THREADS -u REIFY_OCCT_NEXTEST_HARD_CAP \
              bash \"${GEN_CFG}\")
        tt=\$(awk '${_DEFAULT_TT_AWK}' \"\$cfg\")
        oc=\$(awk '${_OCCT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ -n \"\$tt\" ] && [ -n \"\$oc\" ] && [ \"\$tt\" -gt \"\$oc\" ]
    "

# ---------------------------------------------------------------------------
# Tests 17j-17k (task 5984, REVIEW-AMENDMENT pass): the two gaps a reviewer
# found in the 17a-17i block above.  Appended rather than interleaved so the
# existing letters keep their meaning in the commit history and in the S7
# accounting table; each cross-references the test it belongs beside.
#
# 17j closes a CLAMP-BRANCH coverage gap next to Test 17g.
# 17k turns gen-nextest-config.sh's PROSE uniqueness claim about its sed anchor
#   into a checked invariant.
# Both are compile-free (bash/sed/awk only), so they raise the nextest-less and
# ambient S7 counts by the same amount.
# ---------------------------------------------------------------------------
echo ""
echo "--- Tests 17j-17k (task 5984 review amendment): clamp covers BOTH branches; sed anchor is unique ---"

# Test 17j (sibling of 17g — the OTHER branch into the same clamp):
# the >=1 clamp in gen-nextest-config.sh sits AFTER the
# `case "${REIFY_NEXTEST_TEST_THREADS:-}"` block, so it guards the
# explicit-override branch (tt=$(( 10#${REIFY_NEXTEST_TEST_THREADS} ))) as well
# as the derive branch that 17g exercises.  REIFY_NEXTEST_TEST_THREADS=0 is
# equally digits-only-VALID and equally reachable, but reaches the clamp by a
# DIFFERENT code path.
#
# Why this assert is not redundant with 17g: a natural-looking tidy-up that
# moves the clamp inside the derive branch (where its explanatory comment sits)
# leaves 17g GREEN while REIFY_NEXTEST_TEST_THREADS=0 emits `test-threads = 0`
# and aborts every nextest pass with the config parse error quoted at 17g.
# Falsifiability checked by mutation, not assumed: moving the clamp into the
# derive branch fails THIS assert and only this one.
assert "gen-nextest-config.sh REIFY_NEXTEST_TEST_THREADS=0/NPROC=32: [profile.default] test-threads = 1 (the >=1 clamp also covers the EXPLICIT-OVERRIDE branch, not just the derive branch 17g drives)" \
    bash -c "
        cfg=\$(REIFY_NEXTEST_TEST_THREADS=0 REIFY_OCCT_NPROC=32 \
              env -u REIFY_NEXTEST_TEST_THREADS_HARD_CAP bash \"${GEN_CFG}\")
        val=\$(awk '${_DEFAULT_TT_AWK}' \"\$cfg\")
        rm -f \"\$cfg\"
        [ \"\$val\" = \"1\" ]
    "

# Test 17k (the sed anchor's unstated precondition, made explicit):
# gen-nextest-config.sh rewrites the global cap with a LINE-anchored but NOT
# section-anchored substitution:
#     sed -e "s/^test-threads = [0-9][0-9]*\$/test-threads = \${tt}/"
# The occt anchor next to it is safe because `occt = { max-threads = N }` is
# unique by key NAME.  `test-threads` is NOT: it is a generic nextest PROFILE
# key that a future [profile.ci] / [profile.offline] table could legitimately
# carry with a deliberately different value.  A second such line would be
# rewritten to the SAME host-derived value, silently clobbering that profile's
# setting.
#
# Assertion I in tests/infra/test_nextest_slow_priority.sh cannot catch this:
# its extractor is section-scoped to [profile.default] and would report the
# correctly-rewritten default while the other profile was quietly overwritten.
# The script's comment asserts uniqueness in prose; this assert is what makes
# the claim CHECKED, so adding a second profile turns a silent clobber into a
# loud CI failure that forces the sed to be made section-aware first.
#
# Counted on the SOURCE .config/nextest.toml (the sed template), which is what
# the anchor is matched against.
assert "nextest.toml: exactly ONE line matches gen-nextest-config.sh's '^test-threads = <int>\$' sed anchor (uniqueness precondition of the non-section-scoped substitution — a second profile's test-threads would be silently clobbered)" \
    bash -c "
        n=\$(grep -c '^test-threads = [0-9][0-9]*\$' '$NEXTEST_TOML' || true)
        [ \"\$n\" = \"1\" ]
    "

# ---------------------------------------------------------------------------
# Test 17l (task 6018, REVIEW-AMENDMENT pass): the no-nproc last-resort constant
# in gen-nextest-config.sh and the in-file test-threads literal are ONE constant
# duplicated in two files — assert they are equal.
#
# THE DUPLICATION IS REAL AND IS ONLY DOCUMENTED, NOT CHECKED, WITHOUT THIS.
# gen-nextest-config.sh's HARD_CAP default is `${_nproc:-32}`; that 32 is reached
# only when NEITHER `nproc` NOR `getconf _NPROCESSORS_ONLN` resolves.  Three
# separate comment blocks in that script say it must stay coupled to the
# `test-threads = 32` literal in .config/nextest.toml, but only the toml side was
# pinned (Test 17a).  A future host-class change re-points 17a and the toml
# together — the natural, obvious edit — and silently leaves the script's fallback
# behind, with no test anywhere going red.
#
# The coupling also became a RISKIER guess in task 6018 than it was under 5984: a
# host where neither tool resolves used to land on 16 and now lands on 32, so a
# stale fallback now OVERSUBSCRIBES rather than merely narrowing.  That is exactly
# the task 4621 failure this whole derivation exists to avoid, on the one code
# path that cannot consult the host.
#
# BOTH SIDES ARE EXTRACTED, NEITHER IS HARDCODED HERE — the test-quality standard
# Tests 16c/17h/17i were amended to meet.  An assert with a literal on either side
# would just become a third copy of the same constant to keep in sync.
#
# Falsifiability was checked before this assert was added, not assumed: it goes
# RED when the script constant is changed alone (32 -> 16), RED when the toml
# literal is changed alone (32 -> 24), and RED when the `${_nproc:-N}` form is
# removed from the script entirely (extractor returns empty, caught by the
# non-empty guards rather than passing vacuously).
#
# The extractor deliberately does not spell the `"` or `$` of `"${_nproc:-N}"`:
# a double quote cannot appear in this file's `bash -c "..."` assert form without
# terminating the string.  Anchoring on `tt_hard_cap=` ... `_nproc:-<digits>}` is
# specific enough — it matches exactly one line in the script today.
#
# Compile-free: sed/awk over two tracked files, no cargo, no nextest.  If a future
# change moves the toml literal to a non-integer (e.g. nextest's `"num-cpus"`),
# this assert must be re-pointed or re-based in the SAME commit — _DEFAULT_TT_AWK
# extracts `[0-9]+` and would return empty, so it fails loudly rather than
# silently.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 17l (task 6018 review amendment): the no-nproc fallback constant is coupled to the in-file literal ---"

_TT_FALLBACK_SED='s/.*tt_hard_cap=.*_nproc:-\([0-9][0-9]*\)}.*/\1/p'

assert "gen-nextest-config.sh's no-nproc last-resort HARD_CAP constant equals .config/nextest.toml's [profile.default] test-threads literal (one constant duplicated across two files; BOTH extracted, neither hardcoded here)" \
    bash -c "
        script_const=\$(sed -n '${_TT_FALLBACK_SED}' '$GEN_CFG')
        toml_lit=\$(awk '${_DEFAULT_TT_AWK}' '$NEXTEST_TOML')
        [ -n \"\$script_const\" ] && [ -n \"\$toml_lit\" ] && [ \"\$script_const\" = \"\$toml_lit\" ]
    "

test_summary
