"""cli — verify's argv grammar, usage text and dispatch (PRD D11).

Tokens are read left to right, as scripts/verify.sh's `while/case` loop reads
them: the first error wins, and -h|--help ends parsing the moment it is
reached. The values are then validated in bash's fixed order. Every message is
byte-identical to bash's, except the one listed divergence (defect 5): a
missing or empty option value exits 64 where bash's ${2:?} exits 1.
"""

import os
import re
import sys
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify import bashcall, build, knobs, queries, render
from reify_verify.invocation import Action, Invocation, PlanFormat, Query, Scope
from reify_verify.policy import ProfileSelection, UnknownRoleError, parse_role

EX_USAGE = 64
EX_UNAVAILABLE = 69
ERROR = "verify.sh: ERROR — "
PRD = "docs/prds/v0_6/verify-structured-plan-and-role-table.md"
REPO_ROOT = Path(__file__).resolve().parents[2]

_ACTION_VALUES = frozenset(action.value for action in Action)
GATE_ACTIONS = (Action.TEST, Action.LINT, Action.TYPECHECK, Action.ALL)
CONFIRMABLE_ACTIONS = (Action.TEST, Action.ALL)
VALUE_FLAGS = ("--profile", "--scope", "--test-threads")
SWITCHES = {
    "--narrow": "narrow",
    "--include-infra": "include_infra",
    "--confirm-failed": "confirm_failed",
}
_PLAN_FORMAT_FLAGS = frozenset(plan_format.value for plan_format in PlanFormat)
HELP_TOKENS = ("-h", "--help")
_QUERY_FLAGS = frozenset(query.value for query in Query)
POSITIVE_INTEGER = re.compile(r"[1-9][0-9]*", re.ASCII)


@dataclass(frozen=True)
class ActionFreeReport:
    """A query that stands alone: no action, and nothing read but the env."""

    report: Callable[[Mapping[str, str]], queries.QueryOutput]


@dataclass(frozen=True)
class ActionReport:
    """A query that reads a gate invocation and runs nothing; it may call a bash library."""

    report: Callable[[Invocation, Mapping[str, str], Path], queries.QueryOutput]


PROJECTIONS = {
    PlanFormat.TEXT: render.render_text,
    PlanFormat.JSON: render.render_json,
}

REPORTS = {
    Query.KNOBS: ActionFreeReport(queries.knob_report),
    Query.ROLE_POLICY: ActionFreeReport(queries.role_policy_report),
    Query.SCOPE: ActionReport(queries.scope_report),
}


def _takes_action(query: Query | None) -> bool:
    return isinstance(REPORTS.get(query), ActionReport)


def _choices(members) -> str:
    return "|".join(member.value for member in members)


def _option(term: str, first: str, *more: str) -> tuple[str, ...]:
    return (f"  {term:<31}{first}", *(" " * 33 + line for line in more))


USAGE = (
    "verify.sh — unified verification entrypoint for Reify.",
    "",
    "Usage:",
    f"  verify.sh <{_choices(GATE_ACTIONS)}> [options]",
    *(f"  verify.sh {query.value}" for query in Query if not _takes_action(query)),
    *(
        f"  verify.sh <{_choices(GATE_ACTIONS)}> {query.value} [options]"
        for query in Query
        if _takes_action(query)
    ),
    "",
    "Options:",
    *_option(
        f"--profile {_choices(ProfileSelection)}",
        "Build profile(s) to test; both = debug, then release.",
        "Default: the role's default_profile (DF_VERIFY_ROLE).",
        "Ignored by lint and typecheck.",
    ),
    *_option(
        f"--scope {_choices(Scope)}",
        "all = everything; staged = the index (git diff --cached);",
        "branch = merge-base(main, HEAD) to the working tree.",
        "Default: all. A force_scope_all role always runs all.",
    ),
    *_option(
        "--narrow",
        "With --scope staged: narrow the test, check and clippy",
        "passes to the affected crates.",
    ),
    *_option("--include-infra", "Also run the static infra checks."),
    *_option(
        PlanFormat.TEXT.value,
        "Print the ordered command list, then exit 0 without",
        "running anything.",
    ),
    *_option(
        PlanFormat.JSON.value,
        "As --print-plan, as one JSON record per line: header,",
        "scope, narrowing, environment, then each typed step.",
    ),
    *_option(
        "--test-threads=N",
        "Cap test-execution parallelism at N (a positive integer).",
    ),
    *_option(
        "--confirm-failed",
        "test and all only: re-run the tests a recording run saw",
        "fail and print each id that still fails, one per line.",
        "Exits 0 (none), 100 (ids printed), 64 (another tree).",
    ),
    *_option(
        Query.KNOBS.value,
        "Print every knob verify reads as one JSON line each:",
        "parser, default, meaning and the value under this env.",
    ),
    *_option(
        Query.ROLE_POLICY.value,
        "Print the role table (one JSON line per role) and the",
        "profile table. A run's role is DF_VERIFY_ROLE (empty: task).",
    ),
    *_option(
        Query.SCOPE.value,
        "With a gate action: print the scope decision, the",
        "narrowing and the infra selection as one JSON line.",
    ),
    *_option("-h|--help", "Show this usage."),
)


