#!/usr/bin/env python3
"""validate_tasks_json.py — Schema-invariant validator for .taskmaster/tasks/tasks.json.

Enforces three structural invariants on top-level tasks to prevent ID/dependency
drift after Task 1866's string-ID normalization migration:

  1. Every task `id` is a string matching ``^\d+$`` (not an int, not a slug).
  2. Every entry in a task's `dependencies[]` is a string **and** references an
     existing task id (no orphan deps, no int deps).
  3. No duplicate `id` values across ``master.tasks[]``.

A fourth invariant (subtask IDs and deps) is implemented but **off by default**
(``--check-subtasks`` flag).  It is disabled because upstream ``tm-core``
currently serializes subtask IDs as numbers; enabling it now would make every
auto-commit fail.  A follow-up task (partner of Task 1888) will flip the default
once subtask normalization lands in tm-core.

Usage::

    python3 scripts/validate_tasks_json.py .taskmaster/tasks/tasks.json
    python3 scripts/validate_tasks_json.py --check-subtasks .taskmaster/tasks/tasks.json

Exit 0 on success, 1 if any invariant is violated (all violations printed to
stderr before exit so a single run gives the full picture).
"""

import argparse
import json
import re
import sys
import collections


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Validate tasks.json structural invariants."
    )
    parser.add_argument("path", help="Path to tasks.json")
    parser.add_argument(
        "--check-subtasks",
        action="store_true",
        default=False,
        help=(
            "Also apply invariants to subtask arrays.  Off by default because "
            "tm-core currently emits numeric subtask IDs.  Enable once upstream "
            "normalization lands (partner task to Task 1888)."
        ),
    )
    args = parser.parse_args()

    try:
        with open(args.path, encoding="utf-8") as fh:
            data = json.load(fh)
    except (OSError, json.JSONDecodeError) as exc:
        print(f"ERROR: cannot load {args.path}: {exc}", file=sys.stderr)
        sys.exit(1)

    if not isinstance(data, dict):
        print(
            f"ERROR: schema: top-level JSON must be an object (dict), got {type(data).__name__}",
            file=sys.stderr,
        )
        sys.exit(1)

    errors: list[str] = []
    warnings: list[str] = []
    # Per-tag list of (tag_name, tasks, known_ids) for subtask iteration.
    tag_results: list[tuple[str, list, set]] = []

    for tag_name, tag_value in data.items():
        # Skip known metadata keys that are deliberately not tag namespaces.
        if tag_name.startswith("_"):
            continue
        if not isinstance(tag_value, dict):
            warnings.append(
                f"top-level key {tag_name!r} has no tasks list; skipping"
                f" (value type: {type(tag_value).__name__!r})"
            )
            continue
        tasks_list = tag_value.get("tasks")
        if tasks_list is None:
            warnings.append(
                f"top-level key {tag_name!r} has no tasks list; skipping"
                f" (missing 'tasks' field)"
            )
            continue
        if not isinstance(tasks_list, list):
            warnings.append(
                f"top-level key {tag_name!r} has no tasks list; skipping"
                f" ('tasks' is {type(tasks_list).__name__!r}, expected list)"
            )
            continue
        known_ids = _validate_tasks(tasks_list, errors, context=tag_name)
        tag_results.append((tag_name, tasks_list, known_ids))

    if args.check_subtasks:
        for tag_name, tasks_list, known_ids in tag_results:
            for task in tasks_list:
                subtasks = task.get("subtasks", [])
                if subtasks:
                    parent_id = task.get("id", "?")
                    _validate_subtasks(subtasks, known_ids, parent_id, errors, tag_context=tag_name)

    if errors:
        for err in errors:
            print(err, file=sys.stderr)
        for warn in warnings:
            print(f"WARN: {warn}", file=sys.stderr)
        sys.exit(1)

    for warn in warnings:
        print(f"WARN: {warn}", file=sys.stderr)


