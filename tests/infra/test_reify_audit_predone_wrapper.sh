#!/usr/bin/env bash
# tests/infra/test_reify_audit_predone_wrapper.sh
#
# Regression guard: asserts that the reify-audit-predone-wrapper.sh script
# exists, is executable, handles --help, and errors appropriately on missing
# required flags — without requiring a live fused-memory MCP server.
#
# Background: the wrapper runs the reify-audit freshness guard, then hands the
# task to reify-audit's own live fused-memory loader (task 9269: no snapshot,
# no curl, no jq). This test validates the wrapper's invocation surface and
# that handoff without requiring a live fused-memory MCP server.
#
# See: docs/architecture-audit/f-infra-design.md §11.1
#      task 3731 (root-cause: dead .taskmaster/tasks/tasks.json default)
#      task 7139 (Check 7b inverted: a stale-but-PRESENT binary now FAILS OPEN
#                 rather than blocking every done-flip project-wide; Check 7d
#                 pins that an ABSENT binary still refuses; Check 9 pins that
#                 the rc-0 advisory reaches a channel fused-memory does not
#                 drop, so fail-open is not fail-SILENT)
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

WRAPPER="$REPO_ROOT/scripts/reify-audit-predone-wrapper.sh"

echo "=== reify-audit-predone-wrapper.sh regression guard ==="

# ==============================================================================
# Check 1: wrapper exists
# ==============================================================================
echo ""
echo "--- Check 1: wrapper script exists ---"

assert "scripts/reify-audit-predone-wrapper.sh exists" \
    bash -c '[ -f "$1" ]' -- "$WRAPPER"

# ==============================================================================
# Check 2: wrapper is executable
# ==============================================================================
echo ""
echo "--- Check 2: wrapper script is executable ---"

assert "scripts/reify-audit-predone-wrapper.sh is executable" \
    bash -c '[ -x "$1" ]' -- "$WRAPPER"

# ==============================================================================
# Check 3: --help exits 0 and prints recognizable usage
# ==============================================================================
echo ""
echo "--- Check 3: wrapper --help exits 0 and mentions key flags ---"

assert "wrapper --help exits 0" \
    bash -c 'bash "$1" --help >/dev/null 2>&1' -- "$WRAPPER"

assert "wrapper --help stdout is non-empty" \
    bash -c '[ -n "$(bash "$1" --help 2>/dev/null)" ]' -- "$WRAPPER"

assert "wrapper --help mentions --task" \
    bash -c 'bash "$1" --help 2>/dev/null | grep -q -- "--task"' -- "$WRAPPER"

assert "wrapper --help mentions --pre-done" \
    bash -c 'bash "$1" --help 2>/dev/null | grep -q -- "--pre-done"' -- "$WRAPPER"

# ==============================================================================
# Check 4: missing --task exits non-zero with usage hint on stderr
# ==============================================================================
echo ""
echo "--- Check 4: wrapper without --task exits non-zero with usage hint ---"

assert "wrapper without --task exits non-zero" \
    bash -c '! bash "$1" 2>/dev/null' -- "$WRAPPER"

assert "wrapper without --task emits usage hint to stderr" \
    bash -c 'bash "$1" 2>&1 >/dev/null | grep -qiE "Usage:|requires --task"' -- "$WRAPPER"

# Edge case: `--task --pre-done` (no positional id supplied, next flag consumed
# as value). The validator rejects flag-shaped task ids (leading `--`) and exits
# 125 with a clear message. Previously the loop would silently set
# task_id="--pre-done" and proceed to the MCP step, failing ambiguously.
assert "wrapper with flag-shaped --task value (--task --pre-done) exits non-zero" \
    bash -c '! bash "$1" --task --pre-done 2>/dev/null' -- "$WRAPPER"

assert "wrapper with flag-shaped --task value emits usage hint to stderr" \
    bash -c 'bash "$1" --task --pre-done 2>&1 >/dev/null | grep -qiE "looks like a flag|requires --task"' -- "$WRAPPER"

