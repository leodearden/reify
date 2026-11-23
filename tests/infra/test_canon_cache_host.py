#!/usr/bin/env python3
"""
test_canon_cache_host.py — stdlib unittest for the cache half of the
canonical-build-path primitive end to end (scripts/canon_run.py with
scripts/canon_cache.py; dark-factory plans/canonical-build-path-shared-cache-prd.md
C4-C6, boundary rows 10 and 13-17): real cargo and a real sccache at CANON in
mount namespaces, against a private `rclone serve webdav` store on loopback.

Every tree is tests/infra/canon_cache_fixture.py's workspace: fixture_reader
(env!(CARGO_MANIFEST_DIR), registry shlex, and a per-tree marker in its test)
and c_unit, whose build.rs compiles a C file through registry cc. Builds run
--offline against the pinned CARGO_HOME. "Every cacheable unit hit" is read as:
the second build misses nothing and hits exactly what the first build missed.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_canon_cache_host.sh.

Each missing prerequisite skips with its name: bwrap cannot enter a mount
namespace (a landlock sandbox, an absent binary), rclone is not on PATH, or the
pinned CARGO_HOME's registry cache cannot lock the workspace offline; the
landlock rows also skip where landlock is unavailable.
"""

import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_cache_fixture as cfx  # noqa: E402  (imported after sys.path manipulation)
import canon_run_fixture as fx  # noqa: E402

STAMP = ".reify-build-root"
# A background `&` or a run_all pool starts this suite with INT and QUIT ignored,
# which an invocation started from it would keep as its caller's dispositions.
AT_DEFAULT_SIGNALS = ("/usr/bin/env", "--default-signal=INT,QUIT")
MARKER = "shared-marker"
BURST = 20

# Run from tree A's `canon_run.py exec`: tree B's primitive, then tree A's
# cargo through PATH (argv: B's trampoline, B's root).
NESTED_TREES = """
import subprocess, sys
b = subprocess.run([sys.argv[1], "test", "--offline", "--", "--nocapture"], cwd=sys.argv[2])
a = subprocess.run(["cargo", "test", "--offline", "--", "--nocapture"])
sys.exit(b.returncode or a.returncode)
"""


class HostCacheCase(unittest.TestCase):
    """A scratch CANON and host.toml shared by the test's trees, and a private store."""

    def setUp(self):
        for reason in (
            fx.namespace_unavailable_reason(),
            "" if cfx.rclone_binary() else "rclone is not on PATH",
            cfx.cache_lockfile()[1],
        ):
            if reason:
                self.skipTest(reason)
        tmp = tempfile.TemporaryDirectory(prefix="test-canon-cache-host-")
        self.addCleanup(tmp.cleanup)
        self.tmp = os.path.realpath(tmp.name)
        self.store = cfx.RcloneStore(self.tmp, cfx.rclone_binary())
        self.addCleanup(self.store.stop)
        self.canon = os.path.join(self.tmp, "canon")
        self.host = os.path.join(self.tmp, "host.toml")
        self.channel = os.path.join(self.tmp, "channel.jsonl")
        self.configure("shared")

    def configure(self, policy, store="default"):
        cfx.write_cache_host_config(
            self.host,
            policy=policy,
            store=self.store if store == "default" else store,
            fallback_dir=os.path.join(self.tmp, "fallback"),
        )

    def make_tree(self, name, marker=MARKER):
        return cfx.make_cache_tree(self.tmp, name, marker)

    def env_for(self, tree, **extra):
        env = cfx.host_env(self.tmp, tree)
        self.assertEqual((env["REIFY_CANON_ROOT"], env["REIFY_CANON_HOST_CONFIG"]), (self.canon, self.host))
        env.update(extra)
        return env

    @staticmethod
    def trampoline(tree):
        return os.path.join(tree, "scripts", "canon-bin", "cargo")

    def cargo_test(self, tree, **extra):
        return fx.run([self.trampoline(tree), *fx.CARGO_TEST], cwd=tree, env=self.env_for(tree, **extra))

    def assert_passed_at_canon(self, result, marker=MARKER):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        lines = result.stdout.splitlines()
        self.assertIn(f"MANIFEST_DIR={self.canon}/fixture_reader", lines)
        self.assertIn(f"MARKER={marker}", lines)

    @staticmethod
    def target(tree):
        return os.path.join(tree, "target")

    def row(self, tree):
        rows = cfx.read_journal(self.target(tree))
        self.assertEqual(len(rows), 1, rows)
        return rows[0]

    def sockets(self, tree):
        return sorted(path.name for path in Path(self.target(tree)).glob(".reify-sccache.*.sock"))

    def socket_prefix(self):
        """What SCCACHE_SERVER_UDS of every server and client at this test's CANON starts with."""
        return f"SCCACHE_SERVER_UDS={self.canon}/target/.reify-sccache.".encode()


