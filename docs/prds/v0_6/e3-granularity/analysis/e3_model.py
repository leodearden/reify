"""
e3_model.py — derives the pre-registered §7 endpoints, censoring and stop rule
from E3 records. Pure: no I/O.

It reads the frozen records e3_sources parses, judges them against the frozen
Protocol, and raises ProtocolViolation whenever the data drifts from the
registration. Imports run downward only, to e3_protocol and e3_stats; the
e3_sources records arrive as arguments.
"""

import dataclasses
import enum
import statistics
from dataclasses import dataclass
from datetime import datetime
from fractions import Fraction
from pathlib import Path

from e3_protocol import Amendment, Arm, ProtocolViolation, format_ids, require
from e3_stats import (
    MannWhitneyResult, bootstrap_interval, exact_mann_whitney_u, min_achievable_p,
)

LABEL_DESCRIPTIVE = "DESCRIPTIVE"
LABEL_CONFIRMATORY = "CONFIRMATORY"
EXCLUDED_FREEZE_WINDOW = "freeze-window"
ENDPOINT_METRICS = ("usd_per_kloc", "attempts_per_task", "attempts_per_kloc",
                    "blocking_esc_per_task", "blocking_esc_per_kloc")
ERAS = ("pre", "post", "straddle")


@dataclass(frozen=True)
class ArmTask:
    """One task attributed to an arm (§7.1). counts_toward_floor is the
    addendum (a)(ii) like-for-like rule, decided once here."""

    id: int
    arm: Arm
    prd_slug: str
    scope_weight: int
    is_coalesced: bool
    is_reused: bool
    counts_toward_floor: bool
    amendment: Amendment | None


def attribute(snapshot, protocol):
    """The arm tasks of snapshot, sorted by id: coarse by the x_e3 tag,
    standard by prd_path within the stamped range, each cross-checked
    against the registration."""
    _check_project_root(snapshot, protocol)
    members = _coarse_members(snapshot, protocol) + _standard_members(snapshot, protocol)
    return tuple(sorted(members, key=lambda task: task.id))


def _check_project_root(snapshot, protocol):
    if snapshot.project_root != protocol.project_root:
        raise ProtocolViolation(
            "snapshot project root", protocol.project_root, snapshot.project_root)


def _coarse_members(snapshot, protocol):
    tagged = [r for r in snapshot.tasks if r.x_e3_arm == Arm.COARSE.value]
    slugs = {prd.slug for prd in protocol.prds_in(Arm.COARSE)}
    unknown = {r.id for r in tagged if r.x_e3_prd not in slugs}
    require(not unknown, "coarse x_e3_prd registered",
            f"x_e3_prd in {sorted(slugs)}", f"tasks {format_ids(unknown)}")
    members = []
    for prd in protocol.prds_in(Arm.COARSE):
        records = [r for r in tagged if r.x_e3_prd == prd.slug]
        registered = prd.coalesced | prd.reused
        found = {r.id for r in records}
        require(found == registered, "coarse membership",
                f"{prd.slug}: {format_ids(registered)}", format_ids(found))
        members += [_coarse_task(record, prd, protocol) for record in records]
    return members


def _coarse_task(record, prd, protocol):
    is_reused = record.id in prd.reused
    require(record.x_e3_reused == is_reused, "coarse reused flag",
            f"#{record.id} x_e3_reused={is_reused}", f"x_e3_reused={record.x_e3_reused}")
    stem = Path(record.prd_path).stem if record.prd_path else None
    require(stem == prd.slug, "coarse prd_path matches x_e3_prd",
            f"#{record.id} prd_path stem {prd.slug}", str(record.prd_path))
    require(is_reused or record.constituents, "coalesced constituents",
            f"#{record.id} lists its x_e3_constituents", "none")
    return ArmTask(
        id=record.id,
        arm=Arm.COARSE,
        prd_slug=prd.slug,
        scope_weight=1 if is_reused else len(record.constituents),
        is_coalesced=not is_reused,
        is_reused=is_reused,
        counts_toward_floor=not is_reused,
        amendment=protocol.amendment_for(record.id),
    )


