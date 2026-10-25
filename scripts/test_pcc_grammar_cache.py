#!/usr/bin/env python3
"""
test_pcc_grammar_cache.py — grammar-probe cache isolation in
scripts/prd-capability-check.py: the private XDG_CACHE_HOME, its fingerprint and
derivation, and how a setup failure is represented.  Shared loader and helpers:
scripts/_pcc_test_support.py; runner: tests/infra/test_prd_capability_check.sh.
"""

import hashlib
import os
import shutil
import stat
import tempfile
import unittest
import unittest.mock

from _pcc_test_support import (
    pcc,
    _UNSET_CACHE_MARKER,
    _ts_stub_echo_cache,
    _ts_stub_hangs,
    _write_ts_stub,
)


# ---------------------------------------------------------------------------
# task-5925 (RED): grammar probes must run under a PRIVATE XDG_CACHE_HOME
# ---------------------------------------------------------------------------

class TestGrammarCacheIsolation(unittest.TestCase):
    """Pins that a grammar probe's subprocess gets an ISOLATED tree-sitter cache.

    WHY THIS EXISTS.  `tree-sitter parse` compiles the grammar to a shared object
    cached at ``$XDG_CACHE_HOME/tree-sitter/lib/<language-name>.so`` — keyed by
    LANGUAGE NAME, not by grammar path, and invalidated only on source mtime.
    Every one of the ~235 linked worktrees of this store therefore shares ONE
    ``~/.cache/tree-sitter/lib/reify.so``, so a patched build made in any other
    lane silently serves THIS lane's parses.  That is not hypothetical: it is the
    recorded false PASS at
    docs/prds/v0_6/angle-units-surface-convergence.capability-manifest.md C2.

    The assertions deliberately go beyond "XDG_CACHE_HOME is set" — "is set" is
    satisfied by the ambient value and would enshrine the bug.  Each case pins
    that the child's cache is neither the ambient one nor ``~/.cache``.

    setUp clears REIFY_TS_CACHE_HOME for the WHOLE class, which is load-bearing
    rather than tidiness.  Every case here sets only TREE_SITTER_BIN /
    XDG_CACHE_HOME, so an ambient override — which
    .claude/skills/prd/references/grammar-gate.md actively encourages an operator
    to export — would silently divert every case onto the OVERRIDE branch instead
    of the derived one, and they would all still pass: exactly the vacuous green
    the class docstring above says these assertions exist to avoid.  (It could
    also fail spuriously, if the exported override happened to equal ~/.cache.)
    """

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="prd_gate_test_")
        env_patch = unittest.mock.patch.dict(os.environ, {})
        env_patch.start()
        self.addCleanup(env_patch.stop)
        os.environ.pop(pcc._CACHE_HOME_OVERRIDE_ENV, None)

    def tearDown(self):
        shutil.rmtree(self._tmpdir, ignore_errors=True)

    # ── helpers ───────────────────────────────────────────────────────────────

    def _make_probe(self, kind="grammar",
                    fixture="tests/prd-gate/fixtures/arrow_type.ri"):
        return pcc.Probe(
            capability="test",
            probe_kind=kind,
            fixture=fixture,
            expected={"observation": "present", "match": {}},
        )

    def _reported_cache(self, run) -> str:
        """Extract the XDG_CACHE_HOME the CHILD saw out of a stub's stdout.

        Asserts the line is actually present first: a silent "" would let every
        downstream inequality assertion pass vacuously.
        """
        for line in run.stdout.splitlines():
            if line.startswith("XDG_CACHE_HOME="):
                return line.split("=", 1)[1]
        self.fail(
            "stub did not report XDG_CACHE_HOME; "
            f"exit={run.exit_code} stdout={run.stdout!r} stderr={run.stderr!r}"
        )

    def _assert_isolated(self, value: str, ambient) -> None:
        """The reported cache must be a real private dir, not the shared one."""
        self.assertNotEqual(
            value, _UNSET_CACHE_MARKER,
            "grammar probe child ran with XDG_CACHE_HOME unset — it would fall "
            "back to the host-global ~/.cache/tree-sitter shared by every lane",
        )
        self.assertTrue(value, "reported XDG_CACHE_HOME must be non-empty")
        if ambient is not None:
            self.assertNotEqual(
                value, ambient,
                "grammar probe must NOT inherit the ambient XDG_CACHE_HOME; "
                "'is set' is satisfied by the ambient value and enshrines the bug",
            )
        home_cache = os.path.expanduser("~/.cache")
        self.assertNotEqual(
            value, home_cache,
            f"grammar probe cache must not be the host-global {home_cache}",
        )

    # ── (a) the unbounded arm sets an isolated cache ──────────────────────────

    def test_grammar_probe_sets_xdg_cache_home(self):
        """run_probe(grammar) hands the child an EXPLICIT cache dir.

        The ambient XDG_CACHE_HOME is deleted for the duration so the only way to
        reach a set value is run_probe() supplying one.  Without that deletion
        this test would pass by pure inheritance on any host that happens to
        export XDG_CACHE_HOME — and this one does: the orchestrator sets
        XDG_CACHE_HOME=/tmp/reify-agent-xdg-cache for every agent
        (dark-factory-orchestrator.yaml:1041), which merely RELOCATES the shared
        artifact rather than isolating it.
        """
        stub = _ts_stub_echo_cache(self._tmpdir)
        probe = self._make_probe()
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            os.environ.pop("XDG_CACHE_HOME", None)
            run = pcc.run_probe(probe)
        self.assertEqual(run.exit_code, 0, f"stub failed: {run.stderr!r}")
        value = self._reported_cache(run)
        self.assertNotEqual(
            value, _UNSET_CACHE_MARKER,
            "grammar probe child must receive an explicit XDG_CACHE_HOME",
        )

    # ── (b) and it is not the ambient one, however the host is configured ─────

    def test_grammar_probe_cache_is_not_ambient(self):
        """The child's cache differs from the ambient value AND from ~/.cache.

        Run under BOTH host configurations — an ambient XDG_CACHE_HOME pointed at
        a sentinel dir, and no ambient XDG_CACHE_HOME at all — so the assertion
        holds whichever way the machine running the gate happens to be set up.
        """
        stub = _ts_stub_echo_cache(self._tmpdir)
        probe = self._make_probe()
        sentinel = os.path.join(self._tmpdir, "ambient_cache")
        os.makedirs(sentinel, exist_ok=True)

        with self.subTest(ambient="set"):
            with unittest.mock.patch.dict(
                os.environ,
                {"TREE_SITTER_BIN": stub, "XDG_CACHE_HOME": sentinel},
            ):
                run = pcc.run_probe(probe)
            self._assert_isolated(self._reported_cache(run), sentinel)

        with self.subTest(ambient="unset"):
            with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
                os.environ.pop("XDG_CACHE_HOME", None)
                run = pcc.run_probe(probe)
            self._assert_isolated(self._reported_cache(run), None)

    # ── (c) the dir the child was pointed at is real and writable ─────────────

    def test_grammar_probe_cache_dir_exists(self):
        """The reported path is an existing, writable directory.

        tree-sitter does not create a missing $XDG_CACHE_HOME root for us in
        every version, and a non-writable one degrades to a load failure — so
        "isolated" is only useful if the dir is also usable.
        """
        stub = _ts_stub_echo_cache(self._tmpdir)
        probe = self._make_probe()
        # Ambient popped for the same reason as (a): otherwise an inherited
        # XDG_CACHE_HOME that already exists satisfies this vacuously.
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            os.environ.pop("XDG_CACHE_HOME", None)
            run = pcc.run_probe(probe)
        value = self._reported_cache(run)
        self.assertNotEqual(value, _UNSET_CACHE_MARKER)
        self.assertTrue(
            os.path.isdir(value),
            f"grammar cache dir must exist after the probe ran; got {value!r}",
        )
        self.assertTrue(
            os.access(value, os.W_OK),
            f"grammar cache dir must be writable; got {value!r}",
        )

    # ── (d) regression: adding env= must not disturb the CWD contract ─────────

    def test_grammar_probe_still_runs_in_tree_sitter_reify(self):
        """Grammar probes keep CWD = <repo_root>/tree-sitter-reify.

        The env plumbing is additive; `tree-sitter parse` still needs the grammar
        directory as its CWD to resolve the reify grammar at all.  Pinned here so
        a regression shows up as this test rather than as "No language found".
        """
        stub = _write_ts_stub(
            self._tmpdir, "ts_stub_cwd", 'echo "CWD=$PWD"\nexit 0\n'
        )
        probe = self._make_probe()
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            run = pcc.run_probe(probe)
        self.assertEqual(run.exit_code, 0, f"stub failed: {run.stderr!r}")
        cwd_lines = [ln for ln in run.stdout.splitlines() if ln.startswith("CWD=")]
        self.assertEqual(len(cwd_lines), 1, f"expected one CWD line; got {run.stdout!r}")
        cwd = cwd_lines[0].split("=", 1)[1]
        self.assertTrue(
            cwd.endswith("tree-sitter-reify"),
            f"grammar probe must run inside tree-sitter-reify/; got {cwd!r}",
        )

    # ── (a) the BOUNDED arm is isolated too ───────────────────────────────────

    def test_bounded_grammar_probe_sets_xdg_cache_home(self):
        """run_probe(grammar, timeout=...) isolates the cache as well.

        THE LOAD-BEARING HALF.  grammar_substrate_usable() calls
        run_probe(probe, timeout=_SUBSTRATE_PROBE_TIMEOUT_S), which goes through
        _run_bounded()'s subprocess.Popen — NOT the subprocess.run() the
        unbounded cases above exercise.  The very first real `tree-sitter parse`
        any gate runs takes THIS arm, so a fix applied only to subprocess.run
        leaves the load-bearing path sharing the host-global cache.
        """
        stub = _ts_stub_echo_cache(self._tmpdir)
        probe = self._make_probe()
        sentinel = os.path.join(self._tmpdir, "ambient_cache")
        os.makedirs(sentinel, exist_ok=True)
        with unittest.mock.patch.dict(
            os.environ, {"TREE_SITTER_BIN": stub, "XDG_CACHE_HOME": sentinel},
        ):
            run = pcc.run_probe(probe, timeout=10.0)
        self.assertEqual(run.exit_code, 0, f"stub failed: {run.stderr!r}")
        self._assert_isolated(self._reported_cache(run), sentinel)

    # ── (b) and it is the SAME dir the unbounded arm uses ─────────────────────

    def test_bounded_and_unbounded_agree(self):
        """Both arms report the SAME cache dir for the same probe.

        Fed from one expression in run_probe() precisely so a reader cannot
        conclude the two paths drifted — and so the compile is amortised across
        both rather than paid once per arm.
        """
        stub = _ts_stub_echo_cache(self._tmpdir)
        probe = self._make_probe()
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            unbounded = self._reported_cache(pcc.run_probe(probe))
            bounded = self._reported_cache(pcc.run_probe(probe, timeout=10.0))
        self.assertEqual(
            bounded, unbounded,
            "the bounded and unbounded launch arms must share one cache dir",
        )

    # ── (c) non-grammar probes keep the ambient environment ───────────────────

    def test_check_probe_env_not_overridden(self):
        """A check probe inherits the AMBIENT XDG_CACHE_HOME verbatim.

        `reify check` drives the tree-sitter Rust library linked into the binary,
        never the CLI cache, so overriding its environment would be unjustified
        blast radius on the gate's highest-volume probe kind.  env=None is
        exactly subprocess's inherit-the-parent default; this pins that it stays
        that way.
        """
        stub = _ts_stub_echo_cache(self._tmpdir, name="reify_stub_echo_cache")
        probe = self._make_probe(kind="check")
        sentinel = os.path.join(self._tmpdir, "ambient_cache_check")
        os.makedirs(sentinel, exist_ok=True)
        with unittest.mock.patch.dict(
            os.environ, {"REIFY_BIN": stub, "XDG_CACHE_HOME": sentinel},
        ):
            run = pcc.run_probe(probe)
        self.assertEqual(run.exit_code, 0, f"stub failed: {run.stderr!r}")
        self.assertEqual(
            self._reported_cache(run), sentinel,
            "check probes must inherit the ambient XDG_CACHE_HOME untouched",
        )

    # ── (d) env plumbing must not swallow or reshape a launch failure ─────────

    def test_grammar_probe_launch_failure_still_sentinel(self):
        """A missing tree-sitter still reaches observe() as exit 127 + sentinel.

        Building the isolated environment happens BEFORE the launch, so a bug
        there (an exception, or an env dict that makes Popen fail differently)
        could plausibly reshape or swallow the OSError representation the
        harness-error path depends on.  Asserted on BOTH arms.
        """
        missing = os.path.join(self._tmpdir, "definitely-not-here-xyz")
        self.assertFalse(os.path.exists(missing), "precondition: path must not exist")
        probe = self._make_probe()
        for label, kwargs in (("unbounded", {}), ("bounded", {"timeout": 10.0})):
            with self.subTest(arm=label):
                with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": missing}):
                    run = pcc.run_probe(probe, **kwargs)
                self.assertEqual(run.exit_code, 127, f"{label}: {run.stderr!r}")
                self.assertIn(
                    pcc._BINARY_NOT_FOUND_SENTINEL, run.stderr,
                    f"{label} arm must still represent a launch failure via the "
                    "sentinel channel observe() classifies as HARNESS_ERROR",
                )

    # ── (e) nor the bounded kill/drain + timeout sentinel contract ────────────

    def test_bounded_grammar_probe_timeout_sentinel_intact(self):
        """_run_bounded's kill/drain and timeout sentinel survive the env= param.

        Reuses the existing _ts_stub_hangs factory rather than writing a new hang
        stub: it already models a tree-sitter wedged on its grammar lock, which
        is the case _SUBSTRATE_PROBE_TIMEOUT_S exists for.
        """
        stub = _ts_stub_hangs(self._tmpdir, seconds=30)
        probe = self._make_probe()
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            run = pcc.run_probe(probe, timeout=0.3)
        self.assertEqual(run.exit_code, 124, f"expected timeout exit; got {run!r}")
        self.assertIn(
            pcc._PROBE_TIMEOUT_SENTINEL, run.stderr,
            "a wedged probe must still be distinguishable from a missing one",
        )



