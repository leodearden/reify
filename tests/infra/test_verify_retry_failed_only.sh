#!/usr/bin/env bash
# tests/infra/test_verify_retry_failed_only.sh — INTEGRATION-GATE for task 5290
# (PRD docs/prds/verify-retry-failed-only.md task δ, boundary suite §9.1 B1–B6).
#
# δ is the reify-side terminal gate for the failed-only merge-retry seam. Its
# dependencies α(5287)/β(5288)/γ(5289) already landed each per-suite primitive:
#   α — verify.sh builds the EXACT-match nextest `-E 'test(=id) | …'` subset
#       from the DF-written filter file, under a tree-OID eligibility gate with
#       loud full-fallback (tree drift / no subset / subset too large).
#   β — run_all.sh honors REIFY_RUN_ALL_MEMBER_SUBSET (per-member narrowing).
#   γ — the gui block forwards REIFY_GUI_RETRY_SPECS as `npm test -- <specs>`.
#
# δ delivers the ONE genuinely-new cross-suite runtime signal on top of them:
# the @@REIFY_RETRY_SCOPE=failed_only@@ HONEST MARKER (PRD §4.4 / INV-6). At
# plan-BUILD time verify.sh emits, to STDOUT, a single line
#   @@REIFY_RETRY_SCOPE=failed_only@@ nextest_debug=<n> nextest_release=<n> run_all=<n> gui=<n>
# IFF REIFY_VERIFY_RETRY_SCOPE=failed_only AND ≥1 suite ACTUALLY narrowed —
# SUPPRESSED on every full-fallback/refusal path (tree drift / no subset /
# subset too large / no nextest) and on a non-retry, so dark-factory runtime
# mining can never miscount a full re-verify as a failed_only green gate
# (INV-6 honest events). The per-suite counts come from the SAME single
# construction sites the suites narrow at (INV-5 no re-derivation).
#
# SCOPE BOUNDARY (avoids G7 lock-step duplication): α's fast plan-shape unit
# test (tests/infra/test_verify_retry_subset.sh) OWNS the per-suite nextest
# `-E test(=…)` fragment shape, the tree-OID gate, the three loud refusal
# lines, per-profile precedence, and the size ceiling — its header explicitly
# reserves for δ "the @@REIFY_RETRY_SCOPE@@ marker and the B1–B6 boundary
# test". So δ:
#   - OWNS (RED-tested here): the UNIFIED cross-suite honest marker + its
#     per-suite counts + fallback-SUPPRESSION (the only new δ behavior).
#   - LOCKS (green-on-arrival integration regression, the raison d'être of an
#     INTEGRATION-GATE): B1 (α's nextest subset shape), B4 (β's run_all member
#     count reflected in the marker), B5 (γ's `npm test -- <specs>`) all cohere
#     with the marker on the MERGED α/β/γ contract.
# It does NOT re-assert α's `-E test(=…)` fragment grammar in isolation.
#
# Hermetic, exactly like α: drives ONLY `verify.sh … --print-plan` (build the
# plan, exit 0 — no cargo build, no tests run). The marker and the subset-vs-
# fallback decision are made at plan-BUILD time, so build_plan()'s STDOUT echo
# is a faithful oracle under --print-plan AND lands on STDOUT in a real DF
# retry (D5 captures it). Temp sidecar / filter fixtures are pointed at via the
# REIFY_VERIFY_ATTEMPT_SIDECAR / REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE
# testability envs so nothing touches real repo state. Command-shape assertions
# that depend on cargo-nextest being installed are guarded on a NEXTEST_AVAILABLE
# probe of the plan header's `nextest=` token (α idiom); host-independent
# invariants (run_all/gui counts, marker suppression, byte-identical default)
# are asserted unconditionally.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; classified
# `pool` (hermetic) in tests/infra/run-all-classification.manifest.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY="$REPO_ROOT/scripts/verify.sh"

echo "=== verify.sh failed-only retry INTEGRATION-GATE (task 5290, PRD verify-retry-failed-only δ, B1–B6) ==="

# --- Hermetic fixtures -------------------------------------------------------
# A temp attempt-0 sidecar (tree_oid=deadbeef) and a 2-line exact-id filter
# file, both outside target/ and pointed at via the testability-knob envs so
# nothing touches the real repo state (α idiom).
_TMP="$(mktemp -d "${TMPDIR:-/tmp}/reify-retry-failed-only.XXXXXX")"
trap 'rm -rf "$_TMP"' EXIT