def _standard_members(snapshot, protocol):
    members = []
    for prd in protocol.prds_in(Arm.STANDARD):
        records = [r for r in snapshot.tasks
                   if r.x_e3_arm is None and r.prd_path == prd.prd_path]
        ids = {r.id for r in records}
        require(not ids - prd.stamped_ids(), "standard stamped range",
                f"{prd.slug}: ids within {prd.stamped_range}",
                format_ids(ids - prd.stamped_ids()))
        require(len(ids) == prd.stamped_count, "standard member count",
                f"{prd.slug}: {prd.stamped_count} leaves", f"{len(ids)} {format_ids(ids)}")
        floor_excluded = prd.gate_leaves | {prd.close_leaf}
        require(floor_excluded <= ids, "standard gate/close leaves present",
                f"{prd.slug}: {format_ids(floor_excluded)}",
                f"missing {format_ids(floor_excluded - ids)}")
        members += [
            ArmTask(
                id=record.id,
                arm=Arm.STANDARD,
                prd_slug=prd.slug,
                scope_weight=1,
                is_coalesced=False,
                is_reused=False,
                counts_toward_floor=record.id not in floor_excluded,
                amendment=protocol.amendment_for(record.id),
            )
            for record in records
        ]
    return members


class AttemptClass(enum.Enum):
    """§7.4: a drain-censored attempt is not rework for either arm."""

    COUNTED = "counted"
    DRAIN_CENSORED = "drain-censored"
    IN_FLIGHT = "in-flight"


def classify_attempt(attempt, protocol):
    """DRAIN_CENSORED when an orchestrator shutdown ended the attempt: a
    cancellation within protocol.drain_window of its run's end, or no
    completion at all in a run that has ended. IN_FLIGHT when there is no
    completion and the run is live. Every other attempt is counted rework."""
    if attempt.ended_at is None and attempt.run_ended_at is None:
        return AttemptClass.IN_FLIGHT
    if attempt.ended_at is None:
        return AttemptClass.DRAIN_CENSORED
    at_run_end = (attempt.run_ended_at is not None
                  and abs(attempt.run_ended_at - attempt.ended_at) <= protocol.drain_window)
    if attempt.outcome == "cancelled" and at_run_end:
        return AttemptClass.DRAIN_CENSORED
    return AttemptClass.COUNTED


def freeze_window_set(arm_tasks, attempts, protocol):
    """The arm tasks with any attempt started before the freeze cutoff
    (addendum (b)2); raises unless it is exactly the pre-registered set."""
    arm_ids = {task.id for task in arm_tasks}
    derived = frozenset(
        a.task_id for a in attempts
        if a.task_id in arm_ids and a.started_at < protocol.freeze_cutoff
    )
    require(derived == protocol.expected_freeze_set, "freeze-window set",
            format_ids(protocol.expected_freeze_set), format_ids(derived))
    return derived


@dataclass(frozen=True)
class Anomaly:
    """A data-integrity finding about one task; reported, never corrected."""

    kind: str
    detail: str


@dataclass(frozen=True)
class TaskLedger:
    """One task's records as they stood at an instant, with no arm judgement:
    landed_at needs status done and a merge by then."""

    task_id: int
    status: str
    landed_at: datetime | None
    merges: tuple
    loc: int
    cost: float
    attempts_counted: int
    attempts_drain_censored: int
    attempts_in_flight: int
    blocking_escalations: int
    attempt_starts: tuple
    anomalies: tuple


def task_ledger(record, runs, merges, as_of, protocol):
    """record's merges, cost, attempts and escalations with every source cut
    at as_of, the one place as_of applies, so a past as_of is a frozen
    snapshot. Cost spans every role and is never censored (§7.4 censors
    rework only)."""
    own_merges = tuple(m for m in merges if m.task_id == record.id)
    merged = tuple(m for m in own_merges if m.merged_at <= as_of)
    landed = record.status == "done" and bool(merged)
    attempts = [_attempt_as_of(a, as_of) for a in runs.attempts
                if a.task_id == record.id and a.started_at <= as_of]
    classes = [classify_attempt(a, protocol) for a in attempts]
    return TaskLedger(
        task_id=record.id,
        status=record.status,
        landed_at=min(m.merged_at for m in merged) if landed else None,
        merges=merged,
        loc=sum(m.loc for m in merged),
        cost=sum(i.cost_usd for i in runs.invocations
                 if i.task_id == record.id and i.started_at <= as_of),
        attempts_counted=classes.count(AttemptClass.COUNTED),
        attempts_drain_censored=classes.count(AttemptClass.DRAIN_CENSORED),
        attempts_in_flight=classes.count(AttemptClass.IN_FLIGHT),
        blocking_escalations=len({e.escalation_id for e in runs.escalations
                                  if e.task_id == record.id and e.severity == "blocking"
                                  and e.created_at <= as_of}),
        attempt_starts=tuple(a.started_at for a in attempts),
        anomalies=_anomalies(record, own_merges),
    )


