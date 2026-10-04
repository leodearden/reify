"""task_reference_grammar.py — how free text and node names refer to task numbers.

The grammar scripts/graph-task-conflation-census.py classifies Graphiti facts
with. It re-expresses the measured rules of dark-factory fused-memory's
canonical_labels.py (word-glue lookbehind, '[ \\t]'-padded '#'/':' separators,
letter-start >=3-char project qualifier, single-number node-name anchor),
extended with the run, range and Greek-suffix forms that free-text facts use.

- fact_task_refs: STRICT. Only task-prefixed mentions ('task 12', 'tasks
  4841-4847', 'tasks 4770α, 4771β'), split into local numbers and
  project-qualified ones ('dark_factory:3673', 'DF task 3673').
- mentions_number: BROAD. Any digit-bounded occurrence of a number.
- node_task_numbers: every integer of a Task-N-shaped node name.
- canonical_task_number: the number of a single-number canonical node name.

Stdlib only; every digit class is ASCII '[0-9]' so non-ASCII digits never
parse as task numbers.
"""

import itertools
import re
from dataclasses import dataclass


_NUMBER = "[0-9]+(?![0-9])[A-Za-z\u0370-\u03ff]?"
_RUN_JOINER = (
    "(?:[ \t]*[,/+&\\-\u2013][ \t]*(?:(?:and|or)[ \t]+)?"
    "|[ \t]+(?:and|or|to|through)[ \t]+)"
)
_MENTION_PATTERN = re.compile(
    "(?P<project>(?<![\\w-])(?:dark[ _-]?factory|df)[ \t]+)?"
    "(?<![\\w:-])tasks?(?:\\s+id)?(?:[ \t]*[#:/][ \t]*|\\s+)\\(?"
    f"(?P<run>{_NUMBER}(?:{_RUN_JOINER}#?{_NUMBER})*)",
    re.IGNORECASE,
)
_RANGE_JOINER = re.compile(
    "[A-Za-z\u0370-\u03ff]?[ \t]*(?:-|\u2013|to|through)[ \t]*#?", re.IGNORECASE
)
_MAX_RANGE_SPAN = 100
_QUALIFIED_REF_PATTERN = re.compile(
    r"(?<![\w:/.-])([A-Za-z][A-Za-z0-9_-]{2,})[ \t]*:[ \t]*([0-9]+)(?![0-9])"
)
_TASK_VOCABULARY = frozenset({"task", "tasks"})
_INTEGER = re.compile("[0-9]+")
_TASK_NODE_PREFIX = re.compile(r"\s*tasks?(?![^\W\d])", re.IGNORECASE)
_CANONICAL_TASK_NAME = re.compile(
    r"\s*tasks?(?:[ \t]*[#:][ \t]*|\s+)(?:id\s+)?([0-9]+)\s*", re.IGNORECASE
)


@dataclass(frozen=True)
class TaskRefs:
    """Task numbers a fact names: reify-local ones, and project-qualified ones."""

    local: frozenset[int]
    cross_project: frozenset[int]


def _run_numbers(run: str) -> set[int]:
    matches = list(_INTEGER.finditer(run))
    numbers = {int(m.group()) for m in matches}
    for left, right in itertools.pairwise(matches):
        if _RANGE_JOINER.fullmatch(run[left.end():right.start()]):
            low, high = int(left.group()), int(right.group())
            if 0 < high - low <= _MAX_RANGE_SPAN:
                numbers.update(range(low, high + 1))
    return numbers


def fact_task_refs(text: str) -> TaskRefs:
    """Task numbers named by task-prefixed mentions in `text` (STRICT)."""
    local: set[int] = set()
    cross_project: set[int] = set()
    for mention in _MENTION_PATTERN.finditer(text):
        bucket = cross_project if mention.group("project") else local
        bucket.update(_run_numbers(mention.group("run")))
    for ref in _QUALIFIED_REF_PATTERN.finditer(text):
        if ref.group(1).lower() not in _TASK_VOCABULARY:
            cross_project.add(int(ref.group(2)))
    return TaskRefs(local=frozenset(local), cross_project=frozenset(cross_project))


def mentions_number(text: str, number: int) -> bool:
    """Whether `number` occurs digit-bounded in `text` (BROAD), not as a decimal's head."""
    return re.search(f"(?<![0-9]){number}(?![0-9])(?!\\.[0-9])", text) is not None


def node_task_numbers(name: str) -> frozenset[int]:
    """Every integer in a Task-N-shaped node name; empty for any other name."""
    if not _TASK_NODE_PREFIX.match(name):
        return frozenset()
    return frozenset(int(digits) for digits in _INTEGER.findall(name))


def canonical_task_number(name: str) -> int | None:
    """The number of a single-number canonical task node name ('Task 1997'), else None."""
    match = _CANONICAL_TASK_NAME.fullmatch(name)
    return int(match.group(1)) if match else None