# ==============================================================================
# Check 5: live-loader handoff (task 9269)
# ==============================================================================
# The wrapper fetches no task corpus itself: it hands the task to reify-audit,
# whose own fused-memory loader fetches the one flipping task with get_task. So
# the child's argv names the MCP endpoint (--fused-memory-url) and never a
# --tasks-file, and neither curl nor jq ever runs.
#
# Tripwire curl/jq shims on PATH record any call and exit 97, so a wrapper that
# fetched or filtered a snapshot again fails 5f/5g (and, in practice, 5a-5e and
# 5h too, since the snapshot arm exits 125 before the child runs). The
# reify-audit stub records its argv, one token per line, and exits 0, so the
# wrapper's rc is the child's. It is freshly created, so the freshness guard
# stays silent (that is Check 7/9's territory), and the advisory sentinel is
# redirected into this check's tmpdir so it can never stamp the host-wide path.
echo ""
echo "--- Check 5: wrapper hands the task to reify-audit's live loader ---"

LIVE_TMPDIR=$(mktemp -d /tmp/test-wrapper-live-XXXXXX)
trap 'rm -rf "$LIVE_TMPDIR"' EXIT

for _tripwire in curl jq; do
    cat > "$LIVE_TMPDIR/$_tripwire" <<'TRIPWIRE_EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$0.called"
exit 97
TRIPWIRE_EOF
    chmod +x "$LIVE_TMPDIR/$_tripwire"
done

cat > "$LIVE_TMPDIR/reify-audit" <<'ARGV_STUB_EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$ARGV_OUT"
exit 0
ARGV_STUB_EOF
chmod +x "$LIVE_TMPDIR/reify-audit"

LIVE_ARGV="$LIVE_TMPDIR/argv"
LIVE_ARGV_DEFAULT="$LIVE_TMPDIR/argv-default-url"
LIVE_URL="http://127.0.0.1:9/mcp-from-env"

# _argv_index <argv-file> <token> — print the 0-based index of the first argv
# token equal to <token>; fail if the file or the token is absent.
_argv_index() {
    [ -f "$1" ] || return 1
    awk -v t="$2" '$0 == t { print NR - 1; found = 1; exit } END { exit !found }' "$1"
}

# _argv_pair <argv-file> <flag> <value> — succeed iff some <flag> token is
# immediately followed by <value>.
_argv_pair() {
    [ -f "$1" ] || return 1
    awk -v f="$2" -v v="$3" 'prev == f && $0 == v { found = 1 } { prev = $0 } END { exit !found }' "$1"
}

# _argv_follows_wrapper_defaults <argv-file> <token> — succeed iff <token>
# comes after every flag the wrapper prepends, which is what lets a caller's
# own copy of one of those flags win under reify-audit's last-wins parsing.
_argv_follows_wrapper_defaults() {
    local _token_at _default _default_at
    _token_at=$(_argv_index "$1" "$2") || return 1
    for _default in --fused-memory-url --runs-db --project-root; do
        _default_at=$(_argv_index "$1" "$_default") || return 1
        [ "$_default_at" -lt "$_token_at" ] || return 1
    done
}

set +e
PATH="$LIVE_TMPDIR:$PATH" \
    ARGV_OUT="$LIVE_ARGV" \
    FUSED_MEMORY_MCP_URL="$LIVE_URL" \
    REIFY_AUDIT_BIN="$LIVE_TMPDIR/reify-audit" \
    REIFY_AUDIT_ADVISORY_SENTINEL="$LIVE_TMPDIR/sentinel" \
    bash "$WRAPPER" --task 4242 --pre-done >/dev/null 2>&1
live_rc=$?
set -e

assert "5a: wrapper exits with the child's rc 0 (no 125 snapshot arm)" \
    bash -c 'test "$1" -eq 0' -- "$live_rc"

assert "5b: child argv carries --fused-memory-url $LIVE_URL (FUSED_MEMORY_MCP_URL)" \
    _argv_pair "$LIVE_ARGV" --fused-memory-url "$LIVE_URL"

assert "5c: child argv carries no --tasks-file" \
    bash -c '[ -f "$1" ] && ! grep -qxF -e --tasks-file "$1"' -- "$LIVE_ARGV"

assert "5d: child argv carries --runs-db" \
    _argv_index "$LIVE_ARGV" --runs-db

