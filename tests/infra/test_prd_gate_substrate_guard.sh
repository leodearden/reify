#!/usr/bin/env bash
# Infrastructure test for scripts/prd-gate-substrate-guard.sh (task 5897).
#
# WHAT IS UNDER TEST
# ------------------
# The shared, sourced skip-guard library that the two prd_gate wrappers
# (test_prd_gate_corpus.sh, test_prd_gate_compiler_type_hygiene.sh) use to
# survive a lane where the tree-sitter grammar substrate is unusable — the
# sandboxed-role case where tree-sitter cannot write ~/.cache/tree-sitter/lock/
# and a grammar probe therefore reports HARNESS_ERROR (exit 70), turning a
# missing toolchain into a spurious gate FAIL.
#
# WHY THE LOGIC LIVES IN A LIBRARY, AND WHY THAT MATTERS HERE
# -----------------------------------------------------------
# Both gates derive REPO_ROOT from their own location ("$SCRIPT_DIR/../.."), so
# a test cannot point them at a synthetic tree, and every interesting branch of
# the guard is environment-dependent (is parser.c generated? is the cache
# writable? is the CLI launchable?).  The library instead takes repo_root as an
# ARGUMENT, so the unit layer below can drive it against temp roots holding a
# STUB scripts/prd-capability-check.py that exits 0 / 75 / 64 on demand.  That
# gives genuine hermetic RED/GREEN cycles on every lane, whatever that lane's
# real substrate happens to be.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_prd_gate_substrate_guard ==="

# One per-run scratch root; every fixture is minted UNDER it, so a single
# rm -rf reclaims the lot and no fixture helper needs to append to a cleanup
# array from inside a command-substitution subshell (where the append would be
# silently discarded).
_RUN_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/prd-gate-substrate-guard-XXXXXX")"
cleanup() { rm -rf "$_RUN_ROOT"; }
trap cleanup EXIT

# ── Load the library under test ────────────────────────────────────────────
GUARD_LIB="$REPO_ROOT/scripts/prd-gate-substrate-guard.sh"
if [ ! -f "$GUARD_LIB" ]; then
    echo "  FAIL: $GUARD_LIB not found — the guard library does not exist"
    FAIL=$((FAIL + 1))
    test_summary
fi
# shellcheck source=/dev/null
source "$GUARD_LIB"

# ── Fixture: a synthetic repo root with a stubbed checker ──────────────────
# _mk_stub_root <exit_code> <stdout_line> — mints <root>/scripts/prd-capability-check.py
# as a stub that prints <stdout_line> and exits <exit_code>, then echoes <root>.
#
# The stub reads its exit code and stdout from sibling fixture files rather
# than having them interpolated into its body: the reason strings under test
# contain quotes, parentheses and colons, and baking them through two layers of
# shell + python quoting is exactly the kind of fixture that silently stops
# testing what it claims to.
_mk_stub_root() {
    local rc="$1" line="$2" root
    root="$(mktemp -d "$_RUN_ROOT/root-XXXXXX")" || return 1
    mkdir -p "$root/scripts"
    printf '%s\n' "$line" > "$root/stub-stdout.txt"
    printf '%s\n' "$rc" > "$root/stub-rc.txt"
    cat > "$root/scripts/prd-capability-check.py" <<'PYEOF'
import os, sys

# <root>/scripts/prd-capability-check.py -> <root>
here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
with open(os.path.join(here, "stub-stdout.txt")) as f:
    sys.stdout.write(f.read())
with open(os.path.join(here, "stub-rc.txt")) as f:
    sys.exit(int(f.read().strip()))
PYEOF
    printf '%s\n' "$root"
}

# ── Assertion helpers ──────────────────────────────────────────────────────
# Each returns 0/1 and prints its own diagnostic; assert() captures that output
# and dumps it only on FAIL.  They read the library's output globals with
# ${...:-} defaults so a library that never sets them fails on the comparison
# rather than aborting the whole suite under `set -u`.

_want_usable() {
    local root="$1"
    if ! resolve_grammar_substrate "$root"; then
        echo "expected resolve_grammar_substrate to return 0 (usable), got non-zero"
        return 1
    fi
    if [ "${GRAMMAR_SUBSTRATE_OK:-<unset>}" != "1" ]; then
        echo "GRAMMAR_SUBSTRATE_OK=${GRAMMAR_SUBSTRATE_OK:-<unset>}, want 1"
        return 1
    fi
    return 0
}

