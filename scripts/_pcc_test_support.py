"""
_pcc_test_support.py — shared, downward support for the scripts/test_pcc_*.py
partitions of the scripts/prd-capability-check.py unittest suite.  It loads the
hyphenated checker by path as `pcc` (the filename is not importable by name),
and holds what more than one partition needs: the repo paths and value-cell
constants, the grammar-e2e skip-guard, the measured cache-denial signature and
the TREE_SITTER_BIN stub factories.  It imports no test file and defines no
TestCase.  Run the suite through tests/infra/test_prd_capability_check.sh, or
one partition at a time with `python3 scripts/test_pcc_<area>.py`.
"""

import functools
import importlib.util
import os
import shlex
import sys
import textwrap
import types
import unittest.mock


# ---------------------------------------------------------------------------
# Module loader — load scripts/prd-capability-check.py into `pcc`
# ---------------------------------------------------------------------------

_SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
_HARNESS_PATH = os.path.join(_SCRIPTS_DIR, "prd-capability-check.py")

_spec = importlib.util.spec_from_file_location("prd_capability_check", _HARNESS_PATH)
pcc = importlib.util.module_from_spec(_spec)
# Register in sys.modules before exec_module so @dataclass and typing annotations
# resolve correctly (they look up cls.__module__ in sys.modules at decoration time).
sys.modules["prd_capability_check"] = pcc
_spec.loader.exec_module(pcc)


# ---------------------------------------------------------------------------
# Locate the repo root for resolving committed probe-set paths
# ---------------------------------------------------------------------------

_REPO_ROOT = os.path.dirname(_SCRIPTS_DIR)
_EXAMPLE_PROBE_SET = os.path.join(_REPO_ROOT, "tests", "prd-gate", "example-probe-set.json")
_VALUE_CELLS_FIXTURE = os.path.join(
    "tests", "prd-gate", "fixtures", "value_clean_eval_cells.ri"
)

# The three-line shape `reify eval` really prints for that fixture, and the
# constraint the committed example holds its first cell to.  Named once here:
# renaming the fixture or rewording a printed cell is then one edit, not five.
_VALUE_CELLS_STDOUT = (
    "ValueCleanEvalCells.damping_ratio = 0.018\n"
    "ValueCleanEvalCells.degenerate_ratio = 0\n"
    "ValueCleanEvalCells.undef_ratio = undef\n"
)

_DAMPING_SPEC = {
    "pattern": r"ValueCleanEvalCells\.damping_ratio = ([-+0-9.eE]+)",
    "min": 0.01,
    "max": 0.03,
}


# ---------------------------------------------------------------------------
# Grammar-e2e skip-guard — can a grammar probe run in this lane?
# ---------------------------------------------------------------------------

# Helper: parser.c path for the skip-guard
_TS_GRAMMAR_PARSER = os.path.join(_REPO_ROOT, "tree-sitter-reify", "src", "parser.c")


def _patch_parser_c(exists: bool):
    """Patch os.path.isfile so parser.c reports `exists`; other paths are real.

    Keeps every case that depends on the generated grammar independent of
    whether this lane happened to generate it, without blinding unrelated
    isfile() calls.  Module-level because both the skip-guard tests and the
    --grammar-substrate-status CLI tests now need it: the CLI checks the same
    file, resolved through pcc._find_repo_root(), which agrees with _REPO_ROOT.
    """
    real_isfile = os.path.isfile

    def fake_isfile(path):
        if path == _TS_GRAMMAR_PARSER:
            return exists
        return real_isfile(path)

    return unittest.mock.patch("os.path.isfile", side_effect=fake_isfile)


# The grammar e2e needs tree-sitter to actually LOAD the reify grammar, which
# isfile(parser.c) does not establish: in a sandboxed agent role the grammar is
# generated but tree-sitter cannot write ~/.cache/tree-sitter/lock/, so the probe
# fails to load the language, observe() classifies it _HARNESS_ERROR, and the
# suite reported a spurious FAIL (exit 70) instead of a clean skip.
#
# Order matters.  isfile() runs first and short-circuits, so a lane that never
# generated the grammar pays no subprocess; only when parser.c exists do we spend
# one probe asking whether it can be loaded.
def _compute_grammar_e2e_status() -> tuple:
    """Return (available, reason) for the grammar e2e.  Never raises.

    The bare `except Exception` is deliberate: the only sane failure mode here is
    "cannot confirm the substrate → skip the one test that needs it".  The
    alternative, measured on this branch, is losing every test in the module.
    It is defence-in-depth, not the fix — the trigger is fixed at source in
    run_probe(), which represents an unlaunchable binary rather than raising.
    """
    if not os.path.isfile(_TS_GRAMMAR_PARSER):
        return (False, f"{_TS_GRAMMAR_PARSER} not found — grammar never generated")
    try:
        usable, reason = pcc.grammar_substrate_usable()
    except Exception as exc:
        return (False, f"the grammar substrate could not be interrogated ({exc!r})")
    return (bool(usable), reason)