assert "5d: child argv carries --project-root $REPO_ROOT" \
    _argv_pair "$LIVE_ARGV" --project-root "$REPO_ROOT"

assert "5e: caller's --task 4242 reaches the child" \
    _argv_pair "$LIVE_ARGV" --task 4242

assert "5e: caller's --task reaches the child after the wrapper defaults" \
    _argv_follows_wrapper_defaults "$LIVE_ARGV" --task

assert "5e: caller's --pre-done reaches the child after the wrapper defaults" \
    _argv_follows_wrapper_defaults "$LIVE_ARGV" --pre-done

assert "5f: curl was never invoked" \
    bash -c '[ -f "$1" ] && [ ! -e "$2" ]' -- "$LIVE_ARGV" "$LIVE_TMPDIR/curl.called"

assert "5g: jq was never invoked" \
    bash -c '[ -f "$1" ] && [ ! -e "$2" ]' -- "$LIVE_ARGV" "$LIVE_TMPDIR/jq.called"

# 5h: the production systemd unit sets no FUSED_MEMORY_MCP_URL, so the default
# is what the live hook uses. No trailing slash: /mcp/ 307-redirects and drops
# the mcp-session-id header.
set +e
env -u FUSED_MEMORY_MCP_URL \
    PATH="$LIVE_TMPDIR:$PATH" \
    ARGV_OUT="$LIVE_ARGV_DEFAULT" \
    REIFY_AUDIT_BIN="$LIVE_TMPDIR/reify-audit" \
    REIFY_AUDIT_ADVISORY_SENTINEL="$LIVE_TMPDIR/sentinel" \
    bash "$WRAPPER" --task 4242 --pre-done >/dev/null 2>&1
set -e

assert "5h: with FUSED_MEMORY_MCP_URL unset the child gets --fused-memory-url http://localhost:8002/mcp" \
    _argv_pair "$LIVE_ARGV_DEFAULT" --fused-memory-url http://localhost:8002/mcp

# ==============================================================================
# Check 6: exit-code propagation under `set -e`
# ==============================================================================
echo ""
echo "--- Check 6: exit-code propagation through wrapper under set -e ---"

# 6a: Static check — the wrapper must use an idiomatic form that keeps
# `rc=$?` reachable on the non-zero path (i.e., `cmd || rc=$?` or
# `set +e; cmd; set -e`).  Under `set -euo pipefail`, the original
# `cmd; rc=$?; exit $rc` is dead code when cmd exits non-zero: bash
# aborts before `rc=$?` runs (exit code propagates by ACCIDENT).
# This assertion is RED until step-13 lands the `|| rc=$?` idiom.
assert "wrapper uses idiomatic non-set-e-aborting exit-code pattern" \
    bash -c 'grep -qE '"'"'(\|\| rc=|set \+e)'"'"' "$1"' \
    -- "$REPO_ROOT/scripts/reify-audit-predone-wrapper.sh"

# 6b+6c setup: a fake reify-audit shim in a behavioural tmpdir.
BEHAVIORAL_TMPDIR=$(mktemp -d /tmp/test-wrapper-rc-XXXXXX)
# Update the EXIT trap to cover both tmpdirs.
trap 'rm -rf "$LIVE_TMPDIR" "$BEHAVIORAL_TMPDIR"' EXIT

# Redirect the wrapper's durable-advisory sentinel (Check 9) into the tmpdir for
# EVERY invocation below. Exported deliberately: several checks run the wrapper
# against a deliberately-stale shim, and without this they would stamp the
# host-wide default path — making a real operator sweep report a stale fleet
# because a test ran. Cleaned up with BEHAVIORAL_TMPDIR by the trap above.
export REIFY_AUDIT_ADVISORY_SENTINEL="$BEHAVIORAL_TMPDIR/advisory-sentinel"

# Fake reify-audit: ignores all args, exits with $FAKE_RC (default 0).
cat > "$BEHAVIORAL_TMPDIR/reify-audit" <<'FAKE_AUDIT_EOF'
#!/usr/bin/env bash
exit "${FAKE_RC:-0}"
FAKE_AUDIT_EOF
chmod +x "$BEHAVIORAL_TMPDIR/reify-audit"

