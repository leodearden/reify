#!/usr/bin/env python3
"""
test_canon_run_units.py — stdlib unittest for scripts/canon_run.py that needs
no mount namespace (canonical-build-path PRD, dark-factory
plans/canonical-build-path-shared-cache-prd.md: C1's mode table, C2, C3's bound
check, C4's policy table as composed at CANON, C6's records of real-path and
CANON invocations, C7's trampoline; boundary rows 1-4, 9 and 11).

No case probes or enters a mount namespace, so the suite is deterministic.
One CanonCargo case fills CANON with a copy of the tree, and only
canon_invocation sees it. The BoundRoute cases run the BOUND route with
CANON set to the tree's own path and one extra mountinfo line (the fixture's
BOUND_RUNNER), against a fake sccache.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_canon_run_units.sh.
"""

import errno
import fcntl
import importlib.util
import json
import os
import resource
import shutil
import socket
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_cache_fixture as cfx  # noqa: E402  (imported after sys.path manipulation)
import canon_run_fixture as fx  # noqa: E402
from verify_ledger_env import hermetic_env  # noqa: E402

sys.path.insert(0, str(fx.REPO_ROOT / "scripts"))
from verify_events_lib import read_events  # noqa: E402

canon_run = fx.load_canon_run()
canon_cache = fx.load_canon_cache()

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
    "store_reachable",
}
CACHE_KEYS = (
    'store_url = "http://store.invalid/reify"',
    'store_user = "reify"',
    'store_password = "hunter2"',
    'fallback_dir = "/var/cache/reify-canon"',
    'fallback_cap = "50G"',
)
SERVER = canon_cache.ServerAddress(
    socket="/canon/target/.reify-sccache.1.sock",
    sccache="/real/bin/sccache",
    uncached_log="/canon/target/.reify-canon-uncached.1.log",
    fallback_dir="/fallback",
    fallback_cap="10G",
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
        self.assertIs(config.policy, canon_cache.Policy.SHARED)
        self.assertEqual(config.cache, canon_cache.CacheSettings())

    def test_enabled_incremental(self):
        fx.write_host_config(self.path, enabled=True, policy="incremental")
        config = canon_run.load_host_config(self.path)
        self.assertTrue(config.enabled)
        self.assertIs(config.policy, canon_cache.Policy.INCREMENTAL)

    def test_cache_keys_are_parsed(self):
        fx.write_host_config(self.path, enabled=True, policy="mixed", extra_lines=CACHE_KEYS)
        config = canon_run.load_host_config(self.path)
        self.assertTrue(config.enabled)
        self.assertIs(config.policy, canon_cache.Policy.MIXED)
        self.assertEqual(
            config.cache,
            canon_cache.CacheSettings(
                store_url="http://store.invalid/reify",
                store_user="reify",
                store_password="hunter2",
                fallback_dir="/var/cache/reify-canon",
                fallback_cap="50G",
            ),
        )

    def test_non_http_store_url_refuses(self):
        fx.write_host_config(self.path, enabled=True, extra_lines=('store_url = "ftp://x"',))
        self.assert_refusal_names("store_url", "ftp://x")

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

    def test_stamped_canon_unreachable_refuses_naming_canon_reason_and_both_fixes(self):
        decision = self.decide(stamp=self.CANON, route=canon_run.Route.NONE)
        self.assertIs(decision.mode, canon_run.Mode.REFUSED)
        for needle in (self.CANON, "probe says NONE", "mount namespace", "delete"):
            self.assertIn(needle, decision.reason)

    def test_stamped_other_text_refuses_quoting_it_and_names_only_the_delete_fix(self):
        decision = self.decide(stamp="/somewhere/else")
        self.assertIs(decision.mode, canon_run.Mode.REFUSED)
        self.assertIn("/somewhere/else", decision.reason)
        self.assertIn("delete", decision.reason)
        self.assertNotIn("namespace", decision.reason)

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

    def test_the_two_reportable_real_path_rows_carry_their_cache_reason(self):
        self.assertIs(self.decide(built_in=True).cache_reason, canon_cache.Reason.LEGACY_BUILD_DIR)
        self.assertIs(
            self.decide(enabled=True, route=canon_run.Route.NONE).cache_reason,
            canon_cache.Reason.CANON_UNAVAILABLE,
        )

    def test_every_other_row_has_no_cache_reason(self):
        for row in (
            {"stamp": self.CANON, "route": canon_run.Route.BOUND},
            {"stamp": self.CANON, "route": canon_run.Route.NAMESPACE},
            {"stamp": self.CANON, "route": canon_run.Route.NONE},
            {"stamp": "/somewhere/else"},
            {"enabled": False},
            {"enabled": True, "route": canon_run.Route.BOUND},
            {"enabled": True, "route": canon_run.Route.NAMESPACE},
        ):
            with self.subTest(row=row):
                self.assertIs(self.decide(**row).cache_reason, canon_cache.Reason.NONE)


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

    def test_store_reachable_is_null_without_store_url(self):
        fx.write_host_config(self.host, enabled=True)
        self.assertIsNone(self.status()["store_reachable"])

    def test_store_reachable_is_false_for_a_closed_loopback_port(self):
        with socket.socket() as closed:
            closed.bind(("127.0.0.1", 0))
            port = closed.getsockname()[1]
        fx.write_host_config(
            self.host, enabled=True, extra_lines=(f'store_url = "http://127.0.0.1:{port}/reify"',)
        )
        self.assertIs(self.status()["store_reachable"], False)

    def test_default_shared_host_is_not_refused_for_its_policy(self):
        fx.write_host_config(self.host, enabled=True)
        report = self.status()
        self.assertEqual(report["policy"], "shared")
        self.assertEqual(report["build_dir_mode"], "real_path", report["reason"])

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


STARTUP_IGNORED = {signal.SIGPIPE, signal.SIGXFSZ}


def sigign_probe(out):
    """A command writing its SigIgn line to out, for fx.ignored_signals."""
    return ("/bin/sh", "-c", 'grep SigIgn /proc/self/status > "$0"', out)


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

    def test_exec_hands_the_command_sigpipe_and_sigxfsz_at_their_defaults(self):
        """CPython ignores both at startup; the command must not inherit that."""
        out = os.path.join(self.tmp, "sigign")
        result = fx.run_canon(self.tree, self.sub, "exec", "--", *sigign_probe(out), env=self.env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(STARTUP_IGNORED & fx.ignored_signals(out), set())

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


# A caller at default dispositions (a pool member may inherit INT/QUIT ignored)
# that runs its arguments, then echoes CONTINUED with their status.
BASH_CALLER = (*fx.AT_DEFAULT_SUPERVISED_SIGNALS, "/bin/bash", "-c", '"$@"; echo "CONTINUED rc=$?"', "bash")


def journal_rows(build_dir):
    events = read_events(canon_cache.journal_path(build_dir))
    assert events.skipped_lines == 0, events
    return list(events.records)


class RealPathSupervision(CliCase):
    """C6's real_path record: on an enabled host, a legacy or CANON-less build is measured, never changed.

    CANON is unprovisioned here, so a fresh build directory is the canon_unavailable row.
    """

    def setUp(self):
        super().setUp()
        self.channel = os.path.join(self.tmp, "channel.jsonl")
        self.host = self.env["REIFY_CANON_HOST_CONFIG"]
        fx.write_host_config(self.host, enabled=True)

    def built_in(self):
        Path(self.target, "debug", ".fingerprint").mkdir(parents=True)

    def assert_supervised_call(self, result, argv_tail, *, status=0):
        """One call with the caller's env plus a real_path supervisor record naming the tree."""
        self.assertEqual(result.returncode, status, result.stderr)
        [call] = fx.read_calls(self.fake)
        self.assertEqual(call["argv"], [self.fake_cargo, *argv_tail])
        self.assertEqual(call["cwd"], self.sub)
        active = canon_run.parse_active(call["env"].pop("REIFY_CANON_ACTIVE", None))
        self.assertEqual(call["env"], self.env)
        tree_stat = os.stat(self.tree)
        self.assertEqual((active.dev, active.ino, active.real_root), (tree_stat.st_dev, tree_stat.st_ino, self.tree))
        self.assertEqual(active.supervisor, canon_run.Supervisor(canon_run.Mode.REAL_PATH, None))

    def assert_row(self, reason, subcommand="build"):
        [row] = journal_rows(self.target)
        expected = {
            "mode": "real_path", "policy": "shared", "backend": "none", "reason": reason,
            "subcommand": subcommand, "hits": 0, "misses": 0, "write_errors": 0, "store_attempts": 0,
            "detached_server": 0, "uncached_machine_flags": 0,
        }
        self.assertEqual({name: row[name] for name in expected}, expected)
        return row

    def assert_no_record(self):
        self.assertFalse(os.path.exists(canon_cache.journal_path(self.target)))
        self.assertFalse(os.path.exists(self.channel))

    def test_fresh_target_with_canon_unavailable_is_supervised_and_recorded(self):
        self.env["REIFY_VERIFY_EVENT_LOG"] = self.channel
        self.assert_supervised_call(self.cargo("build", "--offline"), ["build", "--offline"])
        self.assert_row("canon_unavailable")
        journal = Path(canon_cache.journal_path(self.target))
        self.assertEqual(Path(self.channel).read_bytes(), journal.read_bytes())
        self.assertFalse(os.path.exists(os.path.join(self.target, ".reify-build-root")))

    def test_unstamped_built_in_target_is_recorded_as_a_legacy_build_dir(self):
        self.built_in()
        self.assert_supervised_call(self.cargo("build"), ["build"])
        self.assert_row("legacy_build_dir")

    def test_exec_kind_records_an_empty_subcommand(self):
        result = fx.run_canon(self.tree, self.sub, "exec", "--", self.fake_cargo, "x", env=self.env)
        self.assert_supervised_call(result, ["x"])
        self.assert_row("canon_unavailable", subcommand="")

    def test_command_status_passes_through(self):
        self.env["FAKE_CARGO_EXIT"] = "5"
        self.assert_supervised_call(self.cargo("build"), ["build"], status=5)
        self.assert_row("canon_unavailable")

    def test_disabled_host_keeps_the_env_byte_identical_with_no_record(self):
        fx.write_host_config(self.host, enabled=False)
        self.built_in()
        self.env["REIFY_VERIFY_EVENT_LOG"] = self.channel
        self.assert_one_untouched_call(self.cargo("build"), ["build"])
        self.assert_no_record()

    def test_build_dir_outside_the_tree_is_untouched_with_no_record(self):
        elsewhere = os.path.join(self.tmp, "elsewhere")
        self.env.update(CARGO_TARGET_DIR=elsewhere, REIFY_VERIFY_EVENT_LOG=self.channel)
        self.assert_one_untouched_call(self.cargo("build"), ["build"])
        self.assert_no_record()
        self.assertFalse(os.path.exists(canon_cache.journal_path(elsewhere)))

    def test_nested_real_path_call_runs_untouched_with_no_second_row(self):
        self.built_in()
        tree = canon_run.find_tree(self.tree, {})
        supervisor = canon_run.Supervisor(canon_run.Mode.REAL_PATH, None)
        self.env.update(
            REIFY_CANON_ACTIVE=canon_run.format_active(tree, self.env["REIFY_CANON_ROOT"], supervisor),
            REIFY_VERIFY_EVENT_LOG=self.channel,
        )
        self.assert_one_untouched_call(self.cargo("build"), ["build"])
        self.assert_no_record()


class BoundRoute(CliCase):
    """C4-C6 on the BOUND route, with CANON the tree itself (fx.BOUND_RUNNER) and a fake sccache."""

    def setUp(self):
        super().setUp()
        self.sccache_dir = cfx.fake_sccache_dir(self.tmp)
        self.env.update(
            REIFY_CANON_ROOT=self.tree,
            PATH=f"{self.tree}/scripts/canon-bin:{self.fake}:{self.sccache_dir}:/usr/bin:/bin",
            FAKE_SCCACHE_STATS=json.dumps({"hits": {"Rust": 4}, "misses": {"C/C++": 1}, "write_errors": 0}),
        )
        self.host = self.env["REIFY_CANON_HOST_CONFIG"]
        self.enable("shared")

    def enable(self, policy):
        fallback = os.path.join(self.tmp, "fallback")
        fx.write_host_config(self.host, enabled=True, policy=policy, extra_lines=(f'fallback_dir = "{fallback}"',))

    def bound(self, *args):
        return fx.run_bound(self.tree, self.sub, *args, env=self.env, runner_dir=self.tmp)

    def one_call(self, result, status=0):
        self.assertEqual(result.returncode, status, result.stderr)
        [call] = fx.read_calls(self.fake)
        self.assertEqual(call["cwd"], self.sub)
        return call

    def leftovers(self):
        return sorted(name for name in os.listdir(self.target) if name.startswith(".reify-sccache."))

    def test_shared_cargo_runs_under_a_private_server_and_is_recorded(self):
        call = self.one_call(self.bound("cargo", "build"))
        env = call["env"]
        self.assertEqual(call["argv"], [self.fake_cargo, "build"])
        self.assertRegex(env["SCCACHE_SERVER_UDS"], rf"^{self.target}/\.reify-sccache\.[0-9]+\.sock$")
        self.assertEqual((env["RUSTC_WRAPPER"], env["CARGO_INCREMENTAL"]), ("sccache", "0"))
        self.assertEqual(env["CARGO_HOME"], canon_cache.PINNED_CARGO_HOME)
        self.assertEqual(env["REIFY_CANON_SCCACHE"], os.path.join(self.sccache_dir, "sccache"))
        supervisor = canon_run.parse_active(env["REIFY_CANON_ACTIVE"]).supervisor
        self.assertEqual(supervisor.mode, canon_run.Mode.CANON)
        self.assertEqual(supervisor.cache.policy, canon_cache.Policy.SHARED)
        self.assertEqual(supervisor.cache.server.socket, env["SCCACHE_SERVER_UDS"])
        self.assertEqual(len(cfx.servers(self.sccache_dir)), 1)
        [row] = journal_rows(self.target)
        expected = {
            "mode": "canon", "policy": "shared", "backend": "local_fallback", "reason": "store_not_configured",
            "hits": 4, "misses": 1, "subcommand": "build",
        }
        self.assertEqual({name: row[name] for name in expected}, expected)
        self.assertEqual(Path(self.target, ".reify-build-root").read_text(), self.tree + "\n")
        self.assertEqual(self.leftovers(), [])

    def test_command_status_passes_through(self):
        self.env["FAKE_CARGO_EXIT"] = "5"
        self.one_call(self.bound("cargo", "build"), status=5)
        self.assertEqual(len(journal_rows(self.target)), 1)

    def test_exec_kind_gets_the_client_variables_and_an_empty_subcommand(self):
        call = self.one_call(self.bound("exec", "--", self.fake_cargo, "x"))
        self.assertEqual(call["argv"], [self.fake_cargo, "x"])
        self.assertIn("SCCACHE_SERVER_UDS", call["env"])
        self.assertNotIn("RUSTC_WRAPPER", call["env"])
        [row] = journal_rows(self.target)
        self.assertEqual((row["mode"], row["subcommand"]), ("canon", ""))

    def test_incremental_runs_no_server(self):
        self.enable("incremental")
        env = self.one_call(self.bound("cargo", "build"))["env"]
        self.assertEqual([name for name in env if name.startswith("SCCACHE_")], [])
        self.assertEqual(cfx.servers(self.sccache_dir), [])
        [row] = journal_rows(self.target)
        self.assertEqual((row["mode"], row["policy"], row["backend"], row["reason"]), ("canon", "incremental", "none", ""))

    def nest_under_a_canon_supervisor(self):
        """REIFY_CANON_ACTIVE naming this tree and a CANON supervisor with a recorded mixed cache."""
        recorded = canon_cache.ServerAddress(
            socket=os.path.join(self.target, ".reify-sccache.1.sock"),
            sccache=os.path.join(self.sccache_dir, "sccache"),
            uncached_log=os.path.join(self.target, ".reify-canon-uncached.1.log"),
            fallback_dir=os.path.join(self.tmp, "fallback"),
            fallback_cap="10G",
        )
        cache = canon_cache.CacheEnvironment(canon_cache.Policy.MIXED, recorded)
        tree = canon_run.find_tree(self.tree, {})
        supervisor = canon_run.Supervisor(canon_run.Mode.CANON, cache)
        self.env["REIFY_CANON_ACTIVE"] = canon_run.format_active(tree, self.tree, supervisor)
        return supervisor

    def test_nested_call_uses_the_recorded_cache_and_opens_no_session(self):
        supervisor = self.nest_under_a_canon_supervisor()
        env = self.one_call(self.bound("cargo", "build"))["env"]
        self.assertEqual(env["SCCACHE_SERVER_UDS"], supervisor.cache.server.socket)
        self.assertEqual(env["CARGO_INCREMENTAL"], "1")
        self.assertEqual(canon_run.parse_active(env["REIFY_CANON_ACTIVE"]).supervisor, supervisor)
        self.assertEqual(cfx.read_sccache_calls(self.sccache_dir), [])
        self.assertFalse(os.path.exists(canon_cache.journal_path(self.target)))

    def test_nested_call_stamps_a_fresh_build_directory_and_opens_no_session(self):
        self.nest_under_a_canon_supervisor()
        by_env = os.path.join(self.target, "nested-env")
        by_option = os.path.join(self.target, "nested-opt")
        for build_dir, target_dir_env, args in (
            (by_env, by_env, ("cargo", "build")),
            (by_option, None, ("cargo", "build", "--target-dir", by_option)),
        ):
            with self.subTest(build_dir=build_dir):
                Path(self.fake, "calls.jsonl").unlink(missing_ok=True)
                self.env.pop("CARGO_TARGET_DIR", None)
                if target_dir_env is not None:
                    self.env["CARGO_TARGET_DIR"] = target_dir_env
                self.one_call(self.bound(*args))
                stamp_path = Path(build_dir, ".reify-build-root")
                self.assertTrue(stamp_path.is_file(), f"{stamp_path} was not stamped")
                self.assertEqual(stamp_path.read_text(), self.tree + "\n")
                self.assertEqual(cfx.read_sccache_calls(self.sccache_dir), [])
                for directory in (build_dir, self.target):
                    self.assertFalse(os.path.exists(canon_cache.journal_path(directory)), directory)

    def start_bash_caller(self, *command):
        """bash running the BOUND route on command, then echoing CONTINUED; its own session, killed at cleanup."""
        caller = subprocess.Popen(
            [*BASH_CALLER, *fx.bound_argv(self.tree, "exec", "--", *command, runner_dir=self.tmp)],
            cwd=self.sub, env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            start_new_session=True,
        )
        def end():
            if caller.poll() is None:
                os.killpg(caller.pid, signal.SIGKILL)
            caller.communicate()

        self.addCleanup(end)
        return caller

    def test_a_bash_caller_does_not_continue_after_a_terminal_interrupt(self):
        started = os.path.join(self.tmp, "started")
        caller = self.start_bash_caller("/bin/sh", "-c", 'touch "$0"; exec sleep 60', started)
        while not os.path.exists(started):
            if caller.poll() is not None:
                self.fail(f"the caller ended before the command started: {caller.communicate()[1]}")
            time.sleep(0.02)
        os.killpg(caller.pid, signal.SIGINT)
        stdout, stderr = caller.communicate()
        self.assertEqual(caller.returncode, -signal.SIGINT, stderr)
        self.assertNotIn("CONTINUED", stdout)
        self.assertEqual(len(journal_rows(self.target)), 1)
        self.assertEqual(self.leftovers(), [])

    def test_a_command_that_exits_130_itself_lets_a_bash_caller_continue(self):
        caller = self.start_bash_caller("/bin/sh", "-c", "exit 130")
        stdout, stderr = caller.communicate()
        self.assertEqual(caller.returncode, 0, stderr)
        self.assertIn("CONTINUED rc=130", stdout)

    def test_every_canon_route_hands_the_command_sigpipe_and_sigxfsz_at_their_defaults(self):
        """Only these two: a pool member may legitimately inherit INT/QUIT ignored."""
        for route in ("outermost", "nested"):
            with self.subTest(route=route):
                if route == "nested":
                    self.nest_under_a_canon_supervisor()
                out = os.path.join(self.tmp, f"sigign-{route}")
                result = self.bound("exec", "--", *sigign_probe(out))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(STARTUP_IGNORED & fx.ignored_signals(out), set())

    def test_sharing_without_a_real_sccache_is_refused_before_anything_runs(self):
        if shutil.which("sccache", path="/usr/bin:/bin"):
            self.skipTest("a real sccache is installed in /usr/bin or /bin")
        self.env["PATH"] = f"{self.tree}/scripts/canon-bin:{self.fake}:/usr/bin:/bin"
        self.assert_refused(self.bound("cargo", "build"), "sccache")
        self.assertFalse(os.path.exists(os.path.join(self.target, ".reify-build-root")))


class CargoPassesThrough(unittest.TestCase):
    """Decision 10's pass-through list (boundary row 2), pure."""

    def setUp(self):
        tmp = scratch(self)
        self.tree = fx.make_tree(tmp, "tree")
        self.alias = os.path.join(tmp, "alias")
        os.symlink(self.tree, self.alias)

    def passes(self, args, *, root=None, **environ):
        """root is the spelling cargo runs under (CANON at a nested call); the real root stays."""
        found = canon_run.find_tree(self.tree, {})
        tree = canon_run.Tree(Path(root or self.tree), Path(self.tree), found.dev, found.ino)
        return canon_run.cargo_passes_through(args, environ, str(tree.root), tree)

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

    def test_install_into_either_spelling_of_the_tree_is_not_pass_through(self):
        """Inside the tree means what build_dir_decision means by it: Tree.contains."""
        for spelling in (self.tree, self.alias):
            with self.subTest(spelling=spelling):
                target = f"{spelling}/t4"
                self.assertFalse(self.passes(["install", "--target-dir", target, "x"], root=self.alias))
                self.assertFalse(self.passes(["install", "x"], root=self.alias, CARGO_TARGET_DIR=target))
                self.assertTrue(self.passes(["install", "x"], root=self.alias))


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

    def assert_exec_refused(self, *needles):
        marker = os.path.join(self.tmp, "marker")
        result = fx.run_canon(self.tree, self.sub, "exec", "--", "touch", marker, env=self.env)
        self.assert_refused(result, self.target, *needles)
        self.assertFalse(os.path.exists(marker))
        return result.stderr

    def test_stamp_naming_another_path_names_only_the_delete_fix(self):
        stamp(self.target, "/somewhere/else")
        stderr = self.assert_exec_refused("delete the build directory")
        self.assertNotIn("namespace", stderr)

    def test_stamp_naming_unprovisioned_canon_names_both_fixes(self):
        stamp(self.target, self.env["REIFY_CANON_ROOT"])
        self.assert_exec_refused("mount namespace", "delete the build directory")


class ResolveCase(CliCase):
    """Adds a second recording fake, an installed trampoline copy and a directory outside any tree."""

    def setUp(self):
        super().setUp()
        self.other_trampolines = os.path.join(self.tmp, "installed-canon-bin")
        os.mkdir(self.other_trampolines)
        shutil.copy2(fx.TRAMPOLINE, os.path.join(self.other_trampolines, "cargo"))
        self.second_fake = fx.fake_cargo_dir(self.tmp, "second-fake-bin")
        self.tree_bin = os.path.join(self.tree, "scripts", "canon-bin")
        self.outside = os.path.join(self.tmp, "outside")
        os.mkdir(self.outside)


class ResolveTable:
    """The real-cargo rule's rows; `resolve(*entries)` is the cargo one copy of the rule picks.

    C7 makes the trampoline carry its own copy of canon_run.py's rule, so every row runs
    against both copies (one concrete class each) and a drift in either goes red.
    """

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

    def test_falls_back_to_an_unmarked_cargo_before_the_first_trampoline(self):
        self.assertEqual(self.resolve(self.fake, self.tree_bin), self.fake_cargo)
        self.assertEqual(
            self.resolve(self.fake, self.tree_bin, self.other_trampolines), self.fake_cargo
        )

    def test_none_without_an_unmarked_cargo(self):
        self.assertIsNone(self.resolve(self.tree_bin, self.other_trampolines))
        self.assertIsNone(self.resolve())


class ResolveRealCargo(ResolveTable, ResolveCase):
    """canon_run.py's copy, called in-process."""

    def resolve(self, *entries):
        return canon_run.resolve_real_cargo(os.pathsep.join(entries))


class TrampolineResolvesRealCargo(ResolveTable, ResolveCase):
    """The trampoline's copy: run outside any tree, its pick is the fake that recorded the call."""

    def resolve(self, *entries):
        fakes = (self.fake, self.second_fake)
        for fake in fakes:
            Path(fake, "calls.jsonl").unlink(missing_ok=True)
        env = dict(self.env, PATH=os.pathsep.join(entries))
        result = fx.run([self.trampoline, "build"], cwd=self.outside, env=env)
        calls = [call for fake in fakes for call in fx.read_calls(fake)]
        if result.returncode == 127:
            self.assertEqual(calls, [])
            return None
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(calls), 1, calls)
        return calls[0]["argv"][0]


class TrampolineRouting(ResolveCase):
    """The trampoline inside and outside a tree, with real and marked cargos in either order."""

    def test_trampoline_after_the_real_cargo_runs_it(self):
        self.env["PATH"] = os.pathsep.join((self.fake, self.tree_bin))
        for cwd in (self.sub, self.outside):
            with self.subTest(cwd=cwd):
                Path(self.fake, "calls.jsonl").unlink(missing_ok=True)
                self.assert_one_untouched_call(self.cargo("build", cwd=cwd), ["build"], cwd=cwd)

    def test_two_trampolines_on_path_do_not_loop(self):
        self.env["PATH"] = os.pathsep.join((self.tree_bin, self.other_trampolines, self.fake))
        self.env["FAKE_CARGO_EXIT"] = "7"
        for cwd in (self.sub, self.outside):
            with self.subTest(cwd=cwd):
                Path(self.fake, "calls.jsonl").unlink(missing_ok=True)
                result = self.cargo("build", cwd=cwd)
                self.assertEqual(result.returncode, 7, result.stderr)
                self.assertEqual(len(fx.read_calls(self.fake)), 1)

    def test_no_real_cargo_exits_127_with_one_line(self):
        self.env["PATH"] = os.pathsep.join((self.tree_bin, self.other_trampolines))
        for cwd in (self.sub, self.outside):
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


def load_pcc():
    path = fx.REPO_ROOT / "scripts" / "prd-capability-check.py"
    spec = importlib.util.spec_from_file_location("pcc_reference", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules["pcc_reference"] = module
    spec.loader.exec_module(module)
    return module


class CanonInvocationCase(unittest.TestCase):
    """C3/C4 composition of what a command gets at CANON, without entering a namespace."""

    CACHE_VARIABLES = {
        "RUSTC_WRAPPER": "sccache",
        "CARGO_BUILD_RUSTC_WRAPPER": "sccache",
        "CARGO_INCREMENTAL": "0",
        "SCCACHE_DIR": "/x",
        "SCCACHE_SERVER_PORT": "4226",
    }

    def setUp(self):
        self.tmp = scratch(self)
        self.real = fx.make_tree(self.tmp, "tree")
        self.tree = canon_run.find_tree(self.real, {})
        self.canon = os.path.join(self.tmp, "canon")
        self.fake = fx.fake_cargo_dir(self.tmp)
        self.environ = fx.scratch_env(
            self.tmp, PATH=f"{self.real}/scripts/canon-bin:{self.fake}:/usr/bin:/bin"
        )
        self.cache = canon_cache.CacheEnvironment(canon_cache.Policy.INCREMENTAL, None)

    def invoke(self, command, *, cargo=True, environ=None, tree=None, cwd=None, cache=None):
        return canon_run.canon_invocation(
            command,
            self.environ if environ is None else environ,
            tree or self.tree,
            self.canon,
            cwd or self.real,
            cache or self.cache,
            cargo=cargo,
        )


class Rewriting(CanonInvocationCase):
    """Boundary row 3: absolute paths into the tree move to CANON; nothing else does."""

    def test_tree_paths_in_arguments_move_to_canon(self):
        for command, expected in (
            (["--manifest-path", f"{self.real}/Cargo.toml"], ["--manifest-path", f"{self.canon}/Cargo.toml"]),
            ([f"--manifest-path={self.real}/Cargo.toml"], [f"--manifest-path={self.canon}/Cargo.toml"]),
            (["--target-dir", f"{self.real}/target"], ["--target-dir", f"{self.canon}/target"]),
            ([self.real], [self.canon]),
        ):
            with self.subTest(command=command):
                self.assertEqual(list(self.invoke(command).argv[1:]), expected)

    def test_other_arguments_are_unchanged(self):
        command = [f"{self.real}-sibling/x", "/elsewhere/x", "relative/x", f"--x={self.real}-sibling"]
        self.assertEqual(list(self.invoke(command).argv[1:]), command)

    def test_exec_kind_rewrites_the_full_command(self):
        invocation = self.invoke(["/bin/ls", f"{self.real}/sub"], cargo=False)
        self.assertEqual(list(invocation.argv), ["/bin/ls", f"{self.canon}/sub"])

    def test_cargo_variables_move_to_canon_and_others_do_not(self):
        environ = dict(self.environ, CARGO_TARGET_DIR=f"{self.real}/target", FOO_DIR=f"{self.real}/x")
        env = self.invoke(["build"], environ=environ).env
        self.assertEqual(env["CARGO_TARGET_DIR"], f"{self.canon}/target")
        self.assertEqual(env["FOO_DIR"], f"{self.real}/x")

    def test_either_spelling_of_the_tree_moves_to_canon(self):
        alias = os.path.join(self.tmp, "alias")
        os.symlink(self.real, alias)
        tree = canon_run.Tree(Path(alias), Path(self.real), self.tree.dev, self.tree.ino)
        invocation = self.invoke([f"{alias}/a", f"{self.real}/b"], tree=tree, cwd=alias)
        self.assertEqual(list(invocation.argv[1:]), [f"{self.canon}/a", f"{self.canon}/b"])

    def test_cwd_and_pwd_move_to_canon(self):
        invocation = self.invoke(["build"], cwd=f"{self.real}/sub")
        self.assertEqual(invocation.cwd, f"{self.canon}/sub")
        self.assertEqual(invocation.env["PWD"], f"{self.canon}/sub")


class Exports(CanonInvocationCase):
    def test_active_record_names_the_tree_and_its_canon_supervisor(self):
        record = json.loads(self.invoke(["build"]).env["REIFY_CANON_ACTIVE"])
        self.assertEqual(
            record,
            {
                "dev": self.tree.dev,
                "ino": self.tree.ino,
                "real_root": self.real,
                "canon": self.canon,
                "supervisor": {"mode": "canon", "cache": self.cache.to_record()},
            },
        )

    def test_ledger_lane_is_the_real_root(self):
        environ = dict(self.environ, REIFY_VERIFY_LEDGER_LANE_DIR="/some/other/lane")
        env = self.invoke(["build"], environ=environ).env
        self.assertEqual(env["REIFY_VERIFY_LEDGER_LANE_DIR"], self.real)

    def test_path_puts_canon_trampoline_then_cargo_home_first(self):
        cargo_bin = os.path.join(self.environ["HOME"], ".cargo", "bin")
        environ = dict(
            self.environ,
            PATH=f"{self.real}/scripts/canon-bin:{cargo_bin}:{self.fake}:/usr/bin:/bin",
        )
        expected = [f"{self.canon}/scripts/canon-bin", cargo_bin, self.fake, "/usr/bin", "/bin"]
        env = self.invoke(["build"], environ=environ).env
        self.assertEqual(env["PATH"].split(os.pathsep), expected)
        nested = self.invoke(["build"], environ=env).env
        self.assertEqual(nested["PATH"].split(os.pathsep), expected)

    def test_grammar_cache_home_is_pcc_derived_for_the_real_root(self):
        pcc = load_pcc()

        def derived(root):
            return pcc.grammar_cache_home(
                pcc.ProbeContext(
                    repo_root=str(root),
                    reify_bin=None,
                    reify_bin_refusal="not used by this test",
                    tree_sitter_bin="tree-sitter",
                )
            )

        repo_tree = canon_run.find_tree(str(fx.REPO_ROOT), {})
        env = self.invoke(["build"], tree=repo_tree, cwd=str(fx.REPO_ROOT)).env
        at_canon = derived(self.canon)
        self.addCleanup(shutil.rmtree, at_canon, ignore_errors=True)
        self.assertEqual(env["REIFY_TS_CACHE_HOME"], derived(fx.REPO_ROOT))
        self.assertNotEqual(env["REIFY_TS_CACHE_HOME"], at_canon)

    def test_grammar_cache_home_is_dropped_for_a_tree_without_pcc(self):
        environ = dict(self.environ, REIFY_TS_CACHE_HOME="/tmp/caller-chosen")
        self.assertNotIn("REIFY_TS_CACHE_HOME", self.invoke(["build"], environ=environ).env)

    def test_infra_suites_do_not_inherit_the_primitive_variables(self):
        composed = self.invoke(
            ["build"], cache=canon_cache.CacheEnvironment(canon_cache.Policy.SHARED, SERVER)
        ).env
        names = {name for name in composed if name.startswith("REIFY_CANON_")}
        names |= {"REIFY_CANON_ROOT", "REIFY_CANON_HOST_CONFIG"}
        with mock.patch.dict(os.environ, {name: "ambient" for name in names}):
            self.assertEqual(names & set(hermetic_env()), set())


class NestedComposition(CanonInvocationCase):
    """A nested cargo applies the supervisor record's CacheEnvironment, never re-reading host.toml."""

    def test_nested_cargo_composes_exactly_like_the_outermost_cargo(self):
        os.symlink(self.real, self.canon)
        for cache in (
            canon_cache.CacheEnvironment(canon_cache.Policy.SHARED, SERVER),
            canon_cache.CacheEnvironment(canon_cache.Policy.MIXED, SERVER),
            canon_cache.CacheEnvironment(canon_cache.Policy.INCREMENTAL, None),
        ):
            with self.subTest(policy=cache.policy):
                outermost = self.invoke(["build"], cache=cache)
                command = self.invoke(["/bin/sh"], cargo=False, cache=cache)
                recorded = canon_run.parse_active(command.env["REIFY_CANON_ACTIVE"]).supervisor
                self.assertEqual(recorded, canon_run.Supervisor(canon_run.Mode.CANON, cache))
                nested_tree = canon_run.find_tree(self.canon, command.env)
                self.assertEqual(nested_tree.real_root, Path(self.real))
                nested = canon_run.canon_invocation(
                    ["build"], command.env, nested_tree, self.canon, self.canon, recorded.cache, cargo=True
                )
                self.assertEqual(nested, outermost)


class Reentry(CanonInvocationCase):
    """The NAMESPACE route re-enters this same canon_run.py at CANON, so one supervisor shape serves both routes."""

    ARGUMENTS = ["cargo", "test", "--offline", "--", "--nocapture"]

    def expected_bwrap(self, cwd_at_canon):
        return [
            "/usr/bin/bwrap", "--dev-bind", "/", "/", "--bind", self.real, self.canon,
            "--chdir", cwd_at_canon, "--die-with-parent", "--info-fd", "7",
        ]

    def test_bwrap_binds_the_real_root_at_canon_and_reports_to_the_info_fd(self):
        bwrap = canon_run.reentry_bwrap(self.tree, self.canon, f"{self.real}/sub", 7)
        self.assertEqual(bwrap, self.expected_bwrap(f"{self.canon}/sub"))

    def test_an_aliased_spelling_binds_the_real_root(self):
        alias = os.path.join(self.tmp, "alias")
        os.symlink(self.real, alias)
        tree = canon_run.Tree(Path(alias), Path(self.real), self.tree.dev, self.tree.ino)
        bwrap = canon_run.reentry_bwrap(tree, self.canon, alias, 7)
        self.assertEqual(bwrap, self.expected_bwrap(self.canon))

    def test_the_command_reenters_canon_run_at_canon(self):
        self.assertEqual(
            canon_run.reentry_command(self.canon, self.ARGUMENTS),
            [sys.executable, "-I", f"{self.canon}/scripts/canon_run.py", *self.ARGUMENTS],
        )

    def test_env_is_the_callers_plus_a_record_without_a_supervisor(self):
        supervisor = canon_run.Supervisor(canon_run.Mode.REAL_PATH, None)
        environ = dict(
            self.environ,
            REIFY_CANON_ACTIVE=canon_run.format_active(self.tree, self.canon, supervisor),
            RUSTC_WRAPPER="caller",
        )
        env = canon_run.reentry_env(environ, self.tree, self.canon)
        self.assertEqual(env, dict(environ, REIFY_CANON_ACTIVE=canon_run.format_active(self.tree, self.canon)))
        self.assertIsNone(canon_run.parse_active(env["REIFY_CANON_ACTIVE"]).supervisor)


class NamespaceArgv(unittest.TestCase):
    """Ruling step 3 (esc-10418-3): env inside the namespace restores exactly the signals bwrap was made to ignore."""

    BWRAP = ["/usr/bin/bwrap", "--dev-bind", "/", "/", "--info-fd", "7"]
    COMMAND = ["/usr/bin/python3", "-I", "/canon/scripts/canon_run.py", "exec", "--", "true"]

    def test_every_supervised_signal_is_restored_by_its_name_in_number_order(self):
        argv = canon_run.namespace_argv(self.BWRAP, self.COMMAND, frozenset(canon_cache.SUPERVISED_SIGNALS))
        env_layer = ["/usr/bin/env", "--default-signal=HUP,INT,QUIT,TERM", "--"]
        self.assertEqual(argv, [*self.BWRAP, "--", *env_layer, *self.COMMAND])

    def test_one_restored_signal_is_named_alone(self):
        argv = canon_run.namespace_argv(self.BWRAP, self.COMMAND, frozenset({signal.SIGHUP}))
        self.assertEqual(argv, [*self.BWRAP, "--", "/usr/bin/env", "--default-signal=HUP", "--", *self.COMMAND])

    def test_nothing_to_restore_leaves_out_the_env_layer(self):
        argv = canon_run.namespace_argv(self.BWRAP, self.COMMAND, frozenset())
        self.assertEqual(argv, [*self.BWRAP, "--", *self.COMMAND])


class NamespaceParent(unittest.TestCase):
    """PRD C3 (esc-10418-3): a signal to the NAMESPACE route's started pid ends the command as in real-path mode.

    run_in_namespace runs around a fake bwrap (fixture NAMESPACE_PARENT_RUNNER), so no namespace is needed.
    """

    def setUp(self):
        self.tmp = scratch(self)

    def start(self, **kwargs):
        parent = fx.start_namespace_parent(self.tmp, **kwargs)
        self.addCleanup(lambda: parent.poll() is None and os.killpg(parent.pid, signal.SIGKILL))
        return parent

    def wait_for(self, name, parent):
        while not os.path.exists(os.path.join(self.tmp, name)):
            self.assertIsNone(parent.poll(), f"the parent ended before {name}")
            time.sleep(0.02)

    def assert_forwarded(self, signum):
        parent = self.start()
        self.wait_for("started", parent)
        os.kill(parent.pid, signum)
        self.assertEqual(parent.wait(), fx.namespace_death_status(signum))

    def test_sigterm_to_the_parent_alone_reaches_the_command(self):
        self.assert_forwarded(signal.SIGTERM)

    def test_sighup_to_the_parent_alone_reaches_the_command(self):
        self.assert_forwarded(signal.SIGHUP)

    def test_sigint_to_the_parent_alone_reaches_the_command(self):
        self.assert_forwarded(signal.SIGINT)

    def test_sigquit_to_the_parent_alone_reaches_the_command(self):
        self.assert_forwarded(signal.SIGQUIT)

    def test_a_command_that_traps_the_signal_and_exits_130_leaves_the_parent_exiting_130(self):
        """bwrap reports 130 either way; only the pidfd's exit record tells a death from an exit."""
        parent = self.start(trap_exit=True)
        self.wait_for("started", parent)
        os.kill(parent.pid, signal.SIGINT)
        self.assertEqual(parent.wait(), 130)

    def test_a_signal_before_the_command_pid_is_known_is_delivered_once_it_is(self):
        parent = self.start(go=False)
        self.wait_for("spawned", parent)
        while not fx.catches(parent.pid, signal.SIGTERM):
            self.assertIsNone(parent.poll())
            time.sleep(0.02)
        os.kill(parent.pid, signal.SIGTERM)
        Path(self.tmp, "go").touch()
        self.assertEqual(parent.wait(), fx.namespace_death_status(signal.SIGTERM))

    def test_a_signal_while_bwrap_is_being_started_reaches_the_command(self):
        parent = self.start(signal_during_start=True, command_secs="5")
        self.assertEqual(parent.wait(), fx.namespace_death_status(signal.SIGTERM))

    def test_a_descriptor_the_caller_passes_reaches_bwrap(self):
        read_end, write_end = os.pipe()
        self.addCleanup(os.close, read_end)
        self.addCleanup(os.close, write_end)
        os.set_inheritable(write_end, True)
        parent = self.start(pass_fds=(write_end,))
        self.wait_for("started", parent)
        self.assertIn(write_end, json.loads(Path(self.tmp, "bwrap_fds.json").read_text()))
        os.kill(parent.pid, signal.SIGTERM)
        self.assertEqual(parent.wait(), fx.namespace_death_status(signal.SIGTERM))

    def test_a_signal_the_caller_ignores_stays_ignored_and_the_rest_are_restored(self):
        parent = self.start(ignore=(signal.SIGHUP,))
        self.wait_for("started", parent)
        ignored = fx.ignored_signals(os.path.join(self.tmp, "sigign"))
        self.assertEqual(ignored & set(canon_cache.SUPERVISED_SIGNALS), {signal.SIGHUP})
        os.kill(parent.pid, signal.SIGTERM)
        self.assertEqual(parent.wait(), fx.namespace_death_status(signal.SIGTERM))


def at_default_sigint():
    """A child's preexec_fn: SIGINT at default, which a pool member may have inherited ignored."""
    signal.signal(signal.SIGINT, signal.SIG_DFL)


class ExitedReturncode(unittest.TestCase):
    """canon_run.exited_returncode: the kernel's exit record for the process a pidfd names, in subprocess's terms."""

    def setUp(self):
        if not fx.pidfd_exit_record_available():
            self.skipTest(fx.PIDFD_EXIT_RECORD_MISSING)

    def pidfd(self, pid):
        pidfd = os.pidfd_open(pid)
        self.addCleanup(os.close, pidfd)
        return pidfd

    def test_a_child_killed_by_sigint_gives_minus_sigint(self):
        child = subprocess.Popen(["sleep", "30"], preexec_fn=at_default_sigint)
        pidfd = self.pidfd(child.pid)
        child.send_signal(signal.SIGINT)
        child.wait()
        self.assertEqual(canon_run.exited_returncode(pidfd), -signal.SIGINT)

    def test_a_child_that_exited_130_gives_130(self):
        child = subprocess.Popen(["/bin/sh", "-c", "exit 130"])
        pidfd = self.pidfd(child.pid)
        child.wait()
        self.assertEqual(canon_run.exited_returncode(pidfd), 130)

    def test_a_grandchild_its_own_parent_reaped_gives_how_it_died(self):
        """The outer process -> bwrap -> re-run relation: the record outlives a reap by someone else.

        sh starts `&` jobs with SIGINT ignored, so env gives the grandchild back its default first.
        """
        parent = subprocess.Popen(
            ["/bin/sh", "-c", "/usr/bin/env --default-signal=INT sleep 30 & echo $!; wait $!"],
            stdout=subprocess.PIPE, text=True,
        )
        self.addCleanup(parent.wait)
        grandchild = int(parent.stdout.readline())
        pidfd = self.pidfd(grandchild)
        while Path(f"/proc/{grandchild}/comm").read_text() != "sleep\n":
            time.sleep(0.01)
        signal.pidfd_send_signal(pidfd, signal.SIGINT)
        parent.communicate()
        self.assertEqual(canon_run.exited_returncode(pidfd), -signal.SIGINT)

    def test_a_running_process_gives_none(self):
        child = subprocess.Popen(["sleep", "30"])
        self.addCleanup(child.wait)
        self.addCleanup(child.kill)
        self.assertIsNone(canon_run.exited_returncode(self.pidfd(child.pid)))


class ReadChildPid(unittest.TestCase):
    """bwrap's --info-fd record, read to its end: the pid of the child it started, or None."""

    def read(self, data):
        read_end, write_end = os.pipe()
        with os.fdopen(write_end, "wb") as writer:
            writer.write(data)
        return canon_run.read_child_pid(read_end)

    def test_bwrap_0_9_0s_record_gives_the_child_pid(self):
        self.assertEqual(self.read(b'{\n    "child-pid": 1269159,\n    "mnt-namespace": 4026536262\n}'), 1269159)

    def test_nothing_written_before_bwrap_exited_gives_none(self):
        self.assertIsNone(self.read(b""))

    def test_text_that_is_not_json_gives_none(self):
        self.assertIsNone(self.read(b"child-pid: 12"))

    def test_a_json_list_gives_none(self):
        self.assertIsNone(self.read(b"[12]"))

    def test_a_child_pid_that_is_not_an_integer_gives_none(self):
        self.assertIsNone(self.read(b'{"child-pid": "12"}'))


class AwaitRestored(unittest.TestCase):
    """The namespace parent's wait for bwrap's child to restore the forwarded signals, and the pidfd it holds."""

    TERM = frozenset({signal.SIGTERM})
    SELECT_CEILING = 1024

    def exited_pidfd(self):
        """A pidfd for a child that has exited and been reaped, so it reads as exited from now on."""
        child = subprocess.Popen(["true"])
        pidfd = os.pidfd_open(child.pid)
        self.addCleanup(os.close, pidfd)
        child.wait()
        return pidfd

    def stranger(self):
        """A live process ignoring SIGTERM, standing in for one that reused the exited child's pid; it dies with us."""
        stranger = subprocess.Popen(
            canon_cache.with_parent_death_signal(["sleep", "infinity"], "KILL"),
            preexec_fn=lambda: signal.signal(signal.SIGTERM, signal.SIG_IGN),
        )
        self.addCleanup(stranger.wait)
        self.addCleanup(stranger.kill)
        return stranger

    def info(self, pid):
        read_end, write_end = os.pipe()
        with os.fdopen(write_end, "w") as writer:
            json.dump({"child-pid": pid}, writer)
        return read_end

    def test_a_pidfd_reporting_exit_ends_the_wait_and_never_reaches_a_stranger_reusing_the_pid(self):
        pidfd, stranger = self.exited_pidfd(), self.stranger()
        canon_run.await_restored(pidfd, stranger.pid, self.TERM)
        canon_run.send_through(pidfd, signal.SIGKILL)
        self.assertIsNone(stranger.poll())

    def test_a_pidfd_numbered_past_selects_ceiling_still_ends_the_wait(self):
        soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
        if soft <= self.SELECT_CEILING:
            if hard != resource.RLIM_INFINITY and hard <= self.SELECT_CEILING:
                self.skipTest(f"RLIMIT_NOFILE hard limit {hard} allows no descriptor past {self.SELECT_CEILING}")
            resource.setrlimit(resource.RLIMIT_NOFILE, (self.SELECT_CEILING + 1, hard))
            self.addCleanup(resource.setrlimit, resource.RLIMIT_NOFILE, (soft, hard))
        high = fcntl.fcntl(self.exited_pidfd(), fcntl.F_DUPFD_CLOEXEC, self.SELECT_CEILING)
        self.addCleanup(os.close, high)
        canon_run.await_restored(high, self.stranger().pid, self.TERM)

    def test_a_child_already_gone_gives_no_pidfd(self):
        child = subprocess.Popen(["true"])
        child.wait()
        self.assertIsNone(canon_run.open_restored_child(self.info(child.pid), self.TERM))

    def test_a_pidfd_the_kernel_will_not_open_is_a_one_line_refusal(self):
        unavailable = OSError(errno.ENOSYS, "Function not implemented")
        with mock.patch.object(os, "pidfd_open", side_effect=unavailable):
            with self.assertRaises(canon_run.Refusal) as refused:
                canon_run.open_restored_child(self.info(os.getpid()), self.TERM)
        self.assertIn(str(os.getpid()), str(refused.exception))
        self.assertNotIn("\n", str(refused.exception))


class CachePolicy(CanonInvocationCase):
    """C4's table as canon_invocation applies it (boundary row 4 for `incremental`)."""

    SHARING = (canon_cache.Policy.SHARED, canon_cache.Policy.MIXED)

    def setUp(self):
        super().setUp()
        self.cargo_bin = os.path.join(self.environ["HOME"], ".cargo", "bin")
        self.tree_guard = os.path.join(self.real, "scripts", "canon-bin", "guard")
        os.makedirs(self.tree_guard, exist_ok=True)
        guard = Path(self.tree_guard, "sccache")
        guard.write_text("#!/bin/sh\n# reify-canon-guard\n")
        guard.chmod(0o755)
        self.environ = dict(
            self.environ,
            FOO="1",
            PATH=f"{self.tree_guard}:{self.real}/scripts/canon-bin:{self.fake}:/usr/bin:/bin",
            CARGO_HOME="/elsewhere",
            **self.CACHE_VARIABLES,
        )

    def cache_for(self, policy):
        server = None if policy is canon_cache.Policy.INCREMENTAL else SERVER
        return canon_cache.CacheEnvironment(policy, server)

    def test_sharing_cargo_env_is_for_cargo_of_the_composed_env(self):
        for policy in self.SHARING:
            with self.subTest(policy=policy):
                cache = self.cache_for(policy)
                composed = self.invoke(["/bin/true"], cargo=False, cache=cache).env
                invocation = self.invoke(["build"], cache=cache)
                self.assertEqual(invocation.env, cache.for_cargo(composed))
                self.assertEqual(invocation.env["RUSTC_WRAPPER"], "sccache")
                self.assertEqual(invocation.env["SCCACHE_SERVER_UDS"], SERVER.socket)
                self.assertEqual(invocation.env["CARGO_HOME"], canon_cache.PINNED_CARGO_HOME)
                self.assertEqual(invocation.argv[0], os.path.join(self.fake, "cargo"))

    def test_sharing_path_puts_guard_trampoline_then_cargo_home_first(self):
        expected = [
            f"{self.canon}/scripts/canon-bin/guard",
            f"{self.canon}/scripts/canon-bin",
            self.cargo_bin,
            self.fake,
            "/usr/bin",
            "/bin",
        ]
        for policy in self.SHARING:
            for command, cargo in ((["build"], True), (["/bin/true"], False)):
                with self.subTest(policy=policy, cargo=cargo):
                    cache = self.cache_for(policy)
                    env = self.invoke(command, cargo=cargo, cache=cache).env
                    self.assertEqual(env["PATH"].split(os.pathsep), expected)
                    nested = self.invoke(command, cargo=cargo, environ=env, cache=cache).env
                    self.assertEqual(nested["PATH"], env["PATH"])

    def test_sharing_exec_kind_gets_for_command(self):
        for policy in self.SHARING:
            with self.subTest(policy=policy):
                cache = self.cache_for(policy)
                env = self.invoke(["/bin/true"], cargo=False, cache=cache).env
                for name in ("RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER", "CARGO_INCREMENTAL"):
                    self.assertEqual(env[name], self.CACHE_VARIABLES[name])
                self.assertEqual(env["SCCACHE_SERVER_UDS"], SERVER.socket)
                self.assertEqual(env["SCCACHE_DIR"], SERVER.fallback_dir)
                self.assertNotIn("SCCACHE_SERVER_PORT", env)
                self.assertEqual(env["CARGO_HOME"], canon_cache.PINNED_CARGO_HOME)
                self.assertEqual(env, cache.for_command(env))

    def test_incremental_cargo_drops_sccache_and_turns_incremental_on(self):
        invocation = self.invoke(["build"])
        env = invocation.env
        for name in ("RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"):
            self.assertNotIn(name, env)
        self.assertEqual([name for name in env if name.startswith("SCCACHE_")], [])
        self.assertEqual(env["CARGO_INCREMENTAL"], "1")
        self.assertEqual(env["CARGO_HOME"], canon_cache.PINNED_CARGO_HOME)
        self.assertEqual(env["FOO"], "1")
        self.assertEqual(invocation.argv[0], os.path.join(self.fake, "cargo"))
        path = env["PATH"].split(os.pathsep)
        self.assertEqual(path[:2], [f"{self.canon}/scripts/canon-bin", self.cargo_bin])
        self.assertEqual([entry for entry in path if entry.endswith("/guard")], [])

    def test_incremental_exec_kind_leaves_cache_variables_as_inherited(self):
        env = self.invoke(["/bin/true"], cargo=False).env
        for name, value in self.CACHE_VARIABLES.items():
            self.assertEqual(env[name], value)
        self.assertEqual(env["CARGO_HOME"], canon_cache.PINNED_CARGO_HOME)


class CacheRefusal(unittest.TestCase):
    """shared and mixed need a real sccache on the composed PATH; incremental needs none."""

    def setUp(self):
        self.tmp = scratch(self)
        self.path = Path(self.tmp, "host.toml")
        self.guard = os.path.join(self.tmp, "guard")
        self.real = os.path.join(self.tmp, "real")
        for directory, text in ((self.guard, "#!/bin/sh\n# reify-canon-guard\n"), (self.real, "#!/bin/sh\n")):
            os.mkdir(directory)
            Path(directory, "sccache").write_text(text)
            Path(directory, "sccache").chmod(0o755)

    def refusal(self, policy, *entries):
        host = canon_run.HostConfig(self.path, True, policy, canon_cache.CacheSettings())
        return canon_run.cache_refusal(host, os.pathsep.join(entries))

    def test_sharing_without_a_real_sccache_names_policy_sccache_and_host_config(self):
        for policy in (canon_cache.Policy.SHARED, canon_cache.Policy.MIXED):
            with self.subTest(policy=policy):
                message = self.refusal(policy, self.guard, "/nonexistent")
                self.assertNotIn("\n", message)
                for needle in (f"'{policy}'", "sccache", str(self.path)):
                    self.assertIn(needle, message)

    def test_sharing_with_a_real_sccache_after_the_guard_is_not_refused(self):
        for policy in (canon_cache.Policy.SHARED, canon_cache.Policy.MIXED):
            with self.subTest(policy=policy):
                self.assertEqual(self.refusal(policy, self.guard, self.real), "")

    def test_incremental_is_never_refused(self):
        self.assertEqual(self.refusal(canon_cache.Policy.INCREMENTAL, self.guard), "")


class CanonCargo(CanonInvocationCase):
    """C4: the real cargo at CANON is the first non-trampoline cargo on the composed PATH,
    wherever trampolines sit."""

    def setUp(self):
        super().setUp()
        cargo_dir = Path(self.environ["HOME"], ".cargo")
        cargo_dir.mkdir()
        self.cargo_home = fx.fake_cargo_dir(cargo_dir, "bin")
        self.installed = os.path.join(self.tmp, "installed-canon-bin")
        os.mkdir(self.installed)
        shutil.copy2(fx.TRAMPOLINE, os.path.join(self.installed, "cargo"))
        self.system = fx.fake_cargo_dir(self.tmp, "system-bin")
        self.tree_bin = f"{self.real}/scripts/canon-bin"

    def invoke_on_path(self, *entries, **environ):
        path = os.pathsep.join(entries)
        return self.invoke(["build"], environ=dict(self.environ, PATH=path, **environ))

    def test_cargo_home_wins_over_a_second_trampoline_copy(self):
        expected = os.path.join(self.cargo_home, "cargo")
        for entries in (
            (self.installed, self.tree_bin, self.cargo_home, self.system),
            (self.tree_bin, self.cargo_home, self.installed, self.system),
            (self.installed, self.cargo_home, self.system),
            (self.installed, self.tree_bin, self.cargo_home),
        ):
            with self.subTest(path=entries):
                self.assertEqual(self.invoke_on_path(*entries).argv[0], expected)

    def test_inside_and_outside_the_namespace_give_the_same_cargo(self):
        expected = os.path.join(self.cargo_home, "cargo")
        entries = (self.installed, self.tree_bin, self.cargo_home, self.system)
        self.assertEqual(self.invoke_on_path(*entries).argv[0], expected)
        shutil.copytree(self.real, self.canon, symlinks=True)
        self.assertEqual(self.invoke_on_path(*entries).argv[0], expected)

    def test_only_trampolines_raise_command_not_found(self):
        empty_home = os.path.join(self.tmp, "empty-home")
        os.mkdir(empty_home)
        with self.assertRaises(canon_cache.CommandNotFound):
            self.invoke_on_path(self.installed, self.tree_bin, HOME=empty_home)


class EnsureStamp(unittest.TestCase):
    """C1's stamp as both CANON routes write it: created once, never over another CANON's."""

    CANON = "/opt/reify-canon/ws"

    def setUp(self):
        self.build = Path(scratch(self), "target")

    def test_fresh_directory_gets_a_stamp_and_nothing_else(self):
        canon_run.ensure_stamp(self.build, self.CANON)
        stamp_path = self.build / ".reify-build-root"
        self.assertEqual(stamp_path.read_text(), self.CANON + "\n")
        self.assertEqual(stamp_path.stat().st_mode & 0o777, 0o644)
        self.assertEqual(sorted(entry.name for entry in self.build.iterdir()), [".reify-build-root"])

    def test_existing_stamp_for_the_same_canon_is_kept(self):
        stamp(self.build, self.CANON)
        canon_run.ensure_stamp(self.build, self.CANON)
        self.assertEqual(canon_run.read_build_dir_state(self.build).stamp, self.CANON)

    def test_directory_already_stamped_for_canon_is_not_written(self):
        stamp(self.build, self.CANON)
        os.utime(self.build, ns=(0, 0))
        before = (self.build / ".reify-build-root").read_bytes()
        canon_run.ensure_stamp(self.build, self.CANON)
        self.assertEqual(self.build.stat().st_mtime_ns, 0)
        self.assertEqual(sorted(entry.name for entry in self.build.iterdir()), [".reify-build-root"])
        self.assertEqual((self.build / ".reify-build-root").read_bytes(), before)

    def test_read_only_directory_already_stamped_for_canon_is_accepted(self):
        if os.geteuid() == 0:
            self.skipTest("root writes into a read-only directory regardless of its mode")
        stamp(self.build, self.CANON)
        self.build.chmod(0o555)
        self.addCleanup(self.build.chmod, 0o755)
        canon_run.ensure_stamp(self.build, self.CANON)

    def test_existing_stamp_for_another_path_refuses(self):
        stamp(self.build, "/somewhere/else")
        with self.assertRaises(canon_run.Refusal) as caught:
            canon_run.ensure_stamp(self.build, self.CANON)
        self.assertIn("/somewhere/else", str(caught.exception))
        self.assertEqual(canon_run.read_build_dir_state(self.build).stamp, "/somewhere/else")


class ActiveRecord(unittest.TestCase):
    """A nested invocation at CANON takes the real root from REIFY_CANON_ACTIVE."""

    def setUp(self):
        tmp = scratch(self)
        self.root = fx.make_tree(tmp, "tree")
        self.root_stat = os.stat(self.root)

    def real_root_with(self, value):
        return canon_run.find_tree(self.root, {"REIFY_CANON_ACTIVE": value}).real_root

    def record(self, **changes):
        fields = {
            "dev": self.root_stat.st_dev,
            "ino": self.root_stat.st_ino,
            "real_root": "/the/real/tree",
            "canon": "/opt/reify-canon/ws",
        }
        return json.dumps(dict(fields, **changes))

    def test_matching_record_supplies_the_real_root(self):
        self.assertEqual(self.real_root_with(self.record()), Path("/the/real/tree"))

    def test_record_for_another_inode_is_ignored(self):
        self.assertEqual(self.real_root_with(self.record(ino=self.root_stat.st_ino + 1)), Path(self.root))

    def test_malformed_record_is_ignored(self):
        for value in ("{not json", self.record(dev="1"), "[]"):
            with self.subTest(value=value):
                self.assertEqual(self.real_root_with(value), Path(self.root))

    def test_supervisor_section_round_trips(self):
        tree = canon_run.find_tree(self.root, {})
        for supervisor in (
            canon_run.Supervisor(canon_run.Mode.CANON, canon_cache.CacheEnvironment(canon_cache.Policy.SHARED, SERVER)),
            canon_run.Supervisor(canon_run.Mode.CANON, canon_cache.CacheEnvironment(canon_cache.Policy.INCREMENTAL, None)),
            canon_run.Supervisor(canon_run.Mode.REAL_PATH, None),
        ):
            with self.subTest(supervisor=supervisor):
                value = canon_run.format_active(tree, "/opt/reify-canon/ws", supervisor)
                self.assertEqual(
                    canon_run.parse_active(value),
                    canon_run.ActiveTree(
                        self.root_stat.st_dev, self.root_stat.st_ino, self.root, "/opt/reify-canon/ws", supervisor
                    ),
                )

    def test_record_without_a_supervisor_section_parses_with_none(self):
        active = canon_run.parse_active(self.record())
        self.assertEqual(active.real_root, "/the/real/tree")
        self.assertIsNone(active.supervisor)

    def test_malformed_supervisor_section_parses_as_none(self):
        for section in (
            "canon",
            {"mode": "bogus", "cache": None},
            {"mode": "canon", "cache": {"policy": "fast", "server": None}},
            {"mode": "canon", "cache": None},
            {"mode": "real_path", "cache": {"policy": "incremental", "server": None}},
            {"cache": None},
        ):
            with self.subTest(section=section):
                active = canon_run.parse_active(self.record(supervisor=section))
                self.assertEqual(active.real_root, "/the/real/tree")
                self.assertIsNone(active.supervisor)


class IsNested(unittest.TestCase):
    """Nested means: the record names this tree, carries a supervisor, and that supervisor's mode is ours."""

    CANON = "/opt/reify-canon/ws"
    CACHE = canon_cache.CacheEnvironment(canon_cache.Policy.SHARED, SERVER)

    def setUp(self):
        root = fx.make_tree(scratch(self), "tree")
        self.tree = canon_run.find_tree(root, {})

    def nested(self, supervisor, decision, **changes):
        fields = dict(dev=self.tree.dev, ino=self.tree.ino, real_root=str(self.tree.real_root), canon=self.CANON)
        fields.update(changes)
        active = canon_run.ActiveTree(supervisor=supervisor, **fields)
        return canon_run.is_nested(active, self.tree, decision)

    def test_rows(self):
        canon = canon_run.Supervisor(canon_run.Mode.CANON, self.CACHE)
        real_path = canon_run.Supervisor(canon_run.Mode.REAL_PATH, None)
        bound = canon_run.Decision(canon_run.Mode.CANON, "", canon_run.Route.BOUND)
        namespace = canon_run.Decision(canon_run.Mode.CANON, "", canon_run.Route.NAMESPACE)
        legacy = canon_run.Decision(
            canon_run.Mode.REAL_PATH, "", cache_reason=canon_cache.Reason.LEGACY_BUILD_DIR
        )
        for supervisor, decision, changes, expected in (
            (canon, bound, {}, True),
            (real_path, legacy, {}, True),
            (canon, namespace, {}, False),
            (real_path, bound, {}, False),
            (canon, legacy, {}, False),
            (None, bound, {}, False),
            (None, legacy, {}, False),
            (canon, bound, {"ino": self.tree.ino + 1}, False),
            (real_path, legacy, {"dev": self.tree.dev + 1}, False),
        ):
            with self.subTest(supervisor=supervisor, decision=decision, changes=changes):
                self.assertIs(self.nested(supervisor, decision, **changes), expected)

    def test_no_record_is_not_nested(self):
        decision = canon_run.Decision(canon_run.Mode.CANON, "", canon_run.Route.BOUND)
        self.assertFalse(canon_run.is_nested(None, self.tree, decision))


if __name__ == "__main__":
    unittest.main()
