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
# USAGE — the composed entrypoint is what gates should call
# ----------------------------------------------------------
#   source "$REPO_ROOT/scripts/prd-gate-substrate-guard.sh"
#   _rc=0
#   prd_gate_resolve_probe_set "<gate>" "$REPO_ROOT" "$COMMITTED" || _rc=$?
#   case "$_rc" in
#       0) PROBE_SET="$PRD_GATE_PROBE_SET" ;;             # full set, or filtered
#       1) echo "SKIP: every probe is a grammar probe"; exit 0 ;;
#       *) echo "  FAIL: probe-set missing, invalid, or harness error"
#          FAIL=$((FAIL + 1)); test_summary ;;
#   esac
#
# The three primitives it composes (resolve_grammar_substrate,
# prd_gate_probe_set_drop_grammar, prd_gate_loud_substrate_skip) remain public
# so the unit layer can drive each branch hermetically.
#
# Output globals (the calling convention mirrors scripts/reify-bin-freshness.sh's
# resolve_trusted_reify_bin, which the same two gates source immediately above
# this one, so both preflights read uniformly):
#   GRAMMAR_SUBSTRATE_OK        1 when a grammar probe can run here, else 0
#   GRAMMAR_SUBSTRATE_REASON    operator-facing reason (on unusable), prefix-stripped
#   PRD_GATE_KEPT_COUNT         check-kind probes retained by the filter
#   PRD_GATE_DROPPED_COUNT      grammar-kind probes dropped by the filter
#   PRD_GATE_PROBE_SET          the probe-set path the caller should use
#   PRD_GATE_SUBSTRATE_STATUS   per-run memo of the preflight answer (see below)

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

# How much of a failed preflight's stderr to quote back in the reason. Bounded
# because a python traceback can run to kilobytes and this string ends up in a
# banner; the TAIL is kept because that is where the exception line lives.
_PRD_GATE_SUBSTRATE_STDERR_TAIL_BYTES=400

# Return codes shared by prd_gate_probe_set_drop_grammar and the composed
# prd_gate_resolve_probe_set. The 1/2 split is load-bearing: 1 is a legitimate
# degenerate SKIP, 2 is a real defect the caller must surface as a FAIL.
_PRD_GATE_RC_OK=0
_PRD_GATE_RC_ALL_GRAMMAR=1
_PRD_GATE_RC_SRC_UNUSABLE=2

# rm -f that tolerates an empty path without tripping a caller's `set -e`.
_prd_gate_rm_tmp() {
    if [ -n "${1:-}" ]; then
        rm -f "$1"
    fi
    return 0
}

# The memo key for one preflight answer. TREE_SITTER_BIN is part of it because
# the answer is a property of (tree, toolchain), not of the tree alone: the
# guard's own e2e tests re-run a gate with TREE_SITTER_BIN pointed at a
# cache-denial stub, and a key that ignored it would hand that run a stale
# "usable" and silently un-test the thing under test.
_prd_gate_substrate_memo_key() {
    printf '%s' "${1}::${TREE_SITTER_BIN:-<default>}"
}

