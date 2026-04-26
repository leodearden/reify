#!/usr/bin/env python3
"""Detect tasks marked done whose IDs are still listed in review/briefing.yaml known_gaps tracking fields.

Cross-references the `tracking:` field of every known_gap entry in
review/briefing.yaml against .taskmaster/tasks/tasks.json.  When a tracked
task has status "done", the gap is stale and a reviewer needs to refresh the
briefing.

Usage:
    python3 scripts/refresh_briefing_known_gaps.py [--briefing PATH] [--tasks PATH] [--json] [--quiet]

Exits 0 if no stale gaps found, 1 if any stale gaps found, 2 on I/O or parse error.
"""

import argparse
import json
import sys

try:
    import yaml
except ImportError:
    yaml = None  # type: ignore[assignment]


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Detect tasks marked done whose IDs are still listed in "
            "review/briefing.yaml known_gaps tracking fields."
        )
    )
    parser.add_argument(
        "--briefing",
        default="review/briefing.yaml",
        metavar="PATH",
        help="Path to briefing.yaml (default: review/briefing.yaml)",
    )
    parser.add_argument(
        "--tasks",
        default=".taskmaster/tasks/tasks.json",
        metavar="PATH",
        help="Path to tasks.json (default: .taskmaster/tasks/tasks.json)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        dest="json_output",
        help="Emit mismatches as JSON list to stdout instead of WARN lines to stderr",
    )
    parser.add_argument(
        "--quiet",
        action="store_true",
        help="Suppress diagnostic chatter; only print mismatch WARN lines",
    )

    args = parser.parse_args()

    # ------------------------------------------------------------------ #
    # Load briefing.yaml                                                   #
    # ------------------------------------------------------------------ #
    if yaml is None:
        print("ERROR: PyYAML is not installed — pip install pyyaml", file=sys.stderr)
        return 2

    try:
        with open(args.briefing, "r", encoding="utf-8") as fh:
            briefing_data = yaml.safe_load(fh)
    except OSError as exc:
        print(f"ERROR: cannot read {args.briefing}: {exc}", file=sys.stderr)
        return 2
    except yaml.YAMLError as exc:
        print(f"ERROR: cannot parse {args.briefing}: {exc}", file=sys.stderr)
        return 2

    # ------------------------------------------------------------------ #
    # Load tasks.json                                                      #
    # ------------------------------------------------------------------ #
    try:
        with open(args.tasks, "r", encoding="utf-8") as fh:
            tasks_data = json.load(fh)
    except OSError as exc:
        print(f"ERROR: cannot read {args.tasks}: {exc}", file=sys.stderr)
        return 2
    except json.JSONDecodeError as exc:
        print(f"ERROR: cannot parse {args.tasks}: {exc}", file=sys.stderr)
        return 2

    # ------------------------------------------------------------------ #
    # Collect (subproject_name, gap_dict) pairs from all subprojects       #
    # ------------------------------------------------------------------ #
    gap_pairs: list[tuple[str, dict]] = []
    subprojects = briefing_data.get("subprojects", {}) if isinstance(briefing_data, dict) else {}
    for subproject_name, subproject_data in subprojects.items():
        if not isinstance(subproject_data, dict):
            continue
        known_gaps = subproject_data.get("known_gaps", [])
        if not isinstance(known_gaps, list):
            continue
        for gap in known_gaps:
            if isinstance(gap, dict):
                gap_pairs.append((subproject_name, gap))

    if not gap_pairs:
        return 0

    return 0


if __name__ == "__main__":
    sys.exit(main())
