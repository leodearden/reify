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

# ------------------------------------------------------------------------------
# Check 5f: a malformed updatedAt degrades PER-ROW, it never aborts the snapshot
# ------------------------------------------------------------------------------
# Defect (task 7236): the sidecar's updatedAt -> done_at fallback guarded only
# the EMPTY-string case, so any non-empty unparseable value raised inside
# fromdateiso8601 and jq exited 5 with ZERO output. The wrapper runs that
# curl|jq pipeline under `set -euo pipefail` and so took its `exit 125`
# ("Infrastructure error") arm: ONE malformed row blocked the done-flip
# project-wide, losing EVERY task's metadata, not just the offending row's.
#
# The contract pinned here mirrors the crate's live loader
# (crates/reify-audit/src/fused_memory_client.rs parse_iso8601_to_epoch, a
# `?`-chained Option<i64>): an unparseable timestamp yields done_at=null FOR
# THAT ROW ONLY, and the row STAYS in the snapshot so the wrapper's
# missing_done_at warning (and P1/P5) can still see it.
#
# Deliberately a SECOND fixture rather than an extension of the a/b/c one
# above: 5c asserts `length == 3` and must keep its meaning. But good and
# malformed rows are mixed inside this ONE payload, because per-row
# degradation -- a bad row not taking down its good neighbour -- is precisely
# the invariant under test.
cat > "$FILTER_TMPDIR/tasks-malformed.json" <<'MALFORMED_EOF'
{
  "tasks": [
    {
      "id": "d1",
      "status": "done",
      "title": "Task D1 (garbage updatedAt)",
      "updatedAt": "garbage",
      "metadata": {
        "files": [], "done_provenance": null, "prd": null,
        "consumer_ref": null, "audit_foundation": null
      }
    },
    {
      "id": "d2",
      "status": "done",
      "title": "Task D2 (good neighbour)",
      "updatedAt": "2026-05-01T12:00:00.000Z",
      "metadata": {
        "files": [], "done_provenance": null, "prd": null,
        "consumer_ref": null, "audit_foundation": null
      }
    },
    {
      "id": "d3",
      "status": "done",
      "title": "Task D3 (non-string updatedAt)",
      "updatedAt": 12345,
      "metadata": {
        "files": [], "done_provenance": null, "prd": null,
        "consumer_ref": null, "audit_foundation": null
      }
    },
    {
      "id": "d4",
      "status": "done",
      "title": "Task D4 (valid ISO-8601, numeric TZ offset)",
      "updatedAt": "2026-05-01T12:00:00+00:00",
      "metadata": {
        "files": [], "done_provenance": null, "prd": null,
        "consumer_ref": null, "audit_foundation": null
      }
    },
    {
      "id": "d5",
      "status": "done",
      "title": "Task D5 (metadata.done_at beats a garbage updatedAt)",
      "updatedAt": "garbage",
      "metadata": {
        "done_at": 1700000000,
        "files": [], "done_provenance": null, "prd": null,
        "consumer_ref": null, "audit_foundation": null
      }
    }
  ]
}
MALFORMED_EOF

jq -n --rawfile text "$FILTER_TMPDIR/tasks-malformed.json" \
    '{result:{content:[{type:"text",text:$text}]}}' \
    > "$FILTER_TMPDIR/fixture-malformed.json"

# Capture the filter's EXIT CODE explicitly (Check 9's `set +e` / rc=$? / `set -e`
# idiom). Check 5's pre-run above discards it via `|| echo '[]'`, but "the filter
# exits 0 rather than 5" is the headline claim of the defect report, so 5f-a
# asserts on the code itself. Keep the same `[]` fallback so the per-row
# assertions FAIL deterministically rather than aborting the suite under set -e.
set +e
jq -r -f "$REPO_ROOT/scripts/reify-audit-snapshot-filter.jq" \
    "$FILTER_TMPDIR/fixture-malformed.json" \
    > "$FILTER_TMPDIR/snapshot-malformed.json" 2>/dev/null
