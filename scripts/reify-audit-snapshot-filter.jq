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
#   2. Fall back to .updatedAt via iso8601_to_epoch_or_null, which parses the
#      shapes crates/reify-audit/src/fused_memory_client.rs
#      parse_iso8601_to_epoch documents -- "Z", "+HH:MM"/"-HH:MM", "+HH"/"-HH",
#      or no TZ (read as UTC), each with an optional ".fraction" -- to the SAME
#      epoch as that loader.  Guarded by Checks 5f and 5h of
#      tests/infra/test_reify_audit_predone_wrapper.sh.
#   3. The conversion is TOTAL: every input yields exactly one value, so a bad
#      .updatedAt gives done_at = null FOR THAT ROW ONLY and the row stays in
#      the snapshot (matching the loader's per-task Option<i64>).  Hence
#      `try (...) catch null` around the WHOLE pipeline (a non-string raises
#      inside test(); an out-of-range field raises inside fromdateiso8601),
#      and an explicit `else null` arm for no-match, because capture yields
#      EMPTY there and an empty value inside map({...}) DROPS the row.  Not
#      `?`: it is `catch EMPTY` (same drop) and binds only to the last filter.
#      Null, not the loader's answer, for non-ISO input it is lax about (text
#      after Z, non-numeric fraction, unpadded or out-of-range fields) and for
#      a basic-format "+HHMM" offset it mis-reads; the wrapper warns on every
#      done row left with a null done_at (its missing_done_at check).
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

def iso8601_pattern:
  "\\A(?<datetime>[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2})(?:\\.[0-9]+)?(?:Z|(?<sign>[+-])(?<offset_hours>[0-9]{2})(?::(?<offset_minutes>[0-9]{2}))?)?\\z";

def utc_offset_seconds:
  if .sign == null then 0
  else (if .sign == "+" then 1 else -1 end)
       * ((.offset_hours | tonumber) * 3600 + ((.offset_minutes // "0") | tonumber) * 60)
  end;

def iso8601_to_epoch_or_null:
  try (
    if test(iso8601_pattern)
    then capture(iso8601_pattern) | (.datetime + "Z" | fromdateiso8601) - utc_offset_seconds
    else null
    end
  ) catch null;

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
            (.metadata.done_at // (.updatedAt | iso8601_to_epoch_or_null))
          else
            null
          end
        )
      }
  )
