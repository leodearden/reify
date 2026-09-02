#!/usr/bin/env bash
# tests/infra/test_reify_audit_predone_wrapper.sh
#
# Regression guard: asserts that the reify-audit-predone-wrapper.sh script
# exists, is executable, handles --help, and errors appropriately on missing
# required flags — without requiring a live fused-memory MCP server.
#
# Background: the wrapper materializes a TaskMetadata JSON snapshot from the
# fused-memory MCP before invoking reify-audit. This test validates the
# wrapper's basic invocation surface so CI stays GREEN before the systemd
# operator action rewires FUSED_MEMORY_PREDONE_HOOK_REIFY.
#
# See: docs/architecture-audit/f-infra-design.md §11.1
#      task 3731 (root-cause: dead .taskmaster/tasks/tasks.json default)
#      task 7139 (Check 7b inverted: a stale-but-PRESENT binary now FAILS OPEN
#                 rather than blocking every done-flip project-wide; Check 7d
#                 pins that an ABSENT binary still refuses)
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
# Check 5: snapshot filter sidecar derives done_at correctly
# ==============================================================================
echo ""
echo "--- Check 5: snapshot filter sidecar (scripts/reify-audit-snapshot-filter.jq) ---"

FILTER_TMPDIR=$(mktemp -d /tmp/test-snapshot-filter-XXXXXX)
trap 'rm -rf "$FILTER_TMPDIR"' EXIT

# Build the JSON-RPC envelope fixture:
#   task A: status=done,    updatedAt present → done_at must be a positive integer
#   task B: status=pending, updatedAt present → done_at must be null (non-done)
#   task C: status=done,    no updatedAt      → done_at must be null (graceful fallback)
#
# The fused-memory get_tasks response shape: .result.content[0].text is a
# JSON string of {tasks:[...]}; the filter does `fromjson | .tasks | map(...)`.
cat > "$FILTER_TMPDIR/tasks.json" <<'TASKS_EOF'
{
  "tasks": [
    {
      "id": "a",
      "status": "done",
      "title": "Task A",
      "updatedAt": "2026-05-01T12:00:00.000Z",
      "metadata": {
        "files": [], "done_provenance": null, "prd": null,
        "consumer_ref": null, "audit_foundation": null
      }
    },
    {
      "id": "b",
      "status": "pending",
      "title": "Task B",
      "updatedAt": "2026-05-10T12:00:00.000Z",
      "metadata": {
        "files": [], "done_provenance": null, "prd": null,
        "consumer_ref": null, "audit_foundation": null
      }
    },
    {
      "id": "c",
      "status": "done",
      "title": "Task C",
      "metadata": {
        "files": [], "done_provenance": null, "prd": null,
        "consumer_ref": null, "audit_foundation": null
      }
    }
  ]
}
TASKS_EOF

# Wrap the tasks JSON in the JSON-RPC envelope (text= raw JSON string).
jq -n --rawfile text "$FILTER_TMPDIR/tasks.json" \
    '{result:{content:[{type:"text",text:$text}]}}' \
    > "$FILTER_TMPDIR/fixture.json"

# Pre-run the filter. On failure (sidecar missing or malformed), fall back to
# '[]' so the jq -e assertions below fail deterministically (FAIL) rather than
# aborting the test via set -e.
jq -r -f "$REPO_ROOT/scripts/reify-audit-snapshot-filter.jq" \
    "$FILTER_TMPDIR/fixture.json" \
    > "$FILTER_TMPDIR/snapshot.json" 2>/dev/null || \
    echo '[]' > "$FILTER_TMPDIR/snapshot.json"

# Write a snapshot with a done task that has done_at=null for 5d.
cat > "$FILTER_TMPDIR/snapshot-with-bad-done.json" <<'BAD_DONE_EOF'
[{"task_id":"x","status":"done","done_at":null,"files":[],"done_provenance":null,"title":"X","prd":null,"consumer_ref":null,"audit_foundation":null}]
BAD_DONE_EOF

