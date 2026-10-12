#!/usr/bin/env python3
"""
test_pcc_grammar_substrate.py — the grammar substrate in
scripts/prd-capability-check.py: the e2e skip-guard's pins,
grammar_cache_denied() and the behavioural grammar_substrate_usable() probe.
Shared loader and helpers: scripts/_pcc_test_support.py; runner:
tests/infra/test_prd_capability_check.sh.
"""

import os
import shutil
import signal
import subprocess
import tempfile
import time
import unittest
import unittest.mock

from _pcc_test_support import (
    pcc,
    _CACHE_DENIED_STDERR,
    _compute_grammar_e2e_status,
    _grammar_e2e_status,
    _grammar_status_calls_at_import,
    _patch_parser_c,
    _ts_stub_cache_denied,
    _ts_stub_clean,
    _ts_stub_forks_grandchild,
    _ts_stub_hangs,
    _ts_stub_not_executable,
    _ts_stub_parse_error,
)


# ---------------------------------------------------------------------------
# 5894 step-13 (RED): the skip-guard must never take the suite down with it
# ---------------------------------------------------------------------------

class TestGrammarAvailabilityGuard(unittest.TestCase):
    """Pins _compute_grammar_e2e_status() — the skip-guard's blast radius.

    Shared rationale.  The guard was a pure isfile() and could not raise; making
    it behavioural gave it a way to fail, and any exception escaping it loses the
    whole suite instead of one test — strictly worse than the spurious
    HARNESS_ERROR this task removes.  The house rule
    (tests/infra/test_prd_gate_corpus.sh:52-60) is that a toolchain which cannot
    be interrogated is a clean SKIP, so these hold the guard to it for ANY
    exception.  Tested through the UN-memoized function, so each case observes
    its own patches.
    """

    # Shares _pcc_test_support._patch_parser_c() with TestMain's CLI tests
    # (test_pcc_main.py).

    def test_substrate_probe_exception_degrades_to_unavailable(self):
        """(a) ANY exception from the substrate probe → unavailable, never propagated."""
        with _patch_parser_c(True), \
             unittest.mock.patch.object(
                 pcc, "grammar_substrate_usable",
                 side_effect=RuntimeError("boom")):
            available, reason = _compute_grammar_e2e_status()
        self.assertFalse(
            available,
            "a substrate that cannot be interrogated must degrade to 'skip the "
            "grammar e2e', not take the module down",
        )
        self.assertIn(
            "boom", reason,
            "the swallowed exception must survive into the skip message; a "
            "silent 'unavailable' is the coverage hole this guard trades for",
        )

    def test_usable_substrate_with_parser_c_is_available(self):
        """(b) parser.c present + substrate usable → available.

        The hardening must not silently disable the e2e on a healthy lane; that
        would be a coverage loss disguised as a fix.
        """
        with _patch_parser_c(True), \
             unittest.mock.patch.object(
                 pcc, "grammar_substrate_usable", return_value=(True, "")):
            available, _ = _compute_grammar_e2e_status()
        self.assertTrue(available)

    def test_missing_parser_c_short_circuits_without_probing(self):
        """(c) No parser.c → unavailable, and the substrate is never probed.

        Pins the existing cheap short-circuit: a lane that never generated the
        grammar must pay no subprocess.  Asserted via call_count so a refactor
        cannot quietly reorder the conjunction.
        """
        probe = unittest.mock.Mock(return_value=(True, ""))
        with _patch_parser_c(False), \
             unittest.mock.patch.object(pcc, "grammar_substrate_usable", probe):
            available, reason = _compute_grammar_e2e_status()
        self.assertFalse(available)
        self.assertEqual(
            probe.call_count, 0,
            "isfile(parser.c) must short-circuit before any subprocess is spent",
        )
        self.assertIn("parser.c", reason, "the skip message must name what is missing")

    def test_hermetic_import_spawns_no_substrate_probe(self):
        """The behavioural half is LAZY — importing a consumer must not probe.

        Every partition that consumes _grammar_e2e_status or
        _require_grammar_substrate records its own snapshot as the last
        statement of its module body, so the pin is on the import of each of
        them, in whatever order unittest loaded them, and survives whatever
        tests have run by now.
        """
        snapshots = _grammar_status_calls_at_import()
        self.assertIn(
            __name__, snapshots,
            "this module's own tail snapshot is missing, so the pin is vacuous",
        )
        probed = {name: calls for name, calls in snapshots.items() if calls}
        self.assertEqual(
            probed, {},
            "_grammar_e2e_status() must not be evaluated at import time; only "
            "the grammar e2e should ever spend the subprocess (evaluated by the "
            f"time these importers finished loading: {sorted(probed)})",
        )


