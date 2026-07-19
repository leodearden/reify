#!/usr/bin/env bash
# Tests for scripts/verify.sh role→FIFO selection (task δ, PRD §9 contract C3, §8 T-b).
#
# Oracle assertions use verify.sh --print-plan to inspect the env block, confirming:
#   (a) merge role + merge FIFO present → exports --jobserver-auth=fifo:<merge-tmp>
#   (b) task  role + task  FIFO present → exports --jobserver-auth=fifo:<task-tmp>
#   (c) merge role + merge FIFO absent  → CARGO_MAKEFLAGS left unset (no export)
#   (d) ISOLATION: merge role + only task FIFO present → left unset
#       (proves the guard checks the role's OWN FIFO, not 'any FIFO present')
#   (e) dark-factory-orchestrator.yaml has NO active CARGO_MAKEFLAGS: key (ownership move C3)
#
# Hermetic: mktemp FIFOs at random paths; real /tmp/reify-jobserver-* NEVER touched.
# DF_VERIFY_ROLE is set INLINE per verify.sh invocation (run_all.sh exports role=task
# suite-wide; per-invocation assignment overrides the exported default).
#
# Auto-discovered by tests/infra/run_all.sh via test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY="$REPO_ROOT/scripts/verify.sh"

# ---------------------------------------------------------------------------
# Fixture: hermetic temp FIFOs (never touch live /tmp/reify-jobserver-*)
# ---------------------------------------------------------------------------
MERGE_FIFO="$(mktemp -u /tmp/test-jb-merge-XXXXXX)"
TASK_FIFO="$(mktemp -u /tmp/test-jb-task-XXXXXX)"
ABSENT_PATH="$(mktemp -u /tmp/test-jb-absent-XXXXXX)"
LIVE_PID=""   # populated by the liveness-probe cases below (task 5146); killed by _cleanup
N_BALANCER_PID=""   # populated by case (n)'s real-balancer integration test; killed by _cleanup
N_MERGE_FIFO=""
N_TASK_FIFO=""

_cleanup() {
    [ -n "$LIVE_PID" ] && kill "$LIVE_PID" 2>/dev/null || true
    [ -n "$N_BALANCER_PID" ] && kill -9 "$N_BALANCER_PID" 2>/dev/null || true
    rm -f "$MERGE_FIFO" "$TASK_FIFO" "${TASK_FIFO}.owner" 2>/dev/null || true
    [ -n "$N_MERGE_FIFO" ] && rm -f "$N_MERGE_FIFO" "${N_MERGE_FIFO}.owner" "${N_MERGE_FIFO}.owner.tmp" 2>/dev/null || true
    [ -n "$N_TASK_FIFO" ] && rm -f "$N_TASK_FIFO" "${N_TASK_FIFO}.owner" "${N_TASK_FIFO}.owner.tmp" 2>/dev/null || true
}
trap _cleanup EXIT

mkfifo "$MERGE_FIFO"
mkfifo "$TASK_FIFO"
# $ABSENT_PATH is intentionally NOT created (used to simulate absent FIFO)

export MERGE_FIFO TASK_FIFO ABSENT_PATH

# ---------------------------------------------------------------------------
# (a) merge role + merge FIFO present → exports --jobserver-auth=fifo:<merge-tmp>
# ---------------------------------------------------------------------------
echo ""
echo "--- (a) merge role + merge FIFO present → env exports merge fifo path ---"
_PLAN_A="$(DF_VERIFY_ROLE=merge REIFY_JOBSERVER_MERGE_FIFO="$MERGE_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_A

assert "(a) merge role + merge FIFO present: exports --jobserver-auth=fifo:<merge-tmp>" \
    bash -c 'printf "%s\n" "$_PLAN_A" | grep -qF "CARGO_MAKEFLAGS=--jobserver-auth=fifo:$MERGE_FIFO"'