def carve_out_ledgers(snapshot, runs, merges, as_of, protocol):
    """The §7.1 carve-outs' ledgers, reported descriptively beside the arms."""
    return tuple(
        task_ledger(snapshot.task(task_id), runs, merges, as_of, protocol)
        for task_id in sorted(protocol.carve_outs)
        if snapshot.task(task_id) is not None
    )


def _attempt_as_of(attempt, as_of):
    """The attempt as it stood at as_of: a completion or run end after it had
    not happened yet."""
    ended = attempt.ended_at is not None and attempt.ended_at <= as_of
    run_ended = attempt.run_ended_at is not None and attempt.run_ended_at <= as_of
    return dataclasses.replace(
        attempt,
        ended_at=attempt.ended_at if ended else None,
        outcome=attempt.outcome if ended else None,
        run_ended_at=attempt.run_ended_at if run_ended else None,
    )


@dataclass(frozen=True)
class TaskOutcome:
    """One arm task's ledger as of an instant, with its arm judgement.
    ledger.landed_at is set whenever the task is done with a merge by then,
    even when excluded is set; only an eligible landing enters an endpoint."""

    task: ArmTask
    ledger: TaskLedger
    excluded: str | None
    era: str | None

    @property
    def eligible(self):
        return self.excluded is None

    @property
    def landed_eligible(self):
        return self.eligible and self.ledger.landed_at is not None

    @property
    def right_censored(self):
        return self.eligible and self.ledger.landed_at is None


def task_outcomes(arm_tasks, snapshot, runs, merges, freeze_set, as_of, protocol):
    """Every arm task's outcome as of as_of (each source cut by task_ledger)."""
    return tuple(
        _task_outcome(task, snapshot.task(task.id), runs, merges, freeze_set, as_of, protocol)
        for task in arm_tasks
    )


def _task_outcome(task, record, runs, merges, freeze_set, as_of, protocol):
    ledger = task_ledger(record, runs, merges, as_of, protocol)
    return TaskOutcome(
        task=task,
        ledger=ledger,
        excluded=EXCLUDED_FREEZE_WINDOW if task.id in freeze_set else None,
        era=_era(ledger.attempt_starts, protocol),
    )


def _era_boundary(protocol):
    require(len(protocol.era_events) == 1, "one era boundary",
            "exactly one §7.6 era event to split at", str(len(protocol.era_events)))
    return protocol.era_events[0].at


def _era(starts, protocol):
    """pre / post / straddle by the task's attempt starts against the §7.6
    boundary; None when it was never dispatched."""
    if not starts:
        return None
    boundary = _era_boundary(protocol)
    if all(start < boundary for start in starts):
        return "pre"
    if all(start >= boundary for start in starts):
        return "post"
    return "straddle"


def _anomalies(record, own_merges):
    commits = {m.commit for m in own_merges}
    found = []
    if record.status == "done" and not own_merges:
        found.append(Anomaly("done-without-merge", f"#{record.id} is done with no task merge"))
    if record.status != "done" and own_merges:
        found.append(Anomaly("merged-but-not-done",
                             f"#{record.id} is {record.status} with merges {sorted(commits)}"))
    if own_merges and record.provenance_commit and record.provenance_commit not in commits:
        found.append(Anomaly("provenance-not-a-task-merge",
                             f"#{record.id} done_provenance {record.provenance_commit}"
                             f" not in {sorted(commits)}"))
    return tuple(found)


@dataclass(frozen=True)
class PrdEndpoints:
    """Endpoints 1-3 (§6) for one PRD over its landed eligible tasks, plus the
    addendum (b)1 censoring report. Always descriptive on its own."""

    prd_slug: str
    arm: Arm
    n_tasks: int
    n_landed: int
    n_right_censored: int
    n_freeze_excluded: int
    landed_cost: float
    landed_loc: int
    usd_per_kloc: float | None
    attempts_per_task: float | None
    attempts_per_kloc: float | None
    blocking_esc_per_task: float | None
    blocking_esc_per_kloc: float | None
    landed_scope_share: Fraction | None
    unlanded_attempts: int
    unlanded_blocking_escalations: int
    label: str = LABEL_DESCRIPTIVE


