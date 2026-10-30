"""render — the plan's projections; the only producer of plan text (PRD D2, D7, D8).

render_text is `--print-plan`: byte-identical to scripts/verify.sh's print
loop. render_json is `--print-plan=json`: one JSON record per text section
item, each with "v": 1, and each env and step record's `render` the exact text
line render_text prints for it. Every line it returns is printed with a trailing newline; the narrowing
header keeps the closure's text verbatim, so it spans several physical lines
when affected_crates printed one crate per line (PRD §2.4 defect 7).

render_step is the one producer of a step's text. The kind decides the
loader-path scrub: a TOOL step's text starts with it, a CARGO or BUILTIN
step's never does, and the node lane carries it inside its own braces.
"""

import json

from reify_verify.environment import (
    JOBSERVER_VAR,
    CargoEnvMissing,
    CargoEnvSourced,
    EnvLine,
    Exported,
    JobserverUnset,
    UnsetCause,
)
from reify_verify.jsondata import to_data
from reify_verify.plan import (
    ArtifactAssertion,
    Cargo,
    Chain,
    Command,
    Guarded,
    HeadStamp,
    InfraLeafLoop,
    NodeLaneJoin,
    NodeLaneStart,
    NodeProject,
    Packages,
    Plan,
    PlanHeader,
    RetryMarker,
    Script,
    Step,
    StepKind,
    Workspace,
)

ENVIRONMENT_MARKER = (
    "# --- environment (process-level; inherited by every command below EXCEPT where a "
    "command overrides it inline — see the LD_LIBRARY_PATH scrub on non-cargo lines) ---"
)
COMMANDS_MARKER = "# --- commands (executed in order; '&&' semantics — stop on first failure) ---"
EMPTY_PLAN = "# (no commands — nothing to verify for this action/scope)"
LD_SCRUB = 'export LD_LIBRARY_PATH="${REIFY_AMBIENT_LD_LIBRARY_PATH-}"; '
KILL_AFTER = "timeout --kill-after=60"
NODE_BG_PID = "_VERIFY_NODE_BG_PID"
JSON_VERSION = 1


def render_text(plan: Plan) -> tuple[str, ...]:
    return (
        *_retry_marker(plan.retry_marker),
        *_header(plan),
        ENVIRONMENT_MARKER,
        *map(_env_text, plan.environment),
        COMMANDS_MARKER,
        *(() if plan.steps else (EMPTY_PLAN,)),
        *(line for step in plan.steps for line in _step_lines(step)),
    )


def render_json(plan: Plan) -> tuple[str, ...]:
    records = (
        *(("retry_scope", to_data(marker)) for marker in _present(plan.retry_marker)),
        ("header", _header_data(plan.header)),
        ("scope", plan.scope.to_json()),
        ("narrowing", plan.narrowing.to_json()),
        *(("heavy_partition", {"heavy": heavy.value}) for heavy in _present(plan.heavy_partition)),
        *(("env", _env_data(line)) for line in plan.environment),
        *(("step", _step_data(index, step)) for index, step in enumerate(plan.steps)),
    )
    return tuple(json.dumps({"v": JSON_VERSION, "record": name, **data}) for name, data in records)


def _present(value) -> tuple:
    return () if value is None else (value,)


def _header_data(header: PlanHeader) -> dict:
    return {
        "action": header.action.value,
        "profile": header.profile_selection.value,
        "profiles": [profile.value for profile in header.profiles],
        "scope": header.scope.value,
        "include_infra": int(header.include_infra),
        "nextest": int(header.nextest),
        "role": header.role.value,
    }


def _env_data(line: EnvLine) -> dict:
    """name and value, or the cause when a variable is left unset; `render` is the text line."""
    match line:
        case CargoEnvSourced(path=path):
            kind, name, value, cause = "source", None, path, None
        case CargoEnvMissing():
            kind, name, value, cause = "source_missing", None, None, None
        case Exported(name=name, value=value):
            kind, cause = "export", None
        case JobserverUnset(cause=unset_cause):
            kind, name, value, cause = "unset", JOBSERVER_VAR, None, unset_cause.value
        case _:
            raise TypeError(f"_env_data: not an EnvLine: {line!r}")
    return {"kind": kind, "name": name, "value": value, "cause": cause, "render": _env_text(line)}


def _step_data(index: int, step: Step) -> dict:
    return {
        "index": index,
        "kind": step.kind.value,
        "phase": step.phase.value,
        "gate_path": step.gate_path,
        "guard_file": step.guard_file,
        "scrub": step.kind.scrubs,
        "executes": step.kind.execution.value,
        "command": to_data(step.command, tagged=True),
        "annotation": list(step.annotation),
        "render": render_step(step),
    }


def _step_lines(step: Step) -> tuple[str, ...]:
    if step.kind is StepKind.SENTINEL:
        return step.annotation
    return (*step.annotation, render_step(step))


def render_step(step: Step) -> str:
    text = render_command(step.command)
    return f"{LD_SCRUB}{text}" if step.kind.scrubs else text


