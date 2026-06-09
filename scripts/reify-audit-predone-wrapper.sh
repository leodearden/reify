#!/usr/bin/env bash
# scripts/reify-audit-predone-wrapper.sh
#
# Pre-done hook wrapper for reify-audit (post-Taskmaster removal).
#
# WHY THIS WRAPPER EXISTS
# -----------------------
# reify-audit is a pure-logic library (no MCP client, no scheduler) and
# requires an explicit --tasks-file with a JSON array of TaskMetadata. Before
# task 3731, the CLI had a dead default tasks-file path (the Taskmaster artifact
# deleted in commit 1402b46c63 on 2026-05-12). Any invocation without an
# explicit --tasks-file exited 125 ("infrastructure error") and silently
# blocked done-flips via the fused-memory pre-done hook.
#
# This wrapper materializes a fresh TaskMetadata JSON snapshot from the
# fused-memory MCP (http://localhost:8002/mcp) into a tempfile, then execs
# reify-audit with --tasks-file <tempfile>. The snapshot is cleaned up on EXIT.
#
# The JSON-RPC response is mapped to TaskMetadata objects via the canonical
# sidecar filter: scripts/reify-audit-snapshot-filter.jq
# (single point of truth shared with the /audit skill references).
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

# ── Freshness guard ──────────────────────────────────────────────────────────
# Source the shared freshness guard library. The guard is called in REFUSE mode
# after flag validation: if REIFY_AUDIT_BIN predates the last crates/reify-audit
# commit, the wrapper exits 125 with a reinstall hint rather than running a
# stale detector. REFUSE mode is used here (not REBUILD) because auto-install
# on the synchronous per-done-flip hot path would add minutes per flip and race
# across concurrent flips. The operator reinstall command:
#   cargo install --path crates/reify-audit --root ~/.cargo --force
# shellcheck source=scripts/reify-audit-freshness.sh
source "$REPO_ROOT/scripts/reify-audit-freshness.sh"

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
# Note: --help/-h is matched anywhere in argv, including positions that would
# normally be flag values (e.g. `--task --help`). This is an intentional
# convenience — operators use --help to discover the interface, and the
# ambiguity is harmless in practice (the systemd hook never passes --help).
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
    exit 125
fi

# Reject flag-shaped task ids (e.g. `--task --pre-done` with no id supplied).
# The loop above would set task_id to the next argv token regardless of whether
# it looks like a flag; a leading `--` means the caller forgot the task id.
case "$task_id" in
    --*)
        echo "reify-audit-predone-wrapper.sh: error: --task value looks like a flag ('$task_id'); did you forget the task id?" >&2
        echo "" >&2
        usage >&2
        exit 125
        ;;
esac

# ── Freshness check (fail-closed, before any MCP calls) ─────────────────────
# Refuse if REIFY_AUDIT_BIN predates the last crates/reify-audit commit.
# Exit code 125 carries the reinstall hint to stderr so the operator can fix it.
reify_audit_guard "$REIFY_AUDIT_BIN" refuse "$REPO_ROOT"

# ── Materialize snapshot from fused-memory MCP ───────────────────────────────
SNAPSHOT=$(mktemp /tmp/reify-audit-snapshot-XXXXXX.json)
# Separate stderr tempfiles for curl and jq so operators can distinguish
# "MCP unavailable" from "envelope shape changed" from "sidecar filter bug".
CURL_ERR=$(mktemp /tmp/reify-audit-curl-err-XXXXXX)
JQ_ERR=$(mktemp /tmp/reify-audit-jq-err-XXXXXX)
trap 'rm -f "$SNAPSHOT" "$CURL_ERR" "$JQ_ERR"' EXIT

# JSON-RPC get_tasks call. The fused-memory MCP speaks JSON-RPC 2.0 over HTTP.
# Response shape: {"result":{"content":[{"type":"text","text":"<json-string>"}],...}}
# where the text value is a JSON object with a .tasks array.
get_tasks_payload=$(printf '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_tasks","arguments":{"project_root":"%s"}}}' "$REPO_ROOT")

if ! curl -sf \
    --max-time "$MCP_TIMEOUT" \
    -X POST "$MCP_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -d "$get_tasks_payload" 2>"$CURL_ERR" \
    | jq -r -f "$REPO_ROOT/scripts/reify-audit-snapshot-filter.jq" \
    > "$SNAPSHOT" 2>"$JQ_ERR"; then
    echo "reify-audit-predone-wrapper.sh: error: failed to fetch tasks from fused-memory MCP at $MCP_URL" >&2
    echo "  Check: systemctl --user status fused-memory" >&2
    if [ -s "$CURL_ERR" ]; then
        echo "  curl stderr: $(head -5 "$CURL_ERR")" >&2
    fi
    if [ -s "$JQ_ERR" ]; then
        echo "  jq stderr: $(head -5 "$JQ_ERR")" >&2
    fi
    exit 125
fi

# Sanity-check: snapshot must be a non-empty JSON array.
if ! jq -e 'type == "array"' "$SNAPSHOT" >/dev/null 2>&1; then
    echo "reify-audit-predone-wrapper.sh: error: fused-memory get_tasks returned unexpected shape (not a JSON array)" >&2
    echo "  Snapshot: $(cat "$SNAPSHOT" 2>/dev/null | head -5)" >&2
    exit 125
fi

# Post-snapshot sanity: if any done task lacks done_at, P1 will silently
# skip it. Warn loudly to stderr (but don't block — legacy fused-memory
# rows may legitimately lack updatedAt). See task 3731 review feedback
# and docs/architecture-audit/f-infra-design.md §11.2.
missing_done_at=$(jq -r '[ .[] | select(.status == "done" and .done_at == null) | .task_id ] | join(",")' "$SNAPSHOT")
if [ -n "$missing_done_at" ]; then
    echo "reify-audit-predone-wrapper.sh: WARNING: done tasks with no done_at (P1 will skip them): $missing_done_at" >&2
fi

# ── Invoke reify-audit with explicit --tasks-file ────────────────────────────
# Pass ALL original args through; do not consume any. The EXIT trap handles
# snapshot cleanup after reify-audit returns (exec would skip the trap).
#
# Idiomatic exit-code forwarding under `set -e`: reify-audit deliberately
# returns 1-254 to indicate the count of High-severity findings (the EXPECTED
# gating signal, not an error). A bare `cmd; rc=$?; exit $rc` would abort on
# `set -e` BEFORE `rc=$?` ran — the propagation would still work by accident
# (bash exits with the child's code on set-e abort), but `rc=$?; exit $rc`
# would be dead code, and any future cleanup/diagnostic code added between
# the invocation and `exit $rc` would be silently skipped. The `|| rc=$?`
# form makes the failure path explicit and keeps the post-invocation block
# reachable. See task 3731 review cycle 2.
rc=0
"$REIFY_AUDIT_BIN" \
    --tasks-file "$SNAPSHOT" \
    --runs-db "$RUNS_DB" \
    --project-root "$REPO_ROOT" \
    "$@" || rc=$?
exit "$rc"