def _ratio(numerator, denominator, scale=1):
    return scale * numerator / denominator if denominator else None


def prd_endpoints(outcomes, protocol, drop=frozenset()):
    """Per-PRD endpoints in protocol order; drop removes tasks from every count."""
    kept = [o for o in outcomes if o.task.id not in drop]
    return tuple(
        _prd_endpoint(prd, [o for o in kept if o.task.prd_slug == prd.slug])
        for prd in protocol.prds
    )


def _pooled(landed):
    """(cost, loc, counted attempts, blocking escalations) summed over landed."""
    ledgers = [o.ledger for o in landed]
    return (sum(ledger.cost for ledger in ledgers), sum(ledger.loc for ledger in ledgers),
            sum(ledger.attempts_counted for ledger in ledgers),
            sum(ledger.blocking_escalations for ledger in ledgers))


def _prd_endpoint(prd, outcomes):
    landed = [o for o in outcomes if o.landed_eligible]
    censored = [o for o in outcomes if o.right_censored]
    cost, loc, attempts, escalations = _pooled(landed)
    scope = sum(o.task.scope_weight for o in landed + censored)
    return PrdEndpoints(
        prd_slug=prd.slug,
        arm=prd.arm,
        n_tasks=len(outcomes),
        n_landed=len(landed),
        n_right_censored=len(censored),
        n_freeze_excluded=sum(1 for o in outcomes if not o.eligible),
        landed_cost=cost,
        landed_loc=loc,
        usd_per_kloc=_ratio(cost, loc, 1000),
        attempts_per_task=_ratio(attempts, len(landed)),
        attempts_per_kloc=_ratio(attempts, loc, 1000),
        blocking_esc_per_task=_ratio(escalations, len(landed)),
        blocking_esc_per_kloc=_ratio(escalations, loc, 1000),
        landed_scope_share=(Fraction(sum(o.task.scope_weight for o in landed), scope)
                            if scope else None),
        unlanded_attempts=sum(o.ledger.attempts_counted for o in censored),
        unlanded_blocking_escalations=sum(o.ledger.blocking_escalations for o in censored),
    )


@dataclass(frozen=True)
class ArmCensoring:
    """Addendum (b)3: the three censoring counts, kept separate, per arm."""

    arm: Arm
    freeze_window_excluded: int
    right_censored: int
    drain_censored_attempts: int


def arm_censoring_counts(outcomes):
    return tuple(
        ArmCensoring(
            arm=arm,
            freeze_window_excluded=sum(1 for o in outcomes
                                       if o.task.arm is arm and not o.eligible),
            right_censored=sum(1 for o in outcomes if o.task.arm is arm and o.right_censored),
            drain_censored_attempts=sum(o.ledger.attempts_drain_censored for o in outcomes
                                        if o.task.arm is arm and o.eligible),
        )
        for arm in Arm
    )


@dataclass(frozen=True)
class PrdFloor:
    """One PRD's qualifying landings (addendum (a)(ii)), in landing order."""

    prd_slug: str
    arm: Arm
    qualifying: tuple
    floor_met: bool
    floor_instant: datetime | None


@dataclass(frozen=True)
class RuleI:
    """Addendum (a)(i): every close leaf done by a merge or cancelled. The
    task store records no cancellation instant, so cancelled close leaves
    leave the rule UNDETERMINED (neither met nor unmet) until an operator
    fixes its instant, no earlier than last_merge, the latest close-leaf
    merge. cancelled stays empty while any close leaf is still open."""

    met: bool
    instant: datetime | None
    cancelled: tuple
    last_merge: datetime | None
    operator_fixed: bool

    @property
    def undetermined(self):
        return bool(self.cancelled) and not self.met


class OperatorInstantRefused(Exception):
    """An operator-fixed instant that the protocol does not admit or that the
    records contradict."""


@dataclass(frozen=True)
class StopRuleProbe:
    """Addendum (a): rule (ii) the k-landing floor in every PRD, rule (i) every
    close leaf terminal; the trigger is the earliest instant of a met rule,
    unless an undetermined rule (i) may have come first. Rule (iii) is a
    human decision and is not probed."""

    per_prd: tuple
    rule_ii_met: bool
    rule_ii_instant: datetime | None
    rule_i: RuleI
    trigger_instant: datetime | None


