#!/usr/bin/env python3
"""
test_docker_create_probe.py — stdlib unittest for scripts/docker-create-probe.sh,
the bounded "can docker create a container" check that build-bundle.sh and the
armed clean-install proof run in place of a bare `docker info` (task 10181).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered
member is the thin wrapper tests/infra/test_docker_create_probe.sh, which
invokes this file.

Hermetic: no docker daemon. docker appears only as a fake on PATH whose `info`
and `create` each answer, refuse or hang as the test asks. The probe's
consumers are tested beside their other tests in tests/infra/test_release_alpha.py.
"""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
PROBE = REPO_ROOT / "scripts" / "docker-create-probe.sh"
BOUND_KNOB = "REIFY_DOCKER_CREATE_PROBE_SECS"

FAKE_DOCKER = r"""#!/usr/bin/env bash
printf '%s\n' "$*" >>"$FAKE_DOCKER_LOG"
behave() {
    case "$1" in
        ok) [ "$2" = create ] && echo fake-container-id; exit 0 ;;
        fail) echo "fake-docker: $2 refused" >&2; exit 1 ;;
        hang) exec sleep 60 ;;
    esac
    exit 1
}
case "$1" in
    info) behave "$FAKE_DOCKER_INFO" info ;;
    create) behave "$FAKE_DOCKER_CREATE" create ;;
    rm) exit 0 ;;
    *) exit 1 ;;
esac
"""


class ProbeVerdicts(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        root = Path(scratch.name)
        bin_dir = root / "bin"
        bin_dir.mkdir()
        fake = bin_dir / "docker"
        fake.write_text(FAKE_DOCKER, encoding="utf-8")
        fake.chmod(0o755)
        self.log = root / "docker.log"
        self.env = {k: v for k, v in os.environ.items() if k != BOUND_KNOB}
        self.env["PATH"] = f"{bin_dir}{os.pathsep}{os.environ.get('PATH', '')}"
        self.env["FAKE_DOCKER_LOG"] = str(self.log)

    def _probe(self, *args, info="ok", create="ok", secs=None):
        env = dict(self.env, FAKE_DOCKER_INFO=info, FAKE_DOCKER_CREATE=create)
        if secs is not None:
            env[BOUND_KNOB] = secs
        return subprocess.run(
            ["bash", str(PROBE), *args],
            env=env,
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )

    def _calls(self):
        if not self.log.exists():
            return []
        lines = self.log.read_text(encoding="utf-8").splitlines()
        return [line.split() for line in lines]

    def _subcommands(self):
        return [call[0] for call in self._calls()]

    def _call(self, subcommand):
        matching = [call for call in self._calls() if call[0] == subcommand]
        self.assertEqual(len(matching), 1, self._calls())
        return matching[0]

    @staticmethod
    def _name_of(create_call):
        return create_call[create_call.index("--name") + 1]

    def test_a_healthy_docker_is_probed_by_info_then_create_then_rm(self):
        proc = self._probe()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(proc.stdout, "")
        self.assertEqual(self._subcommands(), ["info", "create", "rm"])
        create = self._call("create")
        name = self._name_of(create)
        self.assertIn("--label", create)
        self.assertEqual(
            create[create.index("--label") + 1], "reify.docker-create-probe=1"
        )
        self.assertEqual(create[-2:], ["ubuntu:24.04", "true"])
        self.assertIn(name, self._call("rm"))

    def test_a_positional_image_replaces_the_default(self):
        proc = self._probe("busybox:latest")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        create = self._call("create")
        self.assertEqual(create[-2:], ["busybox:latest", "true"])
        self.assertNotIn("ubuntu:24.04", create)

    def test_a_refused_info_fails_before_any_create(self):
        proc = self._probe(info="fail")
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("fake-docker: info refused", proc.stderr)
        self.assertNotIn("create", self._subcommands())

    def test_a_refused_create_fails_without_an_rm(self):
        proc = self._probe(create="fail")
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("fake-docker: create refused", proc.stderr)
        self.assertNotIn("rm", self._subcommands())

    def test_a_stalled_create_names_the_step_bound_container_and_host_check(self):
        proc = self._probe(create="hang", secs="1")
        self.assertEqual(proc.returncode, 3, proc.stderr)
        name = self._name_of(self._call("create"))
        self.assertIn("docker create", proc.stderr)
        self.assertIn("1s", proc.stderr)
        self.assertIn(name, proc.stderr)
        self.assertIn("waiting for writeback completion", proc.stderr)

    def test_a_stalled_info_fails_before_any_create(self):
        proc = self._probe(info="hang", secs="1")
        self.assertEqual(proc.returncode, 3, proc.stderr)
        self.assertIn("docker info", proc.stderr)
        self.assertNotIn("create", self._subcommands())

    def test_a_usage_error_exits_2_before_any_docker_call(self):
        for secs in ("0", "-5", "abc"):
            with self.subTest(secs=secs):
                proc = self._probe(secs=secs)
                self.assertEqual(proc.returncode, 2, proc.stderr)
                self.assertEqual(self._calls(), [])
        with self.subTest(argv="two positional arguments"):
            proc = self._probe("busybox:latest", "extra")
            self.assertEqual(proc.returncode, 2, proc.stderr)
            self.assertEqual(self._calls(), [])


if __name__ == "__main__":
    unittest.main()
