#!/usr/bin/env bash
# tests/infra/test_verify_background_sweep_ledger.sh — boundary test for task
# 7423's GAP 2: making ONE main-tip sweep completion observable.
#
# WHY THIS EXISTS. dark-factory's main-tip integrity sweep runs reify's
# verify.sh with DF_VERIFY_ROLE=background, inside an EPHEMERAL
# `_mainsweep-<hex>` worktree that its own `finally` removes. Read live on
# 2026-09-18: harness.py::_run_main_tip_sweep emits no log line on any
# non-drift path — the empty-sha, SHA-dedup, no-outcome and PASS exits are all
# bare returns — so a PASSING sweep is structurally silent. Combined with the
# worktree cleanup, a clean completion leaves NO trace at all and a LEFTOVER
# worktree means an UNCLEAN exit: neither outcome was distinguishable from
# "never ran". reify's own merge gate defers release-sensitive re-execution to
# that sweep ("the sweep IS the backstop"), so an unobservable sweep is a reify
# correctness exposure, not an orchestrator ergonomics gap.
#
# WHAT IS UNDER TEST. verify.sh appends ONE structured verdict record per
# COMPLETED background-role run to a durable ledger resolved OUTSIDE the
# running worktree, so the record outlives the worktree whose completion it
# records. Nothing else observes it: the ledger never gates, never retries and
# never alters an exit code.
#
# HOW IT DRIVES A REAL RUN CHEAPLY. Every assertion EXECUTES verify.sh to
# completion under the sweep's own role — a plan-shape oracle cannot observe an
# EXIT-trap write at all. The vehicle is `typecheck`, whose background-role plan
# is five cheap repo checks plus one `cargo check` and, unlike `test`, holds no
# test-run semaphore slot (an infra test that took one would deadlock against
# the outer verify run that already holds it). A stub `cargo` makes the single
# cargo leaf instant and lets the test choose the run's verdict. Measured: ~4s
# per run.
#
# CLASSIFIED `pool`: the only lane-shared state any run here touches is
# tree-sitter-generate.sh, which is idempotent and flock-serialised, and the
# ledger itself is a run-private scratch path in every assertion below.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY_SH="$REPO_ROOT/scripts/verify.sh"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/reify-sweep-ledger-test.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

LEDGER="$WORK/sweep-ledger.jsonl"

HEAD_SHA="$(git -C "$REPO_ROOT" rev-parse HEAD)"
TREE_OID="$(git -C "$REPO_ROOT" rev-parse HEAD:)"

echo "=== verify.sh background-role sweep ledger (task 7423, GAP 2) ==="

# ---------------------------------------------------------------------------
# Stub `cargo`: instant, and its exit code is the run's verdict. Every other
# invocation succeeds silently so availability probes still answer.
# ---------------------------------------------------------------------------
STUB_BIN="$WORK/stub-bin"
mkdir -p "$STUB_BIN"
cat > "$STUB_BIN/cargo" <<'STUB_EOF'
#!/usr/bin/env bash
set -u
case "$*" in
    *"check"*) exit "${REIFY_TEST_STUB_CARGO_RC:-0}" ;;
esac
exit 0
STUB_EOF
chmod +x "$STUB_BIN/cargo"

# run_verify <role> <stub-rc> <ledger-path> [extra verify args...] -> RUN_RC.
# `timeout` guards every run: this suite's whole premise is that the run
# COMPLETES, so a stall is a failure to report, not a suite to hang.
run_verify() {
    local _role="$1" _stub_rc="$2" _ledger="$3"; shift 3
    RUN_RC=0
    (
        cd "$REPO_ROOT"
        PATH="$STUB_BIN:$PATH" \
        REIFY_TEST_STUB_CARGO_RC="$_stub_rc" \
        DF_VERIFY_ROLE="$_role" \
        REIFY_BACKGROUND_SWEEP_LEDGER="$_ledger" \
        timeout 600 bash "$VERIFY_SH" typecheck "$@"
    ) >/dev/null 2>&1 || RUN_RC=$?
}

# ledger_field <ledger> <line-no> <key> — the value of <key> in that record,
# read with a real JSON parser so a prose log line cannot pass by accident.
ledger_field() {
    python3 - "$1" "$2" "$3" <<'PY'
import json, sys
path, lineno, key = sys.argv[1], int(sys.argv[2]), sys.argv[3]
with open(path) as fh:
    lines = [ln for ln in fh.read().splitlines() if ln.strip()]
sys.stdout.write(str(json.loads(lines[lineno - 1]).get(key, '')))
PY
}
# Exported: every assertion runs its predicate in a `bash -c` child.
export -f ledger_field