malformed_rc=$?
set -e
[ "$malformed_rc" -eq 0 ] || echo '[]' > "$FILTER_TMPDIR/snapshot-malformed.json"

assert "5f-a: filter exits 0 on a payload containing a malformed updatedAt" \
    bash -c 'test "$1" -eq 0' -- "$malformed_rc"

# 5f-b: the row-count assertion. This is what discriminates the CORRECT
# `try (...) catch null` fix from the `fromdateiso8601?` form: jq's `?` is
# `try ... catch EMPTY`, and an empty result inside `map({...})` object
# construction makes the WHOLE OBJECT VANISH -- silently dropping the malformed
# row from the snapshot. A dropped row is invisible to the wrapper's warning
# and to P1/P5 alike, which is data loss, not degradation.
assert "5f-b: output keeps all 5 rows (a malformed row is degraded, never dropped)" \
    bash -c 'jq -e '"'"'type == "array" and length == 5'"'"' "$1"' \
    -- "$FILTER_TMPDIR/snapshot-malformed.json"

# Per-row assertions reuse 5b's `length == 1 and (...)` guard verbatim, so a
# DROPPED row FAILS (length 0) instead of passing vacuously.
assert "5f-c: done task with a garbage updatedAt string gets done_at null" \
    bash -c 'jq -e '"'"'[.[] | select(.task_id=="d1")] | length == 1 and (.[0].done_at == null)'"'"' "$1"' \
    -- "$FILTER_TMPDIR/snapshot-malformed.json"

assert "5f-d: the good neighbour row is unaffected (done_at a positive integer)" \
    bash -c 'jq -e '"'"'[.[] | select(.task_id=="d2")] | length == 1 and (.[0].done_at | (type == "number") and (. > 0))'"'"' "$1"' \
    -- "$FILTER_TMPDIR/snapshot-malformed.json"

# 5f-e: a NON-STRING updatedAt raises inside sub(), which `fromdateiso8601?`
# does NOT catch (`?` binds only to fromdateiso8601) -- so this row is the
# second discriminator against that form.
assert "5f-e: done task with a non-string updatedAt gets done_at null" \
    bash -c 'jq -e '"'"'[.[] | select(.task_id=="d3")] | length == 1 and (.[0].done_at == null)'"'"' "$1"' \
    -- "$FILTER_TMPDIR/snapshot-malformed.json"

# 5f-f: pins a KNOWN, ACCEPTED divergence rather than leaving it to be
# rediscovered as a fresh bug: jq's fromdateiso8601 accepts only
# "%Y-%m-%dT%H:%M:%SZ", while the Rust parse_iso8601_to_epoch has a split_tz
# arm that handles "+HH:MM". The two loaders still disagree on offset-form
# timestamps -- but under this fix the jq side degrades that ONE row instead of
# aborting the whole snapshot, which is the property this guard exists for.
assert "5f-f: valid ISO-8601 with a numeric TZ offset degrades to null, not an abort" \
    bash -c 'jq -e '"'"'[.[] | select(.task_id=="d4")] | length == 1 and (.[0].done_at == null)'"'"' "$1"' \
    -- "$FILTER_TMPDIR/snapshot-malformed.json"

assert "5f-g: metadata.done_at precedence still wins over a garbage updatedAt" \
    bash -c 'jq -e '"'"'[.[] | select(.task_id=="d5")] | length == 1 and (.[0].done_at == 1700000000)'"'"' "$1"' \
    -- "$FILTER_TMPDIR/snapshot-malformed.json"

# 5f-h: the degraded rows must be OBSERVABLE. This is the wrapper's own
# post-snapshot sanity-check expression (the same one 5d pins), run against the
# degraded snapshot: it must name exactly d1, d3 and d4. Unreachable if the
# rows were dropped -- which is the whole point of degrading rather than
# vanishing.
assert "5f-h: the wrapper's missing_done_at snippet reports exactly d1,d3,d4" \
    bash -c 'missing=$(jq -r '"'"'[ .[] | select(.status == "done" and .done_at == null) | .task_id ] | sort | join(",")'"'"' "$1"); [ "$missing" = "d1,d3,d4" ]' \
    -- "$FILTER_TMPDIR/snapshot-malformed.json"

