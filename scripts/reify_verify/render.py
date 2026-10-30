"""render — the plan's projections; the only producer of plan text (PRD D2, D7, D8).

render_text is `--print-plan`: byte-identical to scripts/verify.sh's print
loop. Every line it returns is printed with a trailing newline; the narrowing
header keeps the closure's text verbatim, so it spans several physical lines
when affected_crates printed one crate per line (PRD §2.4 defect 7).
"""

from reify_verify.plan import Plan, RetryMarker

ENVIRONMENT_MARKER = (
    "# --- environment (process-level; inherited by every command below EXCEPT where a "
    "command overrides it inline — see the LD_LIBRARY_PATH scrub on non-cargo lines) ---"
)
COMMANDS_MARKER = "# --- commands (executed in order; '&&' semantics — stop on first failure) ---"
EMPTY_PLAN = "# (no commands — nothing to verify for this action/scope)"


def render_text(plan: Plan) -> tuple[str, ...]:
    return (
        *_retry_marker(plan.retry_marker),
        *_header(plan),
        ENVIRONMENT_MARKER,
        *(f"# {line}" for line in plan.environment),
        COMMANDS_MARKER,
        *(() if plan.steps else (EMPTY_PLAN,)),
    )


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
