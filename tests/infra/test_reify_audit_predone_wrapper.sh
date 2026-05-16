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

# -- Summary ------------------------------------------------------------------
test_summary