assert "(a) merge role + merge FIFO present: does NOT mention task FIFO path in CARGO_MAKEFLAGS" \
    bash -c '! printf "%s\n" "$_PLAN_A" | grep "CARGO_MAKEFLAGS" | grep -qF "$TASK_FIFO"'

# ---------------------------------------------------------------------------
# (b) task role + task FIFO present → exports --jobserver-auth=fifo:<task-tmp>
# ---------------------------------------------------------------------------
echo ""
echo "--- (b) task role + task FIFO present → env exports task fifo path ---"
_PLAN_B="$(DF_VERIFY_ROLE=task REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_B

assert "(b) task role + task FIFO present: exports --jobserver-auth=fifo:<task-tmp>" \
    bash -c 'printf "%s\n" "$_PLAN_B" | grep -qF "CARGO_MAKEFLAGS=--jobserver-auth=fifo:$TASK_FIFO"'

assert "(b) task role + task FIFO present: does NOT mention merge FIFO path in CARGO_MAKEFLAGS" \
    bash -c '! printf "%s\n" "$_PLAN_B" | grep "CARGO_MAKEFLAGS" | grep -qF "$MERGE_FIFO"'

# ---------------------------------------------------------------------------
# (c) merge role + merge FIFO absent → CARGO_MAKEFLAGS left unset (per-role guard)
# ---------------------------------------------------------------------------
echo ""
echo "--- (c) merge role + merge FIFO absent → CARGO_MAKEFLAGS left unset ---"
_PLAN_C="$(DF_VERIFY_ROLE=merge REIFY_JOBSERVER_MERGE_FIFO="$ABSENT_PATH" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_C

assert "(c) merge role + merge FIFO absent: 'CARGO_MAKEFLAGS left unset' comment present" \
    bash -c 'printf "%s\n" "$_PLAN_C" | grep -q "CARGO_MAKEFLAGS left unset"'

assert "(c) merge role + merge FIFO absent: no active 'export CARGO_MAKEFLAGS' line" \
    bash -c '! printf "%s\n" "$_PLAN_C" | grep -q "export CARGO_MAKEFLAGS"'

# ---------------------------------------------------------------------------
# (d) ISOLATION: merge role + only task FIFO present (merge FIFO absent)
#     → CARGO_MAKEFLAGS left unset (guard checks the role's OWN FIFO, not 'any FIFO')
# ---------------------------------------------------------------------------
echo ""
echo "--- (d) isolation: merge role + only task FIFO present → unset (role guard checks own FIFO) ---"
_PLAN_D="$(DF_VERIFY_ROLE=merge \
    REIFY_JOBSERVER_MERGE_FIFO="$ABSENT_PATH" \
    REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_D

assert "(d) isolation: merge role + only task FIFO present: 'CARGO_MAKEFLAGS left unset'" \
    bash -c 'printf "%s\n" "$_PLAN_D" | grep -q "CARGO_MAKEFLAGS left unset"'

assert "(d) isolation: merge role + only task FIFO present: no active 'export CARGO_MAKEFLAGS' line" \
    bash -c '! printf "%s\n" "$_PLAN_D" | grep -q "export CARGO_MAKEFLAGS"'

# ---------------------------------------------------------------------------
# (e) dark-factory-orchestrator.yaml has NO active CARGO_MAKEFLAGS: key (ownership move C3)
#     verify.sh apply_env is now the SINGLE source of CARGO_MAKEFLAGS.
#     Matches only a real YAML key line (^\s*CARGO_MAKEFLAGS\s*:); ignores
#     comment prose that mentions CARGO_MAKEFLAGS.
# ---------------------------------------------------------------------------
echo ""
echo "--- (e) dark-factory-orchestrator.yaml: no active CARGO_MAKEFLAGS: key (ownership move C3) ---"
ORCHESTRATOR_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"
export ORCHESTRATOR_YAML

assert "(e) dark-factory-orchestrator.yaml has NO active CARGO_MAKEFLAGS: YAML key line" \
    bash -c '! grep -E "^[[:space:]]*CARGO_MAKEFLAGS[[:space:]]*:" "$ORCHESTRATOR_YAML"'