_want_unusable_reason() {
    local root="$1" want="$2"
    if resolve_grammar_substrate "$root"; then
        echo "expected resolve_grammar_substrate to return non-zero (unusable), got 0"
        return 1
    fi
    if [ "${GRAMMAR_SUBSTRATE_OK:-<unset>}" != "0" ]; then
        echo "GRAMMAR_SUBSTRATE_OK=${GRAMMAR_SUBSTRATE_OK:-<unset>}, want 0"
        return 1
    fi
    if [ "${GRAMMAR_SUBSTRATE_REASON:-<unset>}" != "$want" ]; then
        echo "GRAMMAR_SUBSTRATE_REASON=${GRAMMAR_SUBSTRATE_REASON:-<unset>}"
        echo "                    want=$want"
        return 1
    fi
    return 0
}

_want_unusable_reason_contains() {
    local root="$1"
    shift
    if resolve_grammar_substrate "$root"; then
        echo "expected resolve_grammar_substrate to return non-zero (unusable), got 0"
        return 1
    fi
    if [ "${GRAMMAR_SUBSTRATE_OK:-<unset>}" != "0" ]; then
        echo "GRAMMAR_SUBSTRATE_OK=${GRAMMAR_SUBSTRATE_OK:-<unset>}, want 0"
        return 1
    fi
    local needle
    for needle in "$@"; do
        case "${GRAMMAR_SUBSTRATE_REASON:-}" in
            *"$needle"*) ;;
            *)  echo "reason does not mention '$needle': ${GRAMMAR_SUBSTRATE_REASON:-<unset>}"
                return 1 ;;
        esac
    done
    return 0
}

# ── Block A: resolve_grammar_substrate ─────────────────────────────────────
echo "-- resolve_grammar_substrate (hermetic, stubbed checker) --"

_ROOT_OK="$(_mk_stub_root 0 'grammar substrate: usable')"

# The exact reason wording is irrelevant to the guard; what matters is that the
# "grammar substrate: unusable: " prefix is stripped, so a caller can splice the
# bare reason into its own sentence.  A reason carrying a colon and parentheses
# is used deliberately: a prefix strip implemented with a greedy match or a
# field split on ':' would mangle it.
_UNUSABLE_REASON='cache/lock unwritable: Permission denied (os error 13) (~/.cache/tree-sitter/lock/reify.lock)'
_ROOT_75="$(_mk_stub_root 75 "grammar substrate: unusable: $_UNUSABLE_REASON")"

# 64 is EX_USAGE — what the checker returns for a malformed invocation. It is
# NOT the skip contract, and must never be laundered into one.
_ROOT_64="$(_mk_stub_root 64 'error: PROBE_SET_JSON is required unless --grammar-substrate-status is given')"

assert "usable checker (exit 0) => returns 0, GRAMMAR_SUBSTRATE_OK=1" \
    _want_usable "$_ROOT_OK"

assert "unusable checker (exit 75) => returns 1, OK=0, reason has the 'unusable: ' prefix stripped" \
    _want_unusable_reason "$_ROOT_75" "$_UNUSABLE_REASON"

assert "unexpected exit code (64) => returns 1, OK=0, and the reason names the code" \
    _want_unusable_reason_contains "$_ROOT_64" "64" "unexpected"

# ── Block B: prd_gate_probe_set_drop_grammar ───────────────────────────────
echo "-- prd_gate_probe_set_drop_grammar (hermetic, synthetic probe-sets) --"

# _mk_probe_set <name> <kind> [<kind>...] — writes a synthetic probe-set with
# one probe per <kind> under $_RUN_ROOT/<name>.json and echoes the path. Each
# probe carries a distinct capability/fixture and a non-trivial `expected`
# (including a nested `match`), so a filter that rebuilt probes field-by-field
# instead of passing them through whole would be caught by the deep-equality
# check below rather than passing on a lucky subset of fields.
_mk_probe_set() {
    local name="$1"
    shift
    local path="$_RUN_ROOT/$name.json"
    printf '%s\n' "$@" | python3 -c '
import json, sys
kinds = [ln.strip() for ln in sys.stdin if ln.strip()]
probes = []
for i, kind in enumerate(kinds):
    probes.append({
        "capability": f"{5897 + i} synthetic {kind} capability",
        "probe_kind": kind,
        "fixture": f"tests/prd-gate/fixtures/synthetic_{kind}_{i}.ri",
        "expected": {"observation": "present", "match": {"stderr": f"E_SYNTH_{i}"}},
        "notes": f"row {i}",
    })
json.dump({"probes": probes}, open(sys.argv[1], "w"), indent=2)
' "$path" || return 1
    printf '%s\n' "$path"
}