def stop_rule_probe(outcomes, protocol, operator_rule_i_instant=None):
    """operator_rule_i_instant fixes an undetermined rule (i); passing it in
    any other state raises OperatorInstantRefused."""
    floors = tuple(_prd_floor(prd, outcomes, protocol.landing_floor_k) for prd in protocol.prds)
    rule_ii_met = all(floor.floor_met for floor in floors)
    rule_ii_instant = max(floor.floor_instant for floor in floors) if rule_ii_met else None
    rule_i = _rule_i(outcomes, protocol, operator_rule_i_instant)
    return StopRuleProbe(
        per_prd=floors,
        rule_ii_met=rule_ii_met,
        rule_ii_instant=rule_ii_instant,
        rule_i=rule_i,
        trigger_instant=_trigger_instant(rule_i, rule_ii_instant),
    )


def _trigger_instant(rule_i, rule_ii_instant):
    """The earliest met rule's instant. An undetermined rule (i) is met at
    some unrecorded instant no earlier than its last close-leaf merge, so it
    withholds any trigger it may precede."""
    determined = [instant for instant in (rule_i.instant, rule_ii_instant) if instant is not None]
    if not determined:
        return None
    first = min(determined)
    if rule_i.undetermined and (rule_i.last_merge is None or rule_i.last_merge < first):
        return None
    return first


def _prd_floor(prd, outcomes, k):
    """A qualifying landing is an eligible landing of a task that counts
    toward the floor (ArmTask.counts_toward_floor, the like-for-like rule)."""
    qualifying = sorted(
        ((o.task.id, o.ledger.landed_at) for o in outcomes
         if o.task.prd_slug == prd.slug and o.task.counts_toward_floor and o.landed_eligible),
        key=lambda landing: (landing[1], landing[0]),
    )
    floor_met = len(qualifying) >= k
    return PrdFloor(prd.slug, prd.arm, tuple(qualifying), floor_met,
                    qualifying[k - 1][1] if floor_met else None)


def _rule_i(outcomes, protocol, operator_instant):
    """Rule (i) as of the outcomes' instant: met at the last close-leaf merge
    when every close leaf landed, undetermined when the rest are cancelled
    (unless operator_instant fixes it), unmet while any is open."""
    by_id = {o.task.id: o for o in outcomes}
    missing = {prd.close_leaf for prd in protocol.prds} - set(by_id)
    require(not missing, "close leaves attributed", "every PRD's close leaf among the outcomes",
            f"missing {format_ids(missing)}")
    closes = [by_id[prd.close_leaf].ledger for prd in protocol.prds]
    landed = [ledger.landed_at for ledger in closes if ledger.landed_at is not None]
    cancelled = tuple(ledger.task_id for ledger in closes
                      if ledger.landed_at is None and ledger.status == "cancelled")
    last_merge = max(landed, default=None)
    if len(landed) + len(cancelled) < len(closes):
        _refuse_operator_instant(operator_instant, "a close leaf is still open")
        return RuleI(False, None, (), last_merge, False)
    if not cancelled:
        _refuse_operator_instant(operator_instant, "every close leaf landed, so rule (i)'s "
                                 f"instant is the last close-leaf merge {last_merge.isoformat()}")
        return RuleI(True, last_merge, (), last_merge, False)
    if operator_instant is None:
        return RuleI(False, None, cancelled, last_merge, False)
    if last_merge is not None and operator_instant < last_merge:
        raise OperatorInstantRefused(
            f"rule (i) instant {operator_instant.isoformat()} precedes the last close-leaf merge "
            f"{last_merge.isoformat()}")
    return RuleI(True, operator_instant, cancelled, last_merge, True)


def _refuse_operator_instant(operator_instant, reason):
    if operator_instant is not None:
        raise OperatorInstantRefused(
            f"rule (i) has no undetermined instant to fix: {reason}")


@dataclass(frozen=True)
class Verdict:
    """The §7.3 confirmatory look. label is CONFIRMATORY only when the stop
    rule triggered and the contributing split can reach alpha; rejects_null
    then says which way the one exact test came out."""

    label: str
    reason: str
    split: tuple | None
    min_p: Fraction | None
    test: MannWhitneyResult | None
    trigger_instant: datetime | None
    rejects_null: bool | None


def _arm_values(endpoints, arm, metric):
    return [getattr(e, metric) for e in endpoints
            if e.arm is arm and getattr(e, metric) is not None]