@functools.lru_cache(maxsize=1)
def _grammar_e2e_status() -> tuple:
    """Memoized _compute_grammar_e2e_status(), evaluated on FIRST USE.

    Deliberately not a module-level constant: the behavioural half spawns a real
    tree-sitter (on a cold cache, a cc compile of the grammar first), so as an
    import-time constant every importer paid it — including the ~140 hermetic
    tests that need none of it.
    """
    return _compute_grammar_e2e_status()


def _require_grammar_substrate(test) -> None:
    """skipTest unless a grammar probe can run here.  Call from a test body.

    Carries the MEASURED reason into the skip message: an unusable verdict caused
    by tree-sitter exceeding _SUBSTRATE_PROBE_TIMEOUT_S on a loaded machine is a
    coverage hole, not a real gap, and must be tellable from a genuine denial.
    """
    available, reason = _grammar_e2e_status()
    if not available:
        test.skipTest(f"grammar substrate unavailable — {reason}; skip grammar e2e")


_GRAMMAR_STATUS_CALLS_AT_IMPORT: dict[str, int] = {}


def _snapshot_grammar_status_at_import(importer: str) -> None:
    """Record how often _grammar_e2e_status had run when `importer` finished loading.

    The rule: every partition that imports _grammar_e2e_status or
    _require_grammar_substrate calls `_snapshot_grammar_status_at_import(__name__)`
    as the LAST statement before its entry point.  The memo is shared by every
    importer, so a single snapshot sees only the partitions loaded before it;
    one per importer lets TestGrammarAvailabilityGuard pin that NO import spent
    the substrate probe, whatever order unittest loaded them in.
    """
    info = _grammar_e2e_status.cache_info()
    _GRAMMAR_STATUS_CALLS_AT_IMPORT[importer] = info.hits + info.misses


def _grammar_status_calls_at_import() -> dict[str, int]:
    """Every recorded snapshot, keyed by importer — a copy, so readers cannot mutate it."""
    return dict(_GRAMMAR_STATUS_CALLS_AT_IMPORT)


def _grammar_status_importers() -> set[str]:
    """Every loaded module the snapshot rule binds, this one excepted.

    Found by identity in each module's globals rather than by name, so a
    consumer that never recorded its snapshot is caught whatever it is called.
    """
    consumer_api = (_grammar_e2e_status, _require_grammar_substrate)
    this_module = sys.modules[__name__]
    return {
        name
        for name, module in list(sys.modules.items())
        if isinstance(module, types.ModuleType)
        and module is not this_module
        and any(value is api for value in vars(module).values() for api in consumer_api)
    }


# ---------------------------------------------------------------------------
# Measured tree-sitter cache-denial signature
# ---------------------------------------------------------------------------

# The exact stderr tree-sitter 0.26.8 emits when the grammar loads but its
# on-disk lock/cache directory is not writable.  Measured in a sandboxed agent
# role (`touch ~/.cache/tree-sitter/lock/_probe` → EACCES while the directory is
# drwxrwxr-x owned by the invoking user — the denial is a landlock LSM hook, not
# a mode bit, which is precisely why an os.access(..., W_OK) probe cannot see it).
_CACHE_DENIED_STDERR = (
    'Error: Failed to load language for path "/home/leo/src/reify/tests/prd-gate/'
    'fixtures/arrow_type.ri"\n'
    "Caused by: Failed to load language in current directory:\n"
    "Permission denied (os error 13) "
    "(/home/leo/.cache/tree-sitter/lock/reify-69604127a681544d.lock)\n"
)


# ---------------------------------------------------------------------------
# TREE_SITTER_BIN stub factories (module-level so both the grammar_substrate_usable()
# unit tests and the --grammar-substrate-status CLI tests in TestMain share one
# definition of "what a denied / clean / rejecting tree-sitter looks like").
# ---------------------------------------------------------------------------

