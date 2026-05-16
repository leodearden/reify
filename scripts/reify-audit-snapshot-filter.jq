# scripts/reify-audit-snapshot-filter.jq
#
# WHY A SIDECAR
# -------------
# This filter is the single canonical source for mapping a fused-memory
# `tools/call get_tasks` JSON-RPC response to the TaskMetadata array that
# reify-audit expects via --tasks-file.  It is shared by:
#   - scripts/reify-audit-predone-wrapper.sh  (systemd pre-done hook)
#   - .claude/skills/audit/references/cli-invocation.md §2 (audit skill)
#   - .claude/skills/audit/references/modes.md §§1-4 (audit skill modes)
#
# Keeping it in one file prevents the copy-paste drift that introduced the
# original `done_at: null` bug (task 3731 review cycle 1), and makes the
# filter testable in isolation via:
#   jq -r -f scripts/reify-audit-snapshot-filter.jq < fixture.json
#
# INPUT SHAPE
# -----------
# A fused-memory JSON-RPC response:
#   { "result": { "content": [{ "type": "text", "text": "<json-string>" }] } }
# where "text" is a JSON-stringified object: { "tasks": [ ... ] }.
#
# OUTPUT SHAPE
# ------------
# A JSON array of TaskMetadata objects (as expected by reify-audit):
#   [ { "task_id", "status", "files", "done_provenance", "title",
#       "prd", "consumer_ref", "audit_foundation", "done_at" }, ... ]
#
# done_at DERIVATION
# ------------------
# fused-memory MCP does NOT expose an explicit done-flip timestamp (probed
# 2026-05-16).  For tasks with status=="done", this filter derives done_at
# from the top-level `updatedAt` field as an approximation:
#
#   1. Prefer .metadata.done_at if fused-memory ever starts exposing it
#      (forward-compatible — the // fallback only fires when absent/null).
#   2. Fall back to .updatedAt (ISO-8601 string, e.g. "2026-05-16T05:16:06.954Z"):
#      strip the .NNN millisecond suffix that jq 1.7's fromdateiso8601 rejects
#      via sub("\\.[0-9]+Z$"; "Z"), then convert to epoch-seconds.
#   3. If .updatedAt is also absent, done_at = null (graceful degradation for
#      legacy fused-memory rows; the wrapper warns loudly in this case).
#
# Approximation skew: updatedAt equals the done-flip time only when nothing
# further has been written to the task record after the flip.  Typical skew
# is hours-to-days, well within P1's 14-day grace window.
#
# For non-done tasks, done_at is always null (P1 skips them by status anyway,
# per crates/reify-audit/src/p1_producer_orphan.rs:79).
#
# See docs/architecture-audit/f-infra-design.md §11.2 for full rationale.
# Root-cause: task 3731.

.result.content[0].text
| fromjson
| .tasks
| map(
    .status as $status
    | {
        task_id:          (.id | tostring),
        status:           $status,
        files:            (.metadata.files // []),
        done_provenance:  (.metadata.done_provenance // null),
        title:            .title,
        prd:              (.metadata.prd // null),
        consumer_ref:     (.metadata.consumer_ref // null),
        audit_foundation: (.metadata.audit_foundation // null),
        done_at: (
          if $status == "done" then
            (
              .metadata.done_at //
              ((.updatedAt // "") |
                if . == "" then null
                else (sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601)
                end)
            )
          else
            null
          end
        )
      }
  )
