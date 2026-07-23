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

# --- assertion sections are appended above this line by steps 1/3/5/7 ---
test_summary