def _write_ts_stub(tmpdir: str, name: str, body: str) -> str:
    """Write an executable /bin/sh stub at <tmpdir>/<name> and return its path."""
    path = os.path.join(tmpdir, name)
    with open(path, "w") as f:
        f.write("#!/bin/sh\n" + textwrap.dedent(body))
    os.chmod(path, 0o755)
    return path


def _ts_stub_cache_denied(tmpdir: str, name: str = "ts_stub_denied") -> str:
    """Stub reproducing the measured cache-denial: stderr signature, exit 1.

    The stderr text is written to a sibling file and cat'd rather than embedded
    in a heredoc: the measured signature is unindented, so textwrap.dedent would
    find no common prefix, silently leave the stub body indented, and the heredoc
    terminator would never match.
    """
    stderr_file = os.path.join(tmpdir, name + "_stderr.txt")
    with open(stderr_file, "w") as f:
        f.write(_CACHE_DENIED_STDERR)
    return _write_ts_stub(tmpdir, name, f"""\
        cat {shlex.quote(stderr_file)} >&2
        exit 1
    """)


def _ts_stub_parse_error(tmpdir: str, name: str = "ts_stub_parse_error") -> str:
    """Stub reproducing an ordinary grammar rejection: parse error, exit 1."""
    return _write_ts_stub(tmpdir, name, """\
        echo 'x.ri\t0 ms\t(ERROR [0, 0] - [3, 0])' >&2
        exit 1
    """)


def _ts_stub_clean(tmpdir: str, name: str = "ts_stub_clean") -> str:
    """Stub reproducing a clean parse: exit 0, no output."""
    return _write_ts_stub(tmpdir, name, "exit 0\n")


# Sentinel the echo-cache stub prints when XDG_CACHE_HOME is absent from its
# environment.  A distinct marker rather than "" so a test can tell "the child
# saw no cache override" apart from "the child saw an empty one".
_UNSET_CACHE_MARKER = "<unset>"


def _ts_stub_echo_cache(tmpdir: str, name: str = "ts_stub_echo_cache") -> str:
    """Stub that reports the XDG_CACHE_HOME its own process was handed, exit 0.

    The whole point of the grammar-cache isolation contract is what the CHILD
    sees, which no in-process assertion on os.environ can observe — the parent's
    environment is exactly what is NOT supposed to reach the child.  Echoing it
    back through stdout is the only way to measure it.
    """
    return _write_ts_stub(tmpdir, name, f"""\
        echo "XDG_CACHE_HOME=${{XDG_CACHE_HOME:-{_UNSET_CACHE_MARKER}}}"
        exit 0
    """)



def _ts_stub_hangs(tmpdir: str, name: str = "ts_stub_hangs", seconds: int = 5) -> str:
    """Stub that never answers within the test's patched timeout.

    Models a tree-sitter wedged on ~/.cache/tree-sitter/lock/<grammar>.lock.
    Tests pair it with a sub-second _SUBSTRATE_PROBE_TIMEOUT_S, so the sleep only
    has to outlast that bound.
    """
    return _write_ts_stub(tmpdir, name, f"sleep {seconds}\n")


def _ts_stub_forks_grandchild(
    tmpdir: str, pidfile: str, name: str = "ts_stub_forks", seconds: int = 120
) -> str:
    """Stub that backgrounds a long sleeper, records its pid, then wedges itself.

    Models the wedged tree-sitter the substrate bound is sized for: one that had
    already forked `cc` for the cold-cache grammar compile.  The grandchild
    inherits the probe's stdout/stderr pipes, so killing only the direct child
    leaves it running and still holding them.

    `seconds` outlasts any plausible test bound, so a sleeper found alive is a
    real leak rather than a race with its own exit.
    """
    return _write_ts_stub(tmpdir, name, f"""\
        sleep {seconds} &
        echo $! > {shlex.quote(pidfile)}
        sleep {seconds}
    """)


def _ts_stub_not_executable(tmpdir: str, name: str = "ts_stub_not_exec") -> str:
    """Stub that EXISTS on disk but carries no execute bit.

    Deliberately NOT the missing-CLI case: every isfile()/exists() guard upstream
    passes and only the exec fails (EACCES).  Callers assert os.path.isfile() on
    the returned path so it cannot silently degenerate into that case.
    """
    path = _write_ts_stub(tmpdir, name, "exit 0\n")
    os.chmod(path, 0o644)
    return path