# ===========================================================================
# Block A — a COMPLETED background run is recorded, once, as parseable data.
# ===========================================================================
echo ""
echo "--- Block A: a completed background run appends one structured record ---"

: > "$LEDGER"
run_verify background 0 "$LEDGER"

assert "A1: a completed background run appends EXACTLY ONE line" \
    bash -c '[ "$(grep -c . "$1")" -eq 1 ]' \
    _ "$LEDGER"

assert "A2: that line is a single-line JSON OBJECT, not a prose log line (parsed with a real JSON parser)" \
    bash -c 'python3 -c "
import json,sys
lines=[l for l in open(sys.argv[1]).read().splitlines() if l.strip()]
rec=json.loads(lines[0])
sys.exit(0 if isinstance(rec, dict) else 1)" "$1"' \
    _ "$LEDGER"

assert "A3: the record names the ROLE it was produced under" \
    bash -c '[ "$(ledger_field "$1" 1 role)" = "background" ]' \
    _ "$LEDGER"

assert "A4: the record carries the VERDICT (pass)" \
    bash -c '[ "$(ledger_field "$1" 1 verdict)" = "pass" ]' \
    _ "$LEDGER"

assert "A5: the record carries the process EXIT CODE (0)" \
    bash -c '[ "$(ledger_field "$1" 1 exit_code)" = "0" ]' \
    _ "$LEDGER"

assert "A6: the record carries the HEAD sha the sweep was about — without it a verdict names no commit" \
    bash -c '[ "$(ledger_field "$1" 1 head)" = "$2" ]' \
    _ "$LEDGER" "$HEAD_SHA"

assert "A7: the record carries the TREE OID, in the same field spelling as the confirm sidecar" \
    bash -c '[ "$(ledger_field "$1" 1 tree_oid)" = "$2" ]' \
    _ "$LEDGER" "$TREE_OID"

assert "A8: the record carries a UTC timestamp in the sidecar's %Y-%m-%dT%H:%M:%SZ shape" \
    bash -c 'printf "%s" "$(ledger_field "$1" 1 timestamp)" | grep -qE "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"' \
    _ "$LEDGER"

assert "A9: the recorded run still exits 0 — observing a verdict never changes it" \
    test "$RUN_RC" -eq 0

# ===========================================================================
# Block B — the FAILING half. Today's upstream gap is precisely that only one
# of pass/fail is observable; a ledger that recorded successes alone would
# reproduce it.
# ===========================================================================
echo ""
echo "--- Block B: a failing background run is recorded too ---"

: > "$LEDGER"
run_verify background 17 "$LEDGER"

assert "B1: a FAILING background run appends exactly one record" \
    bash -c '[ "$(grep -c . "$1")" -eq 1 ]' \
    _ "$LEDGER"

assert "B2: its verdict says fail" \
    bash -c '[ "$(ledger_field "$1" 1 verdict)" = "fail" ]' \
    _ "$LEDGER"

assert "B3: its exit_code is the run's OWN observed exit code" \
    bash -c '[ "$(ledger_field "$1" 1 exit_code)" = "$2" ]' \
    _ "$LEDGER" "$RUN_RC"

# The stub's 17 does not survive the executor (reaper_run_in_pgroup collapses a
# plan command's code to 1 — pre-existing, and not this task's to change), so
# B4 asserts the run FAILED rather than a specific number. B3 above is the
# assertion that matters: whatever the process exited with, that is what the
# record says.
assert "B4: and the run really did fail, so B3 is not comparing two zeros" \
    test "$RUN_RC" -ne 0

FAIL_RC="$RUN_RC"

# ===========================================================================
# Block C — every other lane stays out of the ledger.
# ===========================================================================
echo ""
echo "--- Block C: no other role, and no dry run, pollutes the ledger ---"

for _role in task merge offline; do
    : > "$LEDGER"
    run_verify "$_role" 0 "$LEDGER"
    assert "C1: a completed role=$_role run appends NOTHING (only the sweep's own role is observed here)" \
        bash -c '[ ! -s "$1" ]' \
        _ "$LEDGER"
done

: > "$LEDGER"
run_verify background 0 "$LEDGER" --print-plan
assert "C2: --print-plan appends nothing — it executes no checks, so it reaches no verdict to record" \
    bash -c '[ ! -s "$1" ]' \
    _ "$LEDGER"

: > "$LEDGER"
RUN_RC=0
(
    cd "$REPO_ROOT"
    PATH="$STUB_BIN:$PATH" \
    DF_VERIFY_ROLE=background \
    REIFY_BACKGROUND_SWEEP_LEDGER="$LEDGER" \
    timeout 600 bash "$VERIFY_SH" typecheck --bogus-zzz
) >/dev/null 2>&1 || RUN_RC=$?