# _want_filtered <src> <dst> <want_kept> <want_dropped> — runs the filter,
# expecting it to SUCCEED, then checks the counts and the content contract.
_want_filtered() {
    local src="$1" dst="$2" want_kept="$3" want_dropped="$4"
    if ! prd_gate_probe_set_drop_grammar "$src" "$dst"; then
        echo "expected prd_gate_probe_set_drop_grammar to return 0, got non-zero"
        return 1
    fi
    if [ "${PRD_GATE_KEPT_COUNT:-<unset>}" != "$want_kept" ]; then
        echo "PRD_GATE_KEPT_COUNT=${PRD_GATE_KEPT_COUNT:-<unset>}, want $want_kept"
        return 1
    fi
    if [ "${PRD_GATE_DROPPED_COUNT:-<unset>}" != "$want_dropped" ]; then
        echo "PRD_GATE_DROPPED_COUNT=${PRD_GATE_DROPPED_COUNT:-<unset>}, want $want_dropped"
        return 1
    fi
    # Content contract, checked on PARSED json (formatting is not the contract):
    #   1. dst["probes"] is deep-equal to src's non-grammar probes, IN ORDER —
    #      so every field of every kept probe survives untouched;
    #   2. dst still parses as a probe-set through the checker's own
    #      load_probe_set, i.e. the top-level {"probes": [...]} shape is intact.
    #      Asserted against the REAL loader rather than a re-implementation of
    #      its rules, which would be free to agree with a wrong filter.
    python3 - "$src" "$dst" "$REPO_ROOT" <<'PYEOF'
import importlib.util, json, sys

src_path, dst_path, repo_root = sys.argv[1], sys.argv[2], sys.argv[3]
src = json.load(open(src_path))
dst = json.load(open(dst_path))

want = [p for p in src["probes"] if p.get("probe_kind") != "grammar"]
if dst.get("probes") != want:
    print("filtered probes differ from src's non-grammar probes (order/content):")
    print("  got : " + json.dumps(dst.get("probes")))
    print("  want: " + json.dumps(want))
    sys.exit(1)

spec = importlib.util.spec_from_file_location(
    "prd_capability_check", f"{repo_root}/scripts/prd-capability-check.py")
pcc = importlib.util.module_from_spec(spec)
# Register in sys.modules BEFORE exec_module — the same idiom, for the same
# reason, as scripts/test_prd_capability_check.py:34-39: @dataclass resolves
# cls.__module__ through sys.modules at decoration time, so an unregistered
# module raises AttributeError on the checker's first dataclass.
sys.modules["prd_capability_check"] = pcc
spec.loader.exec_module(pcc)
try:
    loaded = pcc.load_probe_set(open(dst_path).read())
except Exception as e:
    print(f"checker's own load_probe_set rejected the filtered set: {e}")
    sys.exit(1)
if len(loaded) != len(want):
    print(f"load_probe_set read {len(loaded)} probes, want {len(want)}")
    sys.exit(1)
PYEOF
}

# _want_filter_refuses <src> <dst> — the degenerate all-grammar case: nothing
# to keep, so the filter must FAIL rather than hand the checker an empty
# probe-set (which it rejects with exit 64 — a gate FAIL, not a skip).
_want_filter_refuses() {
    local src="$1" dst="$2"
    if prd_gate_probe_set_drop_grammar "$src" "$dst"; then
        echo "expected prd_gate_probe_set_drop_grammar to return non-zero on an all-grammar set, got 0"
        return 1
    fi
    if [ "${PRD_GATE_KEPT_COUNT:-<unset>}" != "0" ]; then
        echo "PRD_GATE_KEPT_COUNT=${PRD_GATE_KEPT_COUNT:-<unset>}, want 0"
        return 1
    fi
    return 0
}

# Mirrors the shape of the real corpus-probe-set.json: 1 grammar + 2 check,
# grammar FIRST, so a filter that dropped by index rather than by kind would be
# indistinguishable here — hence the deep-equality check above.
_PS_MIXED="$(_mk_probe_set mixed grammar check check)"
_PS_ALL_CHECK="$(_mk_probe_set all_check check check check)"
_PS_ALL_GRAMMAR="$(_mk_probe_set all_grammar grammar grammar)"

