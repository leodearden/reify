#!/usr/bin/env bash
# scripts/reify-audit-predone-wrapper.sh
#
# Pre-done hook wrapper for reify-audit (post-Taskmaster removal).
#
# WHY THIS WRAPPER EXISTS
# -----------------------
# reify-audit is a pure-logic library (no MCP client, no scheduler) and
# requires an explicit --tasks-file with a JSON array of TaskMetadata. Before
# task 3731, the CLI silently defaulted to .taskmaster/tasks/tasks.json, which
# was deleted in commit 1402b46c63 (2026-05-12). Any invocation without an
# explicit --tasks-file exited 125 ("infrastructure error") and silently
# blocked done-flips via the fused-memory pre-done hook.
#
# This wrapper materializes a fresh TaskMetadata JSON snapshot from the
# fused-memory MCP (http://localhost:8002/mcp) into a tempfile, then execs
# reify-audit with --tasks-file <tempfile>. The snapshot is cleaned up on EXIT.
#
# DESIGN REFERENCE
# ----------------
# docs/architecture-audit/f-infra-design.md §11 (D-1 row), §11.1
# Root-cause: task 3731
#
# SYSTEMD WIRING (operator action required)
# ------------------------------------------
# /home/leo/.config/systemd/user/fused-memory.service must have:
#   Environment=FUSED_MEMORY_PREDONE_HOOK_REIFY=/home/leo/src/reify/scripts/reify-audit-predone-wrapper.sh --task {id} --pre-done
# Then: systemctl --user daemon-reload && systemctl --user restart fused-memory
#
# USAGE
# -----
#   reify-audit-predone-wrapper.sh --task <id> --pre-done [additional reify-audit flags...]
#   reify-audit-predone-wrapper.sh --help

set -euo pipefail

# ── Self-locate so the wrapper works from any worktree ───────────────────────
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ── Constants ────────────────────────────────────────────────────────────────
MCP_URL="${FUSED_MEMORY_MCP_URL:-http://localhost:8002/mcp}"
MCP_TIMEOUT="${FUSED_MEMORY_MCP_TIMEOUT:-10}"
REIFY_AUDIT_BIN="${REIFY_AUDIT_BIN:-/home/leo/.cargo/bin/reify-audit}"
RUNS_DB="${REIFY_AUDIT_RUNS_DB:-$REPO_ROOT/data/orchestrator/runs.db}"

# ── Usage ────────────────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Usage: reify-audit-predone-wrapper.sh --task <id> --pre-done [OPTIONS...]

Materializes a TaskMetadata JSON snapshot from the fused-memory MCP, then
invokes reify-audit with the snapshot as --tasks-file.

Required flags (passed through to reify-audit):
  --task <id>        Task id to check
  --pre-done         Run P5 pre-done check only

Optional flags (passed through to reify-audit):
  --since <date>     Window sweep from ISO date
  --pattern P1|P2|P5 Restrict to one detector
  --runs-db <path>   Override runs.db (default: $RUNS_DB)
  --project-root <path> Override repo root (default: $REPO_ROOT)

Wrapper-local flags (NOT passed to reify-audit):
  --help, -h         Show this help and exit 0

Environment overrides:
  FUSED_MEMORY_MCP_URL       MCP endpoint (default: $MCP_URL)
  FUSED_MEMORY_MCP_TIMEOUT   curl max-time in seconds (default: $MCP_TIMEOUT)
  REIFY_AUDIT_BIN            Path to reify-audit binary (default: $REIFY_AUDIT_BIN)
  REIFY_AUDIT_RUNS_DB        Path to runs.db (default: $RUNS_DB)

Exit codes mirror reify-audit:
  0       No High-severity findings
  1-254   Count of High-severity findings
  125     Infrastructure error (missing flag, MCP unavailable, jq failure, etc.)
EOF
}

# ── --help / -h short-circuit (before any MCP calls) ────────────────────────
for arg in "$@"; do
    case "$arg" in
        --help|-h)
            usage
            exit 0
            ;;
    esac
done

# ── Validate --task is present ───────────────────────────────────────────────
task_id=""
for i in "$@"; do
    if [ "$i" = "--task" ]; then
        task_found_flag=1
    elif [ "${task_found_flag:-0}" = "1" ]; then
        task_id="$i"
        task_found_flag=0
    fi
done

if [ -z "$task_id" ]; then
    echo "reify-audit-predone-wrapper.sh: error: requires --task <id>" >&2
    echo "" >&2
    usage >&2
    exit 2
fi

# ── Materialize snapshot from fused-memory MCP ───────────────────────────────
SNAPSHOT=$(mktemp /tmp/reify-audit-snapshot-XXXXXX.json)
trap 'rm -f "$SNAPSHOT"' EXIT

# JSON-RPC get_tasks call. The fused-memory MCP speaks JSON-RPC 2.0 over HTTP.
# Response shape: {"result":{"content":[{"type":"text","text":"<json-string>"}],...}}
# where the text value is a JSON object with a .tasks array.
get_tasks_payload=$(printf '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_tasks","arguments":{"project_root":"%s"}}}' "$REPO_ROOT")

if ! curl -sf \
    --max-time "$MCP_TIMEOUT" \
    -X POST "$MCP_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -d "$get_tasks_payload" 2>/dev/null \
    | jq -r '
        .result.content[0].text
        | fromjson
        | .tasks
        | map({
            task_id:         (.id | tostring),
            status:          .status,
            files:           (.metadata.files // []),
            done_provenance: (.metadata.done_provenance // null),
            title:           .title,
            prd:             (.metadata.prd // null),
            consumer_ref:    (.metadata.consumer_ref // null),
            audit_foundation:(.metadata.audit_foundation // null),
            done_at:         null
          })
    ' > "$SNAPSHOT" 2>/dev/null; then
    echo "reify-audit-predone-wrapper.sh: error: failed to fetch tasks from fused-memory MCP at $MCP_URL" >&2
    echo "  Check: systemctl --user status fused-memory" >&2
    echo "  Snapshot path (may be empty): $SNAPSHOT" >&2
    exit 125
fi

# Sanity-check: snapshot must be a non-empty JSON array.
if ! jq -e 'type == "array"' "$SNAPSHOT" >/dev/null 2>&1; then
    echo "reify-audit-predone-wrapper.sh: error: fused-memory get_tasks returned unexpected shape (not a JSON array)" >&2
    echo "  Snapshot: $(cat "$SNAPSHOT" 2>/dev/null | head -5)" >&2
    exit 125
fi

# ── Invoke reify-audit with explicit --tasks-file ────────────────────────────
# Pass ALL original args through; do not consume any. The EXIT trap handles
# snapshot cleanup after reify-audit returns (exec would skip the trap).
"$REIFY_AUDIT_BIN" \
    --tasks-file "$SNAPSHOT" \
    --runs-db "$RUNS_DB" \
    --project-root "$REPO_ROOT" \
    "$@"
rc=$?
exit $rc