assert "C3: an argument-validation exit-64 appends nothing — a rejected invocation is not a verdict about the tree" \
    bash -c '[ ! -s "$1" ]' \
    _ "$LEDGER"

assert "C4: and that invocation really did exit 64" \
    test "$RUN_RC" -eq 64

# ===========================================================================
# Block D — the ledger APPENDS. A sweep history of one is not a history.
# ===========================================================================
echo ""
echo "--- Block D: consecutive sweeps accumulate ---"

: > "$LEDGER"
run_verify background 0 "$LEDGER"
FIRST_RECORD="$(cat "$LEDGER")"
run_verify background 17 "$LEDGER"

assert "D1: two consecutive background runs leave TWO records — the ledger appends, never truncates" \
    bash -c '[ "$(grep -c . "$1")" -eq 2 ]' \
    _ "$LEDGER"

assert "D2: the FIRST record survives the second run byte-unchanged" \
    bash -c '[ "$(head -n1 "$1")" = "$2" ]' \
    _ "$LEDGER" "$FIRST_RECORD"

assert "D3: the two records carry their own distinct verdicts (pass then fail), so the history is readable" \
    bash -c '[ "$(ledger_field "$1" 1 verdict)" = "pass" ] && [ "$(ledger_field "$1" 2 verdict)" = "fail" ]' \
    _ "$LEDGER"

# ===========================================================================
# Block E — fail open. An observation channel must never gate the integrity
# gate it observes.
# ===========================================================================
echo ""
echo "--- Block E: an unwritable ledger never changes the run ---"

UNWRITABLE_DIR="$WORK/readonly"
mkdir -p "$UNWRITABLE_DIR"
chmod 500 "$UNWRITABLE_DIR"

run_verify background 0 "$UNWRITABLE_DIR/ledger.jsonl"
assert "E1: an UNWRITABLE ledger path leaves a passing run passing (exit 0)" \
    test "$RUN_RC" -eq 0

run_verify background 17 "$UNWRITABLE_DIR/ledger.jsonl"
assert "E2: an unwritable ledger path leaves a failing run's OWN exit code intact, neither masked nor replaced" \
    bash -c '[ "$1" = "$2" ]' \
    _ "$RUN_RC" "$FAIL_RC"

chmod 700 "$UNWRITABLE_DIR"

# ===========================================================================
# Block F — the record must outlive the worktree that produced it. This is the
# single constraint the whole design turns on: the sweep's worktree is deleted
# in a `finally`, so a ledger resolved relative to the RUNNING worktree is
# destroyed by the very completion it was meant to prove.
# ===========================================================================
echo ""
echo "--- Block F: the default ledger path is resolved outside the running worktree ---"

FAKE_MAIN="$WORK/fake-main-checkout"
mkdir -p "$FAKE_MAIN"

RUN_RC=0
(
    cd "$REPO_ROOT"
    PATH="$STUB_BIN:$PATH" \
    REIFY_TEST_STUB_CARGO_RC=0 \
    DF_VERIFY_ROLE=background \
    REIFY_MAIN_CHECKOUT="$FAKE_MAIN" \
    timeout 600 bash "$VERIFY_SH" typecheck
) >/dev/null 2>&1 || RUN_RC=$?

assert "F1: with no explicit ledger path, the record lands under the MAIN CHECKOUT's data/orchestrator/ — the sweep's ephemeral worktree cannot take it with it" \
    bash -c '[ -n "$(find "$1/data/orchestrator" -type f -name "*.jsonl" 2>/dev/null | head -n1)" ]' \
    _ "$FAKE_MAIN"

assert "F2: that default record is the same structured shape as an explicitly-pointed one" \
    bash -c 'python3 -c "
import glob,json,sys
paths=glob.glob(sys.argv[1] + \"/data/orchestrator/*.jsonl\")
lines=[l for p in paths for l in open(p).read().splitlines() if l.strip()]
rec=json.loads(lines[0])
sys.exit(0 if rec.get(\"role\") == \"background\" and \"verdict\" in rec else 1)" "$1"' \
    _ "$FAKE_MAIN"

assert "F3: the default path is NOT relative to the running worktree — no ledger appeared under it" \
    bash -c '[ -z "$(find "$1/data/orchestrator" -type f -name "*sweep*" 2>/dev/null | head -n1)" ]' \
    _ "$REPO_ROOT"

assert "F4: resolving the default path did not change the run's exit code" \
    test "$RUN_RC" -eq 0

test_summary
