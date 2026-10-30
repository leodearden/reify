"""jsondata — a verify record as JSON data: the one serialiser the reports and the JSON plan share (D7, D11).

to_data maps an Enum to its value, a Knob to its name, a dataclass to its
fields and a tuple to a list, recursively, so every field appears by
construction. tagged=True also names each dataclass under a "type" key, so a
union-typed field (a step's command, a cargo selector) says which record it
holds.
"""

import dataclasses
from enum import Enum

from reify_verify import knobs

TYPE_KEY = "type"


def to_data(value, *, tagged: bool = False):
    if isinstance(value, Enum):
        return value.value
    if isinstance(value, knobs.Knob):
        return value.name
    if dataclasses.is_dataclass(value):
        return record_data(value, tagged=tagged)
    if isinstance(value, tuple):
        return [to_data(item, tagged=tagged) for item in value]
    return value


def record_data(record, *, tagged: bool = False) -> dict:
    """A dataclass record as a JSON object: its fields, and its type under TYPE_KEY when tagged."""
    fields = {
        field.name: to_data(getattr(record, field.name), tagged=tagged)
        for field in dataclasses.fields(record)
    }
    return {TYPE_KEY: type(record).__name__, **fields} if tagged else fields
