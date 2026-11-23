#!/usr/bin/env python3
"""verify.py — entry point of the Python verify package (scripts/reify_verify).

The future target of scripts/verify.sh's exec shim (PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D1). Not wired until
the cutover leaf: scripts/verify.sh remains the gate.

Run as `python3 -I -B scripts/verify.py ...`; -I implies -P, so this file puts
its own directory on sys.path to find the package.

main is PRD D8's internal-error boundary: an uncaught exception, including a
failed import of the package, prints one line and exits 1; its traceback
precedes the line only under REIFY_VERIFY_DEBUG=1.
"""

import os
import sys
import traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from reify_verify import knobs  # noqa: E402  (after sys.path manipulation)
from reify_verify.diagnostics import internal_error  # noqa: E402


def main(argv: list[str]) -> int:
    try:
        from reify_verify import cli

        return cli.main(argv)
    except Exception as err:
        if knobs.resolve(knobs.DEBUG, os.environ).value:
            traceback.print_exc()
            sys.stderr.flush()
        os.write(2, f"{internal_error(err)}\n".encode("utf-8"))
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