# ---------------------------------------------------------------------------
# amend: the cache key must track the GENERATED grammar, not a stale stamp
# ---------------------------------------------------------------------------

class TestGrammarFingerprint(unittest.TestCase):
    """Pins pcc._grammar_fingerprint(repo_root).  Hermetic: no subprocess.

    The fingerprint is half the cache key, so anything that makes it go stale
    silently restores the stale-hit this whole seam exists to kill — and does so
    at exactly the moment a developer has just changed the grammar and most needs
    a fresh compile.  TestGrammarCacheHome pins the derivation that CONSUMES this
    value; this class pins the value itself, which had no direct coverage at all.
    """

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="prd_gate_fingerprint_")

    def tearDown(self):
        shutil.rmtree(self._tmpdir, ignore_errors=True)

    def _repo(self, name: str, grammar: str = None, stamp: str = None) -> str:
        """A synthetic repo root carrying any subset of the fingerprint sources."""
        root = os.path.join(self._tmpdir, name)
        src = os.path.join(root, "tree-sitter-reify", "src")
        os.makedirs(src, exist_ok=True)
        if grammar is not None:
            with open(os.path.join(src, "grammar.json"), "w") as f:
                f.write(grammar)
        if stamp is not None:
            with open(os.path.join(src, ".grammar_hash.stamp"), "w") as f:
                f.write(stamp)
        return root

    # -- (a) it hashes the artifact tree-sitter actually compiles ------------

    def test_hashes_the_generated_grammar_json(self):
        """The fingerprint is sha256(src/grammar.json), asserted by EQUALITY.

        Equality rather than "non-empty": a derivation that keyed on the file's
        SIZE or mtime would satisfy non-emptiness while reintroducing exactly the
        mtime-shaped staleness that warm-lane seeding defeats.
        """
        body = '{"name": "reify", "rules": {}}'
        root = self._repo("lane_a", grammar=body)
        self.assertEqual(
            pcc._grammar_fingerprint(root),
            hashlib.sha256(body.encode("utf-8")).hexdigest(),
        )

    def test_changed_grammar_json_changes_the_fingerprint(self):
        """Regenerating the grammar must move the key."""
        root = self._repo("lane_a", grammar='{"v": 1}')
        before = pcc._grammar_fingerprint(root)
        with open(os.path.join(root, "tree-sitter-reify", "src",
                               "grammar.json"), "w") as f:
            f.write('{"v": 2}')
        self.assertNotEqual(before, pcc._grammar_fingerprint(root))

    # -- (b) THE regression pin: a stale stamp must not mask a new grammar ---

    def test_stale_stamp_does_not_mask_a_changed_grammar(self):
        """A stale src/.grammar_hash.stamp must NOT pin the key.

        THE REACHABLE DEFECT this closes: `edit grammar.js` -> `cargo build` ->
        run the PRD gate.  tree-sitter-reify/build.rs regenerates grammar.json
        and parser.c through its OWN run_tree_sitter_generate() (a direct
        `tree-sitter generate`) and writes its staleness stamp to $OUT_DIR;
        src/.grammar_hash.stamp is refreshed ONLY by
        scripts/tree-sitter-generate.sh.  So after a cargo build the stamp is
        stale by construction.  A fingerprint that PREFERRED it — as an earlier
        revision of _grammar_fingerprint() did — would be unchanged across that
        edit, leaving the cache key unchanged and handing the lane the reify.so
        compiled from the PREVIOUS grammar.  tree-sitter's own mtime backstop
        cannot be relied on to catch it: warm-lane seeding stamps mtimes, which
        is why a content key exists here at all.
        """
        stale = "a" * 64
        root = self._repo("lane_a", grammar='{"v": 1}', stamp=stale)
        before = pcc._grammar_fingerprint(root)

        with open(os.path.join(root, "tree-sitter-reify", "src",
                               "grammar.json"), "w") as f:
            f.write('{"v": 2}')
        # Deliberately NOT refreshed — this is exactly the state cargo leaves.
        with open(os.path.join(root, "tree-sitter-reify", "src",
                               ".grammar_hash.stamp")) as f:
            self.assertEqual(f.read(), stale,
                             "precondition: the stamp must still be the stale one")

        after = pcc._grammar_fingerprint(root)
        self.assertNotEqual(
            before, after,
            "a regenerated grammar.json must move the fingerprint even when "
            "src/.grammar_hash.stamp is stale, which is what cargo leaves behind",
        )
        self.assertNotIn(
            stale, (before, after),
            "the stamp's contents must not be the key at all",
        )

    def test_stamp_without_grammar_json_yields_empty(self):
        """A stamp alone is not a fingerprint source.

        The other half of the pin above: a lane carrying a stamp but no generated
        grammar has nothing to compile, so it belongs in the empty-fingerprint
        bucket rather than keyed on a hash of a grammar that is not there.
        """
        root = self._repo("lane_a", stamp="c" * 64)
        self.assertEqual(pcc._grammar_fingerprint(root), "")

    # -- (c) never raises, whatever the lane looks like ----------------------

    def test_missing_sources_yield_empty_and_never_raise(self):
        """A fresh warm lane (no tree-sitter-reify/ at all) gets "" , not a raise.

        grammar_substrate_usable() runs at test-harness IMPORT time, where a raise
        costs the whole suite instead of one skip.
        """
        root = os.path.join(self._tmpdir, "never_generated")
        os.makedirs(root, exist_ok=True)
        self.assertEqual(pcc._grammar_fingerprint(root), "")

    def test_unreadable_grammar_json_yields_empty_and_never_raises(self):
        """An unreadable grammar.json degrades to "" rather than propagating."""
        root = self._repo("lane_a", grammar='{"v": 1}')
        target = os.path.join(root, "tree-sitter-reify", "src", "grammar.json")
        os.chmod(target, 0o000)
        try:
            try:
                with open(target, "rb"):
                    pass
            except OSError:
                pass
            else:
                self.skipTest("chmod 0o000 did not deny the read (running as root?)")
            self.assertEqual(pcc._grammar_fingerprint(root), "")
        finally:
            # Restored here, not via addCleanup: cleanups run AFTER tearDown, by
            # which point rmtree has removed the file and the chmod would ENOENT.
            os.chmod(target, 0o600)


