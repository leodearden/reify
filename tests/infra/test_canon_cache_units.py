#!/usr/bin/env python3
"""
test_canon_cache_units.py — stdlib unittest for scripts/canon_cache.py, the
cache half of the canonical-build-path primitive (dark-factory
plans/canonical-build-path-shared-cache-prd.md C4-C7): C4's per-policy cargo
and command environment table and its CARGO_HOME pin, the environment's server
invariant, the host.toml cache settings, real-sccache resolution with the
marker reader and PATH walk canon_run shares, the fallback directory, the
guard, the backend choice and status's store check, the private session
(start, start-up retry, its three waits, the stale session-file sweep,
teardown), the build_cache writer, supervise's measurement and signal policy,
and exit_like.

The store link itself (probe, relay, watchdog) lives in scripts/canon_store.py
since task 10441; its cases are in test_canon_store_units.py.

Every session runs canon_cache_fixture's fake sccache; no case enters a mount
namespace or starts a real sccache.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_canon_cache_units.sh.
"""

import dataclasses
import fcntl
import itertools
import json
import os
import random
import re
import resource
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_cache_fixture as cfx  # noqa: E402  (imported after sys.path manipulation)
import canon_run_fixture as fx  # noqa: E402
import canon_store_fixture as sfx  # noqa: E402

sys.path.insert(0, str(fx.REPO_ROOT / "scripts"))
import canon_cache  # noqa: E402
import canon_store  # noqa: E402
from verify_events_lib import read_events  # noqa: E402

ORCHESTRATOR_CONFIG = fx.REPO_ROOT / "dark-factory-orchestrator.yaml"
EVENT_SAMPLE = Path(__file__).resolve().parent / "build_cache_event_sample.jsonl"
C6_FIELDS = {
    "mode", "policy", "backend", "reason", "hits", "misses", "write_errors",
    "uncached_machine_flags", "detached_server", "wall_secs", "cpu_secs",
}
ENVELOPE = {"v", "ts_ns", "pid", "event"}
CARGO_HOME_LINE = re.compile(r"^\s+CARGO_HOME:\s*(\S+)\s*$", re.MULTILINE)
GUARD_LINE = "#!/bin/sh\n# reify-canon-guard\nexit 0\n"
REAL_LINE = "#!/bin/sh\nexit 0\n"
GUARD = fx.REPO_ROOT / "scripts" / "canon-bin" / "guard" / "sccache"
UNRESOLVABLE_HOST = "reify-canon-no-such-host.invalid"  # RFC 6761: .invalid never resolves

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


HANG_GUARD = 60.0


def wait_until(test, predicate, what):
    """Poll until predicate holds; past HANG_GUARD fail naming what never happened (a hang guard, not a bound)."""
    deadline = time.monotonic() + HANG_GUARD
    while not predicate():
        if time.monotonic() > deadline:
            test.fail(f"{what}: not within the {HANG_GUARD:.0f}s hang guard")
        time.sleep(0.02)


def assert_one_line_naming(test, error, path):
    """A refusal a caller can print as one line, naming the path it is about."""
    message = str(error)
    test.assertNotIn("\n", message)
    test.assertIn(path, message)


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


class ServerInvariant(CacheEnvironmentCase):
    """A server address exactly when the policy runs sccache: refused at construction, None at decode."""

    def test_a_mismatched_server_is_refused_naming_the_policy(self):
        for policy, server in (
            (canon_cache.Policy.SHARED, None),
            (canon_cache.Policy.MIXED, None),
            (canon_cache.Policy.INCREMENTAL, self.SERVER),
        ):
            with self.subTest(policy=policy):
                with self.assertRaises(ValueError) as refused:
                    canon_cache.CacheEnvironment(policy, server)
                assert_one_line_naming(self, refused.exception, "server")
                self.assertIn(policy.value, str(refused.exception))

    def test_uses_guard_needs_no_server(self):
        """The PATH probe's seam: which policies put the guard first, before any server exists."""
        self.assertEqual(
            {policy: canon_cache.uses_guard(policy) for policy in canon_cache.Policy},
            {canon_cache.Policy.SHARED: True, canon_cache.Policy.MIXED: True, canon_cache.Policy.INCREMENTAL: False},
        )


class CacheEnvironmentRecord(CacheEnvironmentCase):
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
            dict(good, server=None),
            {"policy": "incremental", "server": good["server"]},
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
            ("store_url", "http://a..b/reify"),
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


class CarriesMarker(unittest.TestCase):
    """The one marker reader: the marker within a file's first MARKER_WINDOW bytes."""

    MARKER = b"reify-canon-test-marker"

    def setUp(self):
        self.tmp = scratch(self)

    def file_with(self, data):
        path = Path(self.tmp, "candidate")
        path.write_bytes(data)
        return str(path)

    def test_a_marker_inside_the_first_1024_bytes_is_carried(self):
        for data in (b"#!/bin/sh\n# " + self.MARKER + b"\n", b"x" * (1024 - len(self.MARKER)) + self.MARKER):
            with self.subTest(at=data.index(self.MARKER)):
                self.assertTrue(canon_cache.carries_marker(self.file_with(data), self.MARKER))

    def test_a_marker_starting_after_byte_1024_is_not(self):
        self.assertFalse(canon_cache.carries_marker(self.file_with(b"x" * 1024 + self.MARKER), self.MARKER))

    def test_a_missing_path_or_a_directory_does_not_carry_it(self):
        self.assertFalse(canon_cache.carries_marker(os.path.join(self.tmp, "missing"), self.MARKER))
        self.assertFalse(canon_cache.carries_marker(self.tmp, self.MARKER))