# 5a: sidecar file exists
assert "snapshot filter sidecar exists" \
    bash -c '[ -f "$1" ]' -- "$REPO_ROOT/scripts/reify-audit-snapshot-filter.jq"

# 5b: done_at derivation for each fixture task
# Use length-1 guard so an empty snapshot (filter missing) causes FAIL rather
# than vacuous pass from jq -e producing no output.
assert "filter: done task with updatedAt gets done_at as positive integer" \
    bash -c 'jq -e '"'"'[.[] | select(.task_id=="a")] | length == 1 and (.[0].done_at | (type == "number") and (. > 0))'"'"' "$1"' \
    -- "$FILTER_TMPDIR/snapshot.json"

assert "filter: pending task gets done_at null" \
    bash -c 'jq -e '"'"'[.[] | select(.task_id=="b")] | length == 1 and (.[0].done_at == null)'"'"' "$1"' \
    -- "$FILTER_TMPDIR/snapshot.json"

assert "filter: done task with no updatedAt gets done_at null" \
    bash -c 'jq -e '"'"'[.[] | select(.task_id=="c")] | length == 1 and (.[0].done_at == null)'"'"' "$1"' \
    -- "$FILTER_TMPDIR/snapshot.json"

# 5c: output is an array of 3 objects, each with all 9 TaskMetadata fields
assert "filter: output is JSON array of 3 with all 9 TaskMetadata fields" \
    bash -c 'jq -e '"'"'type == "array" and length == 3 and all(.[]; has("task_id") and has("status") and has("files") and has("done_provenance") and has("title") and has("prd") and has("consumer_ref") and has("audit_foundation") and has("done_at"))'"'"' "$1"' \
    -- "$FILTER_TMPDIR/snapshot.json"

# 5d: wrapper sanity-check snippet correctly identifies done tasks with no done_at.
# This pins the jq expression used in the wrapper's post-snapshot warning path.
# Does NOT depend on the sidecar — should be GREEN from day 1.
assert "sanity-check jq snippet identifies done task with no done_at" \
    bash -c 'missing=$(jq -r '"'"'[ .[] | select(.status == "done" and .done_at == null) | .task_id ] | join(",")'"'"' "$1"); [ -n "$missing" ]' \
    -- "$FILTER_TMPDIR/snapshot-with-bad-done.json"

# 5e: wrapper references the sidecar (not inline jq) — prevents copy-paste drift
assert "wrapper script references reify-audit-snapshot-filter.jq" \
    bash -c 'grep -qF "reify-audit-snapshot-filter.jq" "$1"' \
    -- "$REPO_ROOT/scripts/reify-audit-predone-wrapper.sh"

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

# 6b+6c setup: fake curl + fake reify-audit shims in a behavioural tmpdir.
BEHAVIORAL_TMPDIR=$(mktemp -d /tmp/test-wrapper-rc-XXXXXX)
# Update the EXIT trap to cover both tmpdirs.
trap 'rm -rf "$FILTER_TMPDIR" "$BEHAVIORAL_TMPDIR"' EXIT

# Fake curl: ignores all args, emits a valid empty-tasks JSON-RPC envelope.
# The sidecar filter expects .result.content[0].text | fromjson | .tasks → []
cat > "$BEHAVIORAL_TMPDIR/curl" <<'FAKE_CURL_EOF'
#!/usr/bin/env bash
printf '%s\n' '{"result":{"content":[{"type":"text","text":"{\"tasks\":[]}"}]}}'
FAKE_CURL_EOF
chmod +x "$BEHAVIORAL_TMPDIR/curl"

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
# Reuses the existing BEHAVIORAL_TMPDIR shim harness (fake curl +
# REIFY_AUDIT_BIN override) from Check 6's setup. The shim is already `chmod +x`
# at :231, which now matters: the guard splits on `[ -x "$bin" ]`.
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

# -- Summary ------------------------------------------------------------------
test_summary