def confirmatory_verdict(probe, prd_endpoints_at_trigger, protocol):
    """The one look, on per-PRD $/kLOC frozen at the trigger instant. The
    only path to CONFIRMATORY: trigger present, then the contributing split's
    minimum achievable p <= alpha (exact), then the exact U test."""
    trigger = probe.trigger_instant
    if trigger is None:
        return Verdict(LABEL_DESCRIPTIVE, _no_trigger_reason(probe.rule_i),
                       None, None, None, None, None)
    coarse = _arm_values(prd_endpoints_at_trigger, Arm.COARSE, "usd_per_kloc")
    standard = _arm_values(prd_endpoints_at_trigger, Arm.STANDARD, "usd_per_kloc")
    split = (len(coarse), len(standard))
    if not coarse or not standard:
        return Verdict(LABEL_DESCRIPTIVE, f"split {split[0]}v{split[1]} at the trigger: an arm "
                       "contributes no $/kLOC value", split, None, None, trigger, None)
    min_p = min_achievable_p(*split)
    if min_p > protocol.alpha:
        return Verdict(LABEL_DESCRIPTIVE, f"split {split[0]}v{split[1]}: minimum achievable p "
                       f"{min_p} > alpha {protocol.alpha}", split, min_p, None, trigger, None)
    test = exact_mann_whitney_u(coarse, standard)
    return Verdict(
        LABEL_CONFIRMATORY,
        f"one-sided exact Mann-Whitney U on per-PRD $/kLOC, split {split[0]}v{split[1]}: "
        f"U={test.u}, p={test.p}, alpha={protocol.alpha}",
        split, min_p, test, trigger, test.p <= protocol.alpha,
    )


def _no_trigger_reason(rule_i):
    if rule_i.undetermined:
        return (f"stop rule (i) undetermined: cancelled close leaf {format_ids(rule_i.cancelled)} "
                "carries no recorded cancellation instant; the one confirmatory look waits for "
                "an operator-fixed rule (i) instant")
    return "stop rule not met: no trigger instant, no confirmatory look"


@dataclass(frozen=True)
class ArmSummary:
    metric: str
    arm: Arm
    n_prds: int
    median: float | None
    interval: tuple | None
    label: str = LABEL_DESCRIPTIVE


@dataclass(frozen=True)
class Descriptives:
    """Arm medians with seeded bootstrap-by-PRD intervals for every endpoint
    metric, and the stratum-B pair side by side (§7.3)."""

    summaries: tuple
    stratum_b: tuple
    label: str = LABEL_DESCRIPTIVE

    def summary(self, metric, arm):
        for summary in self.summaries:
            if summary.metric == metric and summary.arm is arm:
                return summary
        raise KeyError((metric, arm))


def descriptives(prd_endpoints, protocol):
    by_slug = {e.prd_slug: e for e in prd_endpoints}
    return Descriptives(
        summaries=tuple(_arm_summary(prd_endpoints, metric, arm, protocol)
                        for metric in ENDPOINT_METRICS for arm in Arm),
        stratum_b=tuple(by_slug[slug] for slug in protocol.stratum_b_pair),
    )


def _arm_summary(endpoints, metric, arm, protocol):
    values = _arm_values(endpoints, arm, metric)
    return ArmSummary(
        metric=metric,
        arm=arm,
        n_prds=len(values),
        median=statistics.median(values) if values else None,
        interval=bootstrap_interval(
            values, statistics.median, f"{protocol.bootstrap_seed}:{metric}:{arm.value}",
            protocol.bootstrap_resamples, protocol.interval_level),
    )


@dataclass(frozen=True)
class EraCell:
    """§7.6: pooled endpoints of one arm's landed eligible tasks in one era."""

    arm: Arm
    era: str
    n_tasks: int
    usd_per_kloc: float | None
    attempts_per_task: float | None
    blocking_esc_per_task: float | None
    label: str = LABEL_DESCRIPTIVE


def era_split(outcomes):
    return tuple(
        _era_cell(arm, era, [o for o in outcomes
                             if o.task.arm is arm and o.era == era and o.landed_eligible])
        for arm in Arm for era in ERAS
    )


def _era_cell(arm, era, landed):
    cost, loc, attempts, escalations = _pooled(landed)
    return EraCell(arm, era, len(landed), _ratio(cost, loc, 1000),
                   _ratio(attempts, len(landed)), _ratio(escalations, len(landed)))