class SharedStore(HostCacheCase):
    def test_a_tree_at_another_path_hits_every_unit_the_first_tree_missed(self):
        """Boundary row 10."""
        tree_a, tree_b = self.make_tree("tree-a"), self.make_tree("tree-b")
        self.assert_passed_at_canon(self.cargo_test(tree_a))
        self.assert_passed_at_canon(self.cargo_test(tree_b, REIFY_VERIFY_EVENT_LOG=self.channel))
        row_a, row_b = self.row(tree_a), self.row(tree_b)
        self.assertEqual((row_a["backend"], row_a["write_errors"]), ("shared", 0), row_a)
        self.assertGreater(row_a["misses"], 0, row_a)
        self.assertEqual((row_b["mode"], row_b["backend"], row_b["misses"]), ("canon", "shared", 0), row_b)
        self.assertEqual(row_b["hits"], row_a["misses"], (row_a, row_b))
        self.assertGreater(row_b["cpu_secs"], 0, row_b)
        self.assertEqual(row_b["subcommand"], "test")
        self.assertEqual(cfx.read_events(self.channel).records, (row_b,))
        for tree in (tree_a, tree_b):
            with self.subTest(tree=tree):
                self.assertEqual(Path(self.target(tree), STAMP).read_text(), self.canon + "\n")
                self.assertEqual(self.sockets(tree), [])

    def test_a_stopped_store_falls_back_to_the_local_cache(self):
        self.store.stop()
        tree = self.make_tree("tree-c")
        self.assert_passed_at_canon(self.cargo_test(tree))
        row = self.row(tree)
        self.assertEqual((row["backend"], row["reason"]), ("local_fallback", "store_unreachable"), row)

    def test_two_trees_built_at_once_each_get_their_own_build_and_row(self):
        """Boundary row 13."""
        trees = {marker: self.make_tree(f"tree-{marker}", marker) for marker in ("left", "right")}
        builds = {
            marker: subprocess.Popen(
                [self.trampoline(tree), *fx.CARGO_TEST], cwd=tree, env=self.env_for(tree),
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            for marker, tree in trees.items()
        }
        for marker, build in builds.items():
            stdout, stderr = build.communicate()
            with self.subTest(marker=marker):
                self.assert_passed_at_canon(subprocess.CompletedProcess(build.args, build.returncode, stdout, stderr), marker)
                self.assertEqual(self.row(trees[marker])["mode"], "canon")

    def test_a_tree_launched_inside_another_trees_invocation_is_its_own_invocation(self):
        """Boundary row 14: tree B inside tree A's exec gets its own row; A counts none of B's units."""
        solo = self.make_tree("tree-solo")
        self.assert_passed_at_canon(self.cargo_test(solo))
        solo_row = self.row(solo)
        tree_a, tree_b = self.make_tree("tree-a"), self.make_tree("tree-b", "inner")
        result = fx.run_canon(
            tree_a, tree_a, "exec", "--", sys.executable, "-I", "-c", NESTED_TREES, self.trampoline(tree_b), tree_b,
            env=self.env_for(tree_a),
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("MARKER=inner", result.stdout.splitlines())
        self.assertEqual(self.row(tree_b)["mode"], "canon")
        row_a = self.row(tree_a)
        self.assertEqual(row_a["subcommand"], "")
        self.assertEqual(row_a["hits"] + row_a["misses"], solo_row["hits"] + solo_row["misses"], (row_a, solo_row))


class Teardown(HostCacheCase):
    """Boundary row 15: no server or client outlives its invocation, however it ends."""

    def test_a_group_kill_mid_build_leaves_no_process_naming_the_socket(self):
        tree = self.make_tree("tree-k")
        fifo = os.path.join(self.tmp, "block.fifo")
        os.mkfifo(fifo)
        build = subprocess.Popen(
            [self.trampoline(tree), "build", "--offline"], cwd=tree,
            env=self.env_for(tree, C_UNIT_BLOCK_FIFO=fifo),
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, start_new_session=True,
        )
        self.addCleanup(build.wait)
        self.addCleanup(lambda: build.poll() is None and os.killpg(build.pid, signal.SIGKILL))
        writer = self.open_when_read(fifo, build)
        self.addCleanup(os.close, writer)
        self.assertNotEqual(cfx.processes_naming(self.socket_prefix()), [])
        os.killpg(build.pid, signal.SIGKILL)
        build.wait()
        self.assertEqual(cfx.wait_until_none_name(self.socket_prefix()), [])

    def test_a_terminal_interrupt_on_the_namespace_route_still_tears_down_and_records(self):
        """Ctrl-C reaches the whole group, bwrap included; the re-run must still close and record."""
        tree = self.make_tree("tree-i")
        fifo = os.path.join(self.tmp, "block-int.fifo")
        os.mkfifo(fifo)
        build = subprocess.Popen(
            [*AT_DEFAULT_SIGNALS, self.trampoline(tree), "build", "--offline"], cwd=tree,
            env=self.env_for(tree, C_UNIT_BLOCK_FIFO=fifo),
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, start_new_session=True,
        )
        self.addCleanup(lambda: build.poll() is None and os.killpg(build.pid, signal.SIGKILL))
        writer = self.open_when_read(fifo, build)
        self.addCleanup(os.close, writer)
        os.killpg(build.pid, signal.SIGINT)
        _, stderr = build.communicate()
        self.assertNotEqual(build.returncode, 0, stderr)
        leftovers = sorted(path.name for path in Path(self.target(tree)).glob(".reify-sccache.*"))
        self.assertEqual(leftovers, [], stderr)
        self.assertEqual(len(cfx.read_journal(self.target(tree))), 1, stderr)
        self.assertEqual(cfx.wait_until_none_name(self.socket_prefix()), [])

    def test_sigterm_to_the_namespace_routes_started_pid_alone_ends_the_command_and_records(self):
        self.assert_a_signal_to_the_outer_pid_alone_tears_down(signal.SIGTERM, 143)

    def test_sigint_to_the_namespace_routes_started_pid_alone_ends_the_command_and_records(self):
        self.assert_a_signal_to_the_outer_pid_alone_tears_down(signal.SIGINT, 130)

    def assert_a_signal_to_the_outer_pid_alone_tears_down(self, signum, status):
        """PRD C3 (esc-10418-3): the outer process forwards to the re-run, the child pid bwrap's --info-fd names."""
        tree = self.make_tree(f"tree-{signum.name.lower()}")
        started = os.path.join(self.tmp, f"started-{signum.name}")
        outer = subprocess.Popen(
            [*AT_DEFAULT_SIGNALS, fx.PYTHON, "-I", os.path.join(tree, "scripts", "canon_run.py"), "exec", "--",
             "/bin/sh", "-c", 'touch "$0"; exec sleep 60', started],
            cwd=tree, env=self.env_for(tree),
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, start_new_session=True,
        )
        self.addCleanup(lambda: outer.poll() is None and os.killpg(outer.pid, signal.SIGKILL))
        while not os.path.exists(started):
            if outer.poll() is not None:
                self.fail(f"the invocation ended (rc={outer.returncode}) before its command started:"
                          f" {outer.stderr.read()}")
            time.sleep(0.05)
        os.kill(outer.pid, signum)
        _, stderr = outer.communicate()
        self.assertEqual(outer.returncode, status, stderr)
        leftovers = sorted(path.name for path in Path(self.target(tree)).glob(".reify-sccache.*"))
        self.assertEqual(leftovers, [], stderr)
        self.assertEqual(len(cfx.read_journal(self.target(tree))), 1, stderr)
        self.assertEqual(cfx.wait_until_none_name(self.socket_prefix()), [])

    def test_a_normal_exit_leaves_no_socket_and_no_process_naming_it(self):
        tree = self.make_tree("tree-n")
        self.assert_passed_at_canon(self.cargo_test(tree))
        self.assertEqual(self.sockets(tree), [])
        self.assertEqual(cfx.wait_until_none_name(self.socket_prefix()), [])

    def open_when_read(self, fifo, build):
        """The FIFO's write end, once c_unit's build script has opened it for reading."""
        while True:
            try:
                return os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
            except OSError:
                if build.poll() is not None:
                    self.fail(f"the build ended (rc={build.returncode}) before its build script blocked:"
                              f" {build.stderr.read()}")
                time.sleep(0.05)


class ServerBurst(HostCacheCase):
    def test_twenty_invocations_on_one_tree_and_one_store(self):
        """Boundary row 16: each server writes to the store or falls back; never a silent read-only server."""
        tree = self.make_tree("tree-burst")
        guard = os.path.join(tree, "scripts", "canon-bin", "guard", "sccache")
        script = os.path.join(tree, "scripts", "canon_run.py")
        for index in range(BURST):
            Path(tree, f"u{index}.c").write_text(f"int burst_{index}(void) {{ return {index}; }}\n")
        burst = [
            subprocess.Popen(
                [fx.PYTHON, "-I", script, "exec", "--", guard, "cc", "-c", f"u{index}.c", "-o", f"u{index}.o"],
                cwd=tree, env=self.env_for(tree), stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
            )
            for index in range(BURST)
        ]
        for invocation in burst:
            _, stderr = invocation.communicate()
            self.assertEqual(invocation.returncode, 0, stderr)
        rows = cfx.read_journal(self.target(tree))
        self.assertEqual(len(rows), BURST, rows)
        shared = [row for row in rows if row["backend"] == "shared"]
        for row in rows:
            with self.subTest(row=row):
                self.assertIn(row["backend"], ("shared", "local_fallback"))
                if row["backend"] == "shared":
                    self.assertEqual(row["write_errors"], 0)
                if row["reason"] == "read_only_server":
                    self.assertEqual(row["store_attempts"], 3)
        self.assertGreaterEqual(len(self.store.objects()), len(shared))


class SandboxedAgent(HostCacheCase):
    """Boundary row 17: CANON already bound, then landlock, then cargo through the trampoline."""

    SCRIPT = f"cargo {' '.join(fx.CARGO_TEST)}; rc=$?; echo PWD_AFTER=$(pwd -P); exit $rc"

    def setUp(self):
        super().setUp()
        reason = fx.landlock_unavailable_reason()
        if reason:
            self.skipTest(reason)

    def sandboxed_cargo_test(self, tree):
        argv = fx.bwrap_bind(tree, self.canon, *fx.landlock(self.tmp, "/bin/bash", "-c", self.SCRIPT))
        result = fx.run(argv, cwd=tree, env=self.env_for(tree))
        self.assert_passed_at_canon(result)
        self.assertIn(f"PWD_AFTER={tree}", result.stdout.splitlines())
        return self.row(tree)

    def test_shared(self):
        row = self.sandboxed_cargo_test(self.make_tree("tree-shared"))
        self.assertEqual((row["mode"], row["policy"], row["backend"]), ("canon", "shared", "shared"), row)

    def test_mixed_after_shared_hits_registry_and_c_units(self):
        """OQ8: registry and C units key alike under shared and mixed."""
        self.sandboxed_cargo_test(self.make_tree("tree-shared"))
        self.configure("mixed")
        tree = self.make_tree("tree-mixed")
        row = self.sandboxed_cargo_test(tree)
        incremental = Path(self.target(tree), "debug", "incremental")
        self.assertTrue(
            any(entry.name.startswith("fixture_reader-") for entry in incremental.iterdir()),
            sorted(entry.name for entry in incremental.iterdir()),
        )
        self.assertEqual((row["policy"], row["misses"]), ("mixed", 0), row)
        self.assertGreater(row["hits"], 0, row)


if __name__ == "__main__":
    unittest.main()
