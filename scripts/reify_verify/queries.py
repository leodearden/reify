"""queries — the read-only reports verify.py prints instead of running (PRD D11).

Each report is one JSON object per stdout line, every line carrying `"v": 1`.
A report returns its warnings rather than printing them, and cli writes them
to stderr. The knob and role reports de-duplicate theirs; scope_report keeps
every stage's lines, in bash's stage order.
"""

import json
import shutil
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify import knobs, narrowing, policy, scope, selection
from reify_verify.jsondata import to_data
from reify_verify.invocation import Invocation


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


def role_policy_report(env: Mapping[str, str]) -> QueryOutput:
    """Every role's policy and its values resolved under `env` and this host's tools."""
    has_nice = shutil.which("nice", path=env.get("PATH")) is not None
    has_ionice = shutil.which("ionice", path=env.get("PATH")) is not None
    role_lines = []
    warnings = []
    for role in policy.Role:
        resolved, wall_warnings = _resolved(role, env, has_nice, has_ionice)
        role_lines.append(
            {
                "v": 1,
                "kind": "role",
                "role": role.value,
                "policy": to_data(policy.ROLE_TABLE[role]),
                "resolved": resolved,
            }
        )
        warnings.extend(wall_warnings)
    profile_lines = (
        {
            "v": 1,
            "kind": "profile",
            "profile": profile.value,
            "policy": to_data(policy.PROFILE_TABLE[profile]),
        }
        for profile in policy.Profile
    )
    return QueryOutput(
        tuple(map(json.dumps, (*role_lines, *profile_lines))), _first_occurrences(warnings)
    )


def scope_report(invocation: Invocation, env: Mapping[str, str], repo_root: Path) -> QueryOutput:
    """The scope decision, the infra selection and the narrowing of one gate invocation."""
    decision, scope_warnings = scope.decide(invocation, repo_root, env)
    selected, selection_warnings = selection.select_infra(decision, repo_root, env)
    narrowed, narrowing_warnings = narrowing.narrow(decision, invocation.narrow, repo_root, env)
    record = {
        "v": 1,
        "scope": decision.to_json(),
        "narrowing": narrowed.to_json(),
        "infra_selection": selected.to_json(),
    }
    return QueryOutput(
        (json.dumps(record),), (*scope_warnings, *selection_warnings, *narrowing_warnings)
    )


def _resolved(
    role: policy.Role, env: Mapping[str, str], has_nice: bool, has_ionice: bool
) -> tuple[dict, list[str]]:
    role_policy = policy.ROLE_TABLE[role]
    walls = {profile: policy.test_wall(role_policy, profile, env) for profile in policy.Profile}
    prio = policy.resolve_prio(role_policy, role.value, has_nice, has_ionice)
    resolved = {
        "walls": {profile.value: wall.value for profile, wall in walls.items()},
        "prio_prefix": prio.prefix,
        "prio_warning": prio.warning,
        "jobserver_fifo": policy.jobserver_fifo(role_policy, env),
        "heavy_partition": policy.heavy_partition(role_policy, env).value,
    }
    return resolved, [wall.warning for wall in walls.values() if wall.warning]


def _default_json(default: str | knobs.DerivedDefault) -> str | dict:
    if isinstance(default, knobs.DerivedDefault):
        return {"derived": default.description}
    return default


def _first_occurrences(items: list[str]) -> tuple[str, ...]:
    return tuple(dict.fromkeys(items))
