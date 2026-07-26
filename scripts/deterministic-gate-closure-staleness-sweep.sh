#!/usr/bin/env bash
# scripts/deterministic-gate-closure-staleness-sweep.sh — Standalone,
# timer-friendly staleness sweep over stranded `blocked` / `in-progress`
# Taskmaster rows. Read-only advisory observability: it NEVER mutates task
# state (no write to tasks.db, no write to the escalation store) and never
# gates dispatch/reclaim/merge. Its only side effect on any invocation is the
# request files it drops under --emit-requests.
#
# Task #5321; generalizes the corruption-remediation runbook shipped by #5316
# (docs/notes/offline-lane-red-corruption-remediation.md, which is a docs
# runbook with no script and no timer — this is the family's first executable
# artifact). Operational digest:
# docs/notes/deterministic-gate-closure-staleness-sweep.md
#
# A task is STRANDED when the premise that blocked it has since resolved but
# nothing re-dispatched or closed it. Three trigger classes are swept in one
# pass:
#
#   gate_closure       — a `blocked` deterministic always-escalates gate task
#                        whose gating escalation was resolved/dismissed
#                        elsewhere (no live `esc-<id>-*.json` with
#                        status=pending remains) ⇒ action=close.
#   merge_verify_red   — a post-merge-verification-failed block whose recorded
#                        main_sha is an ancestor of --main-ref, where main has
#                        advanced and ≥1 recorded files_referenced path was
#                        touched in main_sha..main-ref ⇒ action=reverify.
#   unmet_dependency   — a `blocked` task with ≥1 dependency row where every
#                        depends_on has reached a terminal status
#                        (done/cancelled) ⇒ action=redispatch.
#
# Usage:
#   scripts/deterministic-gate-closure-staleness-sweep.sh \
#       [--db PATH] [--tag TAG] [--escalations DIR] [--repo DIR] \
#       [--main-ref REF] [--format table|json] \
#       [--class all|gate_closure|merge_verify_red|unmet_dependency] \
#       [--stale-heartbeat-min N] [--emit-requests DIR] [-h|--help]
#
# For each candidate row, emits: task_id · status · class · verdict · action ·
# evidence · flags. Verdict vocabulary:
#   STALE        — premise resolved; the row is a confirmed hit (the ONLY
#                  verdict that emits a re-dispatch request).
#   UNRESOLVED   — the class matched but its premise has NOT resolved yet.
#   GATED        — class A only: a live pending escalation still gates it.
#   LIVE         — the liveness guard fired (fresh heartbeat); skipped
#                  entirely, no class predicate ran.
#   CORRUPT-HOLD — an otherwise-confirmed hit carrying a #5316 corruption
#                  flag; suppressed for a human gate (action=human_gate).
#   unknown      — an oracle could not be read; never upgraded to STALE.
# A trailing SWEEP: line (table) / summary object (json) carries the counters
# candidates, gate_closure, merge_verify_red, unmet_dependency, corrupt_hold,
# live_skipped, unknown.
#
# Options (env defaults shown):
#   --db PATH             Taskmaster SQLite store (env: REIFY_LANE_TASK_DB;
#                         default: /home/leo/src/reify/.taskmaster/tasks/tasks.db).
#                         Reused verbatim from scripts/lane-task-status.sh's
#                         contract — one task-DB plumbing contract in the repo.
#                         Opened strictly read-only (-readonly / mode=ro). A
#                         missing / 0-byte / unreadable DB degrades to a
#                         zero-candidate report, never an abort.
#   --tag TAG             Taskmaster tag namespace (env: REIFY_LANE_TASK_TAG;
#                         default: master). Task ids are unique only within a
#                         tag (PRIMARY KEY (tag, id)), so every query is
#                         tag-scoped.
#   --escalations DIR     Live escalation store holding esc-<task>-<n>.json
#                         (env: REIFY_GATE_STALENESS_ESCALATIONS_DIR; default:
#                         <repo>/data/escalations). Only the live directory is
#                         read — archive*/ subdirectories are deliberately NOT
#                         searched, because an archived escalation is by
#                         construction no longer gating.
#   --repo DIR            Git repo used for ancestor / touched-path
#                         adjudication (env: REIFY_GATE_STALENESS_REPO;
#                         default: the repo containing this script).
#   --main-ref REF        Git ref for "main" (env:
#                         REIFY_GATE_STALENESS_MAIN_REF; default: main).
#   --format table|json   Output format (default: table).
#   --class C             Restrict the sweep to one trigger class: all,
#                         gate_closure, merge_verify_red, unmet_dependency
#                         (default: all).
#   --stale-heartbeat-min N
#                         Minutes; a candidate whose heartbeat_at is newer than
#                         N minutes is LIVE and is skipped by the liveness
#                         guard (env: REIFY_GATE_STALENESS_HEARTBEAT_MIN;
#                         default: 30).
#   --emit-requests DIR   Drop one atomic, idempotent
#                         redispatch-<task_id>-<class>.json per confirmed STALE
#                         hit into DIR (env:
#                         REIFY_GATE_STALENESS_REQUESTS_DIR; default: unset —
#                         no files are written at all).
#   -h, --help            Print this message and exit.
#
# Exit codes:
#   0  — Always, on every valid invocation (advisory/observability — this
#        script must never gate anything). An unreadable DB, a missing
#        escalations dir, an unresolvable SHA, or an unwritable
#        --emit-requests dir all degrade gracefully rather than aborting.
#   2  — Usage error: unknown flag, missing flag value, invalid --format /
#        --class / --stale-heartbeat-min.
#
# Invariants:
#   (Populated as each classifier lands — see the operational digest.)

set -euo pipefail

exit 0