def _pid_is_alive(pid: int) -> bool:
    """True while `pid` names a live (non-zombie) process.

    Reads /proc rather than os.kill(pid, 0): a killed grandchild is reparented
    and can sit as a zombie until its new parent reaps it, which os.kill reports
    identically to a running process.
    """
    try:
        with open(f"/proc/{pid}/stat") as fh:
            # The comm field is parenthesised and may itself contain spaces, so
            # split after the LAST ')': state is the field right after it.
            state = fh.read().rsplit(")", 1)[1].split()[0]
    except (OSError, IndexError):
        return False
    return state != "Z"


def _kill_pid_quietly(pid: int) -> None:
    """Best-effort SIGKILL, so a failing test cannot leak its sleeper."""
    try:
        os.kill(pid, signal.SIGKILL)
    except OSError:
        pass


def _wait_until(predicate, timeout_s: float) -> bool:
    """Poll `predicate` until true or `timeout_s` elapses; return its last value.

    Process death is asynchronous — the signal is delivered and the parent reaps
    at their own pace — so the alternative to polling is a sleep long enough to
    be slow and still short enough to be flaky.
    """
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(0.02)
    return bool(predicate())


# ---------------------------------------------------------------------------
# 5894 step-1 (RED): grammar_cache_denied() — narrow cache-denial predicate
# ---------------------------------------------------------------------------

def _probe_run(exit_code, stderr, stdout=""):
    """A hermetic ProbeRun for the grammar_cache_denied() tests."""
    return pcc.ProbeRun(exit_code=exit_code, stdout=stdout, stderr=stderr)


def _load_failure_stderr(tail):
    """tree-sitter 0.26.8's load-failure layout with `tail` as the root cause."""
    return (
        'Error: Failed to load language for path "x.ri"\n'
        "\n"
        "Caused by:\n"
        "    Failed to load language in current directory:\n"
        f"    {tail}\n"
    )