class HelpRequested(Exception):
    """-h|--help was reached before any argv error."""


class CliError(Exception):
    def __init__(self, lines: tuple[str, ...], exit_code: int = EX_USAGE):
        super().__init__(lines[0])
        self.lines = lines
        self.exit_code = exit_code


def parse_invocation(argv: list[str], env: dict[str, str]) -> Invocation:
    return _validate(_scan(argv), env)


def _scan(argv: list[str]) -> dict:
    raw: dict = {}
    rest = list(argv)
    while rest:
        token = rest.pop(0)
        flag, has_value, value = token.partition("=")
        if token in _ACTION_VALUES:
            _set_action(raw, Action(token))
        elif token in VALUE_FLAGS:
            if not rest or not rest[0]:
                raise CliError((f"{ERROR}{token} requires a value",))
            raw[token] = rest.pop(0)
        elif has_value and flag in VALUE_FLAGS:
            raw[flag] = value
        elif token in SWITCHES:
            raw[SWITCHES[token]] = True
        elif token in _PLAN_FORMAT_FLAGS:
            _set_plan_format(raw, PlanFormat(token))
        elif token in _QUERY_FLAGS:
            _set_query(raw, Query(token))
        elif token in HELP_TOKENS:
            raise HelpRequested
        else:
            raise CliError((f"{ERROR}unknown argument '{token}'", *USAGE))
    return raw


def _set_action(raw: dict, action: Action) -> None:
    if "action" in raw:
        raise CliError(
            (f"{ERROR}action already set to '{raw['action'].value}', got '{action.value}'",)
        )
    raw["action"] = action


def _set_plan_format(raw: dict, plan_format: PlanFormat) -> None:
    if raw.get("plan_format", plan_format) is not plan_format:
        raise CliError(
            (
                f"{ERROR}plan format already set to '{raw['plan_format'].value}', "
                f"got '{plan_format.value}'",
            )
        )
    raw["plan_format"] = plan_format


def _set_query(raw: dict, query: Query) -> None:
    if raw.get("query", query) is not query:
        raise CliError(
            (f"{ERROR}query already set to '{raw['query'].value}', got '{query.value}'",)
        )
    raw["query"] = query


def _validate(raw: dict, env: dict[str, str]) -> Invocation:
    action = raw.get("action")
    query = raw.get("query")
    if action is None and (query is None or _takes_action(query)):
        raise CliError((f"{ERROR}missing action ({_choices(GATE_ACTIONS)})", *USAGE))
    if _takes_action(query):
        _check_action_query(query, action, raw)
    elif action is not None and query is not None:
        raise CliError(
            (f"{ERROR}query '{query.value}' takes no action (got '{action.value}')",)
        )
    profile_arg = _member(ProfileSelection, raw.get("--profile"), "--profile")
    scope = _member(Scope, raw.get("--scope", Scope.ALL.value), "--scope")
    test_threads = _test_threads(raw.get("--test-threads"))
    confirm_failed = raw.get("confirm_failed", False)
    if confirm_failed:
        _check_confirm_failed(action, env)
    try:
        role = parse_role(env.get("DF_VERIFY_ROLE"))
    except UnknownRoleError as err:
        raise CliError((str(err),)) from None
    return Invocation(
        action=action,
        profile_arg=profile_arg,
        scope=scope,
        narrow=raw.get("narrow", False),
        include_infra=raw.get("include_infra", False),
        plan_format=raw.get("plan_format"),
        confirm_failed=confirm_failed,
        test_threads=test_threads,
        role=role,
        query=query,
    )