# ------------------------------------------------------------------------------
# Check 5g: the sidecar is wired to select THIS suite
# ------------------------------------------------------------------------------
# Measured at task 7236, and it contradicts the intuition that a script under
# scripts/ is automatically covered:
#   verify-pipeline-guard.sh requires-full-gate scripts/reify-audit-snapshot-filter.jq -> 1
#   verify-pipeline-guard.sh is-registered      scripts/reify-audit-snapshot-filter.jq -> 1
# i.e. the sidecar is fast-path eligible AND registered by NO route, while
# scripts/verify-pipeline-infra-tests.txt maps the wrapper and the freshness
# lib but had no row for the .jq. So a future .jq-only diff neither routes to
# the full gate nor selects this suite: Check 5f above would never run on the
# very file it guards. This check pins the wiring that closes that hole.
#
# Deliberately NOT asserted: that requires-full-gate exits 0. Surgical
# registration keeps the sidecar OFF the full-gate route on purpose -- the map
# file's own header says such a row must not also be added to
# scripts/verify-pipeline-paths.txt, which would force `--scope all` on every
# future edit to a one-line jq filter.
echo ""
echo "--- Check 5g: sidecar is wired into the verify-pipeline artifact map ---"

VP_INFRA_MAP="$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt"
SIDECAR_REL="scripts/reify-audit-snapshot-filter.jq"
_SELF_TEST_PATH="$SCRIPT_DIR/test_reify_audit_predone_wrapper.sh"

# _map_selects_self <artifact-path> — mirror select_infra_tests()'s parse
# exactly (same comment/blank filter, same two-field `read`, same ACTIVE-ROW
# RULE that BOTH fields must be non-empty per the map header :38-39), then
# expand each matching row's glob under $REPO_ROOT and succeed if it resolves
# to THIS test file. `-f` (not `-e`) mirrors verify.sh's own selective-infra
# emitter loop, so a glob that resolves to a non-regular path is excluded here
# exactly as it would be at runtime. Same idiom as
# tests/infra/test_govtest_slice_reaper.sh Block K.
_map_selects_self() {
    local _want="$1" _artifact _glob _line _expanded
    [ -f "$VP_INFRA_MAP" ] || return 1
    while IFS= read -r _line; do
        read -r _artifact _glob <<< "$_line"
        [ -n "$_artifact" ] || continue
        [ -n "$_glob" ]     || continue
        [ "$_artifact" = "$_want" ] || continue
        for _expanded in "$REPO_ROOT"/$_glob; do
            [ -f "$_expanded" ] || continue
            [ "$_expanded" = "$_SELF_TEST_PATH" ] && return 0
        done
    done < <(grep -v '^[[:space:]]*#' "$VP_INFRA_MAP" | grep -v '^[[:space:]]*$')
    return 1
}

assert "5g-a: verify-pipeline-infra-tests.txt maps the snapshot filter sidecar -> this test" \
    _map_selects_self "$SIDECAR_REL"

assert "5g-b: verify-pipeline-guard.sh reports the snapshot filter sidecar as registered" \
    bash -c 'bash "$1/scripts/verify-pipeline-guard.sh" is-registered "$2" >/dev/null 2>&1' \
    -- "$REPO_ROOT" "$SIDECAR_REL"

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

# Redirect the wrapper's durable-advisory sentinel (Check 9) into the tmpdir for
# EVERY invocation below. Exported deliberately: several checks run the wrapper
# against a deliberately-stale shim, and without this they would stamp the
# host-wide default path — making a real operator sweep report a stale fleet
# because a test ran. Cleaned up with BEHAVIORAL_TMPDIR by the trap above.
export REIFY_AUDIT_ADVISORY_SENTINEL="$BEHAVIORAL_TMPDIR/advisory-sentinel"

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
# Same cross-script structural-contract idiom as Check 5e (:271-273) and Check
# 6a (:209-211): a behavioural-contract pin on another script's control flow,
# not a prose or docstring pin.
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
