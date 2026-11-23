#!/usr/bin/env python3
"""
test_canon_cache_units.py — stdlib unittest for scripts/canon_cache.py, the
cache half of the canonical-build-path primitive (dark-factory
plans/canonical-build-path-shared-cache-prd.md C4-C6): the per-policy cargo
and command environment, the host.toml cache settings, real-sccache
resolution and the fallback directory.

No case enters a mount namespace or starts a real sccache.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_canon_cache_units.sh.
"""

import json
import os
import re
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)

sys.path.insert(0, str(fx.REPO_ROOT / "scripts"))
import canon_cache  # noqa: E402

ORCHESTRATOR_CONFIG = fx.REPO_ROOT / "dark-factory-orchestrator.yaml"
CARGO_HOME_LINE = re.compile(r"^\s+CARGO_HOME:\s*(\S+)\s*$", re.MULTILINE)
GUARD_LINE = "#!/bin/sh\n# reify-canon-guard\nexit 0\n"
REAL_LINE = "#!/bin/sh\nexit 0\n"


def scratch(test):
    tmp = tempfile.TemporaryDirectory(prefix="test-canon-cache-units-")
    test.addCleanup(tmp.cleanup)
    return os.path.realpath(tmp.name)


def executable(directory, name, text):
    path = Path(directory, name)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(0o755)
    return str(path)


class CacheEnvironmentCase(unittest.TestCase):
    INPUT = {
        "RUSTC_WRAPPER": "x",
        "CARGO_BUILD_RUSTC_WRAPPER": "x",
        "CARGO_INCREMENTAL": "0",
        "SCCACHE_SERVER_PORT": "4226",
        "SCCACHE_WEBDAV_ENDPOINT": "e",
        "SCCACHE_DIR": "/x",
        "CARGO_HOME": "/elsewhere",
        "CARGO_TERM_COLOR": "always",
        "FOO": "1",
    }
    SERVER = canon_cache.ServerAddress(
        socket="/canon/target/.reify-sccache.1.sock",
        sccache="/real/bin/sccache",
        uncached_log="/canon/target/.reify-canon-uncached.1.log",
        fallback_dir="/fallback",
        fallback_cap="10G",
    )

    def environment(self, policy):
        server = None if policy is canon_cache.Policy.INCREMENTAL else self.SERVER
        return canon_cache.CacheEnvironment(policy, server)

    def assert_server_variables(self, env):
        self.assertEqual(env["SCCACHE_SERVER_UDS"], self.SERVER.socket)
        self.assertEqual(env["SCCACHE_DIR"], self.SERVER.fallback_dir)
        self.assertEqual(env["SCCACHE_CACHE_SIZE"], self.SERVER.fallback_cap)
        self.assertEqual(env["SCCACHE_IDLE_TIMEOUT"], "1800")
        self.assertEqual(env["REIFY_CANON_SCCACHE"], self.SERVER.sccache)
        self.assertEqual(env["REIFY_CANON_UNCACHED_LOG"], self.SERVER.uncached_log)
        self.assertNotIn("SCCACHE_SERVER_PORT", env)
        self.assertEqual([name for name in env if name.startswith("SCCACHE_WEBDAV_")], [])


class ForCargo(CacheEnvironmentCase):
    def test_shared(self):
        env = self.environment(canon_cache.Policy.SHARED).for_cargo(self.INPUT)
        self.assert_server_variables(env)
        self.assertEqual(env["RUSTC_WRAPPER"], "sccache")
        self.assertEqual(env["CARGO_INCREMENTAL"], "0")
        self.assertNotIn("CARGO_BUILD_RUSTC_WRAPPER", env)
        self.assertEqual(env["CARGO_HOME"], canon_cache.PINNED_CARGO_HOME)
        self.assertEqual(env["FOO"], "1")
        self.assertEqual(env["CARGO_TERM_COLOR"], "always")

    def test_mixed_differs_from_shared_only_in_incremental(self):
        shared = self.environment(canon_cache.Policy.SHARED).for_cargo(self.INPUT)
        mixed = self.environment(canon_cache.Policy.MIXED).for_cargo(self.INPUT)
        self.assertEqual(mixed["CARGO_INCREMENTAL"], "1")
        self.assertEqual(dict(mixed, CARGO_INCREMENTAL="0"), shared)

    def test_incremental(self):
        env = self.environment(canon_cache.Policy.INCREMENTAL).for_cargo(self.INPUT)
        for name in ("RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"):
            self.assertNotIn(name, env)
        self.assertEqual([name for name in env if name.startswith("SCCACHE_")], [])
        self.assertEqual(env["CARGO_INCREMENTAL"], "1")
        self.assertEqual(env["CARGO_HOME"], canon_cache.PINNED_CARGO_HOME)
        self.assertEqual(env["FOO"], "1")

    def test_input_is_never_mutated(self):
        for policy in canon_cache.Policy:
            with self.subTest(policy=policy):
                before = dict(self.INPUT)
                self.environment(policy).for_cargo(before)
                self.environment(policy).for_command(before)
                self.assertEqual(before, self.INPUT)