SIDECAR="$_TMP/attempt.json"
printf '{"tree_oid":"deadbeef","profiles":"debug","timestamp":"2026-07-23T00:00:00Z"}\n' > "$SIDECAR"

FILTER="$_TMP/filter.txt"
ID1="reify_core::foo::test_alpha"
ID2="reify_core::bar::test_beta"
printf '%s\n%s\n' "$ID1" "$ID2" > "$FILTER"

# Two gui spec paths and two run_all member basenames for the run_all/gui arms.
GSPEC1="src/__tests__/a.test.ts"
GSPEC2="src/__tests__/b.test.ts"
RAMEMBER1="test_x.sh"
RAMEMBER2="test_y.sh"

# --- nextest availability probe (off the byte-identical default plan) --------
PLAN_DEFAULT="$(bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true
_HEADER="$(printf '%s\n' "$PLAN_DEFAULT" | grep '^# verify.sh plan' || true)"
NEXTEST_AVAILABLE=0
case "$_HEADER" in
    *"nextest=1"*) NEXTEST_AVAILABLE=1 ;;
esac
echo "(nextest available on this host: $NEXTEST_AVAILABLE)"

# The single marker token δ owns (PRD §4.4). Assembled so the greps below pin
# the exact stdout substring without over-constraining the surrounding prose.
MARKER="@@REIFY_RETRY_SCOPE=failed_only@@"

# ---------------------------------------------------------------------------
# B6 (marker PRESENCE). A genuinely-narrowed failed_only nextest retry — the
# eligible config (matching sidecar tree_oid + a within-ceiling 2-id filter) —
# must emit the @@REIFY_RETRY_SCOPE=failed_only@@ honest marker on STDOUT at
# plan-BUILD time, so a real DF retry (which runs build_plan in execute mode)
# and this --print-plan oracle both surface it. NEXTEST-guarded: the narrowing
# here is the nextest subset, which only applies when cargo-nextest is on PATH
# (else the marker legitimately suppresses — asserted host-independently via the
# run_all/gui arms in the count section below).
# RED at base: verify.sh emits no marker yet (greened by step-2).
# ---------------------------------------------------------------------------
echo ""
echo "--- B6: a narrowed nextest failed_only retry emits the honest marker on STDOUT ---"

PLAN_MARK="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "narrowed nextest retry: STDOUT carries the $MARKER honest marker" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$PLAN_MARK" "$MARKER"
fi

# ---------------------------------------------------------------------------
# B6 per-suite COUNTS (+ the B4/B5 count integration). The marker is a single
# line carrying the APPLIED subset size for each suite, sourced from the SAME
# construction sites the suites narrow at (INV-5 no re-derivation):
#   (a) nextest — a 2-id filter ⇒ nextest_debug=2 (default profile=debug, so
#       nextest_release=0). NEXTEST-guarded (the count is the applied subset).
#   (b) run_all — REIFY_RUN_ALL_MEMBER_SUBSET with 2 members ⇒ run_all=2
#       (verify.sh COUNTS only; run_all.sh owns the member-skip). Host-indep.
#   (c) gui     — REIFY_GUI_RETRY_SPECS with 2 validated specs ⇒ gui=2. Host-indep.
# Each case layers its suite subset on top of an otherwise-eligible nextest
# retry (matching sidecar + 2-id filter) so the marker fires (step-2 gate) and
# the four keys are all present on the ONE marker line.
# RED until step-4 computes + prints the counts (step-2 emits the bare token).
# ---------------------------------------------------------------------------
echo ""
echo "--- B6 counts: marker carries per-suite applied subset sizes (nextest / run_all / gui) ---"

# (a) nextest-only narrowed retry.
PLAN_CA="$PLAN_MARK"   # identical config to the B6-presence capture above

# (b) + a 2-member run_all subset.
PLAN_CB="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    REIFY_RUN_ALL_MEMBER_SUBSET="$RAMEMBER1 $RAMEMBER2" \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true

# (c) + a 2-spec gui subset.
PLAN_CC="$(REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    REIFY_GUI_RETRY_SPECS="$GSPEC1 $GSPEC2" \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "count (a): marker carries nextest_debug=2 (the applied 2-id subset)" \
        bash -c 'printf "%s\n" "$1" | grep -F -- "$2" | grep -qF -- "nextest_debug=2"' \
        _ "$PLAN_CA" "$MARKER"
    assert "count (a): marker carries nextest_release=0 (no release pass on profile=debug)" \
        bash -c 'printf "%s\n" "$1" | grep -F -- "$2" | grep -qF -- "nextest_release=0"' \
        _ "$PLAN_CA" "$MARKER"
