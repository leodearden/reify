"""queries — the read-only reports verify.py prints instead of running (PRD D11).

Each report is one JSON object per stdout line, every line carrying `"v": 1`.
A report returns its knob warnings rather than printing them: cli writes them
to stderr, each once.
"""

import json
from collections.abc import Mapping
from dataclasses import dataclass

from reify_verify import knobs


@dataclass(frozen=True)
class QueryOutput:
    stdout_lines: tuple[str, ...]
    warnings: tuple[str, ...]


def knob_report(env: Mapping[str, str]) -> QueryOutput:
    lines = []
    warnings = []
    for knob in knobs.KNOBS:
        resolution = knobs.resolve(knob, env)
        lines.append(
            json.dumps(
                {
                    "v": 1,
                    "name": knob.name,
                    "parser": knob.parser.value,
                    "default": _default_json(knob.default),
                    "meaning": knob.meaning,
                    "value": resolution.value,
                }
            )
        )
        if resolution.warning:
            warnings.append(resolution.warning)
    return QueryOutput(tuple(lines), _first_occurrences(warnings))


def _default_json(default: str | knobs.DerivedDefault) -> str | dict:
    if isinstance(default, knobs.DerivedDefault):
        return {"derived": default.description}
    return default


def _first_occurrences(items: list[str]) -> tuple[str, ...]:
    return tuple(dict.fromkeys(items))
