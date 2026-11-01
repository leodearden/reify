"""Shared loaders and path facts for the scripts/test_prd_decompose_verify*.py partitions.

Each partition puts its own directory on sys.path before importing this module, so every
invocation form resolves it: `python3 scripts/<partition>.py`,
`python3 -m unittest scripts/<partition>.py` from the repo root, and the wrapper's
`unittest discover -s scripts -t scripts`.
"""

import functools
import importlib.util
import os
import shutil
import sys

_SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
PDV_PY = os.path.join(_SCRIPTS_DIR, "prd-decompose-verify.py")
REPO_ROOT = os.path.dirname(_SCRIPTS_DIR)
PDV_MJS = os.path.join(_SCRIPTS_DIR, "prd-decompose-verify.mjs")


@functools.cache
def load_pdv():
    """Load prd-decompose-verify.py as `pdv` (the module under test), once per process."""
    pdv_spec = importlib.util.spec_from_file_location("prd_decompose_verify", PDV_PY)
    pdv = importlib.util.module_from_spec(pdv_spec)
    sys.modules["prd_decompose_verify"] = pdv
    pdv_spec.loader.exec_module(pdv)
    return pdv


@functools.cache
def load_pcc():
    """Return the prd-capability-check.py instance pdv registered in sys.modules."""
    load_pdv()
    return sys.modules["prd_capability_check"]


def require_reify_binary(test):
    """skipTest unless α would run a reify binary here; return α's ProbeContext.

    The boundary e2e runs α as a subprocess that inherits os.environ, so the
    in-process ProbeContext.from_environment() answer is the one it reaches: with
    two unvouched profile binaries α refuses (exit 64), and the skip carries that
    refusal rather than probing whichever binary is stale.
    """
    ctx = load_pcc().ProbeContext.from_environment()
    if ctx.reify_bin is None:
        test.skipTest(f"no trusted reify binary: {ctx.reify_bin_refusal}")
    if shutil.which(ctx.reify_bin) is None:
        test.skipTest("reify binary not built")
    return ctx


def reify_missing_fixture_stderr(fixture: str) -> str:
    """reify's own rendering of a missing fixture, which names it (reify-cli main.rs)."""
    return f"Error reading {fixture}: No such file or directory (os error 2)"
