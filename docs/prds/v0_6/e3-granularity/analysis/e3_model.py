"""
e3_model.py — derives the pre-registered §7 endpoints, censoring and stop rule
from E3 records. Pure: no I/O.

It reads the frozen records e3_sources parses, judges them against the frozen
Protocol, and raises ProtocolViolation whenever the data drifts from the
registration. Imports run downward only: e3_protocol, e3_sources' records and
e3_stats.
"""

import dataclasses
import enum
from dataclasses import dataclass
from datetime import datetime
from fractions import Fraction
from pathlib import Path

from e3_protocol import Amendment, Arm, ProtocolViolation, format_ids, require

LABEL_DESCRIPTIVE = "DESCRIPTIVE"
LABEL_CONFIRMATORY = "CONFIRMATORY"
EXCLUDED_FREEZE_WINDOW = "freeze-window"


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
class TaskOutcome:
    """One arm task's state as of an instant. landed_at is set whenever the
    task is done with a merge by then, even when excluded is set; only an
    eligible landing enters an endpoint."""

    task: ArmTask
    status: str
    landed_at: datetime | None
    merges: tuple
    loc: int
    cost: float
    attempts_counted: int
    attempts_drain_censored: int
    attempts_in_flight: int
    blocking_escalations: int
    excluded: str | None
    era: str | None
    anomalies: tuple

    @property
    def eligible(self):
        return self.excluded is None

    @property
    def landed_eligible(self):
        return self.eligible and self.landed_at is not None

    @property
    def right_censored(self):
        return self.eligible and self.landed_at is None


def task_outcomes(arm_tasks, snapshot, runs, merges, freeze_set, as_of, protocol):
    """Every arm task's outcome with every source cut at as_of, the one place
    as_of applies, so a past as_of is a frozen snapshot."""
    return tuple(
        _task_outcome(task, snapshot.task(task.id), runs, merges, freeze_set, as_of, protocol)
        for task in arm_tasks
    )


def _task_outcome(task, record, runs, merges, freeze_set, as_of, protocol):
    own_merges = tuple(m for m in merges if m.task_id == task.id)
    merged = tuple(m for m in own_merges if m.merged_at <= as_of)
    landed = record.status == "done" and bool(merged)
    attempts = [_attempt_as_of(a, as_of) for a in runs.attempts
                if a.task_id == task.id and a.started_at <= as_of]
    classes = [classify_attempt(a, protocol) for a in attempts]
    return TaskOutcome(
        task=task,
        status=record.status,
        landed_at=min(m.merged_at for m in merged) if landed else None,
        merges=merged,
        loc=sum(m.loc for m in merged),
        cost=sum(i.cost_usd for i in runs.invocations
                 if i.task_id == task.id and i.started_at <= as_of),
        attempts_counted=classes.count(AttemptClass.COUNTED),
        attempts_drain_censored=classes.count(AttemptClass.DRAIN_CENSORED),
        attempts_in_flight=classes.count(AttemptClass.IN_FLIGHT),
        blocking_escalations=len({e.escalation_id for e in runs.escalations
                                  if e.task_id == task.id and e.severity == "blocking"
                                  and e.created_at <= as_of}),
        excluded=EXCLUDED_FREEZE_WINDOW if task.id in freeze_set else None,
        era=_era([a.started_at for a in attempts], protocol),
        anomalies=_anomalies(record, own_merges),
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


def _prd_endpoint(prd, outcomes):
    landed = [o for o in outcomes if o.landed_eligible]
    censored = [o for o in outcomes if o.right_censored]
    loc = sum(o.loc for o in landed)
    cost = sum(o.cost for o in landed)
    attempts = sum(o.attempts_counted for o in landed)
    escalations = sum(o.blocking_escalations for o in landed)
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
        unlanded_attempts=sum(o.attempts_counted for o in censored),
        unlanded_blocking_escalations=sum(o.blocking_escalations for o in censored),
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
            drain_censored_attempts=sum(o.attempts_drain_censored for o in outcomes
                                        if o.task.arm is arm and o.eligible),
        )
        for arm in Arm
    )
