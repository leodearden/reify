"""
verify_ledger_env.py — the one hermetic environment for the infra suites that
run scripts/verify.sh, run_all.sh or scripts/verify-ledger.py under test
(task 7424's verify instrumentation ledger).

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom.

A nesting verify, and run_all.sh, hand every member state that changes what a
run under test does. The role and release-skip knobs change the plan.
REIFY_INFRA_SUITE_ACTIVE drops the infra pool. An inherited REIFY_VERIFY_RUN_ID
marks the run nested, so it records nothing. The ledger, lane, timing,
event-channel and /proc paths point it at the outer run's files. A new ledger
knob belongs in AMBIENT_VERIFY_ENV, and every suite then scrubs it.
"""

import os

AMBIENT_VERIFY_ENV = frozenset(
    {
        "DF_VERIFY_ROLE",
        "REIFY_INFRA_SUITE_ACTIVE",
        "REIFY_RELEASE_DELTA_SKIP",
        "REIFY_VERIFY_RUN_ID",
        "REIFY_RUN_ALL_TIMING_OUT",
        "REIFY_VERIFY_EVENT_LOG",
        "REIFY_VERIFY_LEDGER_DIR",
        "REIFY_VERIFY_LEDGER_LANE_DIR",
        "REIFY_VERIFY_LEDGER_PROC_ROOT",
    }
)


def hermetic_env(**overrides):
    """os.environ without GIT_* or AMBIENT_VERIFY_ENV, then `overrides`."""
    env = {
        k: v
        for k, v in os.environ.items()
        if not k.startswith("GIT_") and k not in AMBIENT_VERIFY_ENV
    }
    env.update(overrides)
    return env
