"""
e3_stats.py — the exact statistics E3's §7.3 comparison needs, as pure functions.

exact_mann_whitney_u is the one-sided confirmatory test on per-PRD values:
prediction 1 says coarse is LOWER, so U counts the coarse-standard pairs
ordered against it (a tie scores 1/2), and p is the exact share of the
C(n, n_coarse) equiprobable arrangements whose U is at most the observed one,
found by full enumeration. Every quantity is an exact Fraction so the
inclusive p <= alpha boundary never hinges on float rounding.

bootstrap_interval is the descriptive percentile interval, seeded so reruns
are byte-identical. Stdlib only; no module state.
"""

import math
import random
from dataclasses import dataclass
from fractions import Fraction
from itertools import combinations


@dataclass(frozen=True)
class MannWhitneyResult:
    u: Fraction
    p: Fraction
    arrangements: int
    n_coarse: int
    n_standard: int


def _twice_u(coarse, standard):
    """2U: two per coarse-standard pair with coarse above, one per tie."""
    return sum(
        2 if c > s else 1 if c == s else 0
        for c in coarse
        for s in standard
    )


def exact_mann_whitney_u(coarse, standard):
    coarse, standard = list(coarse), list(standard)
    if not coarse or not standard:
        raise ValueError(
            f"Mann-Whitney U needs two non-empty groups, got {len(coarse)}v{len(standard)}"
        )
    observed = _twice_u(coarse, standard)
    pooled = coarse + standard
    indices = range(len(pooled))
    at_most_observed = 0
    arrangements = 0
    for chosen in combinations(indices, len(coarse)):
        picked = set(chosen)
        group_c = [pooled[i] for i in chosen]
        group_s = [pooled[i] for i in indices if i not in picked]
        arrangements += 1
        if _twice_u(group_c, group_s) <= observed:
            at_most_observed += 1
    return MannWhitneyResult(
        u=Fraction(observed, 2),
        p=Fraction(at_most_observed, arrangements),
        arrangements=arrangements,
        n_coarse=len(coarse),
        n_standard=len(standard),
    )


def min_achievable_p(n_coarse, n_standard):
    return Fraction(1, math.comb(n_coarse + n_standard, n_coarse))


def _nearest_rank(sorted_values, quantile):
    rank = max(1, math.ceil(quantile * len(sorted_values)))
    return sorted_values[rank - 1]


def bootstrap_interval(values, statistic, seed, resamples, level):
    """Percentile interval of statistic over seeded resamples; None if empty."""
    values = list(values)
    if not values:
        return None
    rng = random.Random(seed)
    replicates = sorted(
        statistic([rng.choice(values) for _ in values]) for _ in range(resamples)
    )
    level = Fraction(level)
    return (
        _nearest_rank(replicates, (1 - level) / 2),
        _nearest_rank(replicates, (1 + level) / 2),
    )
