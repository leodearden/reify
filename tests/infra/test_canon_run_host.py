#!/usr/bin/env python3
"""
test_canon_run_host.py — stdlib unittest for scripts/canon_run.py with real
cargo and real mount namespaces (canonical-build-path PRD, dark-factory
plans/canonical-build-path-shared-cache-prd.md: C1-C4 end to end under the
`incremental` policy; boundary rows 9, 11, 12 and 17).

Each case compiles a scratch one-crate workspace whose test prints the
CARGO_MANIFEST_DIR rustc baked into it, so the output says which path the build
ran at. The vendored gui/src-tauri/sandbox/landlock_exec.py makes mount
namespaces deterministically unavailable where a row needs that.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_canon_run_host.sh.

Where bwrap cannot create a mount namespace (a landlock sandbox, an absent
binary), the rows that need one SKIP with the named reason; likewise the rows
that need landlock.
"""

import json
import os
import shlex
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)

STAMP = ".reify-build-root"

DUMP = """
import json, os, sys
names = ("PWD", "PATH", "REIFY_VERIFY_LEDGER_LANE_DIR", "REIFY_CANON_ACTIVE")
dump = {name: os.environ.get(name) for name in names}
dump["getcwd"] = os.getcwd()
dump["pid_namespace"] = os.readlink("/proc/self/ns/pid")
with open(sys.argv[1], "w") as out:
    json.dump(dump, out)
"""


class HostCase(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-canon-run-host-")
        self.addCleanup(tmp.cleanup)
        self.tmp = os.path.realpath(tmp.name)
        self.tree = fx.make_rust_tree(self.tmp, "tree")
        self.trampoline = os.path.join(self.tree, "scripts", "canon-bin", "cargo")
        self.wrapper, self.wrapper_log = fx.recording_wrapper(self.tmp)
        self.env = fx.cargo_env(self.tmp, self.tree)
        self.canon = self.env["REIFY_CANON_ROOT"]
        self.stamp = Path(self.tree, "target", STAMP)

    def enable_host(self):
        fx.write_host_config(self.env["REIFY_CANON_HOST_CONFIG"], enabled=True, policy="incremental")

    def with_caller_cache_variables(self):
        self.env.update(RUSTC_WRAPPER=self.wrapper, CARGO_INCREMENTAL="0")

    def assert_passed_at(self, result, root):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f"MANIFEST_DIR={root}/fixture_reader", result.stdout.splitlines())

    def wrapper_calls(self):
        return self.wrapper_log.read_text().splitlines() if self.wrapper_log.exists() else []

    def assert_refused(self, result, *needles):
        self.assertEqual(result.returncode, 126, result.stdout + result.stderr)
        lines = result.stderr.splitlines()
        self.assertEqual(len(lines), 1, result.stderr)
        self.assertTrue(lines[0].startswith("canon_run: "), lines[0])
        for needle in needles:
            self.assertIn(needle, lines[0])


class RealPathOnDisabledHost(HostCase):
    """Boundary row 9: with the host disabled, the trampoline is plain cargo."""

    def test_cargo_test_builds_at_the_real_path_with_the_caller_wrapper(self):
        self.with_caller_cache_variables()
        result = fx.run([self.trampoline, *fx.CARGO_TEST], cwd=self.tree, env=self.env)
        self.assert_passed_at(result, self.tree)
        self.assertNotEqual(self.wrapper_calls(), [])
        self.assertFalse(self.stamp.exists())