# 6b: Non-zero exit code propagation (7 simulates 7 High-severity findings).
# Note: passes against the CURRENT dead-code pattern too (set -e abort
# propagates the child's code by accident).  6a is the structural lock; 6b
# guards against future refactors that break propagation on the non-zero path.
set +e
PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    FAKE_RC=7 \
    REIFY_AUDIT_BIN="$BEHAVIORAL_TMPDIR/reify-audit" \
    bash "$WRAPPER" --task abc --pre-done >/dev/null 2>&1
actual_rc_6b=$?
set -e

assert "wrapper propagates child exit code 7 (simulated 7 High findings)" \
    bash -c 'test "$1" -eq 7' -- "$actual_rc_6b"

# 6c: Zero exit code propagation (no High-severity findings).
set +e
PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    FAKE_RC=0 \
    REIFY_AUDIT_BIN="$BEHAVIORAL_TMPDIR/reify-audit" \
    bash "$WRAPPER" --task abc --pre-done >/dev/null 2>&1
actual_rc_6c=$?
set -e

assert "wrapper propagates child exit code 0 (no High findings)" \
    bash -c 'test "$1" -eq 0' -- "$actual_rc_6c"

# ==============================================================================
# Check 7: freshness guard integration (RED until step-6 wires the guard)
# ==============================================================================
echo ""
echo "--- Check 7: freshness guard (stale binary FAILS OPEN; unrunnable binary refuses) ---"

# 7a: Static wiring — the wrapper must have an actual `source` line for the
# freshness guard library (not just a mention in a comment). A commented-out
# reference passes a bare substring grep but does not wire the guard.
# Pattern: `^[[:space:]]*(source|.)` at line start, then whitespace, then the
# filename — matches the real source line and rejects `# source ...` comments.
assert "wrapper has an actual 'source' line for reify-audit-freshness.sh" \
    bash -c 'grep -qE '"'"'^[[:space:]]*(source|\.)[[:space:]]+.*reify-audit-freshness\.sh'"'"' "$1"' \
    -- "$REPO_ROOT/scripts/reify-audit-predone-wrapper.sh"

# 7b: Behavioral — a stale but PRESENT binary must FAIL OPEN (task #7139).
#
# DELIBERATE CONTRACT CHANGE. This check previously asserted the opposite:
#   "wrapper exits 125 when REIFY_AUDIT_BIN is stale"
#   "stale guard exit (125) is distinct from child propagation codes 7 and 0"
# Those two assertions WERE the defect, expressed as a test. This wrapper is a
# SYNCHRONOUS pre-done hook: dark-factory's
# fused_memory/middleware/pre_done_hook.py allows the status flip only on rc 0
# (:222-223) and blocks it on ANY non-zero rc, so a 125 here wedges EVERY
# done-flip in the project until a human runs `cargo install`. crates/reify-audit
# lands ~4 commits/day, so the freshness epoch advances several times a day and
# that outage recurs by construction — it ran ~15.7h over 2026-08-30/31 and
# produced three escalations (esc-7042-2, esc-6315-2, esc-6120-5), none of which
# identified the real cause.
#
# This is NOT a weakened test. Coverage moves rather than shrinking: 7b-ii is a
# stronger assertion than the one it replaces (it pins the exact regression that
# caused the outage), and 7d below is entirely new — it pins that fail-open did
# not become fail-SILENT.
#
# Reuses the existing BEHAVIORAL_TMPDIR shim harness (fake reify-audit +
# REIFY_AUDIT_BIN override) from Check 6's setup. The shim is already
# `chmod +x` there, which now matters: the guard splits on `[ -x "$bin" ]`.
# Touch it to year 2000 (definitely before any crate commit) so the guard fires.
touch -t 200001010000 "$BEHAVIORAL_TMPDIR/reify-audit"

# 7b-i: the child's exit code propagates — the flip is gated by the DETECTOR's
# own findings, not blocked by the freshness guard.
set +e
PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    FAKE_RC=7 \
    REIFY_AUDIT_BIN="$BEHAVIORAL_TMPDIR/reify-audit" \
    bash "$WRAPPER" --task abc --pre-done >/dev/null 2>&1
actual_rc_7b_7=$?
set -e