# ---------------------------------------------------------------------------
# (f) Default FIFO path contract: task role with REIFY_JOBSERVER_TASK_FIFO unset
#     → env comment references the default path /tmp/reify-jobserver-task
#     (matches the default in scripts/jobserver-balancer.py:36-41).
#
#     Hermetic because /tmp/reify-jobserver-task is almost never a FIFO on the
#     test host; if the daemon IS running, this case passes trivially (export
#     line instead of unset comment, but default-path contract still confirmed).
#     We assert default path appears somewhere in the CARGO_MAKEFLAGS output line.
# ---------------------------------------------------------------------------
echo ""
echo "--- (f) task role + REIFY_JOBSERVER_TASK_FIFO unset → default path /tmp/reify-jobserver-task ---"
# env -u: this case asserts the DEFAULT-path contract, so the var must be
# absent regardless of the ambient environment — the η acceptance campaign
# (jobserver-acceptance.py) legitimately exports REIFY_JOBSERVER_TASK_FIFO
# around its baseline verifies, and without isolation this case inherits it
# and fails the whole infra step of any baseline campaign run.
_PLAN_F="$(env -u REIFY_JOBSERVER_TASK_FIFO DF_VERIFY_ROLE=task bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_F

assert "(f) default path: CARGO_MAKEFLAGS output line references /tmp/reify-jobserver-task" \
    bash -c 'printf "%s\n" "$_PLAN_F" | grep "CARGO_MAKEFLAGS" | grep -qF "/tmp/reify-jobserver-task"'

# ---------------------------------------------------------------------------
# Liveness probe (task 5146): verify.sh must not export a stale FIFO's
# CARGO_MAKEFLAGS.  Cases (g)-(l) craft "${TASK_FIFO}.owner" stamps beside the
# already-mkfifo'd $TASK_FIFO and reuse the (a)-(f) mktemp-FIFO +
# REIFY_JOBSERVER_TASK_FIFO override + --print-plan grep idiom.
#
# DEAD_PID is a pid essentially guaranteed not to be alive (near INT_MAX, far
# past any realistic pid_max); WRONG_BOOT_ID is a well-formed but wrong UUID;
# LIVE_PID is a real backgrounded process this file owns and kills on EXIT.
#
# Cases (g), (h) are RED today: apply_env's existence-only guard never skips
# the export, so a stale/boot-mismatched stamp still exports CARGO_MAKEFLAGS.
# ---------------------------------------------------------------------------
DEAD_PID=2147483646
WRONG_BOOT_ID="00000000-0000-0000-0000-000000000000"
HOST_BOOT_ID="$(cat /proc/sys/kernel/random/boot_id 2>/dev/null || true)"

sleep 300 &
LIVE_PID=$!

# ---------------------------------------------------------------------------
# (g) STALE: dead pid + host boot_id → CARGO_MAKEFLAGS left unset (stale) +
#     a 'verify.sh: WARNING' line on stderr
# ---------------------------------------------------------------------------
echo ""
echo "--- (g) STALE: dead pid + host boot_id → CARGO_MAKEFLAGS left unset + WARNING on stderr ---"
printf '%s %s\n' "$DEAD_PID" "$HOST_BOOT_ID" > "${TASK_FIFO}.owner"

_G_STDERR="$(mktemp)"
_PLAN_G="$(DF_VERIFY_ROLE=task REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>"$_G_STDERR" || true)"
export _PLAN_G

assert "(g) STALE dead-pid: no active 'export CARGO_MAKEFLAGS' line" \
    bash -c '! printf "%s\n" "$_PLAN_G" | grep -q "export CARGO_MAKEFLAGS"'

assert "(g) STALE dead-pid: 'CARGO_MAKEFLAGS left unset' + 'stale' comment present" \
    bash -c 'printf "%s\n" "$_PLAN_G" | grep -i "CARGO_MAKEFLAGS left unset" | grep -qi "stale"'