class NamespaceRows(HostCase):
    def setUp(self):
        reason = fx.namespace_unavailable_reason()
        if reason:
            self.skipTest(reason)
        super().setUp()
        self.enable_host()

    def skip_without_landlock(self):
        reason = fx.landlock_unavailable_reason()
        if reason:
            self.skipTest(reason)

    def test_status_reports_canon_for_a_fresh_build_directory(self):
        target = Path(self.tree, "target")
        target.mkdir()
        (target / ".tree-sitter-freshness.epoch").write_text("0\n")
        (target / ".rustc_info.json").write_text("{}\n")
        result = fx.run_canon(self.tree, self.tree, "status", env=self.env)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["build_dir_mode"], "canon", report["reason"])
        self.assertEqual(report["policy"], "incremental")
        self.assertFalse(self.stamp.exists())

    def test_status_reports_canon_under_the_default_shared_policy(self):
        """shared, the default when host.toml names no policy, runs with the private server."""
        fx.write_host_config(self.env["REIFY_CANON_HOST_CONFIG"], enabled=True)
        result = fx.run_canon(self.tree, self.tree, "status", env=self.env)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["policy"], "shared")
        self.assertEqual(report["build_dir_mode"], "canon", report["reason"])
        self.assertIsNone(report["store_reachable"])
        self.assertEqual(report["canon_root"], self.canon)

    def test_cargo_test_builds_at_canon_incrementally_without_the_wrapper(self):
        self.with_caller_cache_variables()
        result = fx.run([self.trampoline, *fx.CARGO_TEST], cwd=self.tree, env=self.env)
        self.assert_passed_at(result, self.canon)
        self.assertEqual(self.stamp.read_text(), self.canon + "\n")
        self.assertEqual(self.wrapper_calls(), [])
        incremental = Path(self.tree, "target", "debug", "incremental")
        self.assertTrue(
            any(entry.name.startswith("fixture_reader-") for entry in incremental.iterdir()),
            sorted(entry.name for entry in incremental.iterdir()),
        )

    def test_cargo_through_an_installed_trampoline_copy_builds_at_canon(self):
        installed = Path(self.tmp, "installed-canon-bin")
        installed.mkdir()
        shutil.copy2(fx.TRAMPOLINE, installed / "cargo")
        self.env["PATH"] = (
            f"{installed}:{self.tree}/scripts/canon-bin:{self.env['HOME']}/.cargo/bin:/usr/bin:/bin"
        )
        self.with_caller_cache_variables()
        result = fx.run([str(installed / "cargo"), *fx.CARGO_TEST], cwd=self.tree, env=self.env)
        self.assert_passed_at(result, self.canon)
        self.assertEqual(self.stamp.read_text(), self.canon + "\n")
        self.assertEqual(self.wrapper_calls(), [])

    def test_nested_cargo_gets_the_policy_and_the_exports(self):
        dump_path = Path(self.tmp, "dump.json")
        script = (
            f"export RUSTC_WRAPPER={shlex.quote(self.wrapper)} CARGO_INCREMENTAL=0; "
            f"cargo {' '.join(fx.CARGO_TEST)} && "
            f"/usr/bin/python3 -I -c {shlex.quote(DUMP)} {shlex.quote(str(dump_path))}"
        )
        crate = os.path.join(self.tree, "fixture_reader")
        result = fx.run_canon(self.tree, crate, "exec", "--", "/bin/bash", "-c", script, env=self.env)
        self.assert_passed_at(result, self.canon)
        self.assertEqual(self.wrapper_calls(), [])
        dump = json.loads(dump_path.read_text())
        self.assertEqual(dump["getcwd"], f"{self.canon}/fixture_reader")
        self.assertEqual(dump["PWD"], f"{self.canon}/fixture_reader")
        self.assertEqual(
            dump["PATH"].split(os.pathsep)[:2],
            [f"{self.canon}/scripts/canon-bin", f"{self.env['HOME']}/.cargo/bin"],
        )
        self.assertEqual(dump["REIFY_VERIFY_LEDGER_LANE_DIR"], self.tree)
        self.assertEqual(json.loads(dump["REIFY_CANON_ACTIVE"])["real_root"], self.tree)
        self.assertEqual(dump["pid_namespace"], os.readlink("/proc/self/ns/pid"))

    def test_canon_bound_to_another_tree_builds_a_fresh_tree_at_its_real_path(self):
        self.skip_without_landlock()
        tree_b = fx.make_rust_tree(self.tmp, "tree-b")
        trampoline_b = os.path.join(tree_b, "scripts", "canon-bin", "cargo")
        env = fx.cargo_env(self.tmp, tree_b)
        argv = fx.bwrap_bind(self.tree, self.canon, *fx.landlock(self.tmp, trampoline_b, *fx.CARGO_TEST))
        result = fx.run(argv, cwd=tree_b, env=env)
        self.assert_passed_at(result, tree_b)
        self.assertFalse(Path(tree_b, "target", STAMP).exists())

    def test_cargo_where_canon_is_already_bound_leaves_the_caller_at_the_real_path(self):
        self.skip_without_landlock()
        self.env["CARGO_INCREMENTAL"] = "0"
        script = f"cargo {' '.join(fx.CARGO_TEST)}; rc=$?; echo PWD_AFTER=$(pwd -P); exit $rc"
        argv = fx.bwrap_bind(self.tree, self.canon, *fx.landlock(self.tmp, "/bin/bash", "-c", script))
        result = fx.run(argv, cwd=self.tree, env=self.env)
        self.assert_passed_at(result, self.canon)
        self.assertEqual(self.stamp.read_text(), self.canon + "\n")
        self.assertIn(f"PWD_AFTER={self.tree}", result.stdout.splitlines())


class LandlockRows(HostCase):
    def setUp(self):
        reason = fx.landlock_unavailable_reason()
        if reason:
            self.skipTest(reason)
        super().setUp()

    def test_build_directory_stamped_for_unreachable_canon_is_refused(self):
        target = Path(self.tree, "target")
        target.mkdir()
        self.stamp.write_text(self.canon + "\n")
        argv = fx.landlock(self.tmp, self.trampoline, *fx.CARGO_TEST)
        result = fx.run(argv, cwd=self.tree, env=self.env)
        self.assert_refused(result, str(target))
        self.assertFalse(Path(target, "debug", "deps").exists())


if __name__ == "__main__":
    unittest.main()
