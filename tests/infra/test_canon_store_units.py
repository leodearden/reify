#!/usr/bin/env python3
"""
test_canon_store_units.py — stdlib unittest for scripts/canon_store.py, the
store link of the canonical-build-path primitive (dark-factory
plans/canonical-build-path-shared-cache-prd.md C5, boundary row 7): the
store's coordinates, the start-up probe, the loopback relay a private sccache
server reaches the store through, and the watchdog that cuts it.

Every store is canon_store_fixture's loopback FakeStore; no case starts an
sccache or enters a mount namespace.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_canon_store_units.sh.
"""

import base64
import http.client
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)
import canon_store_fixture as sfx  # noqa: E402

sys.path.insert(0, str(fx.REPO_ROOT / "scripts"))
import canon_store  # noqa: E402

UNRESOLVABLE_HOST = "reify-canon-no-such-host.invalid"  # RFC 6761: .invalid never resolves
FAST_LINK = canon_store.LinkKnobs(probe_timeout=0.3)


def scratch(test):
    tmp = tempfile.TemporaryDirectory(prefix="test-canon-store-units-")
    test.addCleanup(tmp.cleanup)
    return os.path.realpath(tmp.name)


def store_at(url, user="reify", password="hunter2"):
    return canon_store.Store(url, user, password)


class Standalone(unittest.TestCase):
    """canon_store reads alone: copied without any sibling, it still imports."""

    def test_imports_with_no_sibling_beside_it(self):
        alone = scratch(self)
        shutil.copy2(fx.REPO_ROOT / "scripts" / "canon_store.py", alone)
        completed = subprocess.run(
            ["/usr/bin/python3", "-I", "-c", "import sys; sys.path.insert(0, sys.argv[1]); import canon_store", alone],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)


class StoreValue(unittest.TestCase):
    def test_password_is_not_in_repr(self):
        self.assertNotIn("hunter2", repr(canon_store.Store("http://s/r", "reify", "hunter2")))

    def test_key_prefix_is_the_url_path_without_its_slashes(self):
        self.assertEqual(canon_store.Store("http://s:8080/reify/cache/").key_prefix, "reify/cache")

    def test_a_url_that_cannot_name_the_store_is_refused_at_construction(self):
        for url in ("ftp://x", "https://store.invalid/reify", "http://a..b/reify", "http:///reify", ""):
            with self.subTest(url=url):
                with self.assertRaises(ValueError) as caught:
                    canon_store.Store(url, "reify", "hunter2")
                message = str(caught.exception)
                self.assertIn(canon_store.store_url_problem(url), message)
                self.assertIn(repr(url), message)
                self.assertNotIn("hunter2", message)


class StoreCase(unittest.TestCase):
    def setUp(self):
        self.fake = sfx.FakeStore()
        self.addCleanup(self.fake.stop)
        self.store = store_at(self.fake.url)

    def probe(self, store=None, knobs=FAST_LINK, **kwargs):
        return canon_store.probe_store(store or self.store, knobs, **kwargs)


class ProbeStore(StoreCase):
    """C5's probe: GET <store_url>/.sccache_check, judged on the injected clock."""

    def test_reachable_on_404_and_200(self):
        for status in (404, 200):
            with self.subTest(status=status):
                self.fake.check_status = status
                self.assertTrue(self.probe())
        self.assertEqual(self.fake.requests[-1], ("GET", "/reify/.sccache_check"))

    def test_sends_basic_auth(self):
        self.probe()
        expected = "Basic " + base64.b64encode(b"reify:hunter2").decode()
        self.assertEqual(self.fake.authorizations, [expected])

    def test_unreachable_on_401(self):
        self.fake.set_mode("auth")
        self.assertFalse(self.probe())

    def test_unreachable_on_a_refused_port(self):
        self.assertFalse(self.probe(store_at(f"http://127.0.0.1:{sfx.closed_port()}/reify")))

    def test_unreachable_when_the_store_never_answers(self):
        self.fake.set_mode("hang")
        self.assertFalse(self.probe())

    def test_unreachable_when_the_clock_says_the_answer_was_slow(self):
        readings = iter((100.0, 100.0 + FAST_LINK.slow + 0.5))
        self.assertFalse(self.probe(clock=lambda: next(readings)))

    def test_reachable_when_the_clock_says_the_answer_was_quick(self):
        readings = iter((100.0, 100.0 + FAST_LINK.slow - 0.5))
        self.assertTrue(self.probe(clock=lambda: next(readings)))


class RelayCase(StoreCase):
    def setUp(self):
        super().setUp()
        self.relay = canon_store.Relay("127.0.0.1", self.fake.port)
        self.relay.start()
        self.addCleanup(self.relay.close)
        self.relay_port = int(self.relay.endpoint.rsplit(":", 1)[1])

    def start_slow_transfer(self):
        """A GET through the relay that has received SLOW_HEAD and waits for the rest."""
        connection = http.client.HTTPConnection("127.0.0.1", self.relay_port, timeout=30)
        self.addCleanup(connection.close)
        connection.request("GET", "/slow/object")
        response = connection.getresponse()
        self.assertEqual(response.read(len(sfx.SLOW_HEAD)), sfx.SLOW_HEAD)
        return response

    def assert_cut_short(self, response):
        received = 0
        try:
            while chunk := response.read1(65536):
                received += len(chunk)
        except (ConnectionResetError, http.client.IncompleteRead):
            pass
        self.assertLess(len(sfx.SLOW_HEAD) + received, sfx.SLOW_LENGTH)