class ExecutablesNamed(unittest.TestCase):
    """The one PATH walk: every <entry>/<name> that is an executable regular file, in PATH order."""

    def setUp(self):
        self.tmp = scratch(self)
        self.a, self.b, self.c, self.d = (os.path.join(self.tmp, name) for name in "abcd")
        executable(self.a, "cargo", REAL_LINE)
        executable(self.a, "sccache", REAL_LINE)
        Path(self.b).mkdir()
        Path(self.b, "cargo").write_text(REAL_LINE)
        executable(self.c, "cargo", REAL_LINE)
        Path(self.d, "cargo").mkdir(parents=True)
        self.path = ":".join((self.a, "", self.b, self.c, self.d))

    def test_executable_regular_files_in_path_order(self):
        self.assertEqual(
            canon_cache.executables_named(self.path, "cargo"),
            [os.path.join(self.a, "cargo"), os.path.join(self.c, "cargo")],
        )

    def test_only_the_name_asked_for(self):
        self.assertEqual(canon_cache.executables_named(self.path, "sccache"), [os.path.join(self.a, "sccache")])

    def test_an_empty_path_names_none(self):
        self.assertEqual(canon_cache.executables_named("", "cargo"), [])


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

    def settings_with_an_unusable_configured_dir(self):
        """A fallback_dir under a regular file, so the per-user directory under self.tmp is the one chosen."""
        blocker = Path(self.tmp, "configured-file")
        blocker.write_text("")
        return self.settings(str(blocker / "fallback"))

    def per_user(self):
        return os.path.join(self.tmp, f"reify-canon-fallback-{os.getuid()}")

    def assert_per_user_refused(self):
        with self.assertRaises(canon_cache.CacheSessionError) as refused:
            canon_cache.choose_fallback_dir(self.settings_with_an_unusable_configured_dir(), tmp_root=self.tmp)
        assert_one_line_naming(self, refused.exception, self.per_user())

    def test_a_per_user_directory_writable_by_others_is_refused(self):
        os.mkdir(self.per_user())
        for mode in (0o777, 0o770):
            with self.subTest(mode=oct(mode)):
                os.chmod(self.per_user(), mode)
                self.assert_per_user_refused()

    def test_a_per_user_path_that_is_a_symlink_is_refused(self):
        private = Path(self.tmp, "private")
        private.mkdir()
        private.chmod(0o700)
        os.symlink(private, self.per_user())
        self.assert_per_user_refused()

    def test_a_directory_closed_to_writes_is_refused_only_when_another_user_owns_it(self):
        """0o755 passes the mode check, so ownership alone keeps out another user's planted entries."""
        shared = Path(self.tmp, "shared")
        shared.mkdir()
        shared.chmod(0o755)
        info, uid = os.lstat(shared), os.getuid()
        self.assertEqual(canon_cache.private_dir_problem(info, uid), "")
        problem = canon_cache.private_dir_problem(info, uid + 1)
        self.assertIn(f"uid {uid}", problem)
        self.assertIn(f"not {uid + 1}", problem)

    def test_an_existing_private_per_user_directory_is_used(self):
        os.mkdir(self.per_user())
        os.chmod(self.per_user(), 0o700)
        chosen = canon_cache.choose_fallback_dir(self.settings_with_an_unusable_configured_dir(), tmp_root=self.tmp)
        self.assertEqual(chosen, self.per_user())

    def test_a_tmp_root_that_cannot_hold_the_directory_is_a_session_error(self):
        """Regular files in the way, so neither directory can be made even as root."""
        configured_parent, tmp_root = Path(self.tmp, "configured-file"), Path(self.tmp, "tmp-root-file")
        configured_parent.write_text("")
        tmp_root.write_text("")
        with self.assertRaises(canon_cache.CacheSessionError) as refused:
            canon_cache.choose_fallback_dir(self.settings(str(configured_parent / "fallback")), tmp_root=str(tmp_root))
        assert_one_line_naming(self, refused.exception, os.path.join(tmp_root, f"reify-canon-fallback-{os.getuid()}"))


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
        self.assertTrue(canon_cache.carries_marker(str(GUARD), canon_cache.GUARD_MARKER))
        self.assertEqual(sorted(os.listdir(GUARD.parent)), ["sccache"])


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
            canon_cache.choose_backend(canon_cache.CacheSettings(store_url="http://s/r"), lambda: False),
            (canon_cache.Backend.LOCAL_FALLBACK, canon_cache.Reason.STORE_UNREACHABLE),
        )

    def test_reachable_store_is_shared(self):
        self.assertEqual(
            canon_cache.choose_backend(canon_cache.CacheSettings(store_url="http://s/r"), lambda: True),
            (canon_cache.Backend.SHARED, canon_cache.Reason.NONE),
        )