assert "(g) STALE dead-pid: 'verify.sh: WARNING' appears on stderr" \
    bash -c "grep -qF 'verify.sh: WARNING' '$_G_STDERR'"

rm -f "$_G_STDERR" "${TASK_FIFO}.owner"

# ---------------------------------------------------------------------------
# (h) BOOT-MISMATCH: live pid + wrong boot_id → CARGO_MAKEFLAGS left unset
#     (post-reboot pid-reuse guard fires even though the pid itself is alive)
#
#     Guarded on HOST_BOOT_ID being non-empty (task 5146 review comment 3):
#     _jobserver_owner_live()'s STALE-via-boot-mismatch branch only fires
#     when `[ -n "$cur" ]` (verify.sh's own live read of the host boot_id)
#     is true; on an exotic host/container where
#     /proc/sys/kernel/random/boot_id is unreadable, that check is false,
#     the comparison is skipped, and this case would flip from STALE to
#     LIVE — matching the daemon's own fail-open contract ("-"/absent boot
#     info is never treated as proof of staleness), but making the
#     assertion below untestable on such a host. Skip rather than assert a
#     host property this file cannot control.
#
#     NOTE: sibling case (g) needs no such guard — its expected STALE
#     verdict comes from the dead-pid check (`[ ! -d "/proc/$pid" ]`), which
#     is evaluated BEFORE the boot_id comparison and short-circuits it, so
#     (g) holds regardless of HOST_BOOT_ID's value.
# ---------------------------------------------------------------------------
echo ""
echo "--- (h) BOOT-MISMATCH: live pid + wrong boot_id → CARGO_MAKEFLAGS left unset ---"
if [ -z "$HOST_BOOT_ID" ]; then
    echo "  SKIP: (h) BOOT-MISMATCH — host boot_id unreadable here; boot-mismatch branch untestable"
else
printf '%s %s\n' "$LIVE_PID" "$WRONG_BOOT_ID" > "${TASK_FIFO}.owner"

_PLAN_H="$(DF_VERIFY_ROLE=task REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_H

assert "(h) BOOT-MISMATCH: no active 'export CARGO_MAKEFLAGS' line" \
    bash -c '! printf "%s\n" "$_PLAN_H" | grep -q "export CARGO_MAKEFLAGS"'

assert "(h) BOOT-MISMATCH: 'CARGO_MAKEFLAGS left unset' comment present" \
    bash -c 'printf "%s\n" "$_PLAN_H" | grep -q "CARGO_MAKEFLAGS left unset"'

rm -f "${TASK_FIFO}.owner"
fi

# ---------------------------------------------------------------------------
# (i) LIVE: live pid + host boot_id → exports --jobserver-auth=fifo:<task-tmp>
# ---------------------------------------------------------------------------
echo ""
echo "--- (i) LIVE: live pid + host boot_id → exports --jobserver-auth=fifo:<task-tmp> ---"
printf '%s %s\n' "$LIVE_PID" "$HOST_BOOT_ID" > "${TASK_FIFO}.owner"

_PLAN_I="$(DF_VERIFY_ROLE=task REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_I

assert "(i) LIVE: exports --jobserver-auth=fifo:<task-tmp>" \
    bash -c 'printf "%s\n" "$_PLAN_I" | grep -qF "CARGO_MAKEFLAGS=--jobserver-auth=fifo:$TASK_FIFO"'

rm -f "${TASK_FIFO}.owner"

# ---------------------------------------------------------------------------
# (j) NO-STAMP backward-compat: FIFO present, no .owner → exports
#     (UNKNOWN — an old/foreign balancer without a stamp must keep working)
# ---------------------------------------------------------------------------
echo ""
echo "--- (j) NO-STAMP: FIFO present, no .owner → exports (backward-compat, UNKNOWN) ---"
rm -f "${TASK_FIFO}.owner"   # ensure absent

_PLAN_J="$(DF_VERIFY_ROLE=task REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_J