fi

assert "count (b): REIFY_RUN_ALL_MEMBER_SUBSET (2 members) ⇒ marker carries run_all=2" \
    bash -c 'printf "%s\n" "$1" | grep -F -- "$2" | grep -qF -- "run_all=2"' \
    _ "$PLAN_CB" "$MARKER"

assert "count (c): REIFY_GUI_RETRY_SPECS (2 specs) ⇒ marker carries gui=2" \
    bash -c 'printf "%s\n" "$1" | grep -F -- "$2" | grep -qF -- "gui=2"' \
    _ "$PLAN_CC" "$MARKER"

# All four keys present on the single marker line (asserted host-independently
# on the run_all arm, whose marker fires regardless of nextest availability).
assert "count: all four keys (nextest_debug/nextest_release/run_all/gui) present on the marker line" \
    bash -c '
        L=$(printf "%s\n" "$1" | grep -F -- "$2")
        printf "%s\n" "$L" | grep -qF -- "nextest_debug=" && \
        printf "%s\n" "$L" | grep -qF -- "nextest_release=" && \
        printf "%s\n" "$L" | grep -qF -- "run_all=" && \
        printf "%s\n" "$L" | grep -qF -- "gui="
    ' _ "$PLAN_CB" "$MARKER"

# ---------------------------------------------------------------------------
# B2/B3 honesty-SUPPRESSION (PRD §4.4 INV-6). When a failed_only retry actually
# falls back to a FULL re-verify, STDOUT must carry NO marker — otherwise DF's
# runtime mining would miscount a full re-verify as a failed_only green gate.
# All three cases carry NO run_all/gui subset (env -u), so the ONLY suite that
# could narrow is nextest — and it does not, so the marker must be absent:
#   (a) B2 tree-drift — mismatched sidecar tree_oid ⇒ ineligible ⇒ full; the
#       loud α 'retry refused: tree drift' line is on STDERR.
#   (b) B3 no-subset — matching sidecar (eligible) but an absent filter file ⇒
#       nothing narrowed ⇒ full; the loud α 'retry refused: no subset' line is
#       on STDERR. This is the case the ≥1-suite-narrowed gate exists for.
#   (c) byte-identical DEFAULT — no REIFY_VERIFY_RETRY_* envs at all ⇒ no marker.
# RED until step-6: the step-4 emitter fires on mere tree-OID eligibility, so
# (b) — eligible but not narrowed — wrongly emits the marker. (a) and (c) are
# green-on-arrival permanent locks (ineligible / non-retry never set the gate).
# ---------------------------------------------------------------------------
echo ""
echo "--- B2/B3: marker SUPPRESSED on every full-fallback / refusal path (INV-6) ---"

# (a) B2 tree-drift: mismatched sidecar tree_oid, no run_all/gui subset.
DRIFT_OUT="$(env -u REIFY_RUN_ALL_MEMBER_SUBSET -u REIFY_GUI_RETRY_SPECS \
    REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=cafef00d \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true
DRIFT_ERR="$(env -u REIFY_RUN_ALL_MEMBER_SUBSET -u REIFY_GUI_RETRY_SPECS \
    REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=cafef00d \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$FILTER" \
    bash "$VERIFY" test --scope all --print-plan 2>&1 >/dev/null)" || true

assert "B2 tree-drift: STDOUT carries NO $MARKER marker (full fallback, INV-6)" \
    bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
    _ "$DRIFT_OUT" "$MARKER"
assert "B2 tree-drift: STDERR carries the loud 'retry refused: tree drift' line (α)" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "retry refused: tree drift"' \
    _ "$DRIFT_ERR"

# (b) B3 no-subset: matching sidecar (eligible) but an absent filter file, no
# run_all/gui subset. The per-profile filter vars are cleared too so the base
# path fully resolves to the nonexistent file.
NOSUB_OUT="$(env -u REIFY_RUN_ALL_MEMBER_SUBSET -u REIFY_GUI_RETRY_SPECS \
    -u REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_DEBUG \
    -u REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_RELEASE \
    REIFY_VERIFY_RETRY_SCOPE=failed_only \
    REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
    REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
    REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$_TMP/nonexistent-filter.txt" \
    bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true
assert "B3 no-subset: STDOUT carries NO $MARKER marker (eligible but nothing narrowed, INV-6)" \
    bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
    _ "$NOSUB_OUT" "$MARKER"
