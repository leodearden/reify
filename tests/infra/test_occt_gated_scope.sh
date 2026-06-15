#!/usr/bin/env bash
# Infrastructure test for task 2000.
# Validates that the OCCT-touching crate list is correct and that
# orchestrator.yaml routes exactly those crates through the flock gate.
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
# ---------------------------------------------------------------------------
PLAN_HAS_NEXTEST="$(printf '%s\n' "$TEST_PLAN_SEGS" | grep -c 'cargo nextest run' || true)"
GEN_CFG="$REPO_ROOT/scripts/gen-nextest-config.sh"

echo ""
echo "--- Tests 9–12 (task 4503/γ): --config-file plan assertions for env-driven occt cap ---"

# Test 9 (plan-shape): every cargo nextest run line carries --config-file <path>
# where the path contains the 'reify-nextest-occt' prefix.  In --print-plan mode
# (used here) this is the static placeholder; in execute mode it is the real temp path.
assert "every 'cargo nextest run' plan line carries '--config-file' with 'reify-nextest-occt' path" \
    bash -c "
        if [ '${PLAN_HAS_NEXTEST}' -eq 0 ]; then exit 0; fi
        bad=\$(printf '%s\n' \"\$TEST_PLAN_SEGS\" \
            | grep 'cargo nextest run' \
            | grep -v -- '--config-file.*reify-nextest-occt' || true)
        [ -z \"\$bad\" ]
    "

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
# Tests 13a–13c (task 4621): host-relative nproc bound (RED until step-2 impl).
#
# REIFY_OCCT_NPROC injects the CPU count so the derivation is deterministically
# testable on ANY host (workstation 32t or laptop 16t).  REIFY_OCCT_MEMTOTAL_GIB
# is set high (999) so the RAM term (not yet implemented in step-2; added in step-4)
# does not bind and confound these nproc-focused assertions.
#
# Derivation (step-2 and beyond): cap = min(HARD_CAP=24, nproc, [ram_bound])
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

test_summary
