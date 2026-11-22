#!/usr/bin/env python3
"""
test_canon_run_units.py — stdlib unittest for scripts/canon_run.py that needs
no mount namespace (canonical-build-path PRD, dark-factory
plans/canonical-build-path-shared-cache-prd.md: C1's mode table, C2, C3's bound
check, C4's `incremental` row, C7's trampoline; boundary rows 1-4, 9 and 11).

CANON is unprovisioned or the host disabled in every case, so no bwrap probe
runs and the suite is deterministic.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_canon_run_units.sh.
"""

import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)

canon_run = fx.load_canon_run()

STATUS_KEYS = {
    "canon_root",
    "canon_provisioned",
    "host_config",
    "host_enabled",
    "policy",
    "tree_root",
    "build_dir",
    "build_dir_mode",
    "reason",
}
BETA2_KEYS = (
    'store_url = "https://store.invalid/reify"',
    'store_user = "reify"',
    'store_password = "hunter2"',
    'fallback_dir = "/var/cache/reify-canon"',
    'fallback_cap = "50G"',
)


def scratch(test):
    tmp = tempfile.TemporaryDirectory(prefix="test-canon-run-units-")
    test.addCleanup(tmp.cleanup)
    return os.path.realpath(tmp.name)


def stamp(build_dir, text):
    Path(build_dir).mkdir(parents=True, exist_ok=True)
    Path(build_dir, ".reify-build-root").write_text(text + "\n")


def mountinfo_line(mount_point):
    escaped = mount_point.replace("\\", "\\134").replace(" ", "\\040")
    return f"36 35 0:51 / {escaped} rw,relatime shared:1 - ext4 /dev/vda1 rw\n"


def never_probed():
    raise AssertionError("reachability was computed for a row that does not need it")


def never_read_host():
    raise AssertionError("host.toml was read for a row that does not need it")


class HostConfig(unittest.TestCase):
    def setUp(self):
        self.tmp = scratch(self)
        self.path = Path(self.tmp, "host.toml")

    def assert_refusal_names(self, *needles):
        with self.assertRaises(canon_run.Refusal) as caught:
            canon_run.load_host_config(self.path)
        message = str(caught.exception)
        self.assertNotIn("\n", message)
        for needle in (str(self.path), *needles):
            self.assertIn(needle, message)

    def test_absent_file_is_disabled_shared(self):
        config = canon_run.load_host_config(self.path)
        self.assertFalse(config.enabled)
        self.assertIs(config.policy, canon_run.Policy.SHARED)

    def test_enabled_incremental(self):
        fx.write_host_config(self.path, enabled=True, policy="incremental")
        config = canon_run.load_host_config(self.path)
        self.assertTrue(config.enabled)
        self.assertIs(config.policy, canon_run.Policy.INCREMENTAL)

    def test_beta2_keys_are_accepted_and_ignored(self):
        fx.write_host_config(self.path, enabled=True, policy="mixed", extra_lines=BETA2_KEYS)
        config = canon_run.load_host_config(self.path)
        self.assertTrue(config.enabled)
        self.assertIs(config.policy, canon_run.Policy.MIXED)

    def test_unknown_policy_refuses(self):
        fx.write_host_config(self.path, enabled=True, policy="fast")
        self.assert_refusal_names("policy", "fast")

    def test_non_bool_enabled_refuses(self):
        fx.write_host_config(self.path, extra_lines=('enabled = "yes"',))
        self.assert_refusal_names("enabled", "yes")

    def test_unparseable_toml_refuses(self):
        self.path.write_text("enabled = \npolicy = [\n")
        self.assert_refusal_names()


