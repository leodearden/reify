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

import base64
import dataclasses
import fcntl
import http.client
import json
import os
import random
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_cache_fixture as cfx  # noqa: E402  (imported after sys.path manipulation)
import canon_run_fixture as fx  # noqa: E402

sys.path.insert(0, str(fx.REPO_ROOT / "scripts"))
import canon_cache  # noqa: E402

ORCHESTRATOR_CONFIG = fx.REPO_ROOT / "dark-factory-orchestrator.yaml"
CARGO_HOME_LINE = re.compile(r"^\s+CARGO_HOME:\s*(\S+)\s*$", re.MULTILINE)
GUARD_LINE = "#!/bin/sh\n# reify-canon-guard\nexit 0\n"
REAL_LINE = "#!/bin/sh\nexit 0\n"
GUARD = fx.REPO_ROOT / "scripts" / "canon-bin" / "guard" / "sccache"

# A stub compiler or sccache: records its own name, its arguments and the
# CARGO_* names it was given, then exits 0.
RECORDING_STUB = r'''#!/usr/bin/python3 -I
import json
import os
import sys

call = {
    "prog": os.path.basename(sys.argv[0]),
    "argv": sys.argv[1:],
    "cargo": sorted(name for name in os.environ if name.startswith("CARGO_")),
}
with open(os.environ["GUARD_TEST_LOG"], "a") as log:
    log.write(json.dumps(call) + "\n")
'''


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