assert "1 grammar + 2 check => keeps exactly the 2 check probes, in order, field-for-field" \
    _want_filtered "$_PS_MIXED" "$_RUN_ROOT/mixed.filtered.json" 2 1

assert "all-check set => passed through unchanged with DROPPED=0" \
    _want_filtered "$_PS_ALL_CHECK" "$_RUN_ROOT/all_check.filtered.json" 3 0

assert "all-grammar set => KEPT=0 and the filter refuses (never an empty probe-set)" \
    _want_filter_refuses "$_PS_ALL_GRAMMAR" "$_RUN_ROOT/all_grammar.filtered.json"

# ── Block C: prd_gate_loud_substrate_skip ──────────────────────────────────
echo "-- prd_gate_loud_substrate_skip (hermetic) --"

# BOTH STREAMS ARE THE POINT. The precedent this follows
# (tests/infra/test_target_per_lane_independence.sh:139-167) states the reason
# in its own comment: "A quiet stderr-only SKIP line is easy to miss in CI
# output, making a partial-coverage green run indistinguishable from full
# coverage". These gates run under run_all.sh, whose per-test capture is the
# archived verify log — a notice on only one stream is a notice half the
# readers never see. So stdout and stderr are captured to SEPARATE files here
# and each is checked independently; a single 2>&1 capture would pass just as
# happily for a function that wrote to one stream twice.
_LOUD_LABEL="test_prd_gate_corpus"
_LOUD_REASON='cache/lock unwritable: Permission denied (os error 13) (~/.cache/tree-sitter/lock/reify.lock)'

_want_loud_on_both_streams() {
    local out="$_RUN_ROOT/loud.out" err="$_RUN_ROOT/loud.err"
    : > "$out"
    : > "$err"

    if ! prd_gate_loud_substrate_skip "$_LOUD_LABEL" 1 2 "$_LOUD_REASON" \
            > "$out" 2> "$err"; then
        echo "expected prd_gate_loud_substrate_skip to return 0 (it is a notice, not a failure)"
        return 1
    fi

    local stream f needle
    for stream in stdout stderr; do
        if [ "$stream" = "stdout" ]; then f="$out"; else f="$err"; fi
        if [ ! -s "$f" ]; then
            echo "$stream: empty — the notice must land on BOTH streams"
            return 1
        fi
        # The banner must name: the gate, the dropped count, the kept count,
        # and the reason VERBATIM (an abridged reason sends the reader hunting
        # for a cause the log no longer contains).
        for needle in "$_LOUD_LABEL" "$_LOUD_REASON"; do
            if ! grep -qF -- "$needle" "$f"; then
                echo "$stream: banner does not contain '$needle'"
                echo "---- $stream ----"
                cat "$f"
                return 1
            fi
        done
        # Counts are checked in context, not as bare digits: a bare "1"/"2"
        # would match almost any prose and prove nothing.
        if ! grep -qE '1 grammar' "$f"; then
            echo "$stream: banner does not report '1 grammar' row(s) dropped"
            cat "$f"
            return 1
        fi
        if ! grep -qE '2 check' "$f"; then
            echo "$stream: banner does not report '2 check' row(s) still running"
            cat "$f"
            return 1
        fi
    done

    # Bannered, not a lone line — the framing is what makes it survive a wall
    # of CI output.
    if ! grep -q '####' "$out"; then
        echo "stdout: no '####' banner framing"
        cat "$out"
        return 1
    fi
    return 0
}

assert "loud skip => bannered notice on BOTH stdout and stderr, naming gate/counts/reason, returns 0" \
    _want_loud_on_both_streams

# ── Block D: end-to-end, against the REAL gate scripts ─────────────────────
#
# The unit blocks above prove the library's logic. This block proves the
# WIRING: that the two gates actually consult it, and that an unusable
# substrate produces a partial-but-real run instead of exit 70.
#
# WHY AN EXPLICIT REIFY_BIN HANDOFF. resolve_trusted_reify_bin trusts an
# explicit REIFY_BIN outright (its precedence rule 1: a deliberate
# operator/verify.sh handoff, not the auto-discovery path where
# cross-candidate leftovers are the risk). Without the handoff these blocks
# would skip on any lane whose target/.reify-bin-sha predates the branch's own
# commits — i.e. on essentially every task lane, including this one, since a
# shell-only branch never rebuilds the binary and so never restamps the sidecar.
_e2e_reify_bin() {
    if [ -n "${REIFY_BIN:-}" ] && [ -f "${REIFY_BIN}" ]; then
        printf '%s\n' "$REIFY_BIN"; return 0
    fi
    if [ -f "$REPO_ROOT/target/release/reify" ]; then
        printf '%s\n' "$REPO_ROOT/target/release/reify"; return 0
    fi
    if [ -f "$REPO_ROOT/target/debug/reify" ]; then
        printf '%s\n' "$REPO_ROOT/target/debug/reify"; return 0
    fi
    return 1
}