class BuildDirState(unittest.TestCase):
    def setUp(self):
        self.build = Path(scratch(self), "target")

    def test_stamp_is_content_without_trailing_newline(self):
        stamp(self.build, "/opt/reify-canon/ws")
        self.assertEqual(canon_run.read_build_dir_state(self.build).stamp, "/opt/reify-canon/ws")

    def test_no_stamp_file_is_none(self):
        self.build.mkdir()
        self.assertIsNone(canon_run.read_build_dir_state(self.build).stamp)

    def test_profile_fingerprint_is_built_in(self):
        (self.build / "debug" / ".fingerprint").mkdir(parents=True)
        self.assertTrue(canon_run.read_build_dir_state(self.build).built_in)

    def test_triple_profile_fingerprint_is_built_in(self):
        (self.build / "x86_64-unknown-linux-gnu" / "debug" / ".fingerprint").mkdir(parents=True)
        self.assertTrue(canon_run.read_build_dir_state(self.build).built_in)

    def test_missing_dir_is_fresh(self):
        state = canon_run.read_build_dir_state(self.build)
        self.assertIsNone(state.stamp)
        self.assertFalse(state.built_in)

    def test_empty_dir_is_fresh(self):
        self.build.mkdir()
        self.assertFalse(canon_run.read_build_dir_state(self.build).built_in)

    def test_depth_one_bookkeeping_files_are_fresh(self):
        self.build.mkdir()
        (self.build / ".tree-sitter-freshness.epoch").write_text("0\n")
        (self.build / ".rustc_info.json").write_text("{}\n")
        self.assertFalse(canon_run.read_build_dir_state(self.build).built_in)


class DecideMode(unittest.TestCase):
    CANON = "/opt/reify-canon/ws"
    REACHABLE = (canon_run.Route.BOUND, canon_run.Route.NAMESPACE)

    def decide(self, *, stamp=None, built_in=False, enabled=None, route=None):
        """`None` for enabled or route pins that the row never reads it."""
        state = canon_run.BuildDirState(stamp=stamp, built_in=built_in)
        if enabled is None:
            host_enabled = never_read_host
        else:
            def host_enabled():
                return enabled
        if route is None:
            reachability = never_probed
        else:
            def reachability():
                return canon_run.Reachability(route, f"probe says {route.name}")
        return canon_run.decide_mode(state, self.CANON, host_enabled, reachability)

    def test_stamped_canon_reachable_is_canon(self):
        for route in self.REACHABLE:
            with self.subTest(route=route):
                self.assertIs(self.decide(stamp=self.CANON, route=route).mode, canon_run.Mode.CANON)

    def test_stamped_canon_unreachable_refuses_naming_canon_and_reason(self):
        decision = self.decide(stamp=self.CANON, route=canon_run.Route.NONE)
        self.assertIs(decision.mode, canon_run.Mode.REFUSED)
        self.assertIn(self.CANON, decision.reason)
        self.assertIn("probe says NONE", decision.reason)

    def test_stamped_other_text_refuses_quoting_it(self):
        decision = self.decide(stamp="/somewhere/else")
        self.assertIs(decision.mode, canon_run.Mode.REFUSED)
        self.assertIn("/somewhere/else", decision.reason)

    def test_unstamped_built_in_is_real_path_without_reading_host_or_probing(self):
        decision = self.decide(built_in=True)
        self.assertIs(decision.mode, canon_run.Mode.REAL_PATH)

    def test_fresh_on_disabled_host_is_real_path(self):
        self.assertIs(self.decide(enabled=False).mode, canon_run.Mode.REAL_PATH)

    def test_fresh_enabled_unreachable_is_real_path(self):
        decision = self.decide(enabled=True, route=canon_run.Route.NONE)
        self.assertIs(decision.mode, canon_run.Mode.REAL_PATH)

    def test_fresh_enabled_reachable_is_canon(self):
        for route in self.REACHABLE:
            with self.subTest(route=route):
                self.assertIs(self.decide(enabled=True, route=route).mode, canon_run.Mode.CANON)


class CanonBoundTo(unittest.TestCase):
    def setUp(self):
        tmp = scratch(self)
        self.tree = os.path.join(tmp, "tree with space")
        self.other = os.path.join(tmp, "other")
        os.mkdir(self.tree)
        os.mkdir(self.other)

    def test_listed_mount_of_the_tree_is_bound(self):
        self.assertTrue(
            canon_run.canon_bound_to(self.tree, self.tree, mountinfo_line(self.tree))
        )

    def test_unlisted_canon_is_not_bound(self):
        self.assertFalse(
            canon_run.canon_bound_to(self.tree, self.tree, mountinfo_line(self.other))
        )

    def test_listed_mount_of_another_directory_is_not_bound(self):
        self.assertFalse(
            canon_run.canon_bound_to(self.other, self.tree, mountinfo_line(self.other))
        )

    def test_symlink_to_the_tree_is_not_bound(self):
        link = os.path.join(os.path.dirname(self.tree), "link")
        os.symlink(self.tree, link)
        self.assertFalse(canon_run.canon_bound_to(link, self.tree, mountinfo_line(link)))


