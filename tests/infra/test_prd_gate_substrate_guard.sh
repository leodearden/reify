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
# One line per invocation: the memoization assertion below counts REAL
# subprocess launches rather than trusting the library's own report of its
# cache.
with open(os.path.join(here, "stub-calls.txt"), "a") as f:
    f.write("call\n")
with open(os.path.join(here, "stub-stdout.txt")) as f:
    sys.stdout.write(f.read())
with open(os.path.join(here, "stub-rc.txt")) as f:
    sys.exit(int(f.read().strip()))
PYEOF
    printf '%s\n' "$root"
}

# How many times <root>'s stub checker has actually been launched.
_stub_call_count() {
    local f="$1/stub-calls.txt"
    if [ -f "$f" ]; then
        wc -l < "$f" | tr -d ' '
    else
        printf '0\n'
    fi
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

# The unexpected-code reason must also carry the preflight's STDERR. That is the
# whole diagnostic value of this branch: the likeliest unexpected exits of all
# (an unhandled traceback, an unimportable module, a missing python3) write
# nothing at all to STDOUT, so a stdout-only report announces a degradation
# while withholding the one line that would explain it.
_STUB_STDERR_MARK='ModuleNotFoundError: No module named reify_synthetic_missing_dep'
_ROOT_STDERR="$(_mk_stub_root 1 'unused')"
printf '%s\n' "$_STUB_STDERR_MARK" > "$_ROOT_STDERR/stub-stderr.txt"
cat > "$_ROOT_STDERR/scripts/prd-capability-check.py" <<'PYEOF'
import os, sys

# Silent on stdout, loud on stderr — the shape of every unexpected exit that
# actually happens in the wild.
here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
with open(os.path.join(here, "stub-stderr.txt")) as f:
    sys.stderr.write(f.read())
sys.exit(1)
PYEOF

assert "unexpected exit with a silent stdout => the reason quotes the preflight's STDERR" \
    _want_unusable_reason_contains "$_ROOT_STDERR" "$_STUB_STDERR_MARK" "stderr:"

# MEMOIZATION — one preflight per (repo_root, tree-sitter) per shell. The
# preflight costs a real, cc-invoking tree-sitter subprocess on a cold cache and
# a single run consults it repeatedly; the memo makes the repeats free WITHOUT
# letting a fresh shell inherit a stale answer. Asserted by counting the stub's
# actual launches, not by reading the library's own cache variable.
_want_memoized() {
    local root_a="$1" root_b="$2"
    local n1 n2 m1 m2 m3

    PRD_GATE_SUBSTRATE_STATUS=""
    resolve_grammar_substrate "$root_a" || true
    n1="$(_stub_call_count "$root_a")"
    resolve_grammar_substrate "$root_a" || true
    n2="$(_stub_call_count "$root_a")"
    if [ "$n1" != "$n2" ]; then
        echo "a repeat consult of the SAME root re-ran the checker ($n1 -> $n2 launches)"
        return 1
    fi
    if [ "${GRAMMAR_SUBSTRATE_OK:-<unset>}" != "1" ]; then
        echo "the memoized answer came back OK=${GRAMMAR_SUBSTRATE_OK:-<unset>}, want 1"
        return 1
    fi

    m1="$(_stub_call_count "$root_b")"
    resolve_grammar_substrate "$root_b" || true
    m2="$(_stub_call_count "$root_b")"
    if [ "$m2" = "$m1" ]; then
        echo "a DIFFERENT repo_root hit the memo — the key is not root-scoped"
        return 1
    fi
    if [ "${GRAMMAR_SUBSTRATE_OK:-<unset>}" != "0" ]; then
        echo "the second root's own answer was masked by the first root's (OK=${GRAMMAR_SUBSTRATE_OK:-<unset>})"
        return 1
    fi

    # The documented reset. A caller re-checking whether the substrate flipped
    # mid-run needs it, or it just re-reads its own earlier answer.
    PRD_GATE_SUBSTRATE_STATUS=""
    resolve_grammar_substrate "$root_b" || true
    m3="$(_stub_call_count "$root_b")"
    if [ "$m3" = "$m2" ]; then
        echo 'PRD_GATE_SUBSTRATE_STATUS="" did not force a fresh probe'
        return 1
    fi
    return 0
}

assert "the preflight memoizes per repo_root, a different root misses, and an empty memo forces a fresh probe" \
    _want_memoized "$_ROOT_OK" "$_ROOT_75"

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

# _want_filter_rc <src> <dst> <want_rc> — the filter's two refusal modes, which
# must NOT be conflated:
#   1  the degenerate all-grammar set — nothing to keep, so the caller SKIPs
#      rather than handing the checker an empty probe-set (exit 64 — a gate
#      FAIL, not a skip);
#   2  the source is missing, unreadable, or not JSON — a REAL defect in a
#      committed artifact, which the caller must surface as a FAIL. Before this
#      guard existed such a file reached the checker and came back exit 64,
#      which both gates map to a FAIL; collapsing it into 1 would turn a broken
#      committed probe-set into a green skip announcing the substrate as the
#      cause — and only on the sandboxed lanes this guard was written to serve,
#      where the filter path is the one that runs.
_want_filter_rc() {
    local src="$1" dst="$2" want_rc="$3" rc=0
    prd_gate_probe_set_drop_grammar "$src" "$dst" || rc=$?
    if [ "$rc" != "$want_rc" ]; then
        echo "prd_gate_probe_set_drop_grammar returned $rc, want $want_rc"
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

assert "all-grammar set => KEPT=0 and the filter refuses with rc 1 (never an empty probe-set)" \
    _want_filter_rc "$_PS_ALL_GRAMMAR" "$_RUN_ROOT/all_grammar.filtered.json" 1

# A source the filter cannot read at all is NOT the degenerate case, and must
# not be reported as one. Both shapes are exercised because they fail in
# different places — open() vs json.load().
_PS_MISSING="$_RUN_ROOT/does-not-exist.json"
_PS_CORRUPT="$_RUN_ROOT/corrupt.json"
printf '%s\n' '{not json' > "$_PS_CORRUPT"

assert "a nonexistent probe-set => rc 2 (source unusable), NOT rc 1 (degenerate skip)" \
    _want_filter_rc "$_PS_MISSING" "$_RUN_ROOT/missing.filtered.json" 2

assert "a probe-set that is not JSON => rc 2 (source unusable), NOT rc 1 (degenerate skip)" \
    _want_filter_rc "$_PS_CORRUPT" "$_RUN_ROOT/corrupt.filtered.json" 2

# ── Block B2: prd_gate_resolve_probe_set (the composed entrypoint) ──────────
echo "-- prd_gate_resolve_probe_set (hermetic, stubbed checker + synthetic probe-sets) --"

# RUN IN A SUBSHELL, ALWAYS. The composed entrypoint owns an EXIT trap for the
# temp probe-set it mints, and bash EXIT traps do not stack — calling it in
# THIS shell would silently disown this suite's own `trap cleanup EXIT` and leak
# the whole scratch root. A subshell's trap fires at subshell exit and leaves
# the parent's alone, so the filtered file is copied out before it is reclaimed.
#
# Sets: _C_RC, _C_PATH (the PRD_GATE_PROBE_SET it chose), _C_OUT (stdout+stderr
# capture), _C_COPY (a copy of whatever probe-set it pointed at).
_run_composed() {
    local root="$1" committed="$2"
    _C_OUT="$_RUN_ROOT/composed.out"
    _C_COPY="$_RUN_ROOT/composed.copy.json"
    local meta="$_RUN_ROOT/composed.meta"
    rm -f "$_C_OUT" "$_C_COPY" "$meta"
    (
        rc=0
        prd_gate_resolve_probe_set "unit-composed-gate" "$root" "$committed" || rc=$?
        if [ -n "${PRD_GATE_PROBE_SET:-}" ] && [ -f "${PRD_GATE_PROBE_SET:-}" ]; then
            cp "$PRD_GATE_PROBE_SET" "$_C_COPY" || true
        fi
        printf '%s\n%s\n' "$rc" "${PRD_GATE_PROBE_SET:-<unset>}" > "$meta"
    ) > "$_C_OUT" 2>&1
    _C_RC="$(sed -n 1p "$meta")"
    _C_PATH="$(sed -n 2p "$meta")"
    return 0
}

# _want_composed <root> <committed> <want_rc> <want_path: same|other> <want_banner: yes|no>
_want_composed() {
    local root="$1" committed="$2" want_rc="$3" want_path="$4" want_banner="$5"
    _run_composed "$root" "$committed"

    if [ "${_C_RC:-<unset>}" != "$want_rc" ]; then
        echo "prd_gate_resolve_probe_set returned ${_C_RC:-<unset>}, want $want_rc"
        cat "$_C_OUT"
        return 1
    fi
    case "$want_path" in
        same)
            if [ "$_C_PATH" != "$committed" ]; then
                echo "PRD_GATE_PROBE_SET=$_C_PATH, want the committed set unchanged ($committed)"
                return 1
            fi ;;
        other)
            if [ "$_C_PATH" = "$committed" ]; then
                echo "PRD_GATE_PROBE_SET is still the committed set — no filtered copy was minted"
                return 1
            fi ;;
    esac
    if [ "$want_banner" = "yes" ] && ! grep -q 'GRAMMAR probes SKIPPED' "$_C_OUT"; then
        echo "no loud substrate banner — a silent degradation"
        cat "$_C_OUT"
        return 1
    fi
    if [ "$want_banner" = "no" ] && grep -q 'GRAMMAR probes SKIPPED' "$_C_OUT"; then
        echo "banner emitted when it should not have been (it would name the substrate for an unrelated cause)"
        cat "$_C_OUT"
        return 1
    fi
    return 0
}