class TestGrammarCacheDenied(unittest.TestCase):
    """Pins pcc.grammar_cache_denied(run) -> bool.  Hermetic: no subprocess.

    Shared rationale for every case below.  The predicate is deliberately NARROW,
    requiring a grammar *load* failure AND a permission denial simultaneously.
    The negatives are the load-bearing half: they prove it can absorb neither a
    genuine grammar regression (a *parse* error, exit 1 with no load failure) nor
    a non-permission load failure.  Both must keep reaching _HARNESS_ERROR/exit
    70 — the skip this predicate authorizes happens in the CALLERS, never in the
    probe verdict.
    """

    # ── positives ─────────────────────────────────────────────────────────────

    def test_measured_sandbox_signature_is_denied(self):
        """(a) The verbatim measured sandbox stderr → True."""
        self.assertTrue(
            pcc.grammar_cache_denied(_probe_run(1, _CACHE_DENIED_STDERR)),
            "the measured sandbox cache-denial signature must be recognised",
        )

    def test_os_error_13_spelling_alone_is_denied(self):
        """(b) A denial spelled only 'os error 13' (no 'Permission denied') → True.

        tree-sitter surfaces the errno either way depending on which layer
        reports it; keying on only one spelling would make the guard flaky.
        """
        stderr = (
            "Error: Failed to load language for path \"x.ri\"\n"
            "Caused by: os error 13\n"
        )
        self.assertTrue(pcc.grammar_cache_denied(_probe_run(1, stderr)))

    # ── negatives (the narrowness guards) ─────────────────────────────────────

    def test_plain_parse_failure_is_not_denied(self):
        """(c) A real grammar rejection (exit 1, no load failure) → False.

        The ABSENT path.  Swallowing it would silently green a probe that
        legitimately FAILs — the risk this predicate is narrowed against.
        """
        stderr = "x.ri\t0 ms\t(ERROR [0, 0] - [3, 0])\n"
        self.assertFalse(pcc.grammar_cache_denied(_probe_run(1, stderr)))

    def test_successful_run_is_not_denied(self):
        """(d) exit 0 with empty stderr → False."""
        self.assertFalse(pcc.grammar_cache_denied(_probe_run(0, "")))

    def test_load_failure_without_permission_indicator_is_not_denied(self):
        """(e) A load failure with NO permission indicator → False.

        A broken or absent grammar must still reach HARNESS_ERROR (70).  Fails
        first if the predicate is widened to 'any load failure'.
        """
        stderr = (
            "Error: Failed to load language for path \"x.ri\"\n"
            "Caused by: No language found for path\n"
        )
        self.assertFalse(pcc.grammar_cache_denied(_probe_run(1, stderr)))

    def test_permission_denial_without_load_failure_is_not_denied(self):
        """A permission error unrelated to grammar loading → False.

        Both halves are required.  An EACCES reading the *fixture* is a real
        harness error the operator needs to see, not a grammar-substrate skip.
        """
        stderr = "Error: Permission denied (os error 13) (x.ri)\n"
        self.assertFalse(pcc.grammar_cache_denied(_probe_run(1, stderr)))

    # ── the two classifiers must agree about a load failure ───────────────────

    def test_denied_run_is_also_a_harness_error_to_observe(self):
        """Anything this predicate calls a denial must also be HARNESS_ERROR.

        Stated over behaviour rather than over which constant a branch reads.
        Were the two classifiers to disagree, a harness error would be promoted
        into a real PASS/FAIL verdict — the mis-attribution this task removes.
        """
        for stderr in (
            _CACHE_DENIED_STDERR,
            "Error: Failed to load language in current directory:\n"
            "Permission denied (os error 13) (~/.cache/tree-sitter/lock/x.lock)\n",
        ):
            with self.subTest(stderr=stderr[:40]):
                run = _probe_run(1, stderr)
                self.assertTrue(
                    pcc.grammar_cache_denied(run),
                    "precondition: this is a recognised cache denial",
                )
                self.assertEqual(
                    pcc.observe("grammar", run, {}), pcc._HARNESS_ERROR,
                    "a run classified as a cache denial must never reach a "
                    "PRESENT/ABSENT observation",
                )