assert "(j) NO-STAMP: exports --jobserver-auth=fifo:<task-tmp> (backward-compat)" \
    bash -c 'printf "%s\n" "$_PLAN_J" | grep -qF "CARGO_MAKEFLAGS=--jobserver-auth=fifo:$TASK_FIFO"'

# ---------------------------------------------------------------------------
# (k) MALFORMED: owner stamp with an empty/space-only first field → exports
#     (UNKNOWN, not STALE — ambiguous is not proof of death)
# ---------------------------------------------------------------------------
echo ""
echo "--- (k) MALFORMED: owner stamp with empty first field → exports (UNKNOWN, not stale) ---"
printf '   \n' > "${TASK_FIFO}.owner"   # whitespace-only line: read -r pid boot → both empty

_PLAN_K="$(DF_VERIFY_ROLE=task REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_K

assert "(k) MALFORMED: exports --jobserver-auth=fifo:<task-tmp> (UNKNOWN, not stale)" \
    bash -c 'printf "%s\n" "$_PLAN_K" | grep -qF "CARGO_MAKEFLAGS=--jobserver-auth=fifo:$TASK_FIFO"'

rm -f "${TASK_FIFO}.owner"

# ---------------------------------------------------------------------------
# (l) BREAK-GLASS: dead-pid stamp + REIFY_JOBSERVER_SKIP_LIVENESS_PROBE=1 →
#     exports anyway (existence-only guard, pre-5146 behavior forced back on)
# ---------------------------------------------------------------------------
echo ""
echo "--- (l) BREAK-GLASS: dead-pid stamp + REIFY_JOBSERVER_SKIP_LIVENESS_PROBE=1 → exports ---"
printf '%s %s\n' "$DEAD_PID" "$HOST_BOOT_ID" > "${TASK_FIFO}.owner"

_PLAN_L="$(DF_VERIFY_ROLE=task REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    REIFY_JOBSERVER_SKIP_LIVENESS_PROBE=1 \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_L

assert "(l) BREAK-GLASS: exports --jobserver-auth=fifo:<task-tmp> despite stale stamp" \
    bash -c 'printf "%s\n" "$_PLAN_L" | grep -qF "CARGO_MAKEFLAGS=--jobserver-auth=fifo:$TASK_FIFO"'

rm -f "${TASK_FIFO}.owner"

# ---------------------------------------------------------------------------
# (m) BOOT-INFO-UNAVAILABLE: live pid + "-" sentinel boot field → exports
#     (task 5146 review comment 1). read_boot_id() returns the "-" sentinel
#     when /proc/sys/kernel/random/boot_id is unreadable on the balancer's
#     side; _jobserver_owner_live()'s boot comparison must be skipped
#     whenever the stamp's boot field is "-" (its `[ "$boot" != "-" ]` guard),
#     falling back to the pid-alive check alone. This is the ONLY existing
#     case exercising that fail-open branch end-to-end — every real test
#     host has a readable boot_id, so cases (g)-(l)/(i) never hit it via a
#     genuinely-unreadable host file; here we force it via the stamp content
#     itself, independent of host readability.
# ---------------------------------------------------------------------------
echo ""
echo "--- (m) BOOT-INFO-UNAVAILABLE: live pid + '-' boot field → exports (pid-only LIVE) ---"
printf '%s -\n' "$LIVE_PID" > "${TASK_FIFO}.owner"

_PLAN_M="$(DF_VERIFY_ROLE=task REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_M

assert "(m) BOOT-INFO-UNAVAILABLE: exports --jobserver-auth=fifo:<task-tmp> (pid-only LIVE)" \
    bash -c 'printf "%s\n" "$_PLAN_M" | grep -qF "CARGO_MAKEFLAGS=--jobserver-auth=fifo:$TASK_FIFO"'

rm -f "${TASK_FIFO}.owner"
kill "$LIVE_PID" 2>/dev/null || true
wait "$LIVE_PID" 2>/dev/null || true
LIVE_PID=""

