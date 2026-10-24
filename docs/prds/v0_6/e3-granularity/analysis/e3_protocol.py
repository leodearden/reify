"""
e3_protocol.py — the pre-registered E3 analysis parameters as one frozen value.

Everything the §7 analysis is not allowed to choose after the fact lives here,
once: the arms and strata (§3), the coalesced and reused ids (§5), the
standard arm's stamped ranges and counts (§7.1), the comparison's alpha
(§7.3), the drain window (§7.4), the era events (§7.6), and the 2026-10-10
addendum's landing floor k (a), freeze-window cutoff and expected set (b)2 and
amendment ledger ((c), plus the A5-R and (e) annotations). Protocol document:
docs/prds/v0_6/e3-decomposition-granularity-ab.md.

ProtocolViolation is the single fail-loudly mechanism: construction raises it
when the parameters are infeasible, and every later layer raises it when the
data drifts from the registration. No I/O.
"""

import enum
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from fractions import Fraction

UTC = timezone.utc


class ProtocolViolation(Exception):
    """A pre-registered invariant does not hold; names it and the offending values."""

    def __init__(self, invariant, expected, found):
        super().__init__(invariant, expected, found)
        self.invariant = invariant
        self.expected = expected
        self.found = found

    def __str__(self):
        return f"invariant {self.invariant} violated: expected {self.expected}, found {self.found}"


def format_ids(ids):
    """A set of task ids rendered sorted, so violation messages are stable."""
    return "{" + ", ".join(str(i) for i in sorted(ids)) + "}"


class Arm(enum.Enum):
    COARSE = "coarse"
    STANDARD = "standard"


@dataclass(frozen=True)
class ArmPrd:
    """One randomized PRD. Coarse PRDs list their coalesced and reused tasks;
    standard PRDs list the stamped id range their leaves were filed in."""

    slug: str
    arm: Arm
    stratum: str
    close_leaf: int
    coalesced: frozenset
    reused: frozenset
    stamped_range: tuple | None
    stamped_count: int | None
    gate_leaves: frozenset

    @property
    def prd_path(self):
        return f"docs/prds/v0_6/{self.slug}.md"

    def stamped_ids(self):
        if self.stamped_range is None:
            return frozenset()
        low, high = self.stamped_range
        return frozenset(range(low, high + 1))


@dataclass(frozen=True)
class Amendment:
    task_id: int
    date: str
    source: str
    note: str


@dataclass(frozen=True)
class EraEvent:
    at: datetime
    label: str


@dataclass(frozen=True)
class Protocol:
    project_root: str
    prds: tuple
    carve_outs: frozenset
    infrastructure: frozenset
    freeze_cutoff: datetime
    expected_freeze_set: frozenset
    landing_floor_k: int
    alpha: Fraction
    drain_window: timedelta
    era_events: tuple
    amendments: tuple
    stratum_b_pair: tuple
    bootstrap_seed: str
    bootstrap_resamples: int
    interval_level: Fraction

    def __post_init__(self):
        _check_unique_slugs(self.prds)
        for prd in self.prds:
            _check_prd_shape(prd)
            _check_landing_floor(prd, self.landing_floor_k)
            _check_leaves_registered(prd)
        _check_freeze_set_disjoint(self.expected_freeze_set, self.carve_outs)
        _check_stratum_b_pair(self)

    def prd(self, slug):
        for prd in self.prds:
            if prd.slug == slug:
                return prd
        raise KeyError(slug)

    def prds_in(self, arm):
        return tuple(prd for prd in self.prds if prd.arm is arm)

    def registered_ids(self):
        """Every id the registration names: arm members, stamped ranges, carve-outs."""
        ids = set(self.carve_outs)
        for prd in self.prds:
            ids |= prd.coalesced | prd.reused | prd.stamped_ids()
        return frozenset(ids)

    def snapshot_candidate_ids(self):
        """The task ids a snapshot must read: min..max of the registration plus
        the infrastructure tasks."""
        registered = self.registered_ids()
        span = set(range(min(registered), max(registered) + 1))
        return tuple(sorted(span | self.infrastructure))

    def amendment_for(self, task_id):
        for amendment in self.amendments:
            if amendment.task_id == task_id:
                return amendment
        return None


