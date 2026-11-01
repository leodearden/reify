"""nextest — the one nextest-pass emitter and the config file every pass names (PRD D1, §12).

nextest_config_path is the package's only call of gen-nextest-config.sh. The
test region calls it once per plan and hands the value to every pass, so the
file is generated once, and only when a pass will name it.
"""

from collections.abc import Mapping
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

from reify_verify import bashcall, knobs
from reify_verify.bashcall import ScriptCall
from reify_verify.plan import Refused

GEN_NEXTEST_CONFIG = ScriptCall("scripts/gen-nextest-config.sh")
CLI_TEST_THREADS = "REIFY_NEXTEST_CLI_TEST_THREADS"
PLACEHOLDER_NAME = "reify-nextest-occt.<print-plan-placeholder>"


class ConfigOrigin(Enum):
    """Who owns the config file: the caller, nobody (print mode), or the executor's cleanup."""

    OVERRIDE = "override"
    PLACEHOLDER = "placeholder"
    GENERATED = "generated"


@dataclass(frozen=True)
class NextestConfig:
    path: str
    origin: ConfigOrigin


def nextest_config_path(
    executing: bool,
    raw_env: Mapping[str, str],
    planning_env: Mapping[str, str],
    test_threads: int | None,
    repo_root: Path,
) -> NextestConfig | Refused:
    """The --config-file every nextest pass of the plan names.

    The caller's REIFY_VERIFY_NEXTEST_CONFIG wins, read from the raw env as bash
    reads it at load. Print mode names a placeholder and runs nothing. Execute
    mode generates the file under the planning env, the CLI --test-threads
    reaching the generator alone, as bash's per-invocation prefix: an
    exported value would reach every nested infra suite.
    """
    override = knobs.resolve(knobs.NEXTEST_CONFIG, raw_env).value
    if override:
        return NextestConfig(str(override), ConfigOrigin.OVERRIDE)
    if not executing:
        tmpdir = planning_env.get("TMPDIR") or "/tmp"
        return NextestConfig(f"{tmpdir}/{PLACEHOLDER_NAME}", ConfigOrigin.PLACEHOLDER)
    generator_env = {**planning_env, CLI_TEST_THREADS: str(test_threads) if test_threads else ""}
    result = bashcall.run_script(GEN_NEXTEST_CONFIG, (), repo_root, generator_env)
    if result.returncode != 0:
        return Refused(
            (
                *result.stderr_lines,
                f"verify.sh: ERROR — {Path(GEN_NEXTEST_CONFIG.path).name} refused to generate "
                f"the nextest config (exit {result.returncode}); its diagnostic is immediately above",
            ),
            result.returncode,
        )
    return NextestConfig(result.stdout.rstrip("\n"), ConfigOrigin.GENERATED)