class StoreReachable(unittest.TestCase):
    """status's store check: None with no store configured, else what canon_store's probe says."""

    def test_none_without_a_store_url(self):
        self.assertIsNone(canon_cache.store_reachable(canon_cache.CacheSettings()))

    def test_true_for_an_answering_store(self):
        with sfx.FakeStore() as store:
            self.assertIs(canon_cache.store_reachable(canon_cache.CacheSettings(store_url=store.url)), True)

    def test_false_for_a_refused_port(self):
        settings = canon_cache.CacheSettings(store_url=f"http://127.0.0.1:{sfx.closed_port()}/reify")
        self.assertIs(canon_cache.store_reachable(settings), False)


class CacheSettingsStore(unittest.TestCase):
    def test_the_store_is_built_from_the_three_store_keys(self):
        settings = canon_cache.CacheSettings(store_url="http://s:8080/r", store_user="u", store_password="p")
        self.assertEqual(settings.store, canon_store.Store("http://s:8080/r", "u", "p"))

    def test_no_store_without_a_store_url(self):
        self.assertIsNone(canon_cache.CacheSettings(store_user="u", store_password="p").store)


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
        self.knobs = dataclasses.replace(canon_cache.SessionKnobs(), start_lock=self.lock, tmp_root=self.tmp)
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

    def open(
        self, plan, *, settings=None, reachable=True, policy=canon_cache.Policy.SHARED, sccache=None,
        canon_build_dir=None,
    ):
        cfx.set_plan(self.fake, plan)
        session = canon_cache.open_session(
            policy,
            settings or self.settings,
            canon_build_dir=canon_build_dir or self.build,
            sccache=sccache or self.sccache,
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

    def test_an_unresolvable_store_falls_back_without_attempts(self):
        settings = dataclasses.replace(self.settings, store_url=f"http://{UNRESOLVABLE_HOST}:8080/reify")
        session = self.open("ok", settings=settings)
        self.assertEqual(
            (session.backend, session.reason, session.store_attempts),
            (canon_cache.Backend.LOCAL_FALLBACK, canon_cache.Reason.STORE_UNREACHABLE, 0),
        )
        [server] = self.servers()
        self.assertEqual([name for name in server["env"] if name.startswith("SCCACHE_WEBDAV_")], [])

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

    def test_a_sccache_that_is_not_executable_is_refused_before_any_start_attempt(self):
        """With or without /usr/bin/setpriv: no store retry, and the refusal names the real cause."""
        plain = Path(self.tmp, "plain-file-sccache")
        plain.write_text("#!/bin/sh\nexit 0\n")
        plain.chmod(0o644)
        for sccache in (str(plain), os.path.join(self.tmp, "no-such-sccache")):
            with self.subTest(sccache=sccache):
                self.delays.clear()
                with self.assertRaises(canon_cache.CacheSessionError) as refused:
                    self.open("ok", sccache=sccache)
                assert_one_line_naming(self, refused.exception, sccache)
                self.assertEqual(self.delays, [])
                self.assertEqual([name for name in os.listdir(self.build) if name.startswith(".reify-sccache.")], [])

    def test_a_build_directory_that_cannot_be_created_is_a_session_error(self):
        blocker = Path(self.tmp, "a-file")
        blocker.write_text("")
        build = str(blocker / "build")
        with self.assertRaises(canon_cache.CacheSessionError) as refused:
            self.open("ok", canon_build_dir=build)
        assert_one_line_naming(self, refused.exception, build)

    def test_an_unwritable_build_directory_is_a_session_error(self):
        if os.geteuid() == 0:
            self.skipTest("root writes into a read-only directory regardless of its mode")
        os.chmod(self.build, 0o500)
        self.addCleanup(os.chmod, self.build, 0o755)
        with self.assertRaises(canon_cache.CacheSessionError) as refused:
            self.open("ok")
        assert_one_line_naming(self, refused.exception, os.path.join(self.build, f".reify-sccache.{os.getpid()}.err"))
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
        wait_until(self, lambda: not canon_cache.answers(self.socket), "the dying server released its socket")
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
        wait_until(self, lambda: canon_cache.answers(self.socket), "the detached server answers on the socket")
        outcome = session.close()
        self.assertEqual(outcome.detached_server, 1)
        self.assertEqual((outcome.hits, outcome.misses, outcome.write_errors), (40, 9, 0))
        self.assertEqual(detached.wait(), 0)


IGNORES_TERM = """
import pathlib, signal, sys, time
signal.signal(signal.SIGTERM, signal.SIG_IGN)
pathlib.Path(sys.argv[1]).touch()
time.sleep(600)
"""


class TripwireClock:
    """An injected clock ticking 1.0 per read; past `limit` it trips and reads inf, so every deadline passes."""

    def __init__(self, start, limit):
        self._ticks = itertools.count(start, 1.0)
        self._limit = limit
        self.tripped = False

    def __call__(self):
        now = next(self._ticks)
        if now > self._limit:
            self.tripped = True
            return float("inf")
        return now


class SessionWaits(SessionCase):
    """Three waits, three knobs: ready_wait for a start, stop_wait for each reap step, client_timeout per call."""

    def join_or_fail(self, thread, what, unstick):
        thread.join(HANG_GUARD)
        if thread.is_alive():
            unstick()
            self.fail(f"{what}: not within the {HANG_GUARD:.0f}s hang guard")

    def test_start_server_gives_up_on_a_silent_server_after_ready_wait(self):
        files = canon_cache.SessionFiles.in_build_dir(self.build, os.getpid())
        env = dict(
            self.env, SCCACHE_START_SERVER="1", SCCACHE_SERVER_UDS=files.socket, FAKE_SCCACHE_BEHAVIOUR="silent"
        )
        knobs = dataclasses.replace(self.knobs, ready_wait=3.0, stop_wait=3600.0, client_timeout=3600.0)
        clock = TripwireClock(start=1000.0, limit=1020.0)
        process, failure = canon_cache.start_server(self.sccache, env, files, knobs, clock)
        self.addCleanup(process.wait)
        self.addCleanup(process.kill)
        self.assertFalse(clock.tripped, "start_server polled past ready_wait=3 on the injected clock")
        self.assertIs(failure, canon_cache.StartFailure.NOT_READY)
        self.assertIsNone(process.poll(), "gave up by its deadline, not because the server exited")
        canon_cache.stop_now(process, 3600.0)
        self.assertTrue(cfx.process_gone(process.pid))

    def test_reap_kills_a_process_that_ignores_term_after_two_stop_waits(self):
        ready = os.path.join(self.tmp, "ready")
        child = subprocess.Popen([sys.executable, "-I", "-c", IGNORES_TERM, ready])
        self.addCleanup(child.wait)
        self.addCleanup(child.kill)
        wait_until(self, lambda: os.path.exists(ready), "the TERM-ignoring child started")
        reaper = threading.Thread(target=canon_cache.reap, args=(child, 0.1), daemon=True)
        reaper.start()
        self.join_or_fail(reaper, "reap", child.kill)
        self.assertEqual(child.returncode, -signal.SIGKILL)

    def test_close_bounds_each_client_call_by_client_timeout(self):
        self.knobs = dataclasses.replace(self.knobs, ready_wait=60.0, stop_wait=0.2, client_timeout=1.0)
        session = self.open("mute")
        [server] = self.servers()
        outcomes = []
        closer = threading.Thread(target=lambda: outcomes.append(session.close()), daemon=True)
        closer.start()
        self.join_or_fail(closer, "close() against a mute server", lambda: os.kill(server["pid"], signal.SIGKILL))
        [outcome] = outcomes
        self.assertEqual((outcome.hits, outcome.misses, outcome.write_errors, outcome.detached_server), (0, 0, 0, 0))
        clients = [call["argv"][0] for call in cfx.read_sccache_calls(self.fake) if call["role"] == "client"]
        self.assertEqual(clients, ["--show-stats", "--stop-server"])
        self.assertTrue(cfx.process_gone(server["pid"]))
        self.assertEqual(sorted(name for name in os.listdir(self.build) if name.startswith(".reify-sccache")), [])


class StaleSessionFiles(SessionCase):
    """Opening a session removes the session files no live supervisor owns, and nothing else."""

    def dead_pid(self):
        process = subprocess.Popen(["true"])
        process.wait()
        self.assertTrue(cfx.process_gone(process.pid), "fixture precondition: the pid is dead")
        return process.pid

    def live_pid(self):
        process = subprocess.Popen(["sleep", "300"])
        self.addCleanup(process.wait)
        self.addCleanup(process.kill)
        return process.pid

    def leave(self, pid):
        """The three files a supervisor with this pid leaves behind, with their (type, text) by path."""
        files = canon_cache.SessionFiles.in_build_dir(self.build, pid)
        os.mknod(files.socket, stat.S_IFSOCK | 0o600)
        Path(files.err).write_text(f"server {pid} said goodbye\n")
        Path(files.uncached_log).write_text(f"cc {pid}\n")
        return files

    @staticmethod
    def kind_and_text(path):
        mode = os.lstat(path).st_mode
        return stat.S_IFMT(mode), Path(path).read_text() if stat.S_ISREG(mode) else None

    def test_a_dead_supervisors_session_files_are_removed_when_a_session_opens(self):
        left = self.leave(self.dead_pid())
        session = self.open("ok")
        for path in (left.socket, left.err, left.uncached_log):
            self.assertFalse(os.path.lexists(path), path)
        self.assertEqual(session.backend, canon_cache.Backend.SHARED)
        self.assertTrue(canon_cache.answers(session.environment.server.socket))

    def test_a_live_supervisors_session_files_are_kept(self):
        left = self.leave(self.live_pid())
        paths = (left.socket, left.err, left.uncached_log)
        before = {path: self.kind_and_text(path) for path in paths}
        self.open("ok")
        self.assertEqual({path: self.kind_and_text(path) for path in paths}, before)

    def test_a_supervisor_this_user_may_not_signal_counts_as_live(self):
        try:
            os.kill(1, 0)
        except PermissionError:
            pass
        else:
            self.skipTest("this user may signal pid 1, so it cannot stand for another user's supervisor")
        left = self.leave(1)
        paths = (left.socket, left.err, left.uncached_log)
        before = {path: self.kind_and_text(path) for path in paths}
        self.open("ok")
        self.assertEqual([path for path in paths if not os.path.lexists(path)], [])
        self.assertEqual({path: self.kind_and_text(path) for path in paths}, before)

    def test_a_leftover_named_by_the_opening_process_is_removed(self):
        """Otherwise this session would count the stale lines as its own uncached compiles."""
        own = canon_cache.SessionFiles.in_build_dir(self.build, os.getpid())
        Path(own.uncached_log).write_text("cc one\ncc two\n")
        self.open("ok")
        self.assertFalse(os.path.lexists(own.uncached_log))

    def test_names_outside_the_session_grammar_are_kept(self):
        dead = self.dead_pid()
        names = (
            ".reify-canon-journal.jsonl",
            ".reify-build-root",
            ".reify-sccache.0.sock",
            f".reify-sccache.0{dead}.sock",
            f".reify-sccache.{dead}.sock.old",
            f".reify-sccache.{dead}.log",
            f"x.reify-sccache.{dead}.sock",
            f".reify-canon-uncached.{dead}.err",
        )
        for name in names:
            Path(self.build, name).write_text(name)
        self.open("ok")
        for name in names:
            self.assertTrue(os.path.lexists(os.path.join(self.build, name)), name)


def sample_record(**changes):
    fields = dict(
        mode=canon_cache.RecordMode.CANON,
        policy=canon_cache.Policy.SHARED,
        backend=canon_cache.Backend.SHARED,
        reason=canon_cache.Reason.NONE,
        hits=7,
        misses=3,
        write_errors=1,
        uncached_machine_flags=2,
        detached_server=0,
        store_attempts=1,
        wall_secs=12.3456,
        cpu_secs=67.8,
    )
    fields.update(changes)
    return canon_cache.BuildCacheRecord(**fields)


def value_types(record):
    return {name: type(value).__name__ for name, value in record.items() if name not in ("ts_ns", "pid")}


class BuildCacheRecordWriter(unittest.TestCase):
    """C6 / boundary row 8: one record, written by the channel's one encoder to two sinks."""

    def setUp(self):
        self.tmp = scratch(self)
        self.channel = os.path.join(self.tmp, "channel.jsonl")
        self.journal = os.path.join(self.tmp, "build", ".reify-canon-journal.jsonl")
        os.mkdir(os.path.dirname(self.journal))

    def write(self, record=None, channel="default"):
        canon_cache.write_record(
            record or sample_record(),
            "build",
            self.channel if channel == "default" else channel,
            self.journal,
        )

    def test_channel_and_journal_get_one_byte_identical_line(self):
        self.write()
        channel, journal = Path(self.channel).read_bytes(), Path(self.journal).read_bytes()
        self.assertEqual(channel.count(b"\n"), 1)
        self.assertEqual(channel, journal)

    def test_the_reader_sees_the_c6_record(self):
        self.write()
        for path in (self.channel, self.journal):
            with self.subTest(path=path):
                events = read_events(path)
                self.assertEqual((len(events.records), events.skipped_lines), (1, 0))
                (rec,) = events.records
                self.assertEqual((rec["v"], rec["event"]), (1, "build_cache"))
                self.assertEqual(set(rec), ENVELOPE | C6_FIELDS | {"store_attempts", "subcommand"})
                self.assertEqual(
                    {name: rec[name] for name in ("mode", "policy", "backend", "reason", "subcommand")},
                    {"mode": "canon", "policy": "shared", "backend": "shared", "reason": "", "subcommand": "build"},
                )
                for name in ("hits", "misses", "write_errors", "uncached_machine_flags", "detached_server",
                             "store_attempts"):
                    self.assertIs(type(rec[name]), int, name)
                self.assertEqual((rec["wall_secs"], rec["cpu_secs"]), (12.346, 67.8))

    def test_every_record_mode_is_written_as_its_value(self):
        for mode in canon_cache.RecordMode:
            with self.subTest(mode=mode):
                Path(self.journal).unlink(missing_ok=True)
                self.write(sample_record(mode=mode), channel=None)
                (written,) = read_events(self.journal).records
                self.assertEqual(written["mode"], mode.value)

    def test_without_a_channel_only_the_journal_is_written(self):
        self.write(channel=None)
        self.assertFalse(os.path.exists(self.channel))
        self.assertEqual(len(read_events(self.journal).records), 1)

    def test_an_unwritable_journal_raises_nothing(self):
        canon_cache.write_record(sample_record(), "", None, os.path.join(self.tmp, "missing", "journal.jsonl"))

    def test_the_committed_sample_matches_what_the_writer_produces(self):
        sample = read_events(EVENT_SAMPLE)
        self.assertEqual((len(sample.records), sample.skipped_lines), (1, 0))
        self.write()
        (written,) = read_events(self.journal).records
        self.assertEqual(value_types(sample.records[0]), value_types(written))
        self.assertEqual(
            (sample.records[0]["mode"], sample.records[0]["policy"]), ("canon", "shared")
        )


CPU_CHILD = """
import os, sys, time
while time.process_time() < 0.3:
    pass
fields = open("/proc/self/stat").read().rsplit(")", 1)[1].split()
ticks = int(fields[11]) + int(fields[12])
open(sys.argv[1], "w").write(str(ticks / os.sysconf("SC_CLK_TCK")))
"""
FD_DUMP = """
import json, os, sys
fds = {}
for name in os.listdir("/proc/self/fd"):
    try:
        fds[name] = os.readlink(f"/proc/self/fd/{name}")
    except OSError:
        pass
json.dump(fds, open(sys.argv[1], "w"))
"""
SIGIGN_PROBE = 'exec grep SigIgn /proc/self/status > "$0"'
SUPERVISED_SIGNALS = (signal.SIGINT, signal.SIGQUIT, signal.SIGTERM, signal.SIGHUP)
INTERACTIVE_CALLER = {
    signal.SIGINT: signal.default_int_handler,
    signal.SIGQUIT: signal.SIG_DFL,
    signal.SIGTERM: signal.SIG_DFL,
    signal.SIGHUP: signal.SIG_DFL,
}


class Supervise(SessionCase):
    """C6's measurement around one command, and the supervisor's signal and descriptor policy."""

    def setUp(self):
        super().setUp()
        self.journal = os.path.join(self.build, ".reify-canon-journal.jsonl")
        self.channel = os.path.join(self.tmp, "channel.jsonl")

    def supervise(self, argv, *, session="open", record_base=None, channel=None, subcommand="build"):
        if session == "open":
            session = self.open("ok")
        env = self.env if session is None else session.environment.for_command(self.env)
        return canon_cache.supervise(
            argv,
            env,
            self.tmp,
            session=session,
            record_base=record_base or canon_cache.RecordBase(canon_cache.RecordMode.CANON, canon_cache.Policy.SHARED),
            subcommand=subcommand,
            channel=channel,
            journal=self.journal,
        )

    def row(self):
        events = read_events(self.journal)
        self.assertEqual((len(events.records), events.skipped_lines), (1, 0))
        return events.records[0]

    def caller_dispositions(self, handlers):
        """Install handlers as this process's own, restored at cleanup (the suite may inherit SIG_IGN)."""
        for number, handler in handlers.items():
            self.addCleanup(signal.signal, number, signal.signal(number, handler))

    def supervised_ignored_signals(self):
        out = os.path.join(self.tmp, "sigign")
        self.supervise(["/bin/sh", "-c", SIGIGN_PROBE, out])
        return {number for number in SUPERVISED_SIGNALS if number in fx.ignored_signals(out)}

    def start_supervise_runner(self, sleep_secs, **popen):
        """A supervisor process around `sleep sleep_secs`, returned once the command has started."""
        runner = cfx.write_supervise_runner(self.tmp)
        started = os.path.join(self.tmp, "started")
        cfx.set_plan(self.fake, "ok")
        supervisor = subprocess.Popen(
            [sys.executable, "-I", runner, str(fx.REPO_ROOT / "scripts"), self.fake, self.build, self.lock, started,
             sleep_secs],
            **popen,
        )
        self.addCleanup(supervisor.kill)
        while not os.path.exists(started):
            self.assertIsNone(supervisor.poll())
            time.sleep(0.02)
        return supervisor

    def test_status_counters_and_record(self):
        status = self.supervise(["/bin/sh", "-c", "exit 3"], channel=self.channel)
        self.assertEqual(status, 3)
        row = self.row()
        expected = {
            "mode": "canon", "policy": "shared", "backend": "shared", "reason": "",
            "hits": 5, "misses": 2, "write_errors": 1, "store_attempts": 1, "subcommand": "build",
            "detached_server": 0, "uncached_machine_flags": 0,
        }
        self.assertEqual({name: row[name] for name in expected}, expected)
        self.assertGreaterEqual(row["wall_secs"], 0)
        self.assertEqual(Path(self.channel).read_bytes(), Path(self.journal).read_bytes())
        self.assertEqual(sorted(name for name in os.listdir(self.build) if name.startswith(".reify-sccache")), [])

    def test_command_killed_by_a_signal_returns_minus_it_and_is_recorded(self):
        self.caller_dispositions(INTERACTIVE_CALLER)
        self.assertEqual(self.supervise(["/bin/sh", "-c", "kill -TERM $$"]), -signal.SIGTERM)
        self.row()

    def test_cpu_secs_covers_a_grandchild(self):
        spent = os.path.join(self.tmp, "spent")
        self.supervise(["/bin/sh", "-c", f'{sys.executable} -I -c "$0" "$1"', CPU_CHILD, spent])
        self.assertGreaterEqual(self.row()["cpu_secs"], float(Path(spent).read_text()) - 0.001)

    def test_uncached_compiles_are_counted_and_the_log_removed(self):
        self.supervise(["/bin/sh", "-c", 'echo cc >> "$REIFY_CANON_UNCACHED_LOG"; echo rustc >> "$REIFY_CANON_UNCACHED_LOG"'])
        self.assertEqual(self.row()["uncached_machine_flags"], 2)
        self.assertEqual([name for name in os.listdir(self.build) if "uncached" in name], [])

    def test_store_lost_while_the_command_runs(self):
        lost = os.path.join(self.tmp, "lost")
        calls = []

        def probe():
            """Reachable when the session opens; the watchdog's next two probes fail."""
            calls.append(None)
            if len(calls) == 3:
                Path(lost).touch()
            return len(calls) == 1

        self.knobs = dataclasses.replace(self.knobs, link=canon_store.LinkKnobs(watchdog_interval=0.01))
        cfx.set_plan(self.fake, "ok")
        session = canon_cache.open_session(
            canon_cache.Policy.SHARED, self.settings, canon_build_dir=self.build, sccache=self.sccache,
            knobs=self.knobs, env=self.env, probe=probe,
        )
        self.addCleanup(session.close)
        status = self.supervise(
            ["/bin/sh", "-c", 'while [ ! -e "$0" ]; do sleep 0.02; done; exit 3', lost], session=session
        )
        self.assertEqual(status, 3)
        self.assertEqual(self.row()["reason"], "store_lost")

    def test_incremental_starts_no_server(self):
        base = canon_cache.RecordBase(canon_cache.RecordMode.CANON, canon_cache.Policy.INCREMENTAL)
        self.assertEqual(self.supervise(["/bin/true"], session=None, record_base=base), 0)
        self.assertEqual(self.servers(), [])
        row = self.row()
        self.assertEqual(
            (row["policy"], row["backend"], row["reason"], row["hits"]), ("incremental", "none", "", 0)
        )

    def test_real_path_record(self):
        base = canon_cache.RecordBase(
            canon_cache.RecordMode.REAL_PATH,
            canon_cache.Policy.SHARED,
            canon_cache.Backend.NONE,
            canon_cache.Reason.LEGACY_BUILD_DIR,
        )
        self.supervise(["/bin/true"], session=None, record_base=base)
        row = self.row()
        self.assertEqual(
            {name: row[name] for name in ("mode", "backend", "reason", "hits", "misses", "store_attempts")},
            {"mode": "real_path", "backend": "none", "reason": "legacy_build_dir", "hits": 0, "misses": 0,
             "store_attempts": 0},
        )

    def test_missing_command_raises_after_closing_the_session(self):
        session = self.open("ok")
        with self.assertRaises(canon_cache.CommandNotFound):
            self.supervise(["no-such-command-reify-canon"], session=session)
        self.assertEqual(sorted(name for name in os.listdir(self.build) if name.startswith(".reify-sccache")), [])
        self.assertTrue(cfx.process_gone(self.servers()[0]["pid"]))
        self.assertFalse(os.path.exists(self.journal))

    def test_sigterm_to_the_supervisor_is_forwarded_and_teardown_runs(self):
        supervisor = self.start_supervise_runner("600")
        supervisor.send_signal(signal.SIGTERM)
        self.assertEqual(supervisor.wait(), -signal.SIGTERM)
        self.assertEqual(sorted(name for name in os.listdir(self.build) if name.startswith(".reify-sccache")), [])
        self.assertEqual(self.row()["subcommand"], "")

    def assert_ended_by_a_signal_to_the_supervisor_alone(self, signum, status):
        """PRD C3 (esc-10418-3): a signal to the started pid ends the command as it would in real-path mode."""
        supervisor = self.start_supervise_runner("60", start_new_session=True)
        supervisor.send_signal(signum)
        self.assertEqual(supervisor.wait(), status)
        self.assertEqual(sorted(name for name in os.listdir(self.build) if name.startswith(".reify-sccache")), [])
        self.row()

    def test_sigint_to_the_supervisor_alone_reaches_the_command_and_teardown_runs(self):
        self.assert_ended_by_a_signal_to_the_supervisor_alone(signal.SIGINT, -signal.SIGINT)

    def test_sigquit_to_the_supervisor_alone_reaches_the_command_and_teardown_runs(self):
        self.assert_ended_by_a_signal_to_the_supervisor_alone(signal.SIGQUIT, -signal.SIGQUIT)

    def test_command_starts_with_default_dispositions_when_the_caller_has_them(self):
        self.caller_dispositions(INTERACTIVE_CALLER)
        self.assertEqual(self.supervised_ignored_signals(), set())

    def test_signals_the_caller_ignores_stay_ignored_in_the_command(self):
        self.caller_dispositions({
            signal.SIGINT: signal.SIG_IGN,
            signal.SIGHUP: signal.SIG_IGN,
            signal.SIGQUIT: signal.SIG_DFL,
            signal.SIGTERM: signal.SIG_DFL,
        })
        self.assertEqual(self.supervised_ignored_signals(), {signal.SIGINT, signal.SIGHUP})

    def test_dispositions_are_restored_after_the_command(self):
        self.caller_dispositions(INTERACTIVE_CALLER)
        self.supervised_ignored_signals()
        self.assertEqual({number: signal.getsignal(number) for number in INTERACTIVE_CALLER}, INTERACTIVE_CALLER)

    def test_group_sigint_ends_the_command_and_teardown_runs(self):
        supervisor = self.start_supervise_runner("60", start_new_session=True)
        os.killpg(supervisor.pid, signal.SIGINT)
        self.assertEqual(supervisor.wait(), -signal.SIGINT)
        self.assertEqual(sorted(name for name in os.listdir(self.build) if name.startswith(".reify-sccache")), [])
        self.row()

    def test_command_inherits_inheritable_fds_and_no_session_socket(self):
        marker = Path(self.tmp, "inherit-me")
        marker.touch()
        inherited = os.open(marker, os.O_RDONLY)
        self.addCleanup(os.close, inherited)
        os.set_inheritable(inherited, True)
        dump = os.path.join(self.tmp, "fds.json")
        self.supervise([sys.executable, "-I", "-c", FD_DUMP, dump])
        fds = json.loads(Path(dump).read_text())
        self.assertEqual(fds.get(str(inherited)), str(marker))
        sockets = [fd for fd, target in fds.items() if int(fd) > 2 and target.startswith("socket:")]
        self.assertEqual(sockets, [])


class SignalForwarding(unittest.TestCase):
    """canon_cache.SignalForwarder, the one signal rule both of canon_run's routes use (PRD C3)."""

    def setUp(self):
        for number, handler in INTERACTIVE_CALLER.items():
            self.addCleanup(signal.signal, number, signal.signal(number, handler))

    def test_a_signal_before_the_target_is_known_is_held_then_delivered(self):
        sent = []
        with canon_cache.SignalForwarder() as forwarder:
            signal.raise_signal(signal.SIGHUP)
            self.assertEqual(sent, [])
            forwarder.deliver_to(sent.append)
            signal.raise_signal(signal.SIGINT)
        self.assertEqual(sent, [signal.SIGHUP, signal.SIGINT])

    def test_what_the_caller_ignores_stays_ignored_and_the_rest_comes_back_after(self):
        signal.signal(signal.SIGQUIT, signal.SIG_IGN)
        forwarder = canon_cache.SignalForwarder()
        self.assertNotIn(signal.SIGQUIT, forwarder.forwarded)
        with forwarder:
            self.assertEqual(signal.getsignal(signal.SIGQUIT), signal.SIG_IGN)
            self.assertNotIn(signal.getsignal(signal.SIGINT), (signal.SIG_IGN, signal.default_int_handler))
        self.assertEqual(signal.getsignal(signal.SIGQUIT), signal.SIG_IGN)
        self.assertEqual(signal.getsignal(signal.SIGINT), signal.default_int_handler)

    def test_forwarded_is_the_supervised_signals_the_caller_does_not_ignore(self):
        self.assertEqual(canon_cache.SignalForwarder().forwarded, frozenset(canon_cache.SUPERVISED_SIGNALS))
        signal.signal(signal.SIGQUIT, signal.SIG_IGN)
        self.assertEqual(
            canon_cache.SignalForwarder().forwarded, frozenset({signal.SIGTERM, signal.SIGHUP, signal.SIGINT})
        )


class ExitLike(unittest.TestCase):
    """canon_cache.exit_like: a process ends as its command ended, through cfx.EXIT_LIKE_RUNNER.

    The runner starts at default dispositions, since a pool member may inherit INT/QUIT ignored.
    """

    def setUp(self):
        self.runner = cfx.write_exit_like_runner(scratch(self))

    def argv(self, returncode, ignored=(), closing=""):
        names = ",".join(signal.Signals(number).name for number in ignored)
        closer = ["/bin/sh", "-c", f'exec "$@" {closing}', "sh"] if closing else []
        return [*fx.AT_DEFAULT_SUPERVISED_SIGNALS, *closer, sys.executable, "-I", self.runner,
                str(fx.REPO_ROOT / "scripts"), str(returncode), names]

    def assert_ends_with(self, returncode, expected, ignored=(), closing=""):
        result = subprocess.run(self.argv(returncode, ignored, closing=closing), capture_output=True, text=True,
                                check=False)
        self.assertEqual(result.returncode, expected, result.stderr)

    def test_death_by_sigterm_kills_it_with_sigterm(self):
        self.assert_ends_with(-signal.SIGTERM, -signal.SIGTERM)

    def test_death_by_sigterm_kills_it_with_sigterm_when_a_standard_stream_is_closed(self):
        for closing in (">&-", "2>&-"):
            with self.subTest(closing=closing):
                self.assert_ends_with(-signal.SIGTERM, -signal.SIGTERM, closing=closing)

    def test_death_by_sigint_kills_it_with_sigint_despite_pythons_handler(self):
        self.assert_ends_with(-signal.SIGINT, -signal.SIGINT)

    def test_death_by_sigquit_kills_it_with_sigquit_and_dumps_no_core(self):
        if resource.getrlimit(resource.RLIMIT_CORE)[1] == 0:
            self.skipTest("RLIMIT_CORE's hard limit is 0 here: the runner cannot raise its own, so no core check")
        argv = self.argv(-signal.SIGQUIT)
        _, status = os.waitpid(os.posix_spawn(argv[0], argv, os.environ), 0)
        signalled = os.WTERMSIG(status) if os.WIFSIGNALED(status) else None
        self.assertEqual((signalled, os.WCOREDUMP(status)), (signal.SIGQUIT, False))

    def test_a_signal_the_caller_ignores_exits_128_plus_it(self):
        self.assert_ends_with(-signal.SIGTERM, 128 + signal.SIGTERM, ignored=(signal.SIGTERM,))

    def test_an_unsupervised_signal_exits_128_plus_it(self):
        self.assert_ends_with(-signal.SIGKILL, 128 + signal.SIGKILL)

    def test_an_exit_status_is_the_exit_status(self):
        for returncode in (3, 0):
            with self.subTest(returncode=returncode):
                self.assert_ends_with(returncode, returncode)


class PolicyMembers(unittest.TestCase):
    def test_members(self):
        self.assertEqual([policy.value for policy in canon_cache.Policy], ["shared", "mixed", "incremental"])


class RecordModeMembers(unittest.TestCase):
    def test_members(self):
        self.assertEqual([mode.value for mode in canon_cache.RecordMode], ["canon", "real_path"])


if __name__ == "__main__":
    unittest.main()