assert "7b-i: stale-but-present binary — wrapper propagates child exit code 7" \
    bash -c 'test "$1" -eq 7' -- "$actual_rc_7b_7"

# 7b-ii: THE regression that would have prevented the outage. A stale binary
# that finds nothing must let the done-flip through.
set +e
STALE_STDERR_7B=$(PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    FAKE_RC=0 \
    REIFY_AUDIT_BIN="$BEHAVIORAL_TMPDIR/reify-audit" \
    bash "$WRAPPER" --task abc --pre-done 2>&1 >/dev/null)
actual_rc_7b_0=$?
set -e

assert "7b-ii: stale-but-present binary — wrapper exits 0, the done-flip is ALLOWED" \
    bash -c 'test "$1" -eq 0' -- "$actual_rc_7b_0"

# 7b-iii: falling open is LOUD, not silent. The token is what a consumer
# branches on (the deploy probe does, after step-12).
assert "7b-iii: falling open is loud — stderr carries E_AUDIT_BIN_STALE" \
    bash -c 'printf "%s" "$1" | grep -qF "E_AUDIT_BIN_STALE"' -- "$STALE_STDERR_7B"

# 7b-iv: and the operator's fix survives into the hook's captured stderr, which
# dark-factory clips to 2000 chars and surfaces to the MCP caller.
assert "7b-iv: fail-open stderr still tells the operator to 'cargo install'" \
    bash -c 'printf "%s" "$1" | grep -qF "cargo install"' -- "$STALE_STDERR_7B"

# 7c: Regression — re-verify 6b/6c still pass now that the shim is touched
# to a FRESH mtime (so the guard passes and child exit codes propagate normally).
# Freshen the shim (touch to now).
touch "$BEHAVIORAL_TMPDIR/reify-audit"

set +e
PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    FAKE_RC=7 \
    REIFY_AUDIT_BIN="$BEHAVIORAL_TMPDIR/reify-audit" \
    bash "$WRAPPER" --task abc --pre-done >/dev/null 2>&1
actual_rc_7c_7=$?
set -e

assert "7c regression: wrapper propagates child exit code 7 with fresh binary" \
    bash -c 'test "$1" -eq 7' -- "$actual_rc_7c_7"

set +e
PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    FAKE_RC=0 \
    REIFY_AUDIT_BIN="$BEHAVIORAL_TMPDIR/reify-audit" \
    bash "$WRAPPER" --task abc --pre-done >/dev/null 2>&1
actual_rc_7c_0=$?
set -e

assert "7c regression: wrapper propagates child exit code 0 with fresh binary" \
    bash -c 'test "$1" -eq 0' -- "$actual_rc_7c_0"

# 7d: ABSENT binary — the wrapper must still REFUSE (task #7139).
#
# This pins that fail-open did NOT become fail-SILENT. Fail-open means "run the
# stale detector anyway", which presupposes a detector exists. With nothing on
# disk there is nothing to fall open onto, so exiting 0 here would let a
# done-flip through having asserted NOTHING — and dying with 127 from the shell
# would be a worse, less diagnosable block than a guard code. 125 with a named
# token is the only honest answer.
#
# The `set -euo pipefail` interaction is what delivers this: the guard call in
# the wrapper is unchecked, so a 125 return still aborts the wrapper with 125.
NO_SUCH_BIN="$BEHAVIORAL_TMPDIR/no-such-reify-audit"

set +e
MISSING_STDERR_7D=$(PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    REIFY_AUDIT_BIN="$NO_SUCH_BIN" \
    bash "$WRAPPER" --task abc --pre-done 2>&1 >/dev/null)
actual_rc_7d=$?
set -e

assert "7d: ABSENT binary — wrapper exits 125 (fail-open did not become fail-silent)" \
    bash -c 'test "$1" -eq 125' -- "$actual_rc_7d"

assert "7d: ABSENT-binary refusal carries E_AUDIT_BIN_MISSING (not the stale token)" \
    bash -c 'printf "%s" "$1" | grep -qF "E_AUDIT_BIN_MISSING"' -- "$MISSING_STDERR_7D"