class Guard(unittest.TestCase):
    """Boundary row 5: scripts/canon-bin/guard/sccache, the one choke point of every cached compile."""

    PRESENTATION = ("CARGO_INCREMENTAL", "CARGO_LOG", "CARGO_TERM_COLOR", "CARGO_TERM_PROGRESS_WHEN")

    def setUp(self):
        self.tmp = scratch(self)
        self.compilers = os.path.join(self.tmp, "compilers")
        self.rustc = executable(self.compilers, "rustc", RECORDING_STUB)
        self.cc = executable(self.compilers, "cc", RECORDING_STUB)
        self.real_dir = os.path.join(self.tmp, "real")
        self.real = executable(self.real_dir, "sccache", RECORDING_STUB)
        self.log = os.path.join(self.tmp, "calls.jsonl")
        self.uncached = os.path.join(self.tmp, "uncached.log")
        self.bash_only = os.path.join(self.tmp, "bash-only")
        os.mkdir(self.bash_only)
        os.symlink(shutil.which("bash"), os.path.join(self.bash_only, "bash"))
        self.env = {
            "PATH": f"{self.compilers}:{self.bash_only}",
            "GUARD_TEST_LOG": self.log,
            "REIFY_CANON_SCCACHE": self.real,
            "REIFY_CANON_UNCACHED_LOG": self.uncached,
            "CARGO_INCREMENTAL": "0",
            "CARGO_TERM_COLOR": "always",
            "CARGO_TERM_PROGRESS_WHEN": "never",
            "CARGO_LOG": "x",
            "CARGO_HOME": "/h",
        }

    def guard(self, *args, guard=GUARD, **env):
        merged = {name: value for name, value in dict(self.env, **env).items() if value is not None}
        return subprocess.run(
            [str(guard), *args], env=merged, capture_output=True, text=True, check=False
        )

    def calls(self):
        if not os.path.exists(self.log):
            return []
        with open(self.log) as log:
            return [json.loads(line) for line in log]

    def uncached_lines(self):
        if not os.path.exists(self.uncached):
            return []
        with open(self.uncached) as log:
            return log.read().splitlines()

    def test_target_cpu_native_runs_the_compiler_directly_and_is_counted(self):
        result = self.guard(self.rustc, "--crate-name", "x", "-C", "target-cpu=native")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual([call["prog"] for call in calls], ["rustc"])
        self.assertEqual(calls[0]["argv"], ["--crate-name", "x", "-C", "target-cpu=native"])
        self.assertIn("CARGO_INCREMENTAL", calls[0]["cargo"])
        self.assertEqual(self.uncached_lines(), [self.rustc])

    def test_every_machine_flag_spelling_bypasses_the_cache(self):
        for argv in (
            (self.rustc, "--crate-name", "x", "-Ctarget-cpu=native"),
            (self.rustc, "--crate-name", "x", "--codegen", "target-cpu=native"),
            (self.rustc, "--crate-name", "x", "--codegen=target-cpu=native"),
            ("cc", "-march=native", "-c", "u.c"),
            ("cc", "-mtune=native", "-c", "u.c"),
            ("cc", "-mcpu=native", "-c", "u.c"),
        ):
            with self.subTest(argv=argv):
                for path in (self.log, self.uncached):
                    Path(path).unlink(missing_ok=True)
                result = self.guard(*argv)
                self.assertEqual(result.returncode, 0, result.stderr)
                calls = self.calls()
                self.assertEqual([call["prog"] for call in calls], [os.path.basename(argv[0])])
                self.assertEqual(calls[0]["argv"], list(argv[1:]))
                self.assertEqual(self.uncached_lines(), [argv[0]])

    def test_ordinary_compile_goes_to_the_real_sccache_without_presentation_variables(self):
        argv = [self.rustc, "--crate-name", "x", "src/lib.rs"]
        result = self.guard(*argv)
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual([call["prog"] for call in calls], ["sccache"])
        self.assertEqual(calls[0]["argv"], argv)
        self.assertEqual(calls[0]["cargo"], ["CARGO_HOME"])
        self.assertEqual(self.uncached_lines(), [])

    def test_sccache_options_pass_straight_through(self):
        result = self.guard("--show-stats", "--stats-format", "json")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual([call["prog"] for call in calls], ["sccache"])
        self.assertEqual(calls[0]["argv"], ["--show-stats", "--stats-format", "json"])

    def test_without_the_pointer_the_first_unmarked_sccache_on_path_is_used(self):
        guard_copy = os.path.join(self.tmp, "guard-copy")
        os.mkdir(guard_copy)
        shutil.copy2(GUARD, os.path.join(guard_copy, "sccache"))
        path = os.pathsep.join((guard_copy, str(GUARD.parent), self.real_dir, self.compilers, self.bash_only))
        result = self.guard(self.rustc, "src/lib.rs", REIFY_CANON_SCCACHE=None, PATH=path)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([call["prog"] for call in self.calls()], ["sccache"])
        self.assertEqual(self.calls()[0]["argv"], [self.rustc, "src/lib.rs"])

    def test_no_real_sccache_exits_127_with_one_line(self):
        path = os.pathsep.join((str(GUARD.parent), self.compilers, self.bash_only))
        result = self.guard(self.rustc, "src/lib.rs", REIFY_CANON_SCCACHE=None, PATH=path)
        self.assertEqual(result.returncode, 127, result.stderr)
        self.assertEqual(len(result.stderr.splitlines()), 1, result.stderr)
        self.assertEqual(self.calls(), [])

    def test_flagged_compile_runs_without_the_uncached_log(self):
        result = self.guard("cc", "-march=native", "-c", "u.c", REIFY_CANON_UNCACHED_LOG=None)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([call["prog"] for call in self.calls()], ["cc"])
        self.assertFalse(os.path.exists(self.uncached))

    def test_guard_carries_the_marker_canon_cache_skips(self):
        self.assertTrue(canon_cache.is_guard(str(GUARD)))
        self.assertEqual(sorted(os.listdir(GUARD.parent)), ["sccache"])


FAST_KNOBS = dataclasses.replace(canon_cache.SessionKnobs(), probe_timeout=0.3)


def store_settings(url, user="reify", password="hunter2"):
    return canon_cache.CacheSettings(store_url=url, store_user=user, store_password=password)


class StoreCase(unittest.TestCase):
    def setUp(self):
        self.store = cfx.FakeStore()
        self.addCleanup(self.store.stop)
        self.settings = store_settings(self.store.url)

    def probe(self, settings=None, knobs=FAST_KNOBS, **kwargs):
        return canon_cache.probe_store(settings or self.settings, knobs, **kwargs)