# resolve_grammar_substrate [repo_root]
#
# Single preflight entrypoint: can a grammar-kind probe actually run here?
# Sets GRAMMAR_SUBSTRATE_OK (1/0) and, when not, GRAMMAR_SUBSTRATE_REASON.
# Returns 0 (usable) or 1 (unusable — callers drop the grammar rows, never a
# verdict).
#
# MEMOIZED FOR THE LIFETIME OF THE SHELL, via PRD_GATE_SUBSTRATE_STATUS
# ("<repo_root>::<ts-bin>|usable|" / "...|unusable|<reason>"). The preflight
# costs one real, cc-invoking tree-sitter subprocess on a cold cache, and a
# single gate (or the guard's own test suite) consults it more than once. The
# memo is a PLAIN shell variable, deliberately not exported: a fresh shell —
# including every gate run_all.sh spawns — probes for real, which is what keeps
# a per-process answer honest. A harness that genuinely wants to share one
# answer with its children can `export PRD_GATE_SUBSTRATE_STATUS` itself, and
# the key guarantees a child running a different tree-sitter still misses.
# Set PRD_GATE_SUBSTRATE_STATUS="" to force a fresh probe (a caller checking
# whether the substrate flipped mid-run must do this, or it re-reads its own
# earlier answer).
#
# THE `|| RC=$?` TAIL IS LOAD-BEARING TWICE OVER (idiom carried over from
# tests/infra/test_prd_capability_check.sh:50-56, with its rationale): it keeps
# the caller's `set -e` from aborting on the EXPECTED exit 75, and it captures
# the real code — `... || true` followed by `$?` would read 0, because `$?`
# would then be the status of the `|| true` compound rather than of the command
# substitution.
#
# An exit code that is neither 0 nor 75 is reported UNUSABLE with a reason
# naming the code AND quoting the preflight's stderr, deliberately: the checker
# exits 64 (EX_USAGE) for a malformed invocation and 70 (EX_SOFTWARE) for a
# harness error, and the likeliest unexpected exits of all (an unhandled
# traceback, an unimportable module, python3 missing) say nothing at all on
# stdout — so a stdout-only report would announce a degradation while
# withholding the one line that explains it.
resolve_grammar_substrate() {
    local repo_root="${1:-$PWD}"

    GRAMMAR_SUBSTRATE_OK=0
    GRAMMAR_SUBSTRATE_REASON=""

    local key memo
    key="$(_prd_gate_substrate_memo_key "$repo_root")"
    # '|' is the memo's field separator. A path containing one is pathological,
    # but silently mis-keying would be worse than not memoizing at all.
    case "$key" in
        *'|'*) key="" ;;
    esac

    memo="${PRD_GATE_SUBSTRATE_STATUS:-}"
    if [ -n "$key" ] && [ -n "$memo" ] && [ "${memo%%|*}" = "$key" ]; then
        local memo_rest="${memo#*|}"
        if [ "${memo_rest%%|*}" = "usable" ]; then
            GRAMMAR_SUBSTRATE_OK=1
            return 0
        fi
        # Reason is the LAST field, so its own '|'s survive intact.
        GRAMMAR_SUBSTRATE_REASON="${memo_rest#*|}"
        return 1
    fi

    local rc=0 out="" err_file=""
    err_file="$(mktemp "${TMPDIR:-/tmp}/prd-gate-substrate-err-XXXXXX")" || err_file=""
    if [ -n "$err_file" ]; then
        out="$(python3 "$repo_root/scripts/prd-capability-check.py" \
            --grammar-substrate-status 2>"$err_file")" || rc=$?
    else
        out="$(python3 "$repo_root/scripts/prd-capability-check.py" \
            --grammar-substrate-status 2>/dev/null)" || rc=$?
    fi

    if [ "$rc" -eq 0 ]; then
        _prd_gate_rm_tmp "$err_file"
        GRAMMAR_SUBSTRATE_OK=1
        _prd_gate_substrate_memoize "$key" usable ""
        return 0
    fi

    if [ "$rc" -eq "$_PRD_GATE_SUBSTRATE_UNUSABLE_RC" ]; then
        # Expected outcome: stderr is not evidence of anything, so it stays
        # suppressed. Strip the known prefix with ${var#...} (shortest LEADING
        # match on a literal, no globbing of the reason's own punctuation). A
        # reason that somehow arrives without the prefix passes through
        # unchanged rather than being truncated.
        _prd_gate_rm_tmp "$err_file"
        GRAMMAR_SUBSTRATE_REASON="${out#"$_PRD_GATE_SUBSTRATE_UNUSABLE_PREFIX"}"
        if [ -z "$GRAMMAR_SUBSTRATE_REASON" ]; then
            GRAMMAR_SUBSTRATE_REASON="grammar substrate reported unusable, with no reason given"
        fi
        _prd_gate_substrate_memoize "$key" unusable "$GRAMMAR_SUBSTRATE_REASON"
        return 1
    fi

    local err_tail=""
    if [ -n "$err_file" ] && [ -s "$err_file" ]; then
        # Newlines folded to spaces: the reason is printed verbatim into a
        # one-line banner field, and a raw traceback would shred its framing.
        err_tail="$(tail -c "$_PRD_GATE_SUBSTRATE_STDERR_TAIL_BYTES" "$err_file" | tr '\n' ' ')"
    fi
    _prd_gate_rm_tmp "$err_file"

    GRAMMAR_SUBSTRATE_REASON="grammar-substrate preflight returned an unexpected exit code ${rc} (expected 0=usable or ${_PRD_GATE_SUBSTRATE_UNUSABLE_RC}=unusable) — treating the substrate as unusable; stdout: ${out:-<none>}; stderr: ${err_tail:-<none>}"
    _prd_gate_substrate_memoize "$key" unusable "$GRAMMAR_SUBSTRATE_REASON"
    return 1
}

# _prd_gate_substrate_memoize <key> <usable|unusable> <reason>
# No-op on an empty (unmemoizable) key.
_prd_gate_substrate_memoize() {
    if [ -z "${1:-}" ]; then
        return 0
    fi
    PRD_GATE_SUBSTRATE_STATUS="${1}|${2}|${3}"
    return 0
}