class ForCommand(CacheEnvironmentCase):
    def test_shared_and_mixed(self):
        for policy in (canon_cache.Policy.SHARED, canon_cache.Policy.MIXED):
            with self.subTest(policy=policy):
                env = self.environment(policy).for_command(self.INPUT)
                self.assert_server_variables(env)
                self.assertEqual(env["RUSTC_WRAPPER"], "x")
                self.assertEqual(env["CARGO_BUILD_RUSTC_WRAPPER"], "x")
                self.assertEqual(env["CARGO_INCREMENTAL"], "0")
                self.assertEqual(env["CARGO_HOME"], canon_cache.PINNED_CARGO_HOME)
                self.assertEqual(env["FOO"], "1")

    def test_incremental_changes_only_cargo_home(self):
        env = self.environment(canon_cache.Policy.INCREMENTAL).for_command(self.INPUT)
        self.assertEqual(env, dict(self.INPUT, CARGO_HOME=canon_cache.PINNED_CARGO_HOME))

    def test_uses_guard_for_shared_and_mixed_only(self):
        self.assertTrue(self.environment(canon_cache.Policy.SHARED).uses_guard)
        self.assertTrue(self.environment(canon_cache.Policy.MIXED).uses_guard)
        self.assertFalse(self.environment(canon_cache.Policy.INCREMENTAL).uses_guard)


class Record(CacheEnvironmentCase):
    def test_round_trips_through_json(self):
        for policy in canon_cache.Policy:
            with self.subTest(policy=policy):
                environment = self.environment(policy)
                text = json.dumps(environment.to_record())
                self.assertEqual(canon_cache.CacheEnvironment.from_record(json.loads(text)), environment)

    def test_malformed_input_is_none(self):
        good = self.environment(canon_cache.Policy.SHARED).to_record()
        for value in (
            None,
            [],
            "shared",
            {},
            dict(good, policy="fast"),
            dict(good, server="x"),
            dict(good, server=dict(good["server"], socket=7)),
            {key: value for key, value in good.items() if key != "server"},
        ):
            with self.subTest(value=value):
                self.assertIsNone(canon_cache.CacheEnvironment.from_record(value))


class PinnedCargoHome(unittest.TestCase):
    """OQ2: the pin is the orchestrator's CARGO_HOME, in verify_env and &sandbox_cache_env."""

    def test_pin_equals_every_orchestrator_cargo_home(self):
        values = CARGO_HOME_LINE.findall(ORCHESTRATOR_CONFIG.read_text())
        self.assertGreaterEqual(len(values), 2, values)
        self.assertEqual(set(values), {canon_cache.PINNED_CARGO_HOME})