# ==============================================================================
# Check 8: the deploy script's end-to-end probe is not fooled by fail-open
#          (task #7139)
#
# WHY THIS IS A REAL DEFECT, not housekeeping. scripts/deploy-reify-audit-
# predone-hook.sh step 6 dies only when
#   [ "$probe_rc" = "125" ] && grep -q 'reinstall with: cargo install'
# Now that a present-but-stale binary makes the wrapper exit 0, that probe
# would fall straight into the success branch and print "probe exit 0 —
# freshness guard passed, detector ran". The deploy would report SUCCESS while
# leaving the fleet on a stale binary — silently undoing the assertion its
# step 3 exists to make.
#
# That false green is consequential: the deploy script is the before_done
# action of deterministic task #6939, where exit 0 drives a real done-flip with
# done_provenance.kind='deterministic-deploy'.
#
# Fail-open is the right policy for the unattended done-flip hot path. It is
# the WRONG answer for an ATTENDED deploy that just claimed to have installed a
# fresh binary — so the two consumers must diverge, and this check pins that.
#
# Same cross-script structural-contract idiom as Check 6a: a
# behavioural-contract pin on another script's control flow, not a prose or
# docstring pin.
# ==============================================================================
echo ""
echo "--- Check 8: deploy probe detects a fail-open advisory ---"

DEPLOY_SCRIPT="$REPO_ROOT/scripts/deploy-reify-audit-predone-hook.sh"

assert "8-precondition: the deploy script exists" \
    bash -c '[ -f "$1" ]' -- "$DEPLOY_SCRIPT"

# 8a: the probe must branch on the machine TOKEN, per the
# jcodemunch_index.rs:522 convention that "Consumers must branch on this rather
# than on message prose".
assert "8a: deploy script branches on the E_AUDIT_BIN_STALE token" \
    bash -c 'grep -qF "$2" "$1"' -- "$DEPLOY_SCRIPT" 'E_AUDIT_BIN_STALE'

# 8b: REGRESSION PIN — the legacy substring leg must survive. It is what still
# catches the rc-125 unrunnable-binary case, so replacing rather than adding
# would lose coverage.
assert "8b: deploy script still greps the legacy 'reinstall with: cargo install' substring" \
    bash -c 'grep -qF "$2" "$1"' -- "$DEPLOY_SCRIPT" 'reinstall with: cargo install'

# ==============================================================================
# Check 9: the fail-open advisory reaches a channel that survives rc 0
#          (task #7139 review)
#
# WHY THIS IS A REAL DEFECT, not belt-and-braces. Check 7b-iii pins the advisory
# on the wrapper's STDERR — necessary, but NOT sufficient on the one path this
# task exists to fix. dark-factory's fused_memory/middleware/pre_done_hook.py
# launches the wrapper with stderr=PIPE and surfaces the captured text ONLY on a
# non-zero exit (`if returncode == 0: return None`, :222-223). This repo already
# records that exact trap for the sibling knob:
# docs/architecture-audit/f-infra-design.md §11.1.4 — "warn-only makes the gate
# SILENT on the live hook path, not advisory ... captured and discarded ... A
# soak run through the live hook observes nothing".
#
# Since #7139 makes a stale-but-runnable binary exit 0, stderr ALONE would mean
# the E_AUDIT_BIN_STALE alarm is captured and dropped on every live done-flip.
# With crates/reify-audit landing ~4 commits/day, the steady state would be a
# fleet permanently running a stale P5 detector with zero operator-visible
# signal — a loud outage traded for a silent, indefinite degradation, with the
# token surfacing only under an attended deploy probe. The advisory must
# therefore ALSO reach a channel the hook cannot swallow.
# ==============================================================================
echo ""
echo "--- Check 9: fail-open advisory survives rc 0 (durable channel) ---"

ADVISORY_SENTINEL="$BEHAVIORAL_TMPDIR/advisory-sentinel"

# Re-stale the shim (7c freshened it) so the guard fires and falls open.
touch -t 200001010000 "$BEHAVIORAL_TMPDIR/reify-audit"
rm -f "$ADVISORY_SENTINEL"

set +e
PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    FAKE_RC=0 \
    REIFY_AUDIT_BIN="$BEHAVIORAL_TMPDIR/reify-audit" \
    bash "$WRAPPER" --task abc --pre-done >/dev/null 2>&1