# ---------------------------------------------------------------------------
# task-5925 (RED): the cache-dir DERIVATION contract
# ---------------------------------------------------------------------------

class TestGrammarCacheHome(unittest.TestCase):
    """Pins pcc._grammar_cache_home(repo_root).  Hermetic: no subprocess.

    TestGrammarCacheIsolation proves "not the ambient dir".  That is necessary
    and NOT sufficient: it says nothing about the two properties that actually
    defeat the hazard.

      * CROSS-LANE SEPARATION.  ~235 linked worktrees share one .git and one
        host cache.  If every lane derived the same dir, every lane would still
        share one reify.so and the relocation would buy nothing.
      * FINGERPRINT SEPARATION.  tree-sitter invalidates its compiled grammar on
        SOURCE MTIME, and warm-lane seeding stamps mtimes — so mtime is not a
        trustworthy staleness signal here.  A content-derived key makes a
        changed grammar a new directory by construction instead.

    Asserted against the pure derivation, not through a subprocess, so a failure
    names the derivation rather than the plumbing.
    """

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="prd_gate_test_")
        self._made = []

    def tearDown(self):
        shutil.rmtree(self._tmpdir, ignore_errors=True)
        for path in self._made:
            shutil.rmtree(path, ignore_errors=True)

    def _fake_repo(self, name: str, grammar: str = None) -> str:
        """A synthetic repo root, optionally carrying a generated grammar.json.

        Seeds src/grammar.json — the GENERATED ARTIFACT the fingerprint actually
        hashes — rather than src/.grammar_hash.stamp.  Seeding the stamp would
        make these cases exercise a branch the derivation no longer has, and
        would keep passing even if the fingerprint stopped tracking the grammar
        at all; see TestGrammarFingerprint for why the stamp is not trustworthy.
        """
        root = os.path.join(self._tmpdir, name)
        src = os.path.join(root, "tree-sitter-reify", "src")
        os.makedirs(src, exist_ok=True)
        if grammar is not None:
            with open(os.path.join(src, "grammar.json"), "w") as f:
                f.write(grammar + "\n")
        return root

    def _cache_home(self, repo_root: str) -> str:
        """Call the derivation with no REIFY_TS_CACHE_HOME override in effect."""
        with unittest.mock.patch.dict(os.environ, {}):
            os.environ.pop("REIFY_TS_CACHE_HOME", None)
            path = pcc._grammar_cache_home(repo_root)
        self._made.append(path)
        return path

    # ── (a) stable → the compile is amortised ─────────────────────────────────

    def test_cache_home_stable_for_same_repo_root(self):
        """Same repo_root + unchanged fingerprint → the SAME path.

        The amortisation property: every probe in one process, and every process
        in one lane, must share one compiled reify.so.
        """
        root = self._fake_repo("lane_a", "a" * 64)
        self.assertEqual(self._cache_home(root), self._cache_home(root))

    # ── (b) THE core assertion: cross-lane collision is impossible ────────────

    def test_cache_home_differs_across_repo_roots(self):
        """Two DIFFERENT repo roots (same fingerprint) → DIFFERENT paths.

        This is the collision the task exists to kill: without a path component
        every lane on the host lands on one cache and a patched build in any
        other lane silently answers this lane's parses.
        """
        fingerprint = "a" * 64
        a = self._fake_repo("lane_a", fingerprint)
        b = self._fake_repo("lane_b", fingerprint)
        self.assertNotEqual(
            self._cache_home(a), self._cache_home(b),
            "two worktrees of the same store must not share a grammar cache",
        )

    # ── (c) closes the mtime-stamping hole ────────────────────────────────────

    def test_cache_home_differs_on_grammar_change(self):
        """Same repo_root, different grammar fingerprint → DIFFERENT paths.

        tree-sitter's own invalidation is mtime-based and warm-lane seeding
        stamps mtimes, so a content-derived key is what actually makes a changed
        grammar a new cache rather than a stale hit.
        """
        root = self._fake_repo("lane_a", "a" * 64)
        before = self._cache_home(root)
        with open(os.path.join(root, "tree-sitter-reify", "src",
                               "grammar.json"), "w") as f:
            f.write("b" * 64 + "\n")
        after = self._cache_home(root)
        self.assertNotEqual(
            before, after,
            "a changed grammar must land in a new cache dir; mtime-based "
            "invalidation is unreliable under warm-lane mtime stamping",
        )

    # ── (d) it lives under $TMPDIR, never in the shared home cache ────────────

    def test_cache_home_under_tmpdir_and_not_home_cache(self):
        """The dir is under tempfile.gettempdir() and is not ~/.cache."""
        root = self._fake_repo("lane_a", "a" * 64)
        path = self._cache_home(root)
        tmp = tempfile.gettempdir()
        self.assertTrue(
            os.path.abspath(path).startswith(os.path.abspath(tmp) + os.sep),
            f"cache dir must live under {tmp!r}; got {path!r}",
        )
        home_cache = os.path.expanduser("~/.cache")
        self.assertNotEqual(path, home_cache)
        self.assertFalse(
            path.startswith(home_cache),
            f"cache dir must not be inside {home_cache!r}; got {path!r}",
        )

    # ── (e) a lane with no generated grammar must not crash the harness ───────

    def test_cache_home_missing_fingerprint_sources_ok(self):
        """A repo_root with NO tree-sitter-reify/src/ still yields a usable path.

        A fresh warm lane has not generated the grammar yet (parser.c is
        gitignored and absent). It must land in the empty-fingerprint bucket,
        not raise — grammar_substrate_usable() is called at test-harness import
        time, where a raise costs the whole suite instead of one skip.
        """
        root = os.path.join(self._tmpdir, "no_grammar_here")
        os.makedirs(root, exist_ok=True)
        self.assertFalse(os.path.exists(os.path.join(root, "tree-sitter-reify")))
        path = self._cache_home(root)
        self.assertTrue(path)
        self.assertTrue(os.path.isdir(path))

    # ── (f) the operator escape hatch ─────────────────────────────────────────

    def test_cache_home_env_override_honoured(self):
        """A non-empty REIFY_TS_CACHE_HOME is used verbatim, and created.

        The debug seam: an operator inspecting or sharing a cache, and the pin an
        interactive session can set, without having to reverse the hash.
        """
        root = self._fake_repo("lane_a", "a" * 64)
        override = os.path.join(self._tmpdir, "operator_pinned_cache")
        self.assertFalse(os.path.exists(override))
        with unittest.mock.patch.dict(
            os.environ, {"REIFY_TS_CACHE_HOME": override}
        ):
            path = pcc._grammar_cache_home(root)
        self._made.append(path)
        self.assertEqual(path, override, "override must be used verbatim")
        self.assertTrue(os.path.isdir(path), "override dir must be created")

    # ── (g) the returned dir is real and writable ─────────────────────────────

    def test_cache_home_creates_directory(self):
        """The returned path exists and is writable after the call."""
        root = self._fake_repo("lane_a", "a" * 64)
        path = self._cache_home(root)
        self.assertTrue(os.path.isdir(path), f"must exist; got {path!r}")
        self.assertTrue(os.access(path, os.W_OK), f"must be writable; got {path!r}")

    # ── (h) a bad override degrades to the DERIVED dir — isolation preserved ──

    def test_bad_override_falls_back_to_derived(self):
        """An unusable REIFY_TS_CACHE_HOME yields exactly the derived path.

        "Does not raise" is necessary and NOT sufficient: it says nothing about
        WHICH dir the fallback lands on, and a fallback to the ambient shared
        cache would satisfy it while destroying the isolation invariant.  So the
        assertion is EQUALITY with the no-override derivation, which proves the
        fallback is the isolated per-lane dir rather than merely "a path that
        exists".  Only the operator's custom-dir intent is lost.
        """
        root = self._fake_repo("lane_a", "a" * 64)
        derived = self._cache_home(root)

        blocker = os.path.join(self._tmpdir, "override_parent_is_a_file")
        with open(blocker, "w") as f:
            f.write("x")
        bad = os.path.join(blocker, "cache")

        with unittest.mock.patch.dict(
            os.environ, {pcc._CACHE_HOME_OVERRIDE_ENV: bad}
        ):
            path = pcc._grammar_cache_home(root)
        self.assertEqual(
            path, derived,
            "an unusable override must fall back to the DERIVED per-lane dir; "
            "any other target (notably the ambient shared cache) silently "
            "restores the cross-lane collision this seam exists to kill",
        )

    def test_bad_override_never_raises(self):
        """Several OSError shapes all degrade rather than propagate.

        Matches the "Never raises" contract _grammar_fingerprint() already
        upholds, and for the same reason: grammar_substrate_usable() runs at
        test-harness IMPORT time, where a raise costs the whole suite.
        """
        root = self._fake_repo("lane_a", "a" * 64)

        blocker = os.path.join(self._tmpdir, "parent_is_a_file")
        with open(blocker, "w") as f:
            f.write("x")

        locked = os.path.join(self._tmpdir, "parent_unwritable")
        os.makedirs(locked, exist_ok=True)
        os.chmod(locked, 0o500)

        shapes = (
            ("parent-is-a-file", os.path.join(blocker, "cache")),
            ("parent-unwritable", os.path.join(locked, "cache")),
        )
        try:
            for label, bad in shapes:
                with self.subTest(shape=label):
                    # Assert the shape actually FAILS before asserting it
                    # degrades: a case that quietly succeeds (root ignores the
                    # mode bits) would pass vacuously and guard nothing.
                    try:
                        os.makedirs(bad, exist_ok=True)
                    except OSError:
                        pass
                    else:
                        self.skipTest(
                            f"{label}: os.makedirs unexpectedly succeeded, so "
                            "this shape cannot exercise the fallback "
                            "(running as root?)"
                        )
                    with unittest.mock.patch.dict(
                        os.environ, {pcc._CACHE_HOME_OVERRIDE_ENV: bad}
                    ):
                        path = pcc._grammar_cache_home(root)
                    self._made.append(path)
                    self.assertTrue(
                        os.path.isdir(path),
                        f"{label}: the fallback must be a real, usable directory",
                    )
        finally:
            # Restored here rather than via addCleanup: cleanups run AFTER
            # tearDown, by which point rmtree has already removed the dir and
            # the chmod would fail with ENOENT.
            os.chmod(locked, 0o700)

    # ── (i) an uncreatable DERIVED dir is deliberately FAIL-CLOSED ────────────

    def test_uncreatable_derived_dir_raises_oserror(self):
        """A wedged derived path RAISES; it must not degrade to anything.

        The other half of the two-tier contract, pinned so a later well-meaning
        change to a silent ambient fallback fails by NAME here instead of quietly
        restoring the hazard.  run_probe() is what turns this raise into a
        represented exit 127 + sentinel; the derivation itself stays loud.

        Unprivileged-reachable, not just an operator mistake: /tmp is
        world-writable, so any other uid can pre-create reify-ts-cache-<key> as a
        plain file.
        """
        root = self._fake_repo("lane_a", "a" * 64)
        with unittest.mock.patch.object(tempfile, "tempdir", self._tmpdir):
            with unittest.mock.patch.dict(os.environ, {}):
                os.environ.pop(pcc._CACHE_HOME_OVERRIDE_ENV, None)
                derived = pcc._grammar_cache_home(root)
                shutil.rmtree(derived, ignore_errors=True)
                with open(derived, "w") as f:
                    f.write("x")
                self.assertTrue(
                    os.path.isfile(derived),
                    "precondition: the derived path must be a FILE",
                )
                with self.assertRaises(OSError):
                    pcc._grammar_cache_home(root)


    # ── (j) the derived dir is PRIVATE, and an untrusted one is refused ───────

    def _derived(self, root: str) -> str:
        """Derive under a tempdir we own, with no override in effect."""
        with unittest.mock.patch.object(tempfile, "tempdir", self._tmpdir):
            with unittest.mock.patch.dict(os.environ, {}):
                os.environ.pop(pcc._CACHE_HOME_OVERRIDE_ENV, None)
                return pcc._grammar_cache_home(root)

    def test_new_derived_dir_is_private(self):
        """A dir this function creates grants nothing to group or other.

        `tree-sitter parse` LOADS AND EXECUTES <cache>/tree-sitter/lib/reify.so,
        and the derived path is fully predictable inside world-writable /tmp — so
        a default-umask dir (0775 on this host, umask 0002) would let any member
        of the group drop a reify.so the gate then dlopens.
        """
        root = self._fake_repo("lane_a", "a" * 64)
        derived = self._derived(root)
        self.assertEqual(
            stat.S_IMODE(os.lstat(derived).st_mode) & 0o077, 0,
            f"derived cache dir must not be group/other accessible: {derived!r}",
        )

    def test_loose_mode_derived_dir_is_repaired(self):
        """A group/other-writable dir we OWN is tightened in place, not refused.

        Repair rather than raise is deliberate: this host's umask is 0002, so
        every cache dir created before the check existed is 0775, and a bare
        raise would wedge those lanes' gates rather than protect them.  The
        returned path must still be the same usable derived dir.
        """
        root = self._fake_repo("lane_a", "a" * 64)
        derived = self._derived(root)
        os.chmod(derived, 0o777)
        self.assertTrue(
            os.lstat(derived).st_mode & 0o022,
            "precondition: the dir must actually be group/other-writable",
        )
        again = self._derived(root)
        self.assertEqual(again, derived, "repair must not relocate the cache")
        self.assertFalse(
            os.lstat(derived).st_mode & 0o022,
            "a group/other-writable cache dir must be tightened before use",
        )
        self.assertTrue(os.path.isdir(derived), "and must remain usable")

    def test_symlinked_derived_dir_raises(self):
        """A SYMLINK at the derived path is refused, even pointing somewhere we own.

        os.makedirs(exist_ok=True) waves a symlink-to-a-directory straight
        through, and os.stat() would then report the TARGET's ownership — so an
        ownership check alone is redirectable by anyone who can create a name in
        /tmp.  Fail-closed here, same disposition as the pre-created-FILE case:
        run_probe() represents it as exit 127 + the launch-failure sentinel.
        """
        root = self._fake_repo("lane_a", "a" * 64)
        elsewhere = os.path.join(self._tmpdir, "attacker_controlled")
        os.makedirs(elsewhere, exist_ok=True)

        derived = self._derived(root)
        shutil.rmtree(derived, ignore_errors=True)
        os.symlink(elsewhere, derived)
        self.assertTrue(
            os.path.islink(derived) and os.path.isdir(derived),
            "precondition: a symlink RESOLVING to a real directory",
        )
        with self.assertRaises(OSError):
            self._derived(root)


