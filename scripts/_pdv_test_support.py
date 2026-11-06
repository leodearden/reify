"""Shared loaders and path facts for the scripts/test_prd_decompose_verify*.py partitions,
and the one reify-e2e skip policy, which the scripts/test_pcc_*.py partitions share.

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


def require_reify_binary(test, alpha=None):
    """skipTest unless α would run a reify binary with known provenance; return its ctx.

    The one home of the reify-e2e skip policy, for α's partitions and pdv's.
    alpha is the α module instance whose seam answers; default load_pcc().
    Asking α's own ProbeContext.from_environment() keeps the skip and the CLI in
    agreement, and an α subprocess inheriting os.environ reaches the same
    answer.  Three skips:
      * α refused: two unvouched profile binaries (α's exit 64); the refusal
        is the reason, so whichever binary is stale is never probed.
      * a bare name: α's PATH fallback when no profile binary is built, or a
        bare REIFY_BIN.  A PATH reify (a `cargo install`ed one, say) says
        nothing about which source built it, so its verdicts are not this
        checkout's.  A REIFY_BIN path is the caller's explicit choice and runs.
      * the named binary is not there to run.
    """
    alpha = load_pcc() if alpha is None else alpha
    ctx = alpha.ProbeContext.from_environment()
    if ctx.reify_bin is None:
        test.skipTest(f"no trusted reify binary: {ctx.reify_bin_refusal}")
    if not os.path.dirname(ctx.reify_bin):
        test.skipTest(
            f"{ctx.reify_bin!r} is a bare name resolved on PATH, which carries no "
            "provenance; build reify in this checkout or set REIFY_BIN to a path"
        )
    if shutil.which(ctx.reify_bin) is None:
        test.skipTest(f"reify binary {ctx.reify_bin} not built")
    return ctx


def reify_missing_fixture_stderr(fixture: str) -> str:
    """reify's own rendering of a missing fixture, which names it (reify-cli main.rs)."""
    return f"Error reading {fixture}: No such file or directory (os error 2)"
