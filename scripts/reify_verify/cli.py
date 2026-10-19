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

from reify_verify import knobs, queries
from reify_verify.invocation import Action, Invocation, Query, Scope
from reify_verify.policy import ProfileSelection, UnknownRoleError, parse_role

EX_USAGE = 64
EX_UNAVAILABLE = 69
ERROR = "verify.sh: ERROR — "
PRD = "docs/prds/v0_6/verify-structured-plan-and-role-table.md"

_ACTION_VALUES = frozenset(action.value for action in Action)
GATE_ACTIONS = (Action.TEST, Action.LINT, Action.TYPECHECK, Action.ALL)
CONFIRMABLE_ACTIONS = (Action.TEST, Action.ALL)
VALUE_FLAGS = ("--profile", "--scope", "--test-threads")
SWITCHES = {
    "--narrow": "narrow",
    "--include-infra": "include_infra",
    "--print-plan": "print_plan",
    "--confirm-failed": "confirm_failed",
}
HELP_TOKENS = ("-h", "--help")
_QUERY_FLAGS = frozenset(query.value for query in Query)
REPORTS = {Query.KNOBS: queries.knob_report, Query.ROLE_POLICY: queries.role_policy_report}
POSITIVE_INTEGER = re.compile(r"[1-9][0-9]*", re.ASCII)


def _choices(members) -> str:
    return "|".join(member.value for member in members)


def _option(term: str, first: str, *more: str) -> tuple[str, ...]:
    return (f"  {term:<31}{first}", *(" " * 33 + line for line in more))


USAGE = (
    "verify.sh — unified verification entrypoint for Reify.",
    "",
    "Usage:",
    f"  verify.sh <{_choices(GATE_ACTIONS)}> [options]",
    *(f"  verify.sh {query.value}" for query in Query),
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
        "--print-plan",
        "Print the ordered command list, then exit 0 without",
        "running anything.",
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


def _set_query(raw: dict, query: Query) -> None:
    if raw.get("query", query) is not query:
        raise CliError(
            (f"{ERROR}query already set to '{raw['query'].value}', got '{query.value}'",)
        )
    raw["query"] = query


def _validate(raw: dict, env: dict[str, str]) -> Invocation:
    action = raw.get("action")
    query = raw.get("query")
    if action is None and query is None:
        raise CliError((f"{ERROR}missing action ({_choices(GATE_ACTIONS)})", *USAGE))
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
        print_plan=raw.get("print_plan", False),
        confirm_failed=confirm_failed,
        test_threads=test_threads,
        role=role,
        query=query,
    )


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
    if invocation.query is not None:
        output = REPORTS[invocation.query](env)
        _diagnose(output.warnings)
        _print(output.stdout_lines)
        return 0
    _diagnose(
        (
            f"{ERROR}scripts/verify.py does not run action "
            f"'{invocation.action.value}' yet; scripts/verify.sh remains the gate "
            f"until the S18 cutover ({PRD})",
        )
    )
    return EX_UNAVAILABLE


def _print(lines: tuple[str, ...]) -> None:
    sys.stdout.write("".join(f"{line}\n" for line in lines))


def _diagnose(lines: tuple[str, ...]) -> None:
    for line in lines:
        os.write(2, f"{line}\n".encode("utf-8"))
