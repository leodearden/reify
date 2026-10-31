"""Shared loaders and path facts for the scripts/test_prd_decompose_verify*.py partitions.

Each partition puts its own directory on sys.path before importing this module, so every
invocation form resolves it: `python3 scripts/<partition>.py`,
`python3 -m unittest scripts/<partition>.py` from the repo root, and the wrapper's
`unittest discover -s scripts -t scripts`.
"""

import functools
import importlib.util
import os
import sys

_SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
_HARNESS_PATH = os.path.join(_SCRIPTS_DIR, "prd-decompose-verify.py")
REPO_ROOT = os.path.dirname(_SCRIPTS_DIR)
PDV_MJS = os.path.join(_SCRIPTS_DIR, "prd-decompose-verify.mjs")


@functools.cache
def load_pdv():
    """Load prd-decompose-verify.py as `pdv` (the module under test), once per process."""
    pdv_spec = importlib.util.spec_from_file_location("prd_decompose_verify", _HARNESS_PATH)
    pdv = importlib.util.module_from_spec(pdv_spec)
    sys.modules["prd_decompose_verify"] = pdv
    pdv_spec.loader.exec_module(pdv)
    return pdv


@functools.cache
def load_pcc():
    """Return the prd-capability-check.py instance pdv registered in sys.modules."""
    load_pdv()
    return sys.modules["prd_capability_check"]


def reify_missing_fixture_stderr(fixture: str) -> str:
    """reify's own rendering of a missing fixture, which names it (reify-cli main.rs)."""
    return f"Error reading {fixture}: No such file or directory (os error 2)"