class ProbeStore(StoreCase):
    """C5's probe: GET <store_url>/.sccache_check, judged on the injected clock."""

    def test_reachable_on_404_and_200(self):
        for status in (404, 200):
            with self.subTest(status=status):
                self.store.check_status = status
                self.assertTrue(self.probe())
        self.assertEqual(self.store.requests[-1], ("GET", "/reify/.sccache_check"))

    def test_sends_basic_auth(self):
        self.probe()
        expected = "Basic " + base64.b64encode(b"reify:hunter2").decode()
        self.assertEqual(self.store.authorizations, [expected])

    def test_unreachable_on_401(self):
        self.store.set_mode("auth")
        self.assertFalse(self.probe())

    def test_unreachable_on_a_refused_port(self):
        self.assertFalse(self.probe(store_settings(f"http://127.0.0.1:{cfx.closed_port()}/reify")))

    def test_unreachable_when_the_store_never_answers(self):
        self.store.set_mode("hang")
        self.assertFalse(self.probe())

    def test_unreachable_when_the_clock_says_the_answer_was_slow(self):
        readings = iter((100.0, 100.0 + FAST_KNOBS.slow + 0.5))
        self.assertFalse(self.probe(clock=lambda: next(readings)))

    def test_reachable_when_the_clock_says_the_answer_was_quick(self):
        readings = iter((100.0, 100.0 + FAST_KNOBS.slow - 0.5))
        self.assertTrue(self.probe(clock=lambda: next(readings)))


class ChooseBackend(unittest.TestCase):
    """Boundary row 6: which backend a session starts with."""

    def test_no_store_url_never_probes(self):
        def never():
            raise AssertionError("probed without a store_url")

        self.assertEqual(
            canon_cache.choose_backend(canon_cache.CacheSettings(), never),
            (canon_cache.Backend.LOCAL_FALLBACK, canon_cache.Reason.STORE_NOT_CONFIGURED),
        )

    def test_refusing_store_falls_back(self):
        self.assertEqual(
            canon_cache.choose_backend(store_settings("http://s/r"), lambda: False),
            (canon_cache.Backend.LOCAL_FALLBACK, canon_cache.Reason.STORE_UNREACHABLE),
        )

    def test_reachable_store_is_shared(self):
        self.assertEqual(
            canon_cache.choose_backend(store_settings("http://s/r"), lambda: True),
            (canon_cache.Backend.SHARED, canon_cache.Reason.NONE),
        )


class RelayCase(StoreCase):
    def setUp(self):
        super().setUp()
        self.relay = canon_cache.Relay("127.0.0.1", self.store.port)
        self.relay.start()
        self.addCleanup(self.relay.close)
        self.relay_port = int(self.relay.endpoint.rsplit(":", 1)[1])

    def start_slow_transfer(self):
        """A GET through the relay that has received SLOW_HEAD and waits for the rest."""
        connection = http.client.HTTPConnection("127.0.0.1", self.relay_port, timeout=30)
        self.addCleanup(connection.close)
        connection.request("GET", "/slow/object")
        response = connection.getresponse()
        self.assertEqual(response.read(len(cfx.SLOW_HEAD)), cfx.SLOW_HEAD)
        return response

    def assert_cut_short(self, response):
        received = 0
        try:
            while chunk := response.read1(65536):
                received += len(chunk)
        except (ConnectionResetError, http.client.IncompleteRead):
            pass
        self.assertLess(len(cfx.SLOW_HEAD) + received, cfx.SLOW_LENGTH)


class Relay(RelayCase):
    """C5's loopback relay: owned, cuttable, and holding its port until closed."""

    def test_endpoint_is_loopback(self):
        self.assertEqual(self.relay.endpoint, f"http://127.0.0.1:{self.relay_port}")

    def test_get_through_the_relay_is_byte_for_byte_the_store_answer(self):
        direct = cfx.raw_get(self.store.port, "/reify/object")
        self.assertTrue(direct.startswith(b"HTTP/1.1 200"), direct)
        self.assertEqual(cfx.raw_get(self.relay_port, "/reify/object"), direct)

    def test_cut_resets_an_in_flight_transfer(self):
        response = self.start_slow_transfer()
        self.relay.cut()
        self.store.release()
        self.assert_cut_short(response)

    def test_after_cut_new_connections_get_no_answer_and_the_port_stays_bound(self):
        self.relay.cut()
        self.assertEqual(cfx.raw_get(self.relay_port, "/reify/object"), b"")
        self.assertFalse(cfx.can_bind(self.relay_port))

    def test_close_releases_the_port(self):
        self.relay.cut()
        self.relay.close()
        self.assertTrue(cfx.can_bind(self.relay_port))


class FakeClock:
    def __init__(self):
        self.now = 1000.0

    def __call__(self):
        return self.now


