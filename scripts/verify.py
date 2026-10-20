#!/usr/bin/env python3
"""verify.py — entry point of the Python verify package (scripts/reify_verify).

The future target of scripts/verify.sh's exec shim (PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D1). Not wired until
the cutover leaf: scripts/verify.sh remains the gate.

Run as `python3 -I -B scripts/verify.py ...`; -I implies -P, so this file puts
its own directory on sys.path to find the package.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from reify_verify import cli  # noqa: E402  (after sys.path manipulation)

if __name__ == "__main__":
    sys.exit(cli.main(sys.argv[1:]))