def _check_action_query(query: Query, action: Action, raw: dict) -> None:
    """An action query reads a gate invocation and runs nothing."""
    if action not in GATE_ACTIONS:
        raise CliError(
            (
                f"{ERROR}query '{query.value}' takes action {_choices(GATE_ACTIONS)} "
                f"(got '{action.value}')",
            )
        )
    conflicts = (
        raw["plan_format"].value if raw.get("plan_format") else None,
        "--confirm-failed" if raw.get("confirm_failed") else None,
    )
    conflict = next(filter(None, conflicts), None)
    if conflict is not None:
        raise CliError((f"{ERROR}query '{query.value}' does not combine with {conflict}",))


def _member(enum, raw: str | None, flag: str):
    if raw is None:
        return None
    try:
        return enum(raw)
    except ValueError:
        raise CliError(
            (f"{ERROR}invalid {flag} '{raw}' (want {_choices(enum)})",)
        ) from None


def _test_threads(raw: str | None) -> int | None:
    if raw is None:
        return None
    if not POSITIVE_INTEGER.fullmatch(raw):
        raise CliError(
            (f"{ERROR}invalid --test-threads '{raw}' (want positive integer)",)
        )
    return int(raw)


def _check_confirm_failed(action: Action | None, env: dict[str, str]) -> None:
    if action not in CONFIRMABLE_ACTIONS:
        got = action.value if action else ""
        raise CliError(
            (
                f"{ERROR}--confirm-failed is valid only for action "
                f"{_choices(CONFIRMABLE_ACTIONS)} (got '{got}')",
            )
        )
    retry_scope = knobs.resolve(knobs.RETRY_SCOPE, env).value
    if retry_scope:
        raise CliError(
            (
                f"{ERROR}--confirm-failed conflicts with "
                f"{knobs.RETRY_SCOPE.name}='{retry_scope}' (both drive the retry "
                "subset pipeline; refusing rather than picking one silently)",
            )
        )


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(write_through=True, encoding="utf-8")
    env = dict(os.environ)
    try:
        invocation = parse_invocation(argv, env)
    except HelpRequested:
        _print(USAGE)
        return 0
    except CliError as err:
        _diagnose(err.lines)
        return err.exit_code
    return _dispatch(invocation, env)


def _dispatch(invocation: Invocation, env: dict[str, str]) -> int:
    match REPORTS.get(invocation.query):
        case ActionReport(report=report):
            try:
                output = report(invocation, env, REPO_ROOT)
            except bashcall.BashCallError as err:
                _diagnose((f"{ERROR}{err}",))
                return 1
            return _emit(output)
        case ActionFreeReport(report=report):
            return _emit(report(env))
    if invocation.plan_format is not None and not invocation.confirm_failed:
        return _print_plan(invocation, env)
    _diagnose(
        (
            f"{ERROR}scripts/verify.py does not run action "
            f"'{invocation.action.value}' yet; scripts/verify.sh remains the gate "
            f"until the S18 cutover ({PRD})",
        )
    )
    return EX_UNAVAILABLE


def _print_plan(invocation: Invocation, env: dict[str, str]) -> int:
    try:
        outcome = build.plan_invocation(invocation, env, REPO_ROOT)
    except bashcall.BashCallError as err:
        _diagnose((f"{ERROR}{err}",))
        return 1
    match outcome:
        case build.Refused(lines=lines, exit_code=exit_code):
            _diagnose(lines)
            return exit_code
        case build.Planned(plan=plan, diagnostics=diagnostics):
            _diagnose(diagnostics)
            _print(PROJECTIONS[invocation.plan_format](plan))
            return 0


def _emit(output: queries.QueryOutput) -> int:
    _diagnose(output.warnings)
    _print(output.stdout_lines)
    return 0


def _print(lines: tuple[str, ...]) -> None:
    sys.stdout.write("".join(f"{line}\n" for line in lines))


def _diagnose(lines: tuple[str, ...]) -> None:
    for line in lines:
        os.write(2, f"{line}\n".encode("utf-8"))