actual_rc_9a=$?
set -e

# 9a: the flip is still allowed — recording must never become a second gate.
assert "9a: fail-open run still exits 0 (the durable record is not a new gate)" \
    bash -c 'test "$1" -eq 0' -- "$actual_rc_9a"

# 9b: ...and it left a record behind on a channel fused-memory does not drop.
assert "9b: an rc-0 fail-open run leaves a durable sentinel behind" \
    bash -c '[ -f "$1" ]' -- "$ADVISORY_SENTINEL"

# 9c: the record is greppable by the same machine token as the stderr form, so
# a triager greps ONE string across both channels
# (jcodemunch_index.rs:522 convention).
assert "9c: the sentinel carries the stable token E_AUDIT_BIN_STALE" \
    bash -c 'grep -qF "E_AUDIT_BIN_STALE" "$1" 2>/dev/null' -- "$ADVISORY_SENTINEL"

# 9d: and the operator's remedy, so the sentinel is self-describing standalone.
assert "9d: the sentinel carries the reinstall remedy" \
    bash -c 'grep -qF "cargo install" "$1" 2>/dev/null' -- "$ADVISORY_SENTINEL"

# 9e: BOUNDED. The condition persists for hours-to-days across every done-flip
# in the project, so the record must be truncate-written, never appended — an
# append would turn a stale binary into unbounded /tmp growth.
set +e
PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    FAKE_RC=0 \
    REIFY_AUDIT_BIN="$BEHAVIORAL_TMPDIR/reify-audit" \
    bash "$WRAPPER" --task abc --pre-done >/dev/null 2>&1
set -e

assert "9e: the sentinel stays ONE line across repeated runs (truncate, not append)" \
    bash -c 'test "$(wc -l < "$1")" -le 1' -- "$ADVISORY_SENTINEL"

# 9f: no FALSE alarm. A fresh binary must leave no sentinel at all, or a sweep
# reading it would report a stale fleet forever after one stale day.
rm -f "$ADVISORY_SENTINEL"
touch "$BEHAVIORAL_TMPDIR/reify-audit"

set +e
PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    FAKE_RC=0 \
    REIFY_AUDIT_BIN="$BEHAVIORAL_TMPDIR/reify-audit" \
    bash "$WRAPPER" --task abc --pre-done >/dev/null 2>&1
set -e

assert "9f: a FRESH binary writes no sentinel (silence means healthy)" \
    bash -c '[ ! -f "$1" ]' -- "$ADVISORY_SENTINEL"

# 9g: BEST-EFFORT. Recording is a diagnostic, never a gate: if the sentinel path
# is unwritable the done-flip must still go through, or the observability added
# here would reintroduce the very outage this task removed.
touch -t 200001010000 "$BEHAVIORAL_TMPDIR/reify-audit"

set +e
PATH="$BEHAVIORAL_TMPDIR:$PATH" \
    FAKE_RC=0 \
    REIFY_AUDIT_BIN="$BEHAVIORAL_TMPDIR/reify-audit" \
    REIFY_AUDIT_ADVISORY_SENTINEL="$BEHAVIORAL_TMPDIR/no-such-dir/sentinel" \
    bash "$WRAPPER" --task abc --pre-done >/dev/null 2>&1
actual_rc_9g=$?
set -e

assert "9g: an UNWRITABLE sentinel path does not block the flip (best-effort)" \
    bash -c 'test "$1" -eq 0' -- "$actual_rc_9g"

# 9h: STRUCTURAL — the journal is the channel an operator actually watches on
# the live host (the wrapper's parent is fused-memory.service), but asserting on
# journald from a test would need a live journal and a writable syslog socket.
# Pinned structurally instead, the same idiom as Check 6a (:209-211) and Check
# 8a: a contract pin on the script's control flow, not on prose.
assert "9h: the wrapper also emits the advisory to the journal via logger" \
    bash -c 'grep -qE "logger[^|]*-t[[:space:]]+reify-audit-predone" "$1"' \
    -- "$REPO_ROOT/scripts/reify-audit-predone-wrapper.sh"

# -- Summary ------------------------------------------------------------------
test_summary
