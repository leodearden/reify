#!/usr/bin/env bash
# scripts/prd-gate-substrate-guard.sh
#
# Shared grammar-substrate skip-guard library for the prd_gate wrappers
# (tests/infra/test_prd_gate_corpus.sh, test_prd_gate_compiler_type_hygiene.sh).
# Designed to be SOURCED, not executed directly.
#
# WHY THIS GUARD EXISTS
# ----------------------
# In a sandboxed agent role the landlock write-set does not include
# ~/.cache/tree-sitter/, so `tree-sitter parse` cannot take its
# ~/.cache/tree-sitter/lock/<grammar>.lock and fails to LOAD the reify language
# — "Permission denied (os error 13)". That is not a parse failure: the probe
# never ran as a probe. prd-capability-check.py's observe() therefore classifies
# it HARNESS_ERROR and the run exits 70, which both gates map to a gate FAIL.
# The result is a RED that reports a broken capability when the only thing
# actually broken is the toolchain — violating the house rule these gates were
# written to honour: A MISSING TOOLCHAIN IS A CLEAN SKIP, NEVER A SPURIOUS FAIL.
#
# WHY PER-ROW, NOT WHOLE-SCRIPT (the load-bearing design point)
# -------------------------------------------------------------
# The obvious fix — exit 0 from the whole gate when the substrate is unusable —
# throws away far more than the grammar row. Measured probe_kind counts:
#   tests/prd-gate/corpus-probe-set.json                 1 grammar + 2 check
#   tests/prd-gate/compiler-type-hygiene-probe-set.json  1 grammar + 6 check
# CHECK-kind probes do not touch the grammar substrate at all: build_command()
# sends them to `reify check <fixture>`, and only GRAMMAR-kind probes run
# `tree-sitter parse` with cwd=<repo_root>/tree-sitter-reify. So a whole-script
# skip would silently drop 2 and 6 perfectly runnable rows — trading a spurious
# RED for a silent coverage hole in exactly the sandboxed roles this guard
# exists to serve. This library therefore drops the grammar ROWS and keeps
# running the check rows, behind a loud banner that makes the degradation
# impossible to mistake for full coverage.
#
# ONE PREFLIGHT SUBSUMES BOTH PREVIOUS GUARDS
# --------------------------------------------
# Each gate previously carried two hand-rolled guards: an isfile() check on
# tree-sitter-reify/src/parser.c, and a `command -v` check on the tree-sitter
# CLI. `--grammar-substrate-status` covers both and more, in the right order:
# prd-capability-check.py's main() calls grammar_generated() FIRST and returns
# 75 with a parser.c-naming reason WITHOUT spending a subprocess ("Cheap first,
# so a lane with no grammar pays no subprocess"), then grammar_substrate_usable()
# covers the CLI being unlaunchable, the cache/lock being unwritable, and the
# probe not answering within its bound. A grammar-less lane therefore pays
# exactly what the old isfile() cost, and a lane WITH a grammar pays one
# time-bounded tree-sitter subprocess to learn something isfile() could never
# tell it: whether tree-sitter can actually LOAD that grammar.
#
# USAGE
# -----
#   source "$REPO_ROOT/scripts/prd-gate-substrate-guard.sh"
#   if ! resolve_grammar_substrate "$REPO_ROOT"; then
#       prd_gate_probe_set_drop_grammar "$COMMITTED" "$FILTERED" || { ...all-grammar... }
#       prd_gate_loud_substrate_skip "<gate>" "$PRD_GATE_DROPPED_COUNT" \
#           "$PRD_GATE_KEPT_COUNT" "$GRAMMAR_SUBSTRATE_REASON"
#       CORPUS="$FILTERED"
#   fi
#
# Output globals (the calling convention mirrors scripts/reify-bin-freshness.sh's
# resolve_trusted_reify_bin, which the same two gates source immediately above
# this one, so both preflights read uniformly):
#   GRAMMAR_SUBSTRATE_OK      1 when a grammar probe can run here, else 0
#   GRAMMAR_SUBSTRATE_REASON  operator-facing reason (on unusable), prefix-stripped
#   PRD_GATE_KEPT_COUNT       check-kind probes retained by the filter
#   PRD_GATE_DROPPED_COUNT    grammar-kind probes dropped by the filter

# Source guard — prevent double-sourcing.
if [ "${_PRD_GATE_SUBSTRATE_GUARD_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_PRD_GATE_SUBSTRATE_GUARD_SH_SOURCED=1

# The exit code scripts/prd-capability-check.py --grammar-substrate-status uses
# to say "unusable" (EX_TEMPFAIL — environmental, so callers SKIP rather than
# FAIL). Named rather than inlined so the two places that reason about it
# below cannot drift apart.
_PRD_GATE_SUBSTRATE_UNUSABLE_RC=75

# The stdout prefix that mode writes ahead of the reason. Stripped so callers
# can splice the bare reason into a sentence of their own.
_PRD_GATE_SUBSTRATE_UNUSABLE_PREFIX="grammar substrate: unusable: "

# resolve_grammar_substrate [repo_root]
#
# Single preflight entrypoint: can a grammar-kind probe actually run here?
# Sets GRAMMAR_SUBSTRATE_OK (1/0) and, when not, GRAMMAR_SUBSTRATE_REASON.
# Returns 0 (usable) or 1 (unusable — callers drop the grammar rows, never a
# verdict).
#
# THE `|| RC=$?` TAIL IS LOAD-BEARING TWICE OVER (idiom carried over from
# tests/infra/test_prd_capability_check.sh:50-56, with its rationale): it keeps
# the caller's `set -e` from aborting on the EXPECTED exit 75, and it captures
# the real code — `... || true` followed by `$?` would read 0, because `$?`
# would then be the status of the `|| true` compound rather than of the command
# substitution.
#
# An exit code that is neither 0 nor 75 is reported UNUSABLE with a reason
# naming the code, deliberately: the checker exits 64 (EX_USAGE) for a malformed
# invocation and 70 (EX_SOFTWARE) for a harness error, and laundering either
# into a silent "normal skip" would hide a broken preflight behind the very
# banner that is supposed to explain the degradation.
resolve_grammar_substrate() {
    local repo_root="${1:-$PWD}"

    GRAMMAR_SUBSTRATE_OK=0
    GRAMMAR_SUBSTRATE_REASON=""

    local rc=0 out=""
    out="$(python3 "$repo_root/scripts/prd-capability-check.py" \
        --grammar-substrate-status 2>/dev/null)" || rc=$?

    if [ "$rc" -eq 0 ]; then
        GRAMMAR_SUBSTRATE_OK=1
        return 0
    fi

    if [ "$rc" -eq "$_PRD_GATE_SUBSTRATE_UNUSABLE_RC" ]; then
        # Strip the known prefix with ${var#...} (shortest LEADING match on a
        # literal, no globbing of the reason's own punctuation). A reason that
        # somehow arrives without the prefix passes through unchanged rather
        # than being truncated.
        GRAMMAR_SUBSTRATE_REASON="${out#"$_PRD_GATE_SUBSTRATE_UNUSABLE_PREFIX"}"
        if [ -z "$GRAMMAR_SUBSTRATE_REASON" ]; then
            GRAMMAR_SUBSTRATE_REASON="grammar substrate reported unusable, with no reason given"
        fi
        return 1
    fi

    GRAMMAR_SUBSTRATE_REASON="grammar-substrate preflight returned an unexpected exit code ${rc} (expected 0=usable or ${_PRD_GATE_SUBSTRATE_UNUSABLE_RC}=unusable) — treating the substrate as unusable; output: ${out:-<none>}"
    return 1
}
