#!/usr/bin/env python3
"""Normalize .taskmaster/tasks/tasks.json task and subtask IDs to JSON strings.

The Taskmaster CLI serializes new IDs as JSON integers.  This script converts
every ``id`` field under ``.master.tasks[]`` and ``.master.tasks[].subtasks[]``
to a digit-string (e.g. ``5`` -> ``"5"``).  Already-string IDs are untouched.
Non-id fields retain their values; the file is re-serialized with indent=2
so formatting is normalized to match Taskmaster CLI output.

Usage:
    python3 normalize_tasks_json.py <path-to-tasks.json>

Exits 0 on success (including no-op when already normalized).
Exits 1 if the file is missing or cannot be parsed.
"""

import json
import sys


def normalize(data: dict) -> bool:
    """Walk tasks and subtasks, converting int ids to strings in-place.

    Returns True if any change was made, False if the data was already
    fully normalized (so callers can skip a write).
    """
    changed = False
    tasks = data.get("master", {}).get("tasks", [])
    for task in tasks:
        if isinstance(task.get("id"), int):
            task["id"] = str(task["id"])
            changed = True
        for subtask in task.get("subtasks", []):
            if isinstance(subtask.get("id"), int):
                subtask["id"] = str(subtask["id"])
                changed = True
    return changed


def main() -> int:
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <tasks.json>", file=sys.stderr)
        return 1

    path = sys.argv[1]

    try:
        with open(path, "r", encoding="utf-8") as fh:
            text = fh.read()
    except OSError as exc:
        print(f"Error reading {path}: {exc}", file=sys.stderr)
        return 1

    try:
        data = json.loads(text)
    except json.JSONDecodeError as exc:
        print(f"Error parsing {path}: {exc}", file=sys.stderr)
        return 1

    changed = normalize(data)

    if not changed:
        # Already normalized — no-op, avoid spurious write.
        return 0

    normalized = json.dumps(data, indent=2, ensure_ascii=False)

    try:
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(normalized)
    except OSError as exc:
        print(f"Error writing {path}: {exc}", file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
