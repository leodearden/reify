#!/usr/bin/env python3
"""Normalize .taskmaster/tasks/tasks.json task and subtask IDs to JSON strings.

The Taskmaster CLI serializes new IDs as JSON integers.  This script converts
every ``id`` field under each top-level tag namespace (e.g. ``master``,
``feature-branch``) to a digit-string (e.g. ``5`` -> ``"5"``).
Already-string IDs are untouched.  Non-id fields retain their values.

Usage:
    python3 normalize_tasks_json.py <path-to-tasks.json>

Exits 0 on success (including no-op when already normalized).
Exits 1 if the file is missing or cannot be parsed.
"""

import json
import os
import sys
import tempfile


def normalize(data: dict) -> bool:
    """Walk all top-level tag namespaces and convert int ids to strings.

    Iterates over every value in *data* that is a dict with a ``tasks`` list,
    covering ``master`` as well as any future tag namespaces Taskmaster may
    create (e.g. feature branches create sibling keys alongside ``master``).

    Returns True if any change was made, False if already fully normalized
    (so callers can skip a write).
    """
    changed = False
    for tag_data in data.values():
        if not isinstance(tag_data, dict):
            continue
        for task in tag_data.get("tasks", []):
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

    # ensure_ascii=False matches Node.js JSON.stringify behaviour: non-ASCII
    # characters are emitted as raw UTF-8 rather than \uXXXX escapes.  Using
    # ensure_ascii=True (the Python default) would diverge from Taskmaster
    # output and cause encoding churn on files with non-ASCII task titles.
    # Key ordering is preserved because json.loads returns a dict whose
    # insertion order CPython 3.7+ maintains through json.dumps.
    normalized = json.dumps(data, indent=2, ensure_ascii=False)

    # Write atomically via a sibling temp file so a crash or ENOSPC mid-write
    # cannot leave tasks.json truncated/corrupt — os.replace is an atomic
    # rename on POSIX systems.
    dirpath = os.path.dirname(os.path.abspath(path))
    tmp_path = None
    try:
        fd, tmp_path = tempfile.mkstemp(dir=dirpath, suffix=".tmp")
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            fh.write(normalized)
        os.replace(tmp_path, path)
    except OSError as exc:
        if tmp_path is not None:
            try:
                os.unlink(tmp_path)
            except OSError:
                pass
        print(f"Error writing {path}: {exc}", file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