def require(condition, invariant, expected, found):
    """Raise ProtocolViolation(invariant, expected, found) unless condition holds."""
    if not condition:
        raise ProtocolViolation(invariant, expected, found)


def _check_unique_slugs(prds):
    slugs = [prd.slug for prd in prds]
    require(len(slugs) == len(set(slugs)), "unique PRD slugs",
            "each slug once", ", ".join(sorted(slugs)))


def _check_prd_shape(prd):
    if prd.arm is Arm.COARSE:
        require(prd.stamped_range is None and prd.stamped_count is None,
                "coarse PRD shape", f"{prd.slug}: no stamped range",
                f"{prd.stamped_range} / {prd.stamped_count}")
        return
    require(prd.stamped_range is not None and prd.stamped_count is not None
            and not prd.coalesced and not prd.reused,
            "standard PRD shape", f"{prd.slug}: a stamped range and count, no coarse ids",
            f"range {prd.stamped_range}, count {prd.stamped_count}, "
            f"coarse ids {format_ids(prd.coalesced | prd.reused)}")
    require(prd.stamped_count <= len(prd.stamped_ids()), "standard PRD shape",
            f"{prd.slug}: count <= {len(prd.stamped_ids())} ids in range",
            str(prd.stamped_count))


def _check_landing_floor(prd, k):
    if prd.arm is not Arm.COARSE:
        return
    require(len(prd.coalesced) >= k, "landing-floor feasibility",
            f"{prd.slug}: at least k={k} coalesced tasks",
            f"{len(prd.coalesced)} coalesced {format_ids(prd.coalesced)}")


def _check_leaves_registered(prd):
    if prd.arm is Arm.COARSE:
        require(prd.close_leaf in prd.reused, "coarse close leaf is a reused singleton",
                f"{prd.slug}: close leaf in {format_ids(prd.reused)}", str(prd.close_leaf))
        return
    leaves = prd.gate_leaves | {prd.close_leaf}
    outside = leaves - prd.stamped_ids()
    require(not outside, "standard gate/close leaf in stamped range",
            f"{prd.slug}: leaves within {prd.stamped_range}", format_ids(outside))


def _check_freeze_set_disjoint(expected_freeze_set, carve_outs):
    overlap = expected_freeze_set & carve_outs
    require(not overlap, "freeze set disjoint from carve-outs",
            f"no carve-out among {format_ids(expected_freeze_set)}", format_ids(overlap))


def _check_stratum_b_pair(protocol):
    by_slug = {prd.slug: prd for prd in protocol.prds}
    pair = tuple(by_slug.get(slug) for slug in protocol.stratum_b_pair)
    require(len(pair) == 2 and None not in pair
            and pair[0].arm is Arm.COARSE and pair[1].arm is Arm.STANDARD
            and pair[0].stratum == "B" and pair[1].stratum == "B",
            "stratum-B pair", "(coarse stratum-B PRD, standard stratum-B PRD)",
            str(protocol.stratum_b_pair))


def _range(low, high):
    return frozenset(range(low, high + 1))