class ScriptedLink:
    """A scripted probe ('ok', 'slow' or 'fail' per call), a recording relay, and an instant wait."""

    def __init__(self, test, script):
        self.clock = FakeClock()
        self.script = list(script)
        self.probes = 0
        self.cuts = 0
        self.watchdog = canon_cache.Watchdog(self.probe, self, test.knobs, self.clock, self.wait)

    def probe(self):
        outcome = self.script[self.probes]
        self.probes += 1
        if outcome == "slow":
            self.clock.now += FAST_KNOBS.slow + 1.0
        return outcome != "fail"

    def wait(self, stopped, seconds):
        if stopped.is_set() or self.probes == len(self.script):
            return True
        self.clock.now += seconds
        return False

    def cut(self):
        self.cuts += 1

    def run(self):
        self.watchdog.start()
        self.watchdog.join()
        return self.watchdog


class Watchdog(unittest.TestCase):
    """Boundary row 7's trigger: two consecutive failed or slow probes cut the relay."""

    knobs = FAST_KNOBS

    def test_no_two_consecutive_failures_do_not_cut(self):
        link = ScriptedLink(self, ["ok", "slow", "ok", "fail"])
        self.assertFalse(link.run().lost)
        self.assertEqual((link.probes, link.cuts), (4, 0))

    def test_fail_then_slow_cuts_after_the_third_probe(self):
        link = ScriptedLink(self, ["ok", "fail", "slow"])
        self.assertTrue(link.run().lost)
        self.assertEqual((link.probes, link.cuts), (3, 1))

    def test_no_probe_after_the_cut(self):
        link = ScriptedLink(self, ["fail", "fail", "ok", "ok"])
        self.assertTrue(link.run().lost)
        self.assertEqual((link.probes, link.cuts), (2, 1))

    def test_stop_ends_the_thread_without_loss(self):
        link = ScriptedLink(self, [])
        watchdog = canon_cache.Watchdog(link.probe, link, self.knobs, link.clock)
        watchdog.start()
        watchdog.stop()
        self.assertFalse(watchdog.lost)
        self.assertEqual((link.probes, link.cuts), (0, 0))


class StoreLost(RelayCase):
    """Row 7 against the real relay: a store that stops answering mid-transfer is cut."""

    def test_two_failed_probes_cut_the_relay_and_reset_the_transfer(self):
        response = self.start_slow_transfer()
        self.store.set_mode("hang")
        probes = []

        def probe():
            probes.append(canon_cache.probe_store(self.settings, FAST_KNOBS))
            return probes[-1]

        watchdog = canon_cache.Watchdog(probe, self.relay, FAST_KNOBS, wait=lambda stopped, seconds: False)
        watchdog.start()
        watchdog.join()
        self.assertTrue(watchdog.lost)
        self.assertEqual(probes, [False, False])
        self.store.release()
        self.assert_cut_short(response)


STATS = {"hits": {"Rust": 3, "C/C++": 2}, "misses": {"Rust": 1, "C/C++": 1}, "write_errors": 1}
DETACHED_STATS = {"hits": {"Rust": 40}, "misses": {"C/C++": 9}, "write_errors": 0}


class SessionCase(unittest.TestCase):
    """A fake sccache whose server follows a scripted plan, and a scratch build directory."""

    def setUp(self):
        self.tmp = scratch(self)
        self.fake = cfx.fake_sccache_dir(self.tmp)
        self.sccache = os.path.join(self.fake, "sccache")
        self.build = os.path.join(self.tmp, "build")
        os.mkdir(self.build)
        self.lock = os.path.join(self.tmp, "start.lock")
        self.knobs = dataclasses.replace(
            canon_cache.SessionKnobs(), start_lock=self.lock, tmp_root=self.tmp, socket_wait=60.0
        )
        self.delays = []
        self.socket = os.path.join(self.build, f".reify-sccache.{os.getpid()}.sock")
        self.env = {
            "PATH": "/usr/bin:/bin",
            "HOME": self.tmp,
            "FAKE_SCCACHE_LOCK": self.lock,
            "FAKE_SCCACHE_STATS": json.dumps(STATS),
            "SCCACHE_SERVER_PORT": "4226",
            "SCCACHE_DIR": "/inherited",
        }
        self.settings = canon_cache.CacheSettings(
            store_url="http://127.0.0.1:9/reify/cache",
            store_user="u",
            store_password="p",
            fallback_dir=os.path.join(self.tmp, "fallback"),
            fallback_cap="3G",
        )

    def open(self, plan, *, settings=None, reachable=True, policy=canon_cache.Policy.SHARED):
        cfx.set_plan(self.fake, plan)
        session = canon_cache.open_session(
            policy,
            settings or self.settings,
            canon_build_dir=self.build,
            sccache=self.sccache,
            knobs=self.knobs,
            env=self.env,
            probe=lambda: reachable,
            rng=random.Random(7),
            sleep=self.delays.append,
            clock=time.monotonic,
        )
        self.addCleanup(session.close)
        return session

    def servers(self):
        return cfx.servers(self.fake)

    def autostarts(self):
        return [call for call in cfx.read_sccache_calls(self.fake) if call.get("autostart")]


