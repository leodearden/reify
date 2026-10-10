"""
e3_analysis_fixtures.py — hermetic fixture builders for test_e3_analysis.py.

Not a test module (no test_ prefix, so no runner collects it). It builds the
small, self-consistent inputs the E3 analysis suite drives: a mini Protocol, a
fused-memory-shaped task snapshot, a temp runs.db carrying the live column
names, and a temp git repo with fixed committer dates. Everything it writes
goes under a caller-supplied temp directory.

The analysis directory must already be on sys.path (test_e3_analysis.py puts
it there) so e3_protocol imports by name.
"""

from datetime import datetime, timedelta, timezone
from fractions import Fraction

from e3_protocol import Amendment, Arm, ArmPrd, EraEvent, Protocol

UTC = timezone.utc

MINI_PROJECT_ROOT = "/mini/reify"
MINI_FREEZE_CUTOFF = datetime(2026, 8, 28, 7, 7, tzinfo=UTC)
MINI_ERA_BOUNDARY = datetime(2026, 9, 1, tzinfo=UTC)
MINI_CARVE_OUTS = frozenset({150, 151})
MINI_INFRASTRUCTURE = frozenset({900, 901})
MINI_RETIRED = 110


def coarse_slug(index):
    return f"c{index}"


def standard_slug(index):
    return f"s{index}"


def coarse_ids(index):
    """(coalesced, reused, close) of coarse PRD c<index>: N01, N02 and N03."""
    base = 100 * index
    return frozenset({base + 1, base + 2}), frozenset({base + 3}), base + 3


def standard_ids(index):
    """(stamped_range, close, gate) of standard PRD s<index>: a 5-id range."""
    base = 100 * (10 + index)
    return (base + 1, base + 5), base + 5, base + 4


def _coarse_prd(index):
    coalesced, reused, close = coarse_ids(index)
    return ArmPrd(
        slug=coarse_slug(index),
        arm=Arm.COARSE,
        stratum="B" if index == 1 else "A",
        close_leaf=close,
        coalesced=coalesced,
        reused=reused,
        stamped_range=None,
        stamped_count=None,
        gate_leaves=frozenset(),
    )


def _standard_prd(index):
    stamped_range, close, gate = standard_ids(index)
    return ArmPrd(
        slug=standard_slug(index),
        arm=Arm.STANDARD,
        stratum="B" if index == 1 else "A",
        close_leaf=close,
        coalesced=frozenset(),
        reused=frozenset(),
        stamped_range=stamped_range,
        stamped_count=5,
        gate_leaves=frozenset({gate}),
    )


def mini_protocol(n_coarse=2, n_standard=2, **overrides):
    """A small valid Protocol: coarse PRDs c1..cN and standard PRDs s1..sM.

    c1 and s1 form stratum B. The expected freeze set is s1's first leaf, and
    s1's second leaf carries the one amendment. Any Protocol field can be
    overridden; construction re-runs every invariant check.
    """
    first_standard = standard_ids(1)[0][0]
    fields = dict(
        project_root=MINI_PROJECT_ROOT,
        prds=tuple(_coarse_prd(i) for i in range(1, n_coarse + 1))
        + tuple(_standard_prd(j) for j in range(1, n_standard + 1)),
        carve_outs=MINI_CARVE_OUTS,
        infrastructure=MINI_INFRASTRUCTURE,
        freeze_cutoff=MINI_FREEZE_CUTOFF,
        expected_freeze_set=frozenset({first_standard}),
        landing_floor_k=2,
        alpha=Fraction(1, 10),
        drain_window=timedelta(seconds=300),
        era_events=(EraEvent(MINI_ERA_BOUNDARY, "mini era boundary"),),
        amendments=(
            Amendment(first_standard + 1, "2026-09-11", "(c)", "mini scope add"),
        ),
        stratum_b_pair=(coarse_slug(1), standard_slug(1)),
        bootstrap_seed="mini-seed",
        bootstrap_resamples=200,
        interval_level=Fraction(9, 10),
    )
    fields.update(overrides)
    return Protocol(**fields)