class Status(unittest.TestCase):
    def setUp(self):
        self.tmp = scratch(self)
        self.tree = fx.make_tree(self.tmp, "tree")
        self.target = Path(self.tree, "target")
        self.env = fx.scratch_env(self.tmp)
        self.canon = self.env["REIFY_CANON_ROOT"]
        self.host = self.env["REIFY_CANON_HOST_CONFIG"]

    def status(self, cwd=None):
        result = fx.run_canon(self.tree, cwd or self.tree, "status", env=self.env)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(set(report), STATUS_KEYS)
        self.assertEqual(report["canon_root"], self.canon)
        return report

    def test_unstamped_built_in_target_is_real_path(self):
        (self.target / "debug" / ".fingerprint").mkdir(parents=True)
        report = self.status()
        self.assertEqual(report["build_dir_mode"], "real_path")
        self.assertEqual(report["tree_root"], self.tree)
        self.assertEqual(report["build_dir"], str(self.target))

    def test_stamp_with_other_text_is_refused(self):
        stamp(self.target, "/somewhere/else")
        report = self.status()
        self.assertEqual(report["build_dir_mode"], "refused")
        self.assertTrue(report["reason"])

    def test_fresh_target_on_enabled_host_with_unprovisioned_canon_is_real_path(self):
        fx.write_host_config(self.host, enabled=True, policy="incremental")
        report = self.status()
        self.assertEqual(report["build_dir_mode"], "real_path")
        self.assertIs(report["canon_provisioned"], False)
        self.assertIs(report["host_enabled"], True)
        self.assertFalse(Path(self.target, ".reify-build-root").exists())

    def test_stamp_equal_to_unprovisioned_canon_is_refused(self):
        stamp(self.target, self.canon)
        self.assertEqual(self.status()["build_dir_mode"], "refused")

    def test_policy_echoes_host_config(self):
        fx.write_host_config(self.host, enabled=False, policy="mixed")
        report = self.status()
        self.assertEqual(report["policy"], "mixed")
        self.assertEqual(report["host_config"], self.host)

    def test_cwd_outside_any_tree(self):
        outside = Path(self.tmp, "outside")
        outside.mkdir()
        report = self.status(cwd=outside)
        self.assertIsNone(report["tree_root"])
        self.assertEqual(report["build_dir_mode"], "real_path")

    def test_malformed_host_config_refuses_with_one_line(self):
        Path(self.host).write_text("enabled = \n")
        result = fx.run_canon(self.tree, self.tree, "status", env=self.env)
        self.assertEqual(result.returncode, 126)
        lines = result.stderr.splitlines()
        self.assertEqual(len(lines), 1, result.stderr)
        self.assertTrue(lines[0].startswith("canon_run: "), lines[0])


class CliCase(unittest.TestCase):
    """A scratch tree, a recording fake cargo and a PATH routing cargo through the tree's trampoline."""

    def setUp(self):
        self.tmp = scratch(self)
        self.tree = fx.make_tree(self.tmp, "tree")
        self.trampoline = os.path.join(self.tree, "scripts", "canon-bin", "cargo")
        self.fake = fx.fake_cargo_dir(self.tmp)
        self.fake_cargo = os.path.join(self.fake, "cargo")
        self.sub = os.path.join(self.tree, "sub")
        os.mkdir(self.sub)
        self.env = fx.scratch_env(
            self.tmp, PATH=f"{self.tree}/scripts/canon-bin:{self.fake}:/usr/bin:/bin"
        )
        self.target = os.path.join(self.tree, "target")

    def cargo(self, *args, cwd=None):
        return fx.run([self.trampoline, *args], cwd=cwd or self.sub, env=self.env)

    def assert_one_untouched_call(self, result, args, *, cwd=None, argv0=None):
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = fx.read_calls(self.fake)
        self.assertEqual(len(calls), 1, calls)
        self.assertEqual(calls[0]["argv"], [argv0 or self.fake_cargo, *args])
        self.assertEqual(calls[0]["cwd"], cwd or self.sub)
        self.assertEqual(calls[0]["env"], self.env)

    def assert_refused(self, result, *needles):
        self.assertEqual(result.returncode, 126, result.stderr)
        lines = result.stderr.splitlines()
        self.assertEqual(len(lines), 1, result.stderr)
        self.assertTrue(lines[0].startswith("canon_run: "), lines[0])
        for needle in needles:
            self.assertIn(needle, lines[0])
        self.assertEqual(fx.read_calls(self.fake), [])