# ---------------------------------------------------------------------------
# (n) INTEGRATION: real balancer SIGKILLed → verify.sh probes the genuine
#     leftover stamp and falls back to plain cargo (task 5146 review comment
#     2, round 3).
#
#     Cases (g)-(m) above validate _jobserver_owner_live()'s STALE/LIVE/
#     UNKNOWN logic against SYNTHETIC hand-written ".owner" stamps; none of
#     them prove the daemon's ACTUAL on-disk stamp format (field order,
#     trailing newline, whitespace) round-trips through this file's
#     `read -r pid boot` the way the synthetic cases assume — a format drift
#     on either side (this probe, or jobserver-balancer.py's
#     write_owner_stamp()) would only be caught by an end-to-end run like
#     this one. Start the REAL balancer daemon (same one
#     tests/infra/test_jobserver_balancer.sh drives), SIGKILL it once its
#     stamp is on disk (the crash path Block 19b there proves leaves the
#     stamp behind), and run the REAL verify.sh probe against that genuine
#     leftover file end-to-end.
# ---------------------------------------------------------------------------
echo ""
echo "--- (n) INTEGRATION: real balancer SIGKILLed → verify.sh falls back to plain cargo ---"

BALANCER_PY="$REPO_ROOT/scripts/jobserver-balancer.py"
N_MERGE_FIFO="$(mktemp -u /tmp/test-jb-n-merge-XXXXXX)"
N_TASK_FIFO="$(mktemp -u /tmp/test-jb-n-task-XXXXXX)"

REIFY_JOBSERVER_MERGE_FIFO="$N_MERGE_FIFO" \
REIFY_JOBSERVER_TASK_FIFO="$N_TASK_FIFO" \
REIFY_JOBSERVER_TOKENS=4 \
REIFY_JOBSERVER_POLL_INTERVAL=0.05 \
    python3 "$BALANCER_PY" &
N_BALANCER_PID=$!

# Wait (bounded ~5s) for the real daemon to publish its task-FIFO owner stamp.
_n_stamp_present=0
_n_t0=$(date +%s)
while true; do
    if [ -f "${N_TASK_FIFO}.owner" ]; then
        _n_stamp_present=1; break
    fi
    [ $(( $(date +%s) - _n_t0 )) -ge 5 ] && break
    sleep 0.05
done
assert "(n) INTEGRATION: real balancer published its task-FIFO owner stamp (precondition)" \
    test "$_n_stamp_present" -eq 1

# SIGKILL — same crash path Block 19b (test_jobserver_balancer.sh) proves
# leaves the FIFO + stamp behind: no cleanup code runs at all.
kill -9 "$N_BALANCER_PID" 2>/dev/null || true
wait "$N_BALANCER_PID" 2>/dev/null || true
N_BALANCER_PID=""
sleep 0.2   # let the kernel settle the reap before probing

_N_STDERR="$(mktemp)"
_PLAN_N="$(DF_VERIFY_ROLE=task REIFY_JOBSERVER_TASK_FIFO="$N_TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>"$_N_STDERR" || true)"
export _PLAN_N

assert "(n) INTEGRATION: no active 'export CARGO_MAKEFLAGS' line (genuine dead-pid stamp)" \
    bash -c '! printf "%s\n" "$_PLAN_N" | grep -q "export CARGO_MAKEFLAGS"'

assert "(n) INTEGRATION: 'CARGO_MAKEFLAGS left unset' + 'stale' comment present" \
    bash -c 'printf "%s\n" "$_PLAN_N" | grep -i "CARGO_MAKEFLAGS left unset" | grep -qi "stale"'

assert "(n) INTEGRATION: 'verify.sh: WARNING' appears on stderr" \
    bash -c "grep -qF 'verify.sh: WARNING' '$_N_STDERR'"

rm -f "$_N_STDERR" "$N_MERGE_FIFO" "$N_TASK_FIFO" "${N_MERGE_FIFO}.owner" "${N_TASK_FIFO}.owner"
N_MERGE_FIFO=""
N_TASK_FIFO=""

test_summary