class ParseCacheSettings(unittest.TestCase):
    PATH = Path("/home/x/.config/reify-canon/host.toml")

    def test_empty_table_has_defaults(self):
        settings = canon_cache.parse_cache_settings({}, self.PATH)
        self.assertIsNone(settings.store_url)
        self.assertEqual(
            settings.fallback_dir, os.path.join(os.path.expanduser("~"), ".cache", "reify-canon", "fallback")
        )
        self.assertEqual(settings.fallback_cap, "10G")

    def test_all_five_keys_parse(self):
        settings = canon_cache.parse_cache_settings(
            {
                "store_url": "http://store.lan:8080/reify",
                "store_user": "reify",
                "store_password": "hunter2",
                "fallback_dir": "/var/cache/reify-canon",
                "fallback_cap": "50G",
            },
            self.PATH,
        )
        self.assertEqual(settings.store_url, "http://store.lan:8080/reify")
        self.assertEqual(settings.store_user, "reify")
        self.assertEqual(settings.store_password, "hunter2")
        self.assertEqual(settings.fallback_dir, "/var/cache/reify-canon")
        self.assertEqual(settings.fallback_cap, "50G")

    def test_bad_values_raise_one_line_naming_key_value_and_path(self):
        for key, value in (
            ("store_url", "ftp://x"),
            ("store_url", "https://store.invalid/reify"),
            ("store_url", 7),
            ("store_user", ["a"]),
            ("store_password", 1),
            ("fallback_dir", True),
            ("fallback_cap", 10),
        ):
            with self.subTest(key=key, value=value):
                with self.assertRaises(canon_cache.CacheConfigError) as caught:
                    canon_cache.parse_cache_settings({key: value}, self.PATH)
                message = str(caught.exception)
                self.assertNotIn("\n", message)
                for needle in (key, repr(value), str(self.PATH)):
                    self.assertIn(needle, message)

    def test_password_is_not_in_repr(self):
        settings = canon_cache.parse_cache_settings(
            {"store_url": "http://s/r", "store_password": "hunter2"}, self.PATH
        )
        self.assertNotIn("hunter2", repr(settings))


class ResolveRealSccache(unittest.TestCase):
    def setUp(self):
        self.tmp = scratch(self)
        self.guard = os.path.dirname(executable(self.tmp, "guard/sccache", GUARD_LINE))
        self.real = os.path.dirname(executable(self.tmp, "real/sccache", REAL_LINE))
        self.second = os.path.dirname(executable(self.tmp, "second/sccache", REAL_LINE))
        self.plain = os.path.join(self.tmp, "not-executable")
        os.mkdir(self.plain)
        Path(self.plain, "sccache").write_text(REAL_LINE)

    def resolve(self, *entries):
        return canon_cache.resolve_real_sccache(os.pathsep.join(entries))

    def test_first_unmarked_executable(self):
        self.assertEqual(self.resolve(self.real, self.second), os.path.join(self.real, "sccache"))

    def test_marked_copy_earlier_on_path_is_skipped(self):
        self.assertEqual(self.resolve(self.guard, self.plain, self.real), os.path.join(self.real, "sccache"))

    def test_none_when_only_marked_copies_exist(self):
        self.assertIsNone(self.resolve(self.guard, self.plain))
        self.assertIsNone(self.resolve())


class ChooseFallbackDir(unittest.TestCase):
    def setUp(self):
        self.tmp = scratch(self)

    def settings(self, fallback_dir):
        return canon_cache.parse_cache_settings({"fallback_dir": fallback_dir}, Path(self.tmp, "host.toml"))

    def test_configured_directory_when_writable(self):
        configured = os.path.join(self.tmp, "cache", "fallback")
        chosen = canon_cache.choose_fallback_dir(self.settings(configured), tmp_root=self.tmp)
        self.assertEqual(chosen, configured)
        self.assertTrue(os.path.isdir(configured))

    def test_tmp_fallback_when_configured_parent_is_read_only(self):
        if os.geteuid() == 0:
            self.skipTest("root writes into a read-only directory regardless of its mode")
        parent = Path(self.tmp, "read-only")
        parent.mkdir()
        parent.chmod(0o500)
        self.addCleanup(parent.chmod, 0o755)
        chosen = canon_cache.choose_fallback_dir(self.settings(str(parent / "fallback")), tmp_root=self.tmp)
        self.assertEqual(chosen, os.path.join(self.tmp, f"reify-canon-fallback-{os.getuid()}"))
        self.assertTrue(os.path.isdir(chosen))


class PolicyMembers(unittest.TestCase):
    def test_members(self):
        self.assertEqual([policy.value for policy in canon_cache.Policy], ["shared", "mixed", "incremental"])


if __name__ == "__main__":
    unittest.main()