# The loud STDERR refusal line is nextest-dependent: on a nextest host the
# eligible-but-no-subset path emits 'retry refused: no subset' (verify.sh's
# NEXTEST=1 block, verify.sh:1411); on a host WITHOUT cargo-nextest the same
# eligible path instead emits 'retry refused: no nextest' (verify.sh:1486), so
# this exact-line assertion — and its NOSUB_ERR capture — is NEXTEST-guarded to
# keep the pool suite host-independent (matches the B6/count/B1 guarding). The
# STDOUT marker-suppression assertion above stays UNGUARDED: the marker is
# correctly absent on BOTH host states.
if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    NOSUB_ERR="$(env -u REIFY_RUN_ALL_MEMBER_SUBSET -u REIFY_GUI_RETRY_SPECS \
        -u REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_DEBUG \
        -u REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_RELEASE \
        REIFY_VERIFY_RETRY_SCOPE=failed_only \
        REIFY_VERIFY_RETRY_TREE_OID=deadbeef \
        REIFY_VERIFY_ATTEMPT_SIDECAR="$SIDECAR" \
        REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE="$_TMP/nonexistent-filter.txt" \
        bash "$VERIFY" test --scope all --print-plan 2>&1 >/dev/null)" || true
    assert "B3 no-subset: STDERR carries the loud 'retry refused: no subset' line (α)" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "retry refused: no subset"' \
        _ "$NOSUB_ERR"
fi

# (c) byte-identical DEFAULT (no REIFY_VERIFY_RETRY_* envs) — captured off the
# nextest-probe plan above — carries no marker (permanent non-retry lock).
assert "default: byte-identical plan carries NO $MARKER marker (non-retry)" \
    bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
    _ "$PLAN_DEFAULT" "$MARKER"

# ---------------------------------------------------------------------------
# B1/B4/B5 cross-suite INTEGRATION regression locks (green-on-arrival — they
# pin the already-landed α/β/γ per-suite behavior cohering with δ's marker on
# the MERGED contract, the raison d'être of an INTEGRATION-GATE; NO paired
# impl). δ asserts only that, under the unified narrowed config, each suite's
# emitted COMMAND LINE is present alongside the marker — it does NOT re-derive
# α's fragment grammar (exact-match soundness / single-`-E` fold / per-profile
# precedence / ceiling all live in α's own plan-shape unit test), avoiding G7
# lock-step duplication. These check the actual cargo/gui command lines, which
# the marker-only count section above does not.
# ---------------------------------------------------------------------------
echo ""
echo "--- B1/B4/B5: α/β/γ command-line shapes cohere with the δ marker (integration locks) ---"

# B1 (locks α): under the narrowed nextest retry (PLAN_MARK) the debug
# `cargo nextest run` line carries the exact-match subset for BOTH filter ids —
# the α primitive the marker's nextest_debug count is derived from. The debug
# pass is the nextest line WITHOUT ` --release`. NEXTEST-guarded (command shape).
if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "B1: debug nextest line carries the exact-match subset test(=$ID1) AND test(=$ID2) (locks α)" \
        bash -c '
            L=$(printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -v -- " --release" | head -1)
            printf "%s\n" "$L" | grep -qF -- "test(=$2)" && \
            printf "%s\n" "$L" | grep -qF -- "test(=$3)"
        ' _ "$PLAN_MARK" "$ID1" "$ID2"
fi

# B5 (locks γ): with REIFY_GUI_RETRY_SPECS set (PLAN_CC) the gui plan line
# forwards `npm test -- <specs>` (== vitest run <specs>) rather than a bare
# `npm test` — visible in --print-plan, host-independent.
assert "B5: gui plan line forwards 'npm test -- $GSPEC1 $GSPEC2' (locks γ, not a bare npm test)" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "npm test -- $2 $3"' \
    _ "$PLAN_CC" "$GSPEC1" "$GSPEC2"

# B4 (locks β): the marker's run_all count (PLAN_CB) equals the
# REIFY_RUN_ALL_MEMBER_SUBSET member count — verify.sh's reify-side signal that
# it saw β's member subset. Member count computed from the fixture so the lock
# tracks it (word-split is safe: the members are plain .sh basenames).
_B4_toks=($RAMEMBER1 $RAMEMBER2); _B4_N=${#_B4_toks[@]}
assert "B4: marker run_all=$_B4_N equals the REIFY_RUN_ALL_MEMBER_SUBSET member count (β signal)" \
    bash -c 'printf "%s\n" "$1" | grep -F -- "$2" | grep -qF -- "run_all=$3"' \
    _ "$PLAN_CB" "$MARKER" "$_B4_N"

# --- assertion sections are appended above this line by steps 1/3/5/7 ---
test_summary