class TestGrammarCacheDenialAnchoring(unittest.TestCase):
    """The denial is matched whole, and only at or after the load-failure marker.

    The errno half is the #7257 numeric-prefix defect in α's sibling predicate;
    the position half keeps a missing grammar loud when an unrelated EACCES
    happens to precede its load failure.
    """

    HOSTILE_ERRNOS = (
        ("EOWNERDEAD", "Owner died (os error 130)"),
        ("ENOTRECOVERABLE", "State not recoverable (os error 131)"),
        ("ERFKILL", "Operation not possible due to RF-kill (os error 132)"),
        ("EHWPOISON", "Memory page has hardware error (os error 133)"),
    )

    def test_thirteen_x_errnos_are_not_denials(self):
        for label, tail in self.HOSTILE_ERRNOS:
            with self.subTest(errno=label):
                run = _probe_run(1, _load_failure_stderr(tail))
                self.assertFalse(pcc.grammar_cache_denied(run))
                self.assertEqual(
                    pcc.observe("grammar", run, {}), pcc._HARNESS_ERROR
                )

    def test_measured_tail_is_denied(self):
        tail = "Permission denied (os error 13) (/tmp/cache/tree-sitter/lib)"
        run = _probe_run(1, _load_failure_stderr(tail))
        self.assertTrue(pcc.grammar_cache_denied(run))

    def test_bare_end_of_string_errno_is_denied(self):
        run = _probe_run(1, _load_failure_stderr("os error 13").rstrip("\n"))
        self.assertTrue(pcc.grammar_cache_denied(run))

    def test_denial_reported_before_an_unrelated_load_failure_is_not_denied(self):
        """The load failure is a missing grammar, which must stay loud.

        Compare test (e) in TestGrammarCacheDenied: an EACCES on some other
        file that merely precedes it must not authorize a skip.
        """
        stderr = (
            "warning: could not write /home/u/.config/tree-sitter/config.json: "
            "Permission denied (os error 13)\n"
            'Error: Failed to load language for path "x.ri"\n'
            "\n"
            "Caused by:\n"
            "    No language found\n"
        )
        run = _probe_run(1, stderr)
        self.assertFalse(pcc.grammar_cache_denied(run))
        self.assertEqual(pcc.observe("grammar", run, {}), pcc._HARNESS_ERROR)


# ---------------------------------------------------------------------------
# 5894 step-3 (RED): grammar_substrate_usable() — behavioural substrate probe
# ---------------------------------------------------------------------------