# prd_gate_probe_set_drop_grammar <src_json> <dst_json>
#
# Writes <dst_json>: <src_json> with every probe_kind=="grammar" probe removed
# and every other probe PASSED THROUGH WHOLE, in its original order. Sets
# PRD_GATE_KEPT_COUNT / PRD_GATE_DROPPED_COUNT.
#
# Probes are copied as opaque objects rather than rebuilt field-by-field, so a
# probe-set gaining a field tomorrow keeps working here with no edit — and, more
# importantly, so this filter can never quietly alter a row it claims only to
# have carried across.
#
# THE TWO FAILURE MODES ARE REPORTED SEPARATELY, and the distinction is the
# whole point of the split:
#   1 (_PRD_GATE_RC_ALL_GRAMMAR)  nothing would be kept — an all-grammar
#     probe-set. A legitimate degenerate case: the caller must whole-script
#     SKIP, because an empty probe-set is a usage error the checker rejects
#     with exit 64, which both gates map to a gate FAIL.
#   2 (_PRD_GATE_RC_SRC_UNUSABLE) the source could not be read, parsed, or
#     filtered at all. That is a REAL DEFECT — a corrupt or missing committed
#     tests/prd-gate/*.json — and the caller must FAIL, exactly as it did
#     before this guard existed (the corrupt file reached the checker and came
#     back exit 64). Collapsing it into 1 would turn a broken committed
#     artifact into a green skip announcing the wrong cause, on precisely the
#     sandboxed lanes this guard targets.
# (Neither committed probe-set is all-grammar today; the 1 branch is here so a
# future one that is cannot turn a skip into a failure.)
prd_gate_probe_set_drop_grammar() {
    local src="$1" dst="$2"

    PRD_GATE_KEPT_COUNT=0
    PRD_GATE_DROPPED_COUNT=0

    local raw
    raw="$(python3 - "$src" "$dst" <<'PYEOF'
import json, sys

src_path, dst_path = sys.argv[1], sys.argv[2]
with open(src_path) as f:
    data = json.load(f)

probes = data.get("probes", [])
kept = [p for p in probes if p.get("probe_kind") != "grammar"]
dropped = len(probes) - len(kept)

# Only the "probes" key is rewritten; any sibling top-level key the probe-set
# grows later rides along untouched.
out = dict(data, probes=kept)
with open(dst_path, "w") as f:
    json.dump(out, f, indent=2)
    f.write("\n")

# Sentinel-prefixed so the shell can find the counts by name rather than by
# position — a python warning or a sitecustomize print on stdout must not be
# mistaken for the answer.
print(f"PRD_GATE_COUNTS {len(kept)} {dropped}")
PYEOF
)" || {
        # A src that is missing, unreadable, or not JSON. The python's own
        # traceback is left on stderr: it names the file and the parse error,
        # which is the diagnosis the caller's FAIL line cannot carry.
        printf '%s\n' "ERROR: prd_gate_probe_set_drop_grammar: cannot read or parse probe-set '$src'" >&2
        return "$_PRD_GATE_RC_SRC_UNUSABLE"
    }

    # Take the LAST sentinel line, and accept the counts only if both are
    # digits. Positional parsing of the whole stdout would turn any stray
    # output into a non-integer count, whose `-gt` comparison errors (status 2)
    # and would land the caller on the degenerate-SKIP path with the wrong
    # label — a silent coverage drop wearing a misleading cause.
    local counts tag kept dropped
    counts="$(printf '%s\n' "$raw" | grep '^PRD_GATE_COUNTS ' | tail -n 1)" || counts=""
    read -r tag kept dropped <<< "${counts:-}" || true
    case "${kept:-}" in ''|*[!0-9]*) kept="" ;; esac
    case "${dropped:-}" in ''|*[!0-9]*) dropped="" ;; esac
    if [ -z "$kept" ] || [ -z "$dropped" ]; then
        printf '%s\n' "ERROR: prd_gate_probe_set_drop_grammar: no parsable 'PRD_GATE_COUNTS <kept> <dropped>' line in the filter's output (got: ${raw:-<none>})" >&2
        return "$_PRD_GATE_RC_SRC_UNUSABLE"
    fi

    PRD_GATE_KEPT_COUNT="$kept"
    PRD_GATE_DROPPED_COUNT="$dropped"

    if [ "$PRD_GATE_KEPT_COUNT" -gt 0 ]; then
        return "$_PRD_GATE_RC_OK"
    fi
    return "$_PRD_GATE_RC_ALL_GRAMMAR"
}