class Relay(RelayCase):
    """C5's loopback relay: owned, cuttable, and holding its port until closed."""

    def test_endpoint_is_loopback(self):
        self.assertEqual(self.relay.endpoint, f"http://127.0.0.1:{self.relay_port}")

    def test_get_through_the_relay_is_byte_for_byte_the_store_answer(self):
        direct = sfx.raw_get(self.fake.port, "/reify/object")
        self.assertTrue(direct.startswith(b"HTTP/1.1 200"), direct)
        self.assertEqual(sfx.raw_get(self.relay_port, "/reify/object"), direct)

    def test_cut_resets_an_in_flight_transfer(self):
        response = self.start_slow_transfer()
        self.relay.cut()
        self.fake.release()
        self.assert_cut_short(response)

    def test_after_cut_new_connections_get_no_answer_and_the_port_stays_bound(self):
        self.relay.cut()
        self.assertEqual(sfx.raw_get(self.relay_port, "/reify/object"), b"")
        self.assertFalse(sfx.can_bind(self.relay_port))

    def test_close_releases_the_port(self):
        self.relay.cut()
        self.relay.close()
        self.assertTrue(sfx.can_bind(self.relay_port))

    def test_a_refusing_target_resets_the_client_and_still_cuts_and_closes(self):
        relay = canon_store.Relay("127.0.0.1", sfx.closed_port())
        relay.start()
        self.addCleanup(relay.close)
        self.assertEqual(sfx.raw_get(relay.port, "/reify/object"), b"")
        relay.cut()
        relay.close()
        self.assertTrue(sfx.can_bind(relay.port))

    def test_an_unresolvable_store_fails_at_construction(self):
        with self.assertRaises(OSError):
            canon_store.Relay(UNRESOLVABLE_HOST, 80)


class OpenRelay(unittest.TestCase):
    def test_an_unresolvable_store_has_no_relay(self):
        self.assertIsNone(canon_store.open_relay(store_at(f"http://{UNRESOLVABLE_HOST}:8080/reify")))

    def test_a_resolvable_store_gets_an_unstarted_relay(self):
        relay = canon_store.open_relay(store_at(f"http://127.0.0.1:{sfx.closed_port()}/reify"))
        self.addCleanup(relay.close)
        self.assertRegex(relay.endpoint, r"^http://127\.0\.0\.1:[0-9]+$")


class RelayOutOfDescriptors(StoreCase):
    """A relay that cannot get a descriptor resets what it holds and never strands a caller."""

    def run_case(self, case):
        runner = sfx.write_relay_fd_runner(scratch(self))
        completed = subprocess.run(
            [sys.executable, "-I", runner, str(fx.REPO_ROOT / "scripts"), str(self.fake.port), case],
            capture_output=True,
            text=True,
            timeout=120,
        )
        self.assertNotIn("Traceback", completed.stderr)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        return json.loads(completed.stdout)

    def test_a_client_whose_upstream_gets_no_descriptor_is_reset_and_the_relay_serves_on(self):
        self.assertEqual(self.run_case("accept"), {"first": "reset", "second": 200, "closed": True})

    def test_a_loop_that_cannot_start_still_lets_cut_and_close_return_and_refuses(self):
        self.assertEqual(self.run_case("loop"), {"cut": True, "refused": True, "closed": True})

    def test_a_client_that_cannot_be_accepted_does_not_spin_the_relay_and_is_served_later(self):
        """Processor time, not wall time: the relay thread backs off instead of re-selecting a readable listener."""
        verdict = self.run_case("backoff")
        self.assertLess(verdict.pop("cpu_secs"), 0.25)
        self.assertEqual(verdict, {"served": 200, "closed": True})


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
        self.watchdog = canon_store.Watchdog(self.probe, self, test.knobs, self.clock, self.wait)

    def probe(self):
        outcome = self.script[self.probes]
        self.probes += 1
        if outcome == "slow":
            self.clock.now += FAST_LINK.slow + 1.0
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

    knobs = FAST_LINK

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
        watchdog = canon_store.Watchdog(link.probe, link, self.knobs, link.clock)
        watchdog.start()
        watchdog.stop()
        self.assertFalse(watchdog.lost)
        self.assertEqual((link.probes, link.cuts), (0, 0))


class StoreLost(RelayCase):
    """Row 7 against the real relay: a store that stops answering mid-transfer is cut."""

    def test_two_failed_probes_cut_the_relay_and_reset_the_transfer(self):
        response = self.start_slow_transfer()
        self.fake.set_mode("hang")
        probes = []

        def probe():
            probes.append(canon_store.probe_store(self.store, FAST_LINK))
            return probes[-1]

        watchdog = canon_store.Watchdog(probe, self.relay, FAST_LINK, wait=lambda stopped, seconds: False)
        watchdog.start()
        watchdog.join()
        self.assertTrue(watchdog.lost)
        self.assertEqual(probes, [False, False])
        self.fake.release()
        self.assert_cut_short(response)


if __name__ == "__main__":
    unittest.main()