def render_command(command: Command) -> str:
    match command:
        case Script():
            return _script(command)
        case Cargo():
            return _cargo(command)
        case Chain(parts=parts):
            return " && ".join(render_command(part) for part in parts)
        case Guarded():
            otherwise = f"; else echo '{command.else_warning}'" if command.else_warning else ""
            text = (
                f"if test {command.test_flag} {command.path}; then "
                f"{render_command(command.body)}{otherwise}; fi"
            )
            return f"{text} 2>&1" if command.stderr_to_stdout else text
        case NodeProject():
            return f"if test {command.guard_flag} {command.guard_path}; then {_subshell(command)}; fi"
        case NodeLaneStart(projects=projects):
            lane = " && ".join(render_command(project) for project in projects)
            return (
                f"{{ {LD_SCRUB}{lane} ; }} & {NODE_BG_PID}=$!; "
                f"trap 'if kill \"${NODE_BG_PID}\" 2>/dev/null; then :; fi; _verify_cleanup' EXIT"
            )
        case NodeLaneJoin():
            return f'wait "${NODE_BG_PID}"'
        case HeadStamp(path=path):
            return f"git rev-parse HEAD > {path} 2>/dev/null || true"
        case ArtifactAssertion(manifest=manifest, artifact=artifact, error=error):
            return (
                f"if test -f {manifest} && [ ! -f {artifact} ]; then echo '{error}' >&2; false; fi 2>&1"
            )
        case InfraLeafLoop():
            return _infra_leaf_loop(command)
    raise TypeError(f"render_command: not a Command: {command!r}")


def _script(script: Script) -> str:
    env = "".join(f"{name}={value} " for name, value in script.env)
    timed = f"{KILL_AFTER} {script.wall} " if script.wall else ""
    program = f"bash {script.path}" if script.via_bash else f"./{script.path}"
    args = "".join(f" {arg}" for arg in script.args)
    redirect = " 2>&1" if script.stderr_to_stdout else ""
    return f"{env}{timed}{program}{args}{redirect}"


def _cargo(cargo: Cargo) -> str:
    release = " --release" if cargo.release else ""
    match cargo.selector:
        case Workspace():
            selector = "--workspace"
        case Packages(names=names):
            selector = " ".join(f"-p {name}" for name in names)
    args = "".join(f" {arg}" for arg in cargo.args)
    redirect = " 2>&1" if cargo.stderr_to_stdout else ""
    return (
        f"{KILL_AFTER} {cargo.wall} {cargo.prio}cargo {cargo.subcommand}{release} "
        f"{selector}{args}{redirect}"
    )


def _infra_leaf_loop(leaf: InfraLeafLoop) -> str:
    env = "".join(f"{name}={value} " for name, value in leaf.env)
    unset = "".join(f" -u {name}" for name in leaf.unset)
    return (
        f'( for _vt in {leaf.glob}; do [ -f "$_vt" ] || continue; '
        f'{env}{KILL_AFTER} {leaf.wall} {leaf.scrub_prefix}{unset} bash "$_vt" || exit $?; done )'
    )


def _subshell(project: NodeProject) -> str:
    """bash's wrap_subshell: `bash -c '…'` only when the inner text is a compound."""
    inner = " && ".join(project.commands)
    if "&&" in inner:
        inner = f"bash -c '{inner}'"
    return f"(cd {project.dir} && {KILL_AFTER} {project.minutes}m {inner})"


def _env_text(line: EnvLine) -> str:
    return f"# {render_env_line(line)}"


def render_env_line(line: EnvLine) -> str:
    """bash's ENV_LINES entry; a comment-form entry starts `# `, so it prints as `# # …`."""
    match line:
        case CargoEnvSourced(path=path):
            return f". {path}"
        case CargoEnvMissing():
            return "# ~/.cargo/env not found — relying on ambient PATH for cargo"
        case Exported(name=name, value=value):
            return f"export {name}={value}"
        case JobserverUnset(cause=UnsetCause.OFFLINE):
            return (
                "# CARGO_MAKEFLAGS left unset (offline role — off the merge jobserver, draws "
                "from neither task nor merge FIFO)"
            )
        case JobserverUnset(cause=UnsetCause.STALE, fifo=fifo):
            return (
                f"# CARGO_MAKEFLAGS left unset (stale FIFO {fifo} — jobserver balancer down) — "
                "cargo uses its own job pool"
            )
        case JobserverUnset(cause=UnsetCause.MISSING, fifo=fifo):
            return f"# CARGO_MAKEFLAGS left unset (no {fifo} FIFO) — cargo uses its own job pool"
    raise TypeError(f"render_env_line: not an EnvLine: {line!r}")


def _retry_marker(marker: RetryMarker | None) -> tuple[str, ...]:
    if marker is None:
        return ()
    return (
        f"@@REIFY_RETRY_SCOPE=failed_only@@ nextest_debug={marker.nextest_debug} "
        f"nextest_release={marker.nextest_release} run_all={marker.run_all} gui={marker.gui}",
    )


def _header(plan: Plan) -> tuple[str, ...]:
    header, scope, narrowing = plan.header, plan.scope, plan.narrowing
    lines = [
        f"# verify.sh plan — action={header.action.value} "
        f"profile={header.profile_selection.value} scope={header.scope.value} "
        f"include_infra={int(header.include_infra)} nextest={int(header.nextest)} "
        f"role={header.role.value}",
    ]
    if plan.selective_infra_note:
        lines.append(
            f"# NOTE: include_infra=1 under role={header.role.value} gets the selective "
            "per-artifact infra subset only (scripts/verify-pipeline-infra-tests.txt) — the "
            "wholesale infra pool suite now runs at the merge tier exclusively, not here"
        )
    lines.append(
        f"# scope decision — RUN_RUST={int(scope.run_rust)} RUN_GUI={int(scope.run_gui)} "
        f"RUN_OCCT_GATE={int(scope.run_occt_gate)} "
        f"RUN_GUI_VITEST={int(narrowing.run_gui_vitest)}"
    )
    lines.append(
        f"# narrowing — NARROW_ACTIVE={int(narrowing.narrow_active)} "
        f"affected={narrowing.affected_text} closure={narrowing.closure.text} "
        f"from_diff={int(narrowing.closure.from_diff)}"
    )
    if plan.heavy_partition is not None:
        lines.append(f"# heavy partition — HEAVY={plan.heavy_partition.value}")
    return tuple(lines)