# ---------------------------------------------------------------------------
# task-5925 (RED): a cache-dir setup failure must be REPRESENTED, not raised
# ---------------------------------------------------------------------------

class TestGrammarCacheSetupFailure(unittest.TestCase):
    """Pins that building the private grammar cache cannot raise out of run_probe.

    THE CONTRACT BEING DEFENDED is run_probe()'s own: "Represent rather than
    propagate: callers run this at import time in the test harness, where a raise
    costs the whole suite instead of one skip."  _grammar_fingerprint() was
    written to uphold the same rule ("Never raises").  But the env is built at
    ``env = _grammar_probe_env(repo_root)`` OUTSIDE run_probe()'s ``try:``, so
    the os.makedirs() inside _grammar_cache_home() escapes both arms.

    NOT merely an operator typo.  /tmp is world-writable, so any other uid on
    this host can pre-create ``reify-ts-cache-<key>`` as a plain file and wedge
    this lane's gate — the derived-path trigger below is unprivileged-reachable.

    The two triggers are deliberately given DIFFERENT dispositions, and both are
    pinned here so a later change cannot quietly swap them:
      * a bad OVERRIDE degrades to the derived per-lane dir — isolation is
        preserved, only the operator's custom-dir intent is lost;
      * an uncreatable DERIVED dir is represented as a launch failure (exit 127 +
        _BINARY_NOT_FOUND_SENTINEL) — fail-closed and loud.
    Neither may fall back to the ambient shared cache; see (c).
    """

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="prd_gate_cache_setup_")

    def tearDown(self):
        shutil.rmtree(self._tmpdir, ignore_errors=True)

    # ── helpers ───────────────────────────────────────────────────────────────

    def _make_probe(self, kind="grammar",
                    fixture="tests/prd-gate/fixtures/arrow_type.ri"):
        return pcc.Probe(
            capability="test",
            probe_kind=kind,
            fixture=fixture,
            expected={"observation": "present", "match": {}},
        )

    def _unusable_override(self) -> str:
        """A REIFY_TS_CACHE_HOME whose PARENT is a regular file.

        os.makedirs() on it raises NotADirectoryError [Errno 20] — measured, not
        assumed.  Models the operator typo and the unwritable-target case alike;
        both arrive as an OSError subclass out of the same call.
        """
        blocker = os.path.join(self._tmpdir, "a_file_not_a_dir")
        with open(blocker, "w") as f:
            f.write("x")
        return os.path.join(blocker, "cache")

    def _wedge_derived_dir(self) -> str:
        """Point $TMPDIR here and pre-create the DERIVED cache path as a FILE.

        Returns the wedged path.  ``tempfile.tempdir`` is patched rather than the
        TMPDIR env var because tempfile.gettempdir() caches its answer in that
        module global, so an env-only patch would be silently ignored once any
        earlier test had already called it.
        """
        derived = pcc._grammar_cache_home(pcc._find_repo_root())
        shutil.rmtree(derived, ignore_errors=True)
        with open(derived, "w") as f:
            f.write("x")
        self.assertTrue(
            os.path.isfile(derived),
            "precondition: the derived cache path must be a FILE, so that "
            "os.makedirs(exist_ok=True) raises FileExistsError rather than "
            "quietly succeeding",
        )
        return derived

    # ── (a) a bad OVERRIDE must not break the gate, on either arm ─────────────

    def test_bad_override_does_not_raise_unbounded(self):
        """An unusable REIFY_TS_CACHE_HOME still yields a ProbeRun (unbounded).

        An operator typo, or a REIFY_TS_CACHE_HOME that has become unwritable,
        must degrade — it must not turn every grammar probe on the host into an
        uncaught NotADirectoryError.  Measured at HEAD: raises [Errno 20].
        """
        stub = _ts_stub_echo_cache(self._tmpdir)
        probe = self._make_probe()
        with unittest.mock.patch.dict(
            os.environ,
            {"TREE_SITTER_BIN": stub,
             pcc._CACHE_HOME_OVERRIDE_ENV: self._unusable_override()},
        ):
            run = pcc.run_probe(probe)
        self.assertIsInstance(
            run, pcc.ProbeRun,
            "an unusable cache override must be represented, not propagated",
        )

    def test_bad_override_does_not_raise_bounded(self):
        """Same, through _run_bounded()'s Popen arm.

        Asserted separately because the two launch arms are separate code paths
        and grammar_substrate_usable() — the first real probe any gate runs —
        takes THIS one.
        """
        stub = _ts_stub_echo_cache(self._tmpdir)
        probe = self._make_probe()
        with unittest.mock.patch.dict(
            os.environ,
            {"TREE_SITTER_BIN": stub,
             pcc._CACHE_HOME_OVERRIDE_ENV: self._unusable_override()},
        ):
            run = pcc.run_probe(probe, timeout=10.0)
        self.assertIsInstance(
            run, pcc.ProbeRun,
            "an unusable cache override must be represented, not propagated",
        )

    # ── (b) an uncreatable DERIVED dir is represented as a launch failure ─────

    def test_uncreatable_derived_dir_represented_unbounded(self):
        """A wedged derived cache path → exit 127 + the launch-failure sentinel.

        Deliberately NOT a silent ambient fallback: observe() classifies the
        sentinel as _HARNESS_ERROR, which is the loud, attributable outcome.
        Measured at HEAD: raises FileExistsError [Errno 17] instead.
        """
        stub = _ts_stub_echo_cache(self._tmpdir)
        probe = self._make_probe()
        with unittest.mock.patch.object(tempfile, "tempdir", self._tmpdir):
            with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
                os.environ.pop(pcc._CACHE_HOME_OVERRIDE_ENV, None)
                self._wedge_derived_dir()
                run = pcc.run_probe(probe)
        self.assertEqual(run.exit_code, 127, f"got {run!r}")
        self.assertIn(pcc._BINARY_NOT_FOUND_SENTINEL, run.stderr)

    def test_uncreatable_derived_dir_represented_bounded(self):
        """Same, through the bounded arm."""
        stub = _ts_stub_echo_cache(self._tmpdir)
        probe = self._make_probe()
        with unittest.mock.patch.object(tempfile, "tempdir", self._tmpdir):
            with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
                os.environ.pop(pcc._CACHE_HOME_OVERRIDE_ENV, None)
                self._wedge_derived_dir()
                run = pcc.run_probe(probe, timeout=10.0)
        self.assertEqual(run.exit_code, 127, f"got {run!r}")
        self.assertIn(pcc._BINARY_NOT_FOUND_SENTINEL, run.stderr)

    # ── (c) and it must never degrade onto the AMBIENT shared cache ───────────

    def test_cache_setup_failure_never_falls_back_to_ambient(self):
        """No child may be launched against the ambient XDG_CACHE_HOME.

        THE ASSERTION THAT KEEPS THE FIX HONEST.  "does not raise" is satisfiable
        by a one-line ``env = None`` — which would hand the probe straight back
        to the host-global cache every lane shares, silently reintroducing the
        recorded false PASS at
        docs/prds/v0_6/angle-units-surface-convergence.capability-manifest.md C2.
        The stub echoes the cache it was handed, so a fallback of that shape
        shows up here as the sentinel dir appearing in stdout.
        """
        stub = _ts_stub_echo_cache(self._tmpdir)
        probe = self._make_probe()
        sentinel = os.path.join(self._tmpdir, "ambient_cache")
        os.makedirs(sentinel, exist_ok=True)
        with unittest.mock.patch.object(tempfile, "tempdir", self._tmpdir):
            with unittest.mock.patch.dict(
                os.environ,
                {"TREE_SITTER_BIN": stub, "XDG_CACHE_HOME": sentinel},
            ):
                os.environ.pop(pcc._CACHE_HOME_OVERRIDE_ENV, None)
                self._wedge_derived_dir()
                run = pcc.run_probe(probe)
        self.assertNotIn(
            sentinel, run.stdout,
            "a cache-setup failure must NOT be degraded into a run against the "
            "ambient, host-global tree-sitter cache",
        )

    # ── (d) the load-bearing caller must survive it ───────────────────────────

    def test_grammar_substrate_usable_survives_cache_setup_failure(self):
        """grammar_substrate_usable() returns (False, reason), never raises.

        THE LOAD-BEARING CASE.  The test harness calls this at IMPORT time to
        decide whether to skip the grammar e2e; a raise there costs the whole
        suite instead of one skip, which is precisely the outcome run_probe()'s
        represent-don't-propagate contract exists to prevent.
        """
        stub = _ts_stub_echo_cache(self._tmpdir)
        with unittest.mock.patch.object(tempfile, "tempdir", self._tmpdir):
            with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
                os.environ.pop(pcc._CACHE_HOME_OVERRIDE_ENV, None)
                self._wedge_derived_dir()
                usable, reason = pcc.grammar_substrate_usable()
        self.assertFalse(usable, "a wedged grammar cache is an unusable substrate")
        self.assertTrue(reason, "an unusable substrate must explain itself")


if __name__ == "__main__":
    unittest.main()