class RealPathIsUntouched(CliCase):
    """Boundary row 9 at unit level, and C1's last row: the command gets the caller's exact env."""

    def test_trampoline_on_disabled_host_runs_real_cargo_untouched(self):
        result = self.cargo("build", "--offline")
        self.assert_one_untouched_call(result, ["build", "--offline"])
        self.assertFalse(os.path.exists(os.path.join(self.target, ".reify-build-root")))

    def test_exec_on_disabled_host_runs_command_untouched(self):
        result = fx.run_canon(self.tree, self.sub, "exec", "--", self.fake_cargo, "x", env=self.env)
        self.assert_one_untouched_call(result, ["x"])
        self.assertFalse(os.path.exists(os.path.join(self.target, ".reify-build-root")))

    def test_build_dir_outside_the_tree_is_real_path(self):
        stamp(self.target, "/somewhere/else")
        self.env["CARGO_TARGET_DIR"] = os.path.join(self.tmp, "elsewhere")
        result = fx.run_canon(self.tree, self.sub, "exec", "--", self.fake_cargo, "x", env=self.env)
        self.assert_one_untouched_call(result, ["x"])

    def test_trampoline_outside_any_tree_runs_real_cargo_untouched(self):
        plain = os.path.join(self.tmp, "plain")
        os.mkdir(plain)
        result = self.cargo("build", "-p", "x", cwd=plain)
        self.assert_one_untouched_call(result, ["build", "-p", "x"], cwd=plain)


class CargoPassesThrough(unittest.TestCase):
    """Decision 10's pass-through list (boundary row 2), pure."""

    def setUp(self):
        self.tree = scratch(self)

    def passes(self, args, **environ):
        return canon_run.cargo_passes_through(args, environ, self.tree, Path(self.tree))

    def test_pass_through(self):
        for args in (
            ["metadata"],
            ["+1.96.0", "--offline", "metadata"],
            ["--config", "k=v", "tree"],
            ["-Zfoo", "fetch"],
            ["--version"],
            [],
            ["install", "ripgrep"],
        ):
            with self.subTest(args=args):
                self.assertTrue(self.passes(args))

    def test_not_pass_through(self):
        for args in (
            ["install", "--path", "."],
            ["install", "--path=."],
            ["install", "--target-dir", f"{self.tree}/t2", "x"],
            ["t"],
            ["frobnicate"],
            ["build"],
        ):
            with self.subTest(args=args):
                self.assertFalse(self.passes(args))

    def test_install_with_in_tree_target_dir_env_is_not_pass_through(self):
        for name in ("CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"):
            with self.subTest(name=name):
                self.assertFalse(self.passes(["install", "x"], **{name: f"{self.tree}/t3"}))


class PassThroughBehaviour(CliCase):
    """Boundary row 2: in a tree stamped for an unreachable CANON, only the list reaches cargo."""

    def setUp(self):
        super().setUp()
        stamp(self.target, self.env["REIFY_CANON_ROOT"])

    def test_listed_subcommands_reach_real_cargo_untouched(self):
        for args in (["metadata"], ["install", "ripgrep"]):
            with self.subTest(args=args):
                Path(self.fake, "calls.jsonl").unlink(missing_ok=True)
                self.assert_one_untouched_call(self.cargo(*args), args)

    def test_other_subcommands_are_refused(self):
        for args in (["t"], ["install", "--path", "."], ["frobnicate"]):
            with self.subTest(args=args):
                self.assert_refused(self.cargo(*args))


