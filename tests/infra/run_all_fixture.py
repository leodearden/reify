"""
run_all_fixture.py — the environment for running the real tests/infra/run_all.sh
on a tempdir fixture INFRA_DIR.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom.

The gate runs every infra suite as a run_all.sh member, so a suite inherits the
outer run's REIFY_RUN_ALL_* knobs, and each one would change which path,
members or budgets the fixture run takes. run_all_env drops every one of them,
so a knob added to run_all.sh later needs no edit here. It then points the
fixture run at state of its own: its classification manifest, a private pool
lock and flaky ledger, and the PSI gate off.

run_all.sh's own path is deliberately not here. Each suite binds it, and
names it in the argv it executes, in its own text: test_slot_timeout_marker.sh
Section F derives the roster of suites that run a nested run_all.sh from that
text, and it does not follow imports.
"""

import stat
from pathlib import Path

from verify_ledger_env import hermetic_env


def write_executable(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def run_all_env(root, manifest, **overrides):
    """hermetic_env() without REIFY_RUN_ALL_*, pointed at <root>'s private state, then `overrides`."""
    env = {k: v for k, v in hermetic_env().items() if not k.startswith("REIFY_RUN_ALL_")}
    env.update(
        RUN_ALL_CLASSIFICATION_MANIFEST=str(manifest),
        REIFY_RUN_ALL_POOL_LOCK=str(Path(root) / "pool.lock"),
        REIFY_RUN_ALL_POOL_PSI_DISABLE="1",
        REIFY_RUN_ALL_FLAKY_LEDGER=str(Path(root) / "flaky-ledger.jsonl"),
    )
    env.update(overrides)
    return env