# prd_gate_loud_substrate_skip <gate_label> <dropped> <kept> <reason>
#
# Emits a bannered notice that <dropped> grammar-kind row(s) did NOT run, that
# <kept> check-kind row(s) DID, and why. Always returns 0: a partial run on an
# unusable substrate is a legitimate, expected outcome — this only makes the
# degradation impossible to miss rather than silent.
#
# WRITTEN TO BOTH STREAMS, deliberately, adopting
# tests/infra/test_target_per_lane_independence.sh's _skip precedent and its
# stated rationale: "A quiet stderr-only SKIP line is easy to miss in CI
# output, making a partial-coverage green run indistinguishable from full
# coverage." stdout so it lands in the run summary/log that run_all.sh
# archives; stderr so it survives a caller that consumes stdout.
#
# ADAPTED IN ONE RESPECT from that precedent: this reports a PARTIAL
# degradation rather than a whole-group skip, so it names both counts and does
# NOT exit. The caller keeps running its remaining probes.
#
# The reason is printed VERBATIM and unwrapped. It routinely carries a filesystem
# path or an errno string, and a reflowed or abridged one sends the reader
# hunting for a cause the log no longer contains.
prd_gate_loud_substrate_skip() {
    local gate_label="$1" dropped="$2" kept="$3" reason="$4"
    local warn_block
    warn_block="$(cat <<EOF

################################################################
# WARN: ${gate_label} — GRAMMAR probes SKIPPED (substrate unusable)
# ${dropped} grammar row(s) did NOT run; ${kept} check row(s) DID run
# and are fully asserted below, so this is a PARTIAL-coverage green
# run, not a full one.
# Reason: ${reason}
################################################################
EOF
)"
    printf '%s\n' "$warn_block"
    printf '%s\n' "$warn_block" >&2
    return 0
}

# prd_gate_resolve_probe_set <gate_label> <repo_root> <committed_json>
#
# THE COMPOSED ENTRYPOINT both gates call. Runs the preflight; on a usable
# substrate hands back the committed probe-set untouched, and on an unusable
# one mints a grammar-filtered temp copy, emits the loud banner, and hands that
# back instead. Sets PRD_GATE_PROBE_SET to the path to use.
#
# WHY THIS EXISTS RATHER THAN THREE CALLS PER GATE: the preflight → mktemp →
# trap → filter → banner → reassign sequence is identical in both gates down to
# the degenerate-case handling, and a shared library that stopped at the three
# primitives would have left exactly the copy-paste it was written to remove —
# so a fix to the degenerate path (or a third gate) would have to be applied
# twice. The primitives stay public because the unit layer drives each branch
# hermetically; this composition is what production callers use.
#
# Returns:
#   0  use "$PRD_GATE_PROBE_SET" (the committed set, or the filtered copy)
#   1  degenerate all-grammar set — caller must whole-script SKIP (exit 0)
#   2  the committed set is missing/unreadable/invalid — caller must FAIL
#
# IT OWNS THE EXIT TRAP for the temp file it mints, so neither gate installs
# its own. bash EXIT traps do NOT stack (test_helpers.sh says so in its own
# note), so one owner is the only safe arrangement: a caller that later
# installs an EXIT trap of its own would silently disown this cleanup — today
# neither gate has one, and neither should grow one without chaining it here.
prd_gate_resolve_probe_set() {
    local gate_label="$1" repo_root="$2" committed="$3"

    PRD_GATE_PROBE_SET="$committed"

    if resolve_grammar_substrate "$repo_root"; then
        return "$_PRD_GATE_RC_OK"
    fi

    local filtered
    filtered="$(mktemp "${TMPDIR:-/tmp}/prd-gate-filtered-XXXXXX.json")" || {
        printf '%s\n' "ERROR: prd_gate_resolve_probe_set: mktemp failed; cannot mint a filtered probe-set" >&2
        return "$_PRD_GATE_RC_SRC_UNUSABLE"
    }
    PRD_GATE_FILTERED_PROBE_SET="$filtered"
    # Expanded at trap time, not now, so the quoting stays honest.
    trap 'rm -f "$PRD_GATE_FILTERED_PROBE_SET"' EXIT

    local rc=0
    prd_gate_probe_set_drop_grammar "$committed" "$filtered" || rc=$?

    if [ "$rc" -eq "$_PRD_GATE_RC_SRC_UNUSABLE" ]; then
        # NOT a skip: the committed artifact is broken. No banner — a banner
        # here would name the substrate as the cause of a defect that has
        # nothing to do with it.
        return "$_PRD_GATE_RC_SRC_UNUSABLE"
    fi

    if [ "$rc" -ne 0 ]; then
        prd_gate_loud_substrate_skip "$gate_label" \
            "$PRD_GATE_DROPPED_COUNT" 0 "$GRAMMAR_SUBSTRATE_REASON"
        return "$_PRD_GATE_RC_ALL_GRAMMAR"
    fi

    prd_gate_loud_substrate_skip "$gate_label" \
        "$PRD_GATE_DROPPED_COUNT" "$PRD_GATE_KEPT_COUNT" "$GRAMMAR_SUBSTRATE_REASON"
    PRD_GATE_PROBE_SET="$filtered"
    return "$_PRD_GATE_RC_OK"
}
