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
#   3. If .updatedAt cannot be turned into an epoch for ANY reason, done_at =
#      null FOR THAT ROW ONLY, and the row still appears in the snapshot.  The
#      conversion is TOTAL: absent, null, empty, an unparseable string, a
#      non-string (raises inside sub()), or a valid ISO-8601 form that jq's
#      fromdateiso8601 rejects — notably a numeric TZ offset such as
#      "2026-05-01T12:00:00+00:00", which jq's "%Y-%m-%dT%H:%M:%SZ"-only parser
#      does not accept even though the crate's Rust loader does.  Graceful
#      degradation for legacy fused-memory rows; the wrapper warns loudly on
#      every such row (see its missing_done_at sanity check).
#
#      That last divergence is ACCEPTED BUT TRACKED, not just narrated:
#      widening this parser to match parse_iso8601_to_epoch's split_tz arms
#      ("+HH:MM"/"-HH:MM", and the bare no-TZ form it reads as UTC) is filed as
#      fused-memory follow-up ticket tkt_0RT8TN837CA7QP5T4GFH8Q5VYZ. Any such
#      widening MUST stay total: jq's capture/match produce EMPTY on no-match,
#      and an empty result inside this map({...}) DROPS the whole row -- the
#      same data-loss trap that makes `fromdateiso8601?` wrong, below.
#
#      This deliberately matches the crate's live loader — the `?`-chained
#      Option<i64> in crates/reify-audit/src/fused_memory_client.rs
#      (parse_iso8601_to_epoch) — where a bad timestamp yields None for one
#      task and leaves every other task loadable.
#
#      It is `try (…) catch null` around the WHOLE sub|fromdateiso8601
#      pipeline, and NOT the shorter `fromdateiso8601?`, for two measured
#      reasons (task 7236):
#        (a) jq's `?` is `try … catch EMPTY`, not `catch null`.  An empty
#            result inside this `map({...})` object construction makes the
#            WHOLE OBJECT VANISH, so the malformed row is silently DROPPED
#            from the snapshot — data loss, and invisible to the wrapper's
#            warning and to P1/P5, which can only see rows that are present.
#        (b) `?` binds only to fromdateiso8601, so a non-string .updatedAt
#            still raises inside sub() and aborts the entire snapshot.
#
#      Before this was total, ONE unparseable row made jq exit 5 with zero
#      output; the wrapper runs the pipeline under `set -euo pipefail` and
#      took its exit-125 "Infrastructure error" arm, blocking the done-flip
#      project-wide.  Guarded by Check 5f of
#      tests/infra/test_reify_audit_predone_wrapper.sh.
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
                else (try (sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601) catch null)
                end)
            )
          else
            null
          end
        )
      }
  )