E3 = Protocol(
    project_root="/home/leo/src/reify",
    prds=(
        ArmPrd(
            slug="gui-on-demand-measurement", arm=Arm.COARSE, stratum="A",
            close_leaf=6748, coalesced=frozenset({6898, 6899}),
            reused=frozenset({6745, 6748}),
            stamped_range=None, stamped_count=None, gate_leaves=frozenset(),
        ),
        ArmPrd(
            slug="solution-set-completeness", arm=Arm.COARSE, stratum="A",
            close_leaf=6719, coalesced=_range(6900, 6903),
            reused=frozenset({6706, 6719}),
            stamped_range=None, stamped_count=None, gate_leaves=frozenset(),
        ),
        ArmPrd(
            slug="gui-purpose-surface", arm=Arm.COARSE, stratum="A",
            close_leaf=6839, coalesced=frozenset({6904, 6905, 6906}),
            reused=frozenset({6836, 6839}),
            stamped_range=None, stamped_count=None, gate_leaves=frozenset(),
        ),
        ArmPrd(
            slug="spec-conformance-suite", arm=Arm.COARSE, stratum="B",
            close_leaf=6791, coalesced=_range(6907, 6913),
            reused=frozenset({6768, 6789, 6791}),
            stamped_range=None, stamped_count=None, gate_leaves=frozenset(),
        ),
        ArmPrd(
            slug="solver-driver-parity", arm=Arm.STANDARD, stratum="A",
            close_leaf=6704, coalesced=frozenset(), reused=frozenset(),
            stamped_range=(6689, 6704), stamped_count=15,
            gate_leaves=frozenset({
                6693,  # Phase 2 δ, "the flip (integration gate)"
                6700,  # Phase 5 φ, "second integration gate"
            }),
        ),
        ArmPrd(
            slug="geometry-algebra-solver-unification", arm=Arm.STANDARD, stratum="A",
            close_leaf=6687, coalesced=frozenset(), reused=frozenset(),
            stamped_range=(6668, 6687), stamped_count=19,
            gate_leaves=frozenset({
                6677,  # ι, "Integration gate."
            }),
        ),
        ArmPrd(
            slug="driver-contract-implementation", arm=Arm.STANDARD, stratum="B",
            close_leaf=6808, coalesced=frozenset(), reused=frozenset(),
            stamped_range=(6773, 6808), stamped_count=25,
            gate_leaves=frozenset({
                6805,  # χ, "the integration gate for this PRD"
            }),
        ),
    ),
    carve_outs=frozenset({6758, 6759}),
    infrastructure=frozenset({6897, 7029}),
    freeze_cutoff=datetime(2026, 8, 28, 7, 7, tzinfo=UTC),
    expected_freeze_set=frozenset({6668, 6672, 6706, 6768, 6798}),
    landing_floor_k=2,
    alpha=Fraction(1, 10),
    drain_window=timedelta(seconds=300),
    era_events=(
        EraEvent(datetime(2026, 9, 1, tzinfo=UTC),
                 "timeouts.architect 2400->7200, task 7182, merge 9be8de5f, §7.6"),
    ),
    amendments=(
        Amendment(6898, "2026-09-01", "§10 A5-R",
                  "carrier type fixed for the structured measured-value channel"),
        Amendment(6899, "2026-08-28/2026-10-05", "(c)",
                  "dependency #6666 -> #6963; took #6963's GUI build-before-check half"),
        Amendment(6781, "2026-09-11", "(c)",
                  "esc-6781-1 option A: kernel plus post-geometry fold in reify test"),
        Amendment(6911, "2026-09-16", "(c)",
                  "esc-6768-2 option A moved the mnemonic-migration hook into item 2a"),
        Amendment(6683, "2026-09-21", "(c)",
                  "dependency #6583 -> #5436 (+ #5483); two planner guardrails"),
        Amendment(6901, "2026-09-28", "(c)",
                  "esc-5472-2 option 1: maps Partial{InnerSolveUnproven} to a diagnostic"),
        Amendment(6903, "2026-09-28", "(c)",
                  "esc-5472-2 option 1: composes Partial{InnerSolveUnproven}"),
        Amendment(6678, "2026-09-28", "(c)",
                  "esc-5472-2 option 1: owns exact t ~ 3.0 on discrete_mixed.ri"),
        Amendment(6902, "2026-10-08", "(c)",
                  "catalog-membership coordination note (#8355/#8356/#8358)"),
        Amendment(6681, "2026-10-08", "(c)",
                  "catalog-membership coordination note (#8355/#8356/#8358)"),
        Amendment(6690, "2026-10-08", "(c)",
                  "catalog-membership coordination note plus one warm-path test"),
        Amendment(6908, "2026-10-10", "(e)",
                  "writes delta's transitional divergence-baseline rows (esc-6907-3)"),
        Amendment(6909, "2026-10-10", "(e)",
                  "writes eta's rows and the SEEDING_MARKER line (esc-6907-3)"),
    ),
    stratum_b_pair=("spec-conformance-suite", "driver-contract-implementation"),
    bootstrap_seed="72b9c5b08322094677af181a8742a3790c5330da",
    bootstrap_resamples=10000,
    interval_level=Fraction(9, 10),
)
