#!/usr/bin/env python3
"""Detect tasks marked done whose IDs are still listed in review/briefing.yaml known_gaps tracking fields.

Cross-references the `tracking:` field of every known_gap entry in
review/briefing.yaml against .taskmaster/tasks/tasks.json.  When a tracked
task has status "done", the gap is stale and a reviewer needs to refresh the
briefing.

Usage:
    python3 scripts/refresh_briefing_known_gaps.py [--briefing PATH] [--tasks PATH] [--json] [--quiet]

Exit codes:
    0  — no stale gaps found
    1  — one or more stale gaps found (mismatches)
    2  — I/O or parse error (bad path, malformed YAML/JSON)
"""

import argparse
import json
import sys
from pathlib import Path

try:
    import yaml
except ImportError:
    yaml = None  # type: ignore[assignment]

# Resolve defaults relative to this script's location so the script can be
# invoked from any working directory without relying on the caller being at
# the repo root.  The script lives at <repo-root>/scripts/refresh_*.py, so
# the repo root is one level up.
_SCRIPT_DIR = Path(__file__).resolve().parent
_REPO_ROOT = _SCRIPT_DIR.parent
_DEFAULT_BRIEFING = str(_REPO_ROOT / "review" / "briefing.yaml")
_DEFAULT_TASKS = str(_REPO_ROOT / ".taskmaster" / "tasks" / "tasks.json")


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Detect tasks marked done whose IDs are still listed in "
            "review/briefing.yaml known_gaps tracking fields."
        )
    )
    parser.add_argument(
        "--briefing",
        default=_DEFAULT_BRIEFING,
        metavar="PATH",
        help="Path to briefing.yaml (default: <repo-root>/review/briefing.yaml)",
    )
    parser.add_argument(
        "--tasks",
        default=_DEFAULT_TASKS,
        metavar="PATH",
        help="Path to tasks.json (default: <repo-root>/.taskmaster/tasks/tasks.json)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        dest="json_output",
        help="Emit mismatches as JSON list to stdout instead of WARN lines to stderr",
    )
    parser.add_argument(
        "--quiet",
        action="store_true",
        help=(
            "Suppress the informational 'OK: no stale known_gaps detected' message "
            "printed when no mismatches are found. "
            "ERROR lines (I/O or parse failures) are always shown regardless of --quiet."
        ),
    )

    args = parser.parse_args()

    # ------------------------------------------------------------------ #
    # Load briefing.yaml                                                   #
    # ------------------------------------------------------------------ #
    if yaml is None:
        print("ERROR: PyYAML is not installed — pip install pyyaml", file=sys.stderr)
        return 2

    try:
        with open(args.briefing, "r", encoding="utf-8") as fh:
            briefing_data = yaml.safe_load(fh)
    except OSError as exc:
        print(f"ERROR: cannot read {args.briefing}: {exc}", file=sys.stderr)
        return 2
    except yaml.YAMLError as exc:
        print(f"ERROR: cannot parse {args.briefing}: {exc}", file=sys.stderr)
        return 2

    # ------------------------------------------------------------------ #
    # Load tasks.json                                                      #
    # ------------------------------------------------------------------ #
    try:
        with open(args.tasks, "r", encoding="utf-8") as fh:
            tasks_data = json.load(fh)
    except OSError as exc:
        print(f"ERROR: cannot read {args.tasks}: {exc}", file=sys.stderr)
        return 2
    except json.JSONDecodeError as exc:
        print(f"ERROR: cannot parse {args.tasks}: {exc}", file=sys.stderr)
        return 2

    # ------------------------------------------------------------------ #
    # Collect (subproject_name, gap_dict) pairs from all subprojects       #
    # ------------------------------------------------------------------ #
    gap_pairs: list[tuple[str, dict]] = []
    subprojects = briefing_data.get("subprojects", {}) if isinstance(briefing_data, dict) else {}
    for subproject_name, subproject_data in subprojects.items():
        if not isinstance(subproject_data, dict):
            continue
        known_gaps = subproject_data.get("known_gaps", [])
        if not isinstance(known_gaps, list):
            continue
        for gap in known_gaps:
            if isinstance(gap, dict):
                gap_pairs.append((subproject_name, gap))

    if not gap_pairs:
        return 0

    # ------------------------------------------------------------------ #
    # Build task index from tasks.json master tag                          #
    # ------------------------------------------------------------------ #
    # Index top-level tasks AND subtasks so that dotted tracking IDs like
    # "1751.2" resolve correctly.  Reify uses dotted subtask IDs extensively;
    # conflating "subtask exists but not indexed" with "task does not exist"
    # would silently treat done subtasks as orphans — a real coverage hole.
    tasks_index: dict[str, dict] = {}
    master = tasks_data.get("master", {}) if isinstance(tasks_data, dict) else {}
    if isinstance(master, dict):
        for task in master.get("tasks", []):
            if isinstance(task, dict):
                task_id = task.get("id")
                if task_id is not None:
                    task_id_str = str(task_id)
                    tasks_index[task_id_str] = task
                    # Index subtasks as "{parent_id}.{subtask_id}" using the
                    # dotted-ID convention Reify uses (e.g. "1751.2").
                    for subtask in task.get("subtasks", []):
                        if isinstance(subtask, dict):
                            subtask_id = subtask.get("id")
                            if subtask_id is not None:
                                tasks_index[f"{task_id_str}.{subtask_id}"] = subtask

    # ------------------------------------------------------------------ #
    # Cross-reference: find gaps whose tracked task is "done"             #
    # ------------------------------------------------------------------ #
    mismatches: list[dict] = []
    for subproject_name, gap in gap_pairs:
        # Use .get() not [] so that legacy entries without a tracking: field
        # are silently skipped rather than raising KeyError.
        tracking_id = gap.get("tracking")
        if tracking_id is None:
            continue
        tracking_id = str(tracking_id)
        # Orphan tracking IDs (task deleted or never existed) are not
        # actionable from a briefing-refresh perspective — silently skip.
        # A separate scrub-list audit can surface stale tracking: fields.
        task = tasks_index.get(tracking_id)
        if task is None:
            continue
        # Only exact "done" status counts.  In-progress, blocked, deferred,
        # and pending tasks may still legitimately represent open gaps from
        # the reviewer's perspective — they are not yet actionable as stale.
        if task.get("status") == "done":
            mismatches.append(
                {
                    "task_id": tracking_id,
                    "title": task.get("title", ""),
                    "subproject": subproject_name,
                    "what": gap.get("what", ""),
                }
            )

    # ------------------------------------------------------------------ #
    # Emit results                                                         #
    # ------------------------------------------------------------------ #
    # --json: emit structured list to stdout for programmatic consumers
    # (e.g. Stage 2 reconciliation).  No WARN lines on stderr in this mode.
    if args.json_output:
        print(json.dumps(mismatches, indent=2))
    else:
        for m in mismatches:
            print(
                f"WARN: known_gap closed for task {m['task_id']} (\"{m['title']}\") "
                f"— refresh review/briefing.yaml under subproject \"{m['subproject']}\"",
                file=sys.stderr,
            )
        # Print an informational OK message when no mismatches are found,
        # unless --quiet suppresses it.  The post-commit hook uses --quiet so
        # it only speaks when there is something actionable (a WARN line).
        # ERROR lines are always emitted regardless of --quiet.
        if not mismatches and not args.quiet:
            print("OK: no stale known_gaps detected", file=sys.stderr)

    return 1 if mismatches else 0


if __name__ == "__main__":
    sys.exit(main())