def _validate_tasks(tasks: list, errors: list, context: str) -> set:
    """Validate invariants 1-3 for a flat list of tasks.

    Returns the set of known string IDs (for use by subtask validation).
    """
    prefix = f"{context}: " if context else ""

    # Invariant 3: no duplicate IDs.
    # Filter to string ids only: unhashable ids (list/dict) would raise TypeError;
    # non-string ids are reported separately by invariant 1.
    id_counter = collections.Counter(t["id"] for t in tasks if isinstance(t.get("id"), str))
    for id_val, count in id_counter.items():
        if count > 1:
            errors.append(
                f"invariant 3: {prefix}duplicate id {id_val!r} appears {count} times"
            )

    # Invariant 1: every id is a string matching ^\d+$.
    known_ids: set[str] = set()
    for task in tasks:
        tid = task.get("id")
        if not isinstance(tid, str):
            errors.append(
                f"invariant 1 [{prefix}task id={tid!r}]: id is {type(tid).__name__!r}, expected str"
            )
        elif re.fullmatch(r"\d+", tid) is None:
            errors.append(
                f"invariant 1 [{prefix}task id={tid!r}]: id must match ^\\d+$"
            )
        else:
            known_ids.add(tid)

    # Invariant 2: every dep is a string referencing a known id.
    for task in tasks:
        tid = task.get("id", "?")
        deps_raw = task.get("dependencies", [])
        if not isinstance(deps_raw, list):
            errors.append(
                f"invariant 2 [{prefix}task id={tid!r}]: 'dependencies' is {type(deps_raw).__name__!r}, expected list"
            )
            continue
        for dep in deps_raw:
            if not isinstance(dep, str):
                errors.append(
                    f"invariant 2 [{prefix}task id={tid!r}]: dep {dep!r} is {type(dep).__name__!r}, expected str"
                )
            elif dep not in known_ids:
                errors.append(
                    f"invariant 2 [{prefix}task id={tid!r}]: dep {dep!r} is orphan (no matching task id)"
                )

    return known_ids


def _validate_subtasks(
    subtasks: list,
    parent_task_ids: set,
    parent_id: str,
    errors: list,
    *,
    tag_context: str = "",
) -> None:
    """Apply invariants 1-3 to a subtask array (used only with --check-subtasks).

    Subtask deps may reference sibling subtask IDs or parent-task IDs.
    ``tag_context`` is the enclosing tag name (e.g. ``"master"``) and is
    prepended to error messages when set.
    """
    inner = f"subtasks of task {parent_id!r}"
    context = f"{tag_context}: {inner}" if tag_context else inner

    # Invariant 3 within subtasks.
    id_counter = collections.Counter(s["id"] for s in subtasks if "id" in s)
    for id_val, count in id_counter.items():
        if count > 1:
            errors.append(
                f"invariant 3: {context}: duplicate subtask id {id_val!r} appears {count} times"
            )

    # Invariant 1 for subtasks.
    known_subtask_ids: set[str] = set()
    for sub in subtasks:
        sid = sub.get("id")
        if not isinstance(sid, str):
            errors.append(
                f"invariant 1 [{context} id={sid!r}]: id is {type(sid).__name__!r}, expected str"
            )
        elif re.fullmatch(r"\d+", sid) is None:
            errors.append(
                f"invariant 1 [{context} id={sid!r}]: id must match ^\\d+$"
            )
        else:
            known_subtask_ids.add(sid)

    # Invariant 2 for subtasks (deps may be sibling subtask ids or parent task ids).
    allowed_ids = known_subtask_ids | parent_task_ids
    for sub in subtasks:
        sid = sub.get("id", "?")
        deps_raw = sub.get("dependencies", [])
        if not isinstance(deps_raw, list):
            errors.append(
                f"invariant 2 [{context} id={sid!r}]: 'dependencies' is {type(deps_raw).__name__!r}, expected list"
            )
            continue
        for dep in deps_raw:
            if not isinstance(dep, str):
                errors.append(
                    f"invariant 2 [{context} id={sid!r}]: dep {dep!r} is {type(dep).__name__!r}, expected str"
                )
            elif dep not in allowed_ids:
                errors.append(
                    f"invariant 2 [{context} id={sid!r}]: dep {dep!r} is orphan (no matching subtask or task id)"
                )


if __name__ == "__main__":
    main()