class TestGrammarSubstrateUsable(unittest.TestCase):
    """Pins pcc.grammar_substrate_usable() -> (bool, reason).

    Shared rationale.  Every case is driven by a TREE_SITTER_BIN stub, never by
    ambient sandbox state — load-bearing, not tidiness: whether
    ~/.cache/tree-sitter/lock is writable depends on which agent role runs the
    suite, so an ambient-conditioned test would flip RED/GREEN by role and could
    not be turned green by the role that has to fix it.
    """

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="prd_gate_substrate_test_")

    def tearDown(self):
        shutil.rmtree(self._tmpdir, ignore_errors=True)

    def _cache_denied_stub(self):
        return _ts_stub_cache_denied(self._tmpdir)

    def _parse_error_stub(self):
        return _ts_stub_parse_error(self._tmpdir)

    def _clean_stub(self):
        return _ts_stub_clean(self._tmpdir)

    def _not_executable_stub(self):
        return _ts_stub_not_executable(self._tmpdir)

    @staticmethod
    def _call(stub):
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            return pcc.grammar_substrate_usable()

    # ── (a) cache denial → unusable, with an actionable reason ────────────────

    def test_cache_denial_is_unusable_with_reason(self):
        """Cache-denial stderr → (False, reason) naming the tree-sitter cache."""
        usable, reason = self._call(self._cache_denied_stub())
        self.assertFalse(usable, "a denied grammar cache is an unusable substrate")
        self.assertTrue(reason, "an unusable substrate must explain itself")
        # The reason is printed verbatim as a SKIP line, so it has to name the
        # subsystem — "permission denied" alone is what made the original
        # failure cost several probes to attribute.
        lowered = reason.lower()
        self.assertIn("tree-sitter", lowered)
        self.assertIn("cache", lowered)

    # ── (b) a parse error is NOT a broken substrate ───────────────────────────

    def test_parse_error_is_usable(self):
        """An ordinary parse rejection → (True, "").

        Reading an unparseable fixture as a broken substrate would skip the e2e
        precisely when the grammar works — the inverse of the bug being fixed.
        """
        self.assertEqual(self._call(self._parse_error_stub()), (True, ""))

    # ── (c) clean run → usable ────────────────────────────────────────────────

    def test_clean_run_is_usable(self):
        """exit 0 → (True, "")."""
        self.assertEqual(self._call(self._clean_stub()), (True, ""))

    # ── (d) missing CLI → unusable ────────────────────────────────────────────

    def test_missing_cli_is_unusable_with_reason(self):
        """TREE_SITTER_BIN pointing at nothing → (False, reason) naming the CLI.

        run_probe() turns the FileNotFoundError into _BINARY_NOT_FOUND_SENTINEL,
        so this arrives through the same channel as the denial and must be
        distinguished by the reason text, not by the boolean.
        """
        missing = os.path.join(self._tmpdir, "definitely-not-here")
        self.assertFalse(os.path.exists(missing), "precondition: stub must not exist")
        usable, reason = self._call(missing)
        self.assertFalse(usable, "a missing tree-sitter CLI is an unusable substrate")
        self.assertTrue(reason, "an unusable substrate must explain itself")
        self.assertIn("tree-sitter", reason.lower())

    def test_launch_failure_reason_carries_the_offending_path(self):
        """The launch-failure reason must name WHAT could not be launched.

        One sentinel covers every launch OSError, so a missing grammar cwd raises
        the same FileNotFoundError as a missing CLI.  Only the errno detail
        distinguishes them, and it must survive into the SKIP line.
        """
        missing = os.path.join(self._tmpdir, "definitely-not-here")
        _, cli_reason = self._call(missing)
        self.assertIn(
            missing, cli_reason,
            "the errno detail naming the missing CLI must reach the reason",
        )

        # Same sentinel, different subsystem: an absent grammar directory.  The
        # CLI here is a perfectly good stub, so only the cwd can fail.
        bad_root = os.path.join(self._tmpdir, "no-repo")
        with unittest.mock.patch.object(
            pcc, "_find_repo_root", return_value=bad_root
        ):
            usable, cwd_reason = self._call(self._clean_stub())
        self.assertFalse(usable, "an absent grammar directory is an unusable substrate")
        self.assertIn(
            os.path.join(bad_root, "tree-sitter-reify"), cwd_reason,
            "an absent grammar directory must be attributable from the reason, "
            "not silently reported as a missing CLI.  Asserted on the CONCRETE "
            "path, because the reason's static sentence names "
            "'tree-sitter-reify' for every launch failure — including the "
            "missing-CLI case above — so a substring assertion on that alone "
            "would still pass with the errno detail dropped entirely",
        )


    def test_cache_setup_failure_reason_names_cache(self):
        """A cache-setup failure must be attributable from the reason text.

        run_probe() represents an uncreatable private cache dir through the SAME
        _BINARY_NOT_FOUND_SENTINEL as a missing CLI and a missing grammar dir, so
        the reason's static sentence — which today enumerates only that pair —
        would confidently print the wrong subsystem for this third cause.

        Asserted on the reason with the offending path REDACTED, because that
        path is literally named `reify-ts-cache-<key>`: a bare "cache" substring
        check would pass on the errno detail alone while the static sentence
        still named only the CLI and the grammar directory.  Same vacuity trap
        test_launch_failure_reason_carries_the_offending_path documents, inverted.
        """
        # The clean stub deliberately, not the echo-cache one: its own path
        # reaches the reason via _resolve_tree_sitter_bin() and a name carrying
        # "cache" would satisfy the assertion below by itself.
        stub = self._clean_stub()
        self.assertNotIn("cache", stub.lower(), "the stub path must be neutral")
        self.assertNotIn(
            "cache", self._tmpdir.lower(), "the tempdir path must be neutral"
        )

        with unittest.mock.patch.object(tempfile, "tempdir", self._tmpdir):
            with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
                os.environ.pop(pcc._CACHE_HOME_OVERRIDE_ENV, None)
                wedged = pcc._grammar_cache_home(pcc._find_repo_root())
                shutil.rmtree(wedged, ignore_errors=True)
                with open(wedged, "w") as f:
                    f.write("x")
                usable, reason = pcc.grammar_substrate_usable()

        self.assertFalse(usable, "a wedged grammar cache is an unusable substrate")
        self.assertIn(
            wedged, reason,
            "the errno detail naming the offending cache path must reach the "
            "reason — it is what actually distinguishes the three causes",
        )
        redacted = reason.replace(wedged, "<path>").lower()
        self.assertIn(
            "cache", redacted,
            "the static sentence must name the cache-setup cause alongside the "
            "missing-CLI and missing-grammar-dir ones it already enumerates; "
            "otherwise the SKIP line points an operator at the wrong subsystem",
        )
    # ── (e) present-but-unlaunchable CLI → unusable, never a raise ────────────

    def test_non_executable_cli_is_unusable_with_reason(self):
        """A CLI that exists but cannot be launched → (False, reason), not a raise.

        Until run_probe() represented an EACCES launch the way it represents
        ENOENT, the PermissionError escaped instead.  Returning normally is as
        load-bearing as the boolean — the skip-guard cannot recover from a raise.
        """
        stub = self._not_executable_stub()
        self.assertTrue(
            os.path.isfile(stub),
            "precondition: the stub must EXIST, else this degenerates into "
            "test_missing_cli_is_unusable_with_reason",
        )
        usable, reason = self._call(stub)
        self.assertFalse(
            usable,
            "a tree-sitter CLI that cannot be launched is an unusable substrate",
        )
        self.assertTrue(reason, "an unusable substrate must explain itself")
        self.assertIn("tree-sitter", reason.lower())

    # ── (f) a wedged CLI → unusable, and BOUNDED ──────────────────────────────

    def test_hanging_cli_is_unusable_within_the_timeout(self):
        """A tree-sitter that never answers → (False, reason), and it returns.

        The failure mode the behavioural guard introduced: the old isfile() guard
        structurally could not block.  Unbounded, a tree-sitter wedged on the very
        grammar lock this task is about would hang the suite rather than skip one
        test, and the exception-swallowing guard cannot help — a hang raises
        nothing.  Bounded to 0.4s here; production uses
        _SUBSTRATE_PROBE_TIMEOUT_S, generous for cold-cache compiles.
        """
        stub = _ts_stub_hangs(self._tmpdir)
        with unittest.mock.patch.object(pcc, "_SUBSTRATE_PROBE_TIMEOUT_S", 0.4):
            started = time.monotonic()
            usable, reason = self._call(stub)
            elapsed = time.monotonic() - started
        self.assertFalse(usable, "a tree-sitter that will not answer is unusable")
        self.assertLess(
            elapsed, 10.0,
            "the substrate probe must be time-bounded; unbounded, a wedged "
            "tree-sitter hangs the suite at import instead of skipping one test",
        )
        self.assertTrue(reason, "an unusable substrate must explain itself")
        self.assertIn("tree-sitter", reason.lower())

    def test_timeout_is_a_harness_error_not_an_absent_observation(self):
        """A timed-out probe must never read as a real observation.

        subprocess.TimeoutExpired derives from SubprocessError, not OSError, so
        run_probe()'s launch-failure catch cannot cover it; if the sentinel it
        substitutes were not classified, an exit 124 would fall through
        observe()'s grammar branch as an ordinary non-zero exit.
        """
        with unittest.mock.patch.dict(
            os.environ, {"TREE_SITTER_BIN": _ts_stub_hangs(self._tmpdir)}
        ):
            run = pcc.run_probe(
                pcc.Probe(
                    capability="wedged",
                    probe_kind="grammar",
                    fixture=pcc._SUBSTRATE_PROBE_FIXTURE,
                    expected={"observation": "present", "match": {}},
                ),
                timeout=0.4,
            )
        self.assertIn(pcc._PROBE_TIMEOUT_SENTINEL, run.stderr)
        self.assertEqual(
            pcc.observe("grammar", run, {}), pcc._HARNESS_ERROR,
            "a probe that never finished cannot be trusted as PRESENT/ABSENT",
        )

    def test_timeout_kills_the_probes_whole_process_tree(self):
        """A timed-out probe must leave no descendant running.

        subprocess.run kills and reaps only the DIRECT child, so a wedged
        tree-sitter that had already forked `cc` leaked it — still holding the
        inherited stdout/stderr pipes.  The bound exists precisely for the
        wedged-toolchain case, so it must not manufacture a process leak in it;
        the repo tracks that class of orphan separately
        (docs/notes/orphaned-test-binary-reaper.md).
        """
        pidfile = os.path.join(self._tmpdir, "grandchild.pid")
        stub = _ts_stub_forks_grandchild(self._tmpdir, pidfile)
        probe = pcc.Probe(
            capability="wedged with a grandchild",
            probe_kind="grammar",
            fixture=pcc._SUBSTRATE_PROBE_FIXTURE,
            expected={"observation": "present", "match": {}},
        )
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            run = pcc.run_probe(probe, timeout=0.6)
        self.assertIn(
            pcc._PROBE_TIMEOUT_SENTINEL, run.stderr,
            "precondition: the stub must have outlasted the bound",
        )
        self.assertTrue(
            _wait_until(lambda: os.path.isfile(pidfile) and os.path.getsize(pidfile), 5.0),
            "precondition: the stub must have recorded its grandchild's pid",
        )
        with open(pidfile) as fh:
            pid = int(fh.read().strip())
        self.addCleanup(_kill_pid_quietly, pid)
        self.assertTrue(
            _wait_until(lambda: not _pid_is_alive(pid), 5.0),
            "the kill must reach the probe's grandchildren, not only the direct "
            "child; a leaked compile outlives the run still holding its pipes",
        )

    def test_probe_set_runs_stay_unbounded(self):
        """run_probe() defaults to no timeout, so gate verdicts are unchanged.

        A bound on the ordinary probe path would manufacture a HARNESS_ERROR out
        of a slow machine — `reify eval` on a heavy fixture may legitimately run
        long.  The bound is opt-in, for callers that cannot afford to block.
        """
        calls = []

        def fake_run(*args, **kwargs):
            calls.append(kwargs)
            return subprocess.CompletedProcess(args=args, returncode=0, stdout="", stderr="")

        probe = pcc.Probe(
            capability="unbounded",
            probe_kind="grammar",
            fixture=pcc._SUBSTRATE_PROBE_FIXTURE,
            expected={"observation": "present", "match": {}},
        )
        with unittest.mock.patch.object(subprocess, "run", side_effect=fake_run):
            pcc.run_probe(probe)
        # Assert the launch HAPPENED before asserting what it was passed: read
        # through .get() with a None default, this passes vacuously if
        # subprocess.run is never reached at all (build_command raising, or a
        # future short-circuit), silently stopping guarding the thing it exists for.
        self.assertEqual(len(calls), 1, "run_probe() must have launched exactly once")
        captured = calls[0]
        self.assertIn(
            "timeout", captured,
            "run_probe() must pass `timeout` explicitly, so the default is "
            "visible at the call site rather than inherited from subprocess",
        )
        self.assertIsNone(
            captured["timeout"],
            "run_probe() must stay unbounded by default; only callers that ask "
            "for a bound get one",
        )

    # ── shape ─────────────────────────────────────────────────────────────────

    def test_returns_bool_and_str_pair(self):
        """The return is a (bool, str) 2-tuple in both directions."""
        for stub in (self._clean_stub(), self._cache_denied_stub()):
            usable, reason = self._call(stub)
            self.assertIsInstance(usable, bool)
            self.assertIsInstance(reason, str)


# Taken after the whole module body has executed, so it records exactly what
# IMPORT cost — read by TestGrammarAvailabilityGuard to pin that the substrate
# probe stayed lazy.  Must remain the last statement before the entry-point.
_info = _grammar_e2e_status.cache_info()
_GRAMMAR_STATUS_CALLS_AT_IMPORT = _info.hits + _info.misses


if __name__ == "__main__":
    unittest.main()