class Refusals(CliCase):
    """C1: a stamp naming another path, or CANON where it cannot be provided, exits 126."""

    def assert_exec_refused(self):
        marker = os.path.join(self.tmp, "marker")
        result = fx.run_canon(self.tree, self.sub, "exec", "--", "touch", marker, env=self.env)
        self.assert_refused(result, self.target)
        self.assertFalse(os.path.exists(marker))

    def test_stamp_naming_another_path(self):
        stamp(self.target, "/somewhere/else")
        self.assert_exec_refused()

    def test_stamp_naming_unprovisioned_canon(self):
        stamp(self.target, self.env["REIFY_CANON_ROOT"])
        self.assert_exec_refused()


class ResolveRealCargo(CliCase):
    def setUp(self):
        super().setUp()
        self.other_trampolines = os.path.join(self.tmp, "installed-canon-bin")
        os.mkdir(self.other_trampolines)
        shutil.copy2(fx.TRAMPOLINE, os.path.join(self.other_trampolines, "cargo"))
        self.second_fake = fx.fake_cargo_dir(self.tmp, "second-fake-bin")
        self.tree_bin = os.path.join(self.tree, "scripts", "canon-bin")

    def resolve(self, *entries):
        return canon_run.resolve_real_cargo(os.pathsep.join(entries))

    def test_first_unmarked_after_the_first_trampoline(self):
        self.assertEqual(self.resolve(self.tree_bin, self.fake, self.second_fake), self.fake_cargo)
        self.assertEqual(
            self.resolve(self.fake, self.tree_bin, self.second_fake),
            os.path.join(self.second_fake, "cargo"),
        )

    def test_every_trampoline_is_skipped(self):
        self.assertEqual(
            self.resolve(self.tree_bin, self.other_trampolines, self.fake), self.fake_cargo
        )

    def test_first_cargo_when_none_is_marked(self):
        self.assertEqual(self.resolve(self.fake, self.second_fake), self.fake_cargo)

    def test_non_executable_cargo_is_skipped(self):
        plain = os.path.join(self.tmp, "not-executable")
        os.mkdir(plain)
        Path(plain, "cargo").write_text("#!/bin/sh\n")
        self.assertEqual(self.resolve(self.tree_bin, plain, self.fake), self.fake_cargo)

    def test_none_without_an_unmarked_cargo(self):
        self.assertIsNone(self.resolve(self.tree_bin, self.other_trampolines))
        self.assertIsNone(self.resolve())

    def test_two_trampolines_on_path_do_not_loop(self):
        self.env["PATH"] = os.pathsep.join((self.tree_bin, self.other_trampolines, self.fake))
        self.env["FAKE_CARGO_EXIT"] = "7"
        plain = os.path.join(self.tmp, "plain")
        os.mkdir(plain)
        for cwd in (self.sub, plain):
            with self.subTest(cwd=cwd):
                Path(self.fake, "calls.jsonl").unlink(missing_ok=True)
                result = self.cargo("build", cwd=cwd)
                self.assertEqual(result.returncode, 7, result.stderr)
                self.assertEqual(len(fx.read_calls(self.fake)), 1)

    def test_no_real_cargo_exits_127_with_one_line(self):
        self.env["PATH"] = os.pathsep.join((self.tree_bin, self.other_trampolines))
        plain = os.path.join(self.tmp, "plain")
        os.mkdir(plain)
        for cwd in (self.sub, plain):
            with self.subTest(cwd=cwd):
                result = self.cargo("build", cwd=cwd)
                self.assertEqual(result.returncode, 127, result.stderr)
                self.assertEqual(len(result.stderr.splitlines()), 1, result.stderr)


class Usage(CliCase):
    def test_usage_errors_exit_2(self):
        for args in ([], ["exec", "true"], ["exec", "--"], ["frobnicate"]):
            with self.subTest(args=args):
                result = fx.run_canon(self.tree, self.sub, *args, env=self.env)
                self.assertEqual(result.returncode, 2, result.stderr)


if __name__ == "__main__":
    unittest.main()