# A tree-sitter that reproduces the MEASURED sandboxed-role cache denial:
# grammar_cache_denied() requires BOTH a load-failure marker ("Failed to load
# language") AND a permission marker ("Permission denied" / "os error 13"), so
# a stub carrying only one half would be classified as an ordinary parse
# failure and would test nothing. The signature text is written to a sibling
# file and cat'd rather than embedded in the stub body — it is unindented and
# quote-heavy, exactly the shape that a heredoc inside a heredoc mangles.
_mk_cache_denial_stub() {
    local stub="$_RUN_ROOT/ts_stub_denied" sig="$_RUN_ROOT/ts_stub_denied_stderr.txt"
    cat > "$sig" <<'EOF'
Error: Failed to load language for path "tests/prd-gate/fixtures/arrow_type.ri"
Caused by: Failed to load language in current directory:
Permission denied (os error 13) (/home/leo/.cache/tree-sitter/lock/reify-69604127a681544d.lock)
EOF
    {
        printf '%s\n' '#!/bin/sh'
        printf 'cat %s >&2\n' "$sig"
        printf '%s\n' 'exit 1'
    } > "$stub"
    chmod +x "$stub"
    printf '%s\n' "$stub"
}

# _gate_under_denied_substrate <gate_script> <gate_name> <want_probe_count>
#
# Drives the REAL gate with TREE_SITTER_BIN pointed at the cache-denial stub
# and asserts the four things that together define "fixed":
#   (a) exit 0 — a clean skip, not a verdict;
#   (b) NOT exit 70 — checked explicitly and separately, because 70 is the
#       specific spurious-FAIL this task exists to kill and a future refactor
#       could regress to it while still failing (a) for some other reason;
#   (c) the loud banner on STDOUT — a silent partial run is the failure mode
#       the banner exists to prevent, so its absence must fail the test;
#   (d) the gate's own GATE_PASS line reports <want_probe_count> probe(s) —
#       proof of PER-ROW skipping. A whole-script `exit 0` would also satisfy
#       (a) and (b); only this assertion distinguishes the two designs.
#
# LANE-ROBUST BY CONSTRUCTION: where the grammar was never generated the
# preflight answers 75 from the absent parser.c without ever consulting the
# stub, and the identical per-row path fires. So this block asserts the same
# thing whether or not the lane has a grammar — which is why it is the PRIMARY
# assertion and the full-run case below is the guarded one.
_gate_under_denied_substrate() {
    local gate="$1" name="$2" want="$3"
    local bin stub out="$_RUN_ROOT/$2.denied.out" err="$_RUN_ROOT/$2.denied.err" rc=0

    if ! bin="$(_e2e_reify_bin)"; then
        echo "no reify binary available (target/{release,debug}/reify absent and REIFY_BIN unset)"
        return 1
    fi
    stub="$(_mk_cache_denial_stub)"

    REIFY_BIN="$bin" TREE_SITTER_BIN="$stub" bash "$gate" > "$out" 2> "$err" || rc=$?

    if [ "$rc" -eq 70 ]; then
        echo "gate exited 70 (HARNESS_ERROR) — the exact spurious FAIL this guard must prevent"
        cat "$out"; cat "$err"
        return 1
    fi
    if [ "$rc" -ne 0 ]; then
        echo "gate exited $rc, want 0 (clean partial run)"
        cat "$out"; cat "$err"
        return 1
    fi
    if ! grep -q 'GRAMMAR probes SKIPPED' "$out"; then
        echo "no loud substrate banner on stdout — a silent partial run"
        cat "$out"
        return 1
    fi
    if ! grep -qF "$want/$want probe(s)" "$out"; then
        echo "gate did not report $want/$want probe(s) — check-kind rows were not run"
        cat "$out"
        return 1
    fi
    return 0
}

