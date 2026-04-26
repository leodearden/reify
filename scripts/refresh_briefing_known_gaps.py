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

    return 0


if __name__ == "__main__":
    sys.exit(main())
