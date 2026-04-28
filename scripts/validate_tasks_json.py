#!/usr/bin/env python3
"""validate_tasks_json.py — Schema-invariant validator for .taskmaster/tasks/tasks.json.

Enforces three structural invariants on top-level tasks to prevent ID/dependency
drift after Task 1866's string-ID normalization migration:

  1. Every task `id` is a string matching ``^\\d+$`` (not an int, not a slug).
  2. Every entry in a task's `dependencies[]` is a string **and** references an
     existing task id (no orphan deps, no int deps).  A dotted form
     ``<parent>.<subtask>`` is also accepted iff the parent is a known top-level
     id AND the subtask id exists under that parent's ``subtasks[]``.
  3. No duplicate `id` values within any tag's ``tasks[]``; tags are independent
     namespaces (so the same id may appear in ``master`` and in a sibling tag).

A fourth invariant (subtask IDs and deps) is also enforced **by default**
(``--check-subtasks``, on since Task 1989).  The upstream serializer guard in
``normalize_tasks_json.py`` coerces numeric subtask ``id`` fields to strings
on every commit; it does **not** touch ``dependencies[]`` entries, but tm-core
emits deps as strings in practice, so enabling both subtask invariants by
default is safe.  Use ``--no-check-subtasks`` as an explicit escape hatch if
upstream ever regresses.

Top-level key convention:
Tag namespaces are top-level keys whose names do **not** start with ``_``.
Any non-tag metadata must be underscore-prefixed (e.g. ``_meta``, a future
``_schemaVersion``) so the validator silently skips it.  This keeps the
validator forward-compatible with new ``tm-core`` metadata keys without
requiring a code change or emitting noisy warnings.

Usage::

    python3 scripts/validate_tasks_json.py .taskmaster/tasks/tasks.json
    python3 scripts/validate_tasks_json.py --no-check-subtasks .taskmaster/tasks/tasks.json

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
        action=argparse.BooleanOptionalAction,
        default=True,
        help=(
            "Apply invariants to subtask arrays (default: on).  The upstream "
            "normalize_tasks_json.py coerces numeric subtask `id` fields to "
            "strings on every commit but does not touch `dependencies[]` "
            "entries; tm-core emits deps as strings in practice, so this guard "
            "is safe to keep enabled.  Use --no-check-subtasks as an escape "
            "hatch if upstream ever regresses."
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
    # Per-tag list of (tag_name, tasks, known_ids, subtasks_by_parent) for subtask iteration.
    tag_results: list[tuple[str, list, set, dict]] = []

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
        known_ids, subtasks_by_parent = _validate_tasks(tasks_list, errors, context=tag_name)
        tag_results.append((tag_name, tasks_list, known_ids, subtasks_by_parent))

    # Require at least one valid tag namespace when tag-like keys exist.
    # A file where every non-metadata key was WARN-skipped (malformed shape)
    # has had zero tasks validated; treat that as a schema error rather than
    # silently exiting 0.
    candidate_keys = [k for k in data if not k.startswith("_")]
    if candidate_keys and not tag_results:
        errors.append(
            "schema: no valid tag namespace found — every top-level key was "
            "skipped as malformed; tasks.json cannot be validated"
        )

    if args.check_subtasks:
        for tag_name, tasks_list, known_ids, subtasks_by_parent in tag_results:
            for task in tasks_list:
                subtasks = task.get("subtasks", [])
                if subtasks:
                    parent_id = task.get("id", "?")
                    _validate_subtasks(
                        subtasks, known_ids, parent_id, errors,
                        tag_context=tag_name,
                        subtasks_by_parent=subtasks_by_parent,
                    )

    if errors:
        for err in errors:
            print(err, file=sys.stderr)
        for warn in warnings:
            print(f"WARN: {warn}", file=sys.stderr)
        sys.exit(1)

    for warn in warnings:
        print(f"WARN: {warn}", file=sys.stderr)


def _validate_tasks(tasks: list, errors: list, context: str) -> tuple[set, dict]:
    """Validate invariants 1-3 for a flat list of tasks.

    Returns a (known_ids, subtasks_by_parent) tuple.  ``known_ids`` is the set
    of valid string IDs; ``subtasks_by_parent`` maps each parent ID to the set
    of valid subtask IDs beneath it.  Both are used by subtask validation to
    resolve dotted ``<parent>.<subtask>`` dependency references.
    """
    prefix = f"{context}: " if context else ""

    # Invariant 3: no duplicate IDs.
    # Exclude unhashable id types (list/dict/set) which would crash Counter;
    # those are already reported by invariant 1. Hashable non-string ids (e.g.
    # integers) are included so duplicate detection remains intact for all types.
    id_counter = collections.Counter(
        t["id"] for t in tasks
        if "id" in t and not isinstance(t["id"], (list, dict, set))
    )
    for id_val, count in id_counter.items():
        if count > 1:
            errors.append(
                f"invariant 3: {prefix}duplicate id {id_val!r} appears {count} times"
            )

    # Invariant 1: every id is a string matching ^\d+$.
    # Also collect each parent's valid subtask ids so invariant 2 can accept
    # dotted ``<parent>.<subtask>`` deps (tm-core legacy/transitional data).
    known_ids: set[str] = set()
    subtasks_by_parent: dict[str, set[str]] = {}
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
            subtasks_by_parent[tid] = {
                s["id"]
                for s in task.get("subtasks") or []
                if isinstance(s, dict)
                and isinstance(s.get("id"), str)
                and re.fullmatch(r"\d+", s["id"]) is not None
            }

    # Invariant 2: every dep is a string referencing a known id.  Dotted
    # ``<parent>.<subtask>`` form is also accepted when both halves resolve.
    dotted_re = re.compile(r"(\d+)\.(\d+)")
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
                continue
            if dep in known_ids:
                continue
            m = dotted_re.fullmatch(dep)
            if m and m.group(1) in known_ids and m.group(2) in subtasks_by_parent.get(m.group(1), set()):
                continue
            errors.append(
                f"invariant 2 [{prefix}task id={tid!r}]: dep {dep!r} is orphan (no matching task id)"
            )

    return known_ids, subtasks_by_parent


def _validate_subtasks(
    subtasks: list,
    parent_task_ids: set,
    parent_id: str,
    errors: list,
    *,
    tag_context: str = "",
    subtasks_by_parent: dict | None = None,
) -> None:
    """Apply invariants 1-3 to a subtask array (used only with --check-subtasks).

    Subtask deps may reference sibling subtask IDs, parent-task IDs, or dotted
    ``<parent>.<subtask>`` references when ``subtasks_by_parent`` is supplied.
    ``tag_context`` is the enclosing tag name (e.g. ``"master"``) and is
    prepended to error messages when set.
    """
    inner = f"subtasks of task {parent_id!r}"
    context = f"{tag_context}: {inner}" if tag_context else inner

    # Invariant 3 within subtasks.
    # Exclude unhashable id types (list/dict/set) which would crash Counter;
    # those are already reported by invariant 1. Hashable non-string ids (e.g.
    # integers) are included so duplicate detection remains intact for all types.
    id_counter = collections.Counter(
        s["id"] for s in subtasks
        if "id" in s and not isinstance(s["id"], (list, dict, set))
    )
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

    # Invariant 2 for subtasks (deps may be sibling subtask ids, parent task ids,
    # or dotted ``<parent>.<subtask>`` references to known subtasks elsewhere).
    allowed_ids = known_subtask_ids | parent_task_ids
    dotted_re = re.compile(r"(\d+)\.(\d+)")
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
                # Also accept dotted <parent>.<subtask> form when subtasks_by_parent
                # is available (passed from the main loop via _validate_tasks output).
                m = dotted_re.fullmatch(dep)
                if (
                    m is not None
                    and subtasks_by_parent is not None
                    and m.group(1) in parent_task_ids
                    and m.group(2) in subtasks_by_parent.get(m.group(1), set())
                ):
                    continue
                errors.append(
                    f"invariant 2 [{context} id={sid!r}]: dep {dep!r} is orphan (no matching subtask or task id)"
                )


if __name__ == "__main__":
    main()