# _gate_with_usable_substrate <gate_script> <gate_name> <want_probe_count>
#
# NEGATIVE CONTROL: on a lane whose substrate really is usable, the gate must
# run the FULL committed probe-set and print NO banner — i.e. this task changed
# nothing about the healthy path.
#
# RUN AGAINST THE REAL TOOLCHAIN, WITH NO TREE_SITTER_BIN OVERRIDE. A stubbed
# "clean" tree-sitter (exit 0, no output) cannot serve here: both committed
# grammar probes expect observation "present", so an always-exit-0 stub would
# make the corpus gate's grammar row verdict PASS — and the corpus gate asserts
# every verdict is FAIL/UNPROVABLE, so the stub would FAIL the very gate this
# control claims proves healthy. Stubbing would mean encoding, per gate, which
# verdict each committed grammar row must produce — duplicating the probe-set's
# own bookkeeping in a place that would rot silently. The real toolchain asserts
# exactly "usable substrate ⇒ the gate behaves as it does today", which is the
# gate's own committed contract and needs no new assumption.
#
# SELF-GUARDING, LOUDLY: this case genuinely requires a usable substrate, so on
# a lane without one it announces its own skip rather than passing vacuously —
# a skip-guard on a test of a skip-guard, made explicit.
_gate_with_usable_substrate() {
    local gate="$1" name="$2" want="$3"
    local bin out="$_RUN_ROOT/$2.usable.out" err="$_RUN_ROOT/$2.usable.err" rc=0

    if ! bin="$(_e2e_reify_bin)"; then
        echo "no reify binary available (target/{release,debug}/reify absent and REIFY_BIN unset)"
        return 1
    fi

    REIFY_BIN="$bin" bash "$gate" > "$out" 2> "$err" || rc=$?

    if [ "$rc" -ne 0 ]; then
        echo "gate exited $rc on a usable substrate, want 0"
        cat "$out"; cat "$err"
        return 1
    fi
    if grep -q 'GRAMMAR probes SKIPPED' "$out"; then
        echo "gate printed the substrate banner on a USABLE substrate — the guard is over-firing"
        cat "$out"
        return 1
    fi
    if ! grep -qF "$want/$want probe(s)" "$out"; then
        echo "gate did not report the full $want/$want probe(s) on a usable substrate"
        cat "$out"
        return 1
    fi
    return 0
}

# _skip_usable_control <what> — the explicit, loud skip for the guarded case.
_skip_usable_control() {
    echo "  SKIP: $1 — this lane's grammar substrate is unusable, so the"
    echo "        full-run negative control cannot be exercised here."
    echo "        Reason: ${GRAMMAR_SUBSTRATE_REASON:-<none>}"
    echo "        (the PRIMARY denied-substrate assertion above still ran)"
}

echo "-- end-to-end: tests/infra/test_prd_gate_corpus.sh --"

_CORPUS_GATE="$SCRIPT_DIR/test_prd_gate_corpus.sh"

assert "corpus gate under a denied substrate => exit 0 (not 70), loud banner, and its 2 check probes still run" \
    _gate_under_denied_substrate "$_CORPUS_GATE" "corpus" 2

if resolve_grammar_substrate "$REPO_ROOT"; then
    assert "corpus gate on a usable substrate => full 3-probe committed set, no banner" \
        _gate_with_usable_substrate "$_CORPUS_GATE" "corpus" 3
else
    _skip_usable_control "corpus gate full-run control"
fi

echo "-- end-to-end: tests/infra/test_prd_gate_compiler_type_hygiene.sh --"

# THE TWO GATES ARE ASSERTED SEPARATELY, NOT SHARED. This one asserts all-PASS
# (every §8 boundary-table row is green on one commit); the corpus gate asserts
# all-FAIL/UNPROVABLE (a historical-false-premise corpus). Their success lines
# and probe counts differ, so a single shared e2e block would have to be
# parameterized on the very thing most likely to break — and would pass while
# checking the wrong gate's contract. The probe-set is 1 grammar + 6 check, so
# a denied substrate must still run 6.
_HYGIENE_GATE="$SCRIPT_DIR/test_prd_gate_compiler_type_hygiene.sh"

assert "hygiene gate under a denied substrate => exit 0 (not 70), loud banner, and its 6 check probes still run" \
    _gate_under_denied_substrate "$_HYGIENE_GATE" "hygiene" 6

if resolve_grammar_substrate "$REPO_ROOT"; then
    assert "hygiene gate on a usable substrate => full 7-probe committed set, no banner" \
        _gate_with_usable_substrate "$_HYGIENE_GATE" "hygiene" 7
else
    _skip_usable_control "hygiene gate full-run control"
fi

test_summary
