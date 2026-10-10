"""
e3_model.py — derives the pre-registered §7 endpoints, censoring and stop rule
from E3 records. Pure: no I/O.

It reads the frozen records e3_sources parses, judges them against the frozen
Protocol, and raises ProtocolViolation whenever the data drifts from the
registration. Imports run downward only: e3_protocol, e3_sources' records and
e3_stats.
"""

from dataclasses import dataclass
from pathlib import Path

from e3_protocol import Amendment, Arm, ProtocolViolation, format_ids, require


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