class OpenSession(SessionCase):
    """C4's server start and C5's start-up check retry."""

    def test_shared_server_environment(self):
        inherited = os.open(os.devnull, os.O_RDONLY)
        self.addCleanup(os.close, inherited)
        os.set_inheritable(inherited, True)
        session = self.open("ok")
        [server] = self.servers()
        env = server["env"]
        for name, value in (
            ("SCCACHE_START_SERVER", "1"),
            ("SCCACHE_NO_DAEMON", "1"),
            ("SCCACHE_IDLE_TIMEOUT", "0"),
            ("SCCACHE_SERVER_UDS", self.socket),
            ("SCCACHE_WEBDAV_KEY_PREFIX", "reify/cache"),
            ("SCCACHE_WEBDAV_USERNAME", "u"),
            ("SCCACHE_WEBDAV_PASSWORD", "p"),
        ):
            self.assertEqual(env.get(name), value, name)
        self.assertRegex(env["SCCACHE_WEBDAV_ENDPOINT"], r"^http://127\.0\.0\.1:[0-9]+$")
        self.assertNotIn("SCCACHE_SERVER_PORT", env)
        self.assertNotIn("SCCACHE_DIR", env)
        self.assertEqual(server["fds"], [0, 1, 2])
        self.assertEqual(session.backend, canon_cache.Backend.SHARED)
        self.assertEqual(session.reason, canon_cache.Reason.NONE)
        self.assertEqual(session.store_attempts, 1)
        self.assertEqual(session.environment.server.socket, self.socket)
        self.assertEqual(session.environment.policy, canon_cache.Policy.SHARED)
        self.assertIsNotNone(session.watchdog)

    def test_one_failed_start_retries_once_after_a_drawn_delay(self):
        for plan in ("exit,ok", "readonly,ok"):
            with self.subTest(plan=plan):
                Path(self.fake, "calls.jsonl").unlink(missing_ok=True)
                self.delays.clear()
                session = self.open(plan)
                first, second = self.servers()
                self.assertEqual(session.backend, canon_cache.Backend.SHARED)
                self.assertEqual(session.store_attempts, 2)
                self.assertEqual(len(self.delays), 1)
                self.assertTrue(0.5 <= self.delays[0] <= 2.0, self.delays)
                self.assertTrue(cfx.process_gone(first["pid"]))
                self.assertFalse(cfx.process_gone(second["pid"]))
                self.assertEqual(self.autostarts(), [])
                session.close()

    def test_three_failed_starts_fall_back_to_the_local_cache(self):
        session = self.open("exit,exit,exit,ok")
        self.assertEqual(session.backend, canon_cache.Backend.LOCAL_FALLBACK)
        self.assertEqual(session.reason, canon_cache.Reason.STORE_UNREACHABLE)
        self.assertEqual(session.store_attempts, 3)
        self.assertIsNone(session.watchdog)
        fallback = self.servers()[3]["env"]
        self.assertEqual(fallback["SCCACHE_DIR"], self.settings.fallback_dir)
        self.assertEqual(fallback["SCCACHE_CACHE_SIZE"], "3G")
        self.assertEqual([name for name in fallback if name.startswith("SCCACHE_WEBDAV_")], [])

    def test_three_read_only_starts_are_reported_as_read_only(self):
        session = self.open("readonly,readonly,readonly,ok")
        self.assertEqual(session.backend, canon_cache.Backend.LOCAL_FALLBACK)
        self.assertEqual(session.reason, canon_cache.Reason.READ_ONLY_SERVER)
        self.assertEqual(session.store_attempts, 3)

    def test_unreachable_store_falls_back_without_attempts(self):
        session = self.open("ok", reachable=False)
        self.assertEqual(session.backend, canon_cache.Backend.LOCAL_FALLBACK)
        self.assertEqual(session.reason, canon_cache.Reason.STORE_UNREACHABLE)
        self.assertEqual(session.store_attempts, 0)

    def test_no_store_url_falls_back_without_attempts(self):
        settings = dataclasses.replace(self.settings, store_url=None)
        session = self.open("ok", settings=settings)
        self.assertEqual(session.backend, canon_cache.Backend.LOCAL_FALLBACK)
        self.assertEqual(session.reason, canon_cache.Reason.STORE_NOT_CONFIGURED)
        self.assertEqual(session.store_attempts, 0)

    def test_start_lock_is_held_during_the_spawn_and_released_after(self):
        self.open("ok")
        self.assertIs(self.servers()[0]["lock_held"], True)
        with open(self.lock, "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_server_starts_anyway_when_the_lock_wait_runs_out(self):
        self.knobs = dataclasses.replace(self.knobs, start_lock_wait=0.05)
        with open(self.lock, "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            session = self.open("ok")
        self.assertEqual(session.backend, canon_cache.Backend.SHARED)
        self.assertEqual(len(self.servers()), 1)

    def test_incremental_is_refused(self):
        with self.assertRaises(ValueError):
            self.open("ok", policy=canon_cache.Policy.INCREMENTAL)
        self.assertEqual(self.servers(), [])


class CloseSession(SessionCase):
    """C4's teardown: stats, then stop, then the relay; nothing left behind."""

    def test_counters_are_summed_across_languages(self):
        outcome = self.open("ok").close()
        self.assertEqual((outcome.hits, outcome.misses, outcome.write_errors), (5, 2, 1))
        self.assertEqual(outcome.detached_server, 0)
        self.assertEqual((outcome.backend, outcome.reason, outcome.store_attempts), (
            canon_cache.Backend.SHARED, canon_cache.Reason.NONE, 1))

    def test_stats_then_stop_then_relay_close_and_nothing_left(self):
        self.open("ok").close()
        commands = [call for call in cfx.read_sccache_calls(self.fake) if "command" in call]
        self.assertEqual([call["command"] for call in commands], ["stats", "stop"])
        self.assertIs(commands[1]["relay_open"], True)
        self.assertEqual(sorted(name for name in os.listdir(self.build) if name.startswith(".reify-sccache")), [])
        self.assertTrue(cfx.process_gone(self.servers()[0]["pid"]))
        self.assertEqual(self.autostarts(), [])

    def test_close_is_idempotent(self):
        session = self.open("ok")
        self.assertEqual(session.close(), session.close())

    def test_read_only_line_at_exit_is_reported(self):
        outcome = self.open("late_readonly").close()
        self.assertEqual(outcome.reason, canon_cache.Reason.READ_ONLY_SERVER)

    def test_detached_server_on_the_socket_is_counted_and_stopped(self):
        session = self.open("die")
        Path(self.fake, "die-now").touch()
        while self.answers(self.socket):
            time.sleep(0.02)
        os.unlink(self.socket)
        detached = subprocess.Popen(
            [self.sccache],
            env=dict(
                self.env,
                SCCACHE_START_SERVER="1",
                SCCACHE_SERVER_UDS=self.socket,
                FAKE_SCCACHE_BEHAVIOUR="ok",
                FAKE_SCCACHE_STATS=json.dumps(DETACHED_STATS),
            ),
            stdin=subprocess.DEVNULL,
        )
        self.addCleanup(detached.kill)
        while not self.answers(self.socket):
            time.sleep(0.02)
        outcome = session.close()
        self.assertEqual(outcome.detached_server, 1)
        self.assertEqual((outcome.hits, outcome.misses, outcome.write_errors), (40, 9, 0))
        self.assertEqual(detached.wait(), 0)

    @staticmethod
    def answers(path):
        with socket.socket(socket.AF_UNIX) as probe:
            try:
                probe.connect(path)
            except OSError:
                return False
        return True


class PolicyMembers(unittest.TestCase):
    def test_members(self):
        self.assertEqual([policy.value for policy in canon_cache.Policy], ["shared", "mixed", "incremental"])


if __name__ == "__main__":
    unittest.main()