# The filtered copy must be the real thing, not just a different path.
_want_composed_filtered_content() {
    local want_kept="$1"
    local n
    n="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(len(d["probes"])); sys.exit(0 if not [p for p in d["probes"] if p.get("probe_kind")=="grammar"] else 1)' "$_C_COPY")" || {
        echo "the filtered copy still contains a grammar probe"
        return 1
    }
    if [ "$n" != "$want_kept" ]; then
        echo "the filtered copy holds $n probe(s), want $want_kept"
        return 1
    fi
    return 0
}

assert "usable substrate => rc 0, the COMMITTED set is used unchanged, and no banner" \
    _want_composed "$_ROOT_OK" "$_PS_MIXED" 0 same no

assert "unusable substrate => rc 0, a FILTERED copy is used, and the banner fires" \
    _want_composed "$_ROOT_75" "$_PS_MIXED" 0 other yes

assert "the filtered copy the composed entrypoint hands back really has the grammar row removed" \
    _want_composed_filtered_content 2

assert "unusable substrate + all-grammar set => rc 1 (caller whole-script SKIPs)" \
    _want_composed "$_ROOT_75" "$_PS_ALL_GRAMMAR" 1 same yes

assert "unusable substrate + unreadable committed set => rc 2 (caller FAILs) and NO banner" \
    _want_composed "$_ROOT_75" "$_PS_MISSING" 2 same no

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
# substrate produces a partial-but-real run instead of a HARNESS_ERROR verdict.
#
# WHAT THIS BLOCK MAY AND MAY NOT ASSERT (task 5897, review amendment)
# ---------------------------------------------------------------------
# Driving the real gates means their FULL verdict outcome — 3/3 FAIL-or-
# UNPROVABLE for the corpus gate, 7/7 PASS for the hygiene gate — is only as
# trustworthy as the lane's reify binary. #5133's freshness guard exists
# precisely because an auto-discovered target/{release,debug}/reify can predate
# the current tree or belong to a different merge candidate; the two gates
# themselves SKIP in that case rather than publish a verdict from it.
#
# So the binary is resolved through resolve_trusted_reify_bin FIRST, and the
# assertion strength follows its answer:
#   * TRUSTED (an explicit REIFY_BIN handoff, or a sidecar matching HEAD) —
#     assert everything, including exit 0 and the N/N probe count.
#   * NOT TRUSTED — hand the binary over anyway (so the wiring is still
#     exercised) but assert only the properties THIS GUARD owns: that no
#     HARNESS_ERROR/usage exit reached the gate, that the loud banner fired or
#     did not as expected, that the gate reached its assertion stage at all,
#     and — when the gate did reach a verdict — that the count is right. A
#     compiler-touching branch whose lane binary predates its own changes would
#     otherwise red this test for a reason with nothing to do with the
#     substrate guard, in the exact situation where the gate under test would
#     cleanly skip.
#   * NO BINARY AT ALL — skip Block D loudly, matching what both gates do.
source "$REPO_ROOT/scripts/reify-bin-freshness.sh"

_E2E_BIN=""
_E2E_BIN_TRUSTED=0
_E2E_BIN_NOTE=""

_resolve_e2e_bin() {
    if resolve_trusted_reify_bin "$REPO_ROOT"; then
        _E2E_BIN="$REIFY_BIN_RESOLVED"
        _E2E_BIN_TRUSTED=1
        return 0
    fi
    _E2E_BIN_NOTE="$REIFY_BIN_SKIP_REASON"
    _E2E_BIN_TRUSTED=0
    # Untrusted, but present: still worth running, with narrowed assertions.
    # (resolve_trusted_reify_bin already trusts an explicit REIFY_BIN outright,
    # so reaching here means auto-discovery found a binary of unproven
    # provenance — or found nothing.)
    if [ -f "$REPO_ROOT/target/release/reify" ]; then
        _E2E_BIN="$REPO_ROOT/target/release/reify"
        return 0
    fi
    if [ -f "$REPO_ROOT/target/debug/reify" ]; then
        _E2E_BIN="$REPO_ROOT/target/debug/reify"
        return 0
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

# _run_gate <gate_script> <name> <denied|usable> — runs the REAL gate and
# captures its streams. Sets _GATE_RC / _GATE_OUT / _GATE_ERR. Always returns 0;
# the assertion functions below read the capture.
_run_gate() {
    local gate="$1" name="$2" mode="$3" stub
    _GATE_OUT="$_RUN_ROOT/$name.$mode.out"
    _GATE_ERR="$_RUN_ROOT/$name.$mode.err"
    _GATE_RC=0

    if [ "$mode" = "denied" ]; then
        stub="$(_mk_cache_denial_stub)"
        REIFY_BIN="$_E2E_BIN" TREE_SITTER_BIN="$stub" bash "$gate" \
            > "$_GATE_OUT" 2> "$_GATE_ERR" || _GATE_RC=$?
    else
        REIFY_BIN="$_E2E_BIN" bash "$gate" \
            > "$_GATE_OUT" 2> "$_GATE_ERR" || _GATE_RC=$?
    fi
    return 0
}

# _guard_owned_checks <want_probe_count> <yes|no: expect the banner>
#
# The properties this guard owns, asserted on EVERY lane whatever the binary's
# provenance:
#   (a) no `alpha exited 70` / `alpha exited 64` — the HARNESS_ERROR and usage
#       exits are the specific spurious FAILs this task kills, and they are
#       reported by the gate independently of any verdict, so this is the
#       verdict-free form of the "not 70" assertion;
#   (b) the gate script itself did not exit 70;
#   (c) the banner fired (denied) or stayed silent (usable) — a silent partial
#       run, and an over-firing guard, are both failures;
#   (d) the gate REACHED ITS ASSERTION STAGE (test_summary's "Results:" line).
#       This is what distinguishes per-row skipping from a whole-script
#       `exit 0`, which returns long before test_summary and would satisfy
#       (a)-(c) just as well;
#   (e) when the gate did publish a verdict line, its probe count is <want>.
_guard_owned_checks() {
    local want="$1" want_banner="$2"

    if grep -qE 'alpha exited (64|70)' "$_GATE_OUT"; then
        echo "the checker returned a usage/HARNESS_ERROR exit — the exact spurious FAIL this guard must prevent"
        cat "$_GATE_OUT"; cat "$_GATE_ERR"
        return 1
    fi
    if [ "$_GATE_RC" -eq 70 ]; then
        echo "gate exited 70 (HARNESS_ERROR) — the exact spurious FAIL this guard must prevent"
        cat "$_GATE_OUT"; cat "$_GATE_ERR"
        return 1
    fi
    if [ "$want_banner" = "yes" ] && ! grep -q 'GRAMMAR probes SKIPPED' "$_GATE_OUT"; then
        echo "no loud substrate banner on stdout — a silent partial run"
        cat "$_GATE_OUT"
        return 1
    fi
    if [ "$want_banner" = "no" ] && grep -q 'GRAMMAR probes SKIPPED' "$_GATE_OUT"; then
        echo "gate printed the substrate banner on a USABLE substrate — the guard is over-firing"
        cat "$_GATE_OUT"
        return 1
    fi
    if ! grep -q '^Results: ' "$_GATE_OUT"; then
        echo "gate never reached its assertion stage — it skipped whole-script instead of per-row"
        cat "$_GATE_OUT"; cat "$_GATE_ERR"
        return 1
    fi
    if grep -q '^  PASS: ' "$_GATE_OUT" && ! grep -qF "$want/$want probe(s)" "$_GATE_OUT"; then
        echo "gate published a verdict but not for $want/$want probe(s) — the wrong rows ran"
        cat "$_GATE_OUT"
        return 1
    fi
    return 0
}

# _assert_denied <want_probe_count> — the PRIMARY assertion: under a denied
# substrate the gate must degrade to a loud, partial, still-asserting run.
#
# LANE-ROBUST BY CONSTRUCTION: where the grammar was never generated the
# preflight answers 75 from the absent parser.c without ever consulting the
# stub, and the identical per-row path fires. So this asserts the same thing
# whether or not the lane has a grammar — which is why it is the PRIMARY
# assertion and the full-run case below is the guarded one.
_assert_denied() {
    local want="$1"
    _guard_owned_checks "$want" yes || return 1
    if [ "$_E2E_BIN_TRUSTED" != "1" ]; then
        return 0
    fi
    if [ "$_GATE_RC" -ne 0 ]; then
        echo "gate exited $_GATE_RC, want 0 (clean partial run)"
        cat "$_GATE_OUT"; cat "$_GATE_ERR"
        return 1
    fi
    if ! grep -qF "$want/$want probe(s)" "$_GATE_OUT"; then
        echo "gate did not report $want/$want probe(s) — check-kind rows were not run"
        cat "$_GATE_OUT"
        return 1
    fi
    return 0
}

# _assert_usable_control <want_probe_count> — NEGATIVE CONTROL: on a lane whose
# substrate really is usable, the gate must run the FULL committed probe-set and
# print NO banner — i.e. this task changed nothing about the healthy path.
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
_assert_usable_control() {
    local want="$1"
    _guard_owned_checks "$want" no || return 1
    if [ "$_E2E_BIN_TRUSTED" != "1" ]; then
        return 0
    fi
    if [ "$_GATE_RC" -ne 0 ]; then
        echo "gate exited $_GATE_RC on a usable substrate, want 0"
        cat "$_GATE_OUT"; cat "$_GATE_ERR"
        return 1
    fi
    if ! grep -qF "$want/$want probe(s)" "$_GATE_OUT"; then
        echo "gate did not report the full $want/$want probe(s) on a usable substrate"
        cat "$_GATE_OUT"
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

# _usable_control_case <gate> <name> <want> <label> <assert_desc>
#
# SELF-GUARDING, LOUDLY, AND TOLERANT OF A MID-RUN FLIP. This case genuinely
# requires a usable substrate, so on a lane without one it announces its own
# skip rather than passing vacuously — a skip-guard on a test of a skip-guard,
# made explicit.
#
# It also survives the TOCTOU that a naive version would flake on: the parent
# decides "usable" from its own preflight, and the spawned gate then re-runs the
# same 30s-bounded tree-sitter probe. This suite is classified `pool`, so it can
# run concurrently with the two gates it drives (and with its own second gate
# invocation), all contending for ~/.cache/tree-sitter/lock/<grammar>.lock; on a
# cold cache the compile plus lock wait can exceed the bound in the child but
# not in the parent. If the child banner'd, the substrate is re-probed with the
# memo cleared, and a substrate that really has gone unusable downgrades to the
# explicit skip instead of accusing the code under test of over-firing.
_usable_control_case() {
    local gate="$1" name="$2" want="$3" label="$4" desc="$5"

    PRD_GATE_SUBSTRATE_STATUS=""
    if ! resolve_grammar_substrate "$REPO_ROOT"; then
        _skip_usable_control "$label"
        return 0
    fi

    _run_gate "$gate" "$name" usable

    if grep -q 'GRAMMAR probes SKIPPED' "$_GATE_OUT"; then
        PRD_GATE_SUBSTRATE_STATUS=""
        if ! resolve_grammar_substrate "$REPO_ROOT"; then
            _skip_usable_control "$label (the substrate flipped to unusable mid-run)"
            return 0
        fi
    fi

    assert "$desc" _assert_usable_control "$want"
}

_CORPUS_GATE="$SCRIPT_DIR/test_prd_gate_corpus.sh"
_HYGIENE_GATE="$SCRIPT_DIR/test_prd_gate_compiler_type_hygiene.sh"

if ! _resolve_e2e_bin; then
    echo "  SKIP: end-to-end blocks — no reify binary (target/{release,debug}/reify"
    echo "        absent and REIFY_BIN unset), so both gates would skip anyway."
    echo "        Reason: $_E2E_BIN_NOTE"
    echo "        (the hermetic unit blocks above still ran)"
    test_summary
fi

if [ "$_E2E_BIN_TRUSTED" = "1" ]; then
    echo "-- end-to-end: reify binary freshness-verified — asserting full gate outcomes --"
else
    echo "-- end-to-end: reify binary NOT freshness-verified — asserting only this guard's own properties --"
    echo "        (no HARNESS_ERROR/usage exit, the banner, and that the gate reached its"
    echo "         assertion stage; a verdict difference owned by an unverified binary must"
    echo "         not red this test, exactly as it does not red the gates themselves)"
    echo "        Reason: $_E2E_BIN_NOTE"
fi

echo "-- end-to-end: tests/infra/test_prd_gate_corpus.sh --"

_run_gate "$_CORPUS_GATE" "corpus" denied
assert "corpus gate under a denied substrate => no HARNESS_ERROR exit, loud banner, and its 2 check probes still run" \
    _assert_denied 2

_usable_control_case "$_CORPUS_GATE" "corpus" 3 "corpus gate full-run control" \
    "corpus gate on a usable substrate => full 3-probe committed set, no banner"

echo "-- end-to-end: tests/infra/test_prd_gate_compiler_type_hygiene.sh --"

# THE TWO GATES ARE ASSERTED SEPARATELY, NOT SHARED. This one asserts all-PASS
# (every §8 boundary-table row is green on one commit); the corpus gate asserts
# all-FAIL/UNPROVABLE (a historical-false-premise corpus). Their success lines
# and probe counts differ, so a single shared e2e block would have to be
# parameterized on the very thing most likely to break — and would pass while
# checking the wrong gate's contract. The probe-set is 1 grammar + 6 check, so
# a denied substrate must still run 6.
_run_gate "$_HYGIENE_GATE" "hygiene" denied
assert "hygiene gate under a denied substrate => no HARNESS_ERROR exit, loud banner, and its 6 check probes still run" \
    _assert_denied 6

_usable_control_case "$_HYGIENE_GATE" "hygiene" 7 "hygiene gate full-run control" \
    "hygiene gate on a usable substrate => full 7-probe committed set, no banner"

test_summary
