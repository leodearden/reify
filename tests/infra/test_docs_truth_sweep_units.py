#!/usr/bin/env python3
"""
test_docs_truth_sweep_units.py — stdlib unittest for the docs-truth sweep's
systemd --user deployment: deploy/systemd/reify-docs-truth-sweep.{service,timer}
and scripts/install-docs-truth-sweep-units.sh (built on
scripts/lib_systemd_user_install.sh).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_docs_truth_sweep_units.sh, which invokes
this file.

HERMETIC: the installer runs with PATH-stub `systemctl` and `loginctl` first on
PATH (each appends its argv to a calls file), and with XDG_CONFIG_HOME and HOME
pointed at a tempdir, so no real unit is ever installed or enabled.
REIFY_TEST_NO_USER_BUS=1 makes the stub's `show-environment` fail;
REIFY_TEST_LINGER sets the stub's `show-user -p Linger --value` answer.
"""

import configparser
import filecmp
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEPLOY_DIR = REPO_ROOT / "deploy" / "systemd"
SERVICE_NAME = "reify-docs-truth-sweep.service"
TIMER_NAME = "reify-docs-truth-sweep.timer"
INSTALLER = REPO_ROOT / "scripts" / "install-docs-truth-sweep-units.sh"
MAIN_CHECKOUT = "/home/leo/src/reify"
ENTRYPOINT = "scripts/docs-truth-sweep.sh"

STUB_SYSTEMCTL = """#!/bin/sh
echo "systemctl $*" >> "$REIFY_TEST_CALLS_FILE"
if [ "${REIFY_TEST_NO_USER_BUS:-0}" = "1" ]; then
    case " $* " in *" show-environment "*) exit 1 ;; esac
fi
exit 0
"""

STUB_LOGINCTL = """#!/bin/sh
echo "loginctl $*" >> "$REIFY_TEST_CALLS_FILE"
[ "${1:-}" = "show-user" ] && { echo "${REIFY_TEST_LINGER:-yes}"; exit 0; }
exit 0
"""


def read_unit(path):
    parser = configparser.ConfigParser(interpolation=None, strict=False)
    parser.optionxform = str
    with open(path, encoding="utf-8") as unit:
        parser.read_file(unit)
    return parser


class UnitFilesTest(unittest.TestCase):
    def test_service_is_a_oneshot_running_the_bare_main_checkout_entrypoint(self):
        service = read_unit(DEPLOY_DIR / SERVICE_NAME)

        self.assertEqual(service["Service"]["Type"], "oneshot")
        self.assertEqual(service["Service"]["ExecStart"], f"{MAIN_CHECKOUT}/{ENTRYPOINT}")
        self.assertTrue(os.access(REPO_ROOT / ENTRYPOINT, os.X_OK), ENTRYPOINT)
        self.assertNotIn("Install", service.sections())

    def test_timer_is_persistent_owns_the_service_and_installs_into_timers(self):
        timer = read_unit(DEPLOY_DIR / TIMER_NAME)

        self.assertTrue(timer["Timer"].get("OnCalendar", "").strip())
        self.assertEqual(timer["Timer"]["Persistent"], "true")
        self.assertEqual(timer["Timer"]["Unit"], SERVICE_NAME)
        self.assertEqual(timer["Install"]["WantedBy"], "timers.target")


class InstallerTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)
        self.stub_dir = self.tmp / "stub-bin"
        self.stub_dir.mkdir()
        for name, body in (("systemctl", STUB_SYSTEMCTL), ("loginctl", STUB_LOGINCTL)):
            stub = self.stub_dir / name
            stub.write_text(body)
            stub.chmod(0o755)
        self.calls_file = self.tmp / "calls.log"
        self.xdg = self.tmp / "xdg"
        self.unit_dir = self.xdg / "systemd" / "user"

    def run_installer(self, *args, **env_overrides):
        env = {
            key: value for key, value in os.environ.items() if not key.startswith("REIFY_TEST_")
        }
        env.update(
            PATH=f"{self.stub_dir}:{os.environ.get('PATH', '/usr/bin:/bin')}",
            HOME=str(self.tmp / "home"),
            XDG_CONFIG_HOME=str(self.xdg),
            REIFY_TEST_CALLS_FILE=str(self.calls_file),
            **env_overrides,
        )
        return subprocess.run(
            ["bash", str(INSTALLER), *args],
            capture_output=True,
            text=True,
            timeout=60,
            env=env,
        )

    def systemctl_calls(self):
        if not self.calls_file.exists():
            return []
        return [
            line for line in self.calls_file.read_text().splitlines() if line.startswith("systemctl ")
        ]

    def assert_units_installed(self):
        for name in (SERVICE_NAME, TIMER_NAME):
            installed = self.unit_dir / name
            self.assertTrue(installed.is_file(), installed)
            self.assertTrue(filecmp.cmp(installed, DEPLOY_DIR / name, shallow=False), name)

    def test_install_copies_both_units_reloads_then_enables_only_the_timer(self):
        result = self.run_installer()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_units_installed()
        calls = self.systemctl_calls()
        reload_call = "systemctl --user daemon-reload"
        enable_call = f"systemctl --user enable --now {TIMER_NAME}"
        self.assertIn(reload_call, calls)
        self.assertIn(enable_call, calls)
        self.assertLess(calls.index(reload_call), calls.index(enable_call), calls)
        self.assertFalse(
            [call for call in calls if " enable " in f" {call} " and SERVICE_NAME in call], calls
        )

    def test_install_is_idempotent(self):
        self.assertEqual(self.run_installer().returncode, 0)

        result = self.run_installer()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_units_installed()

    def test_no_user_bus_warns_and_installs_nothing(self):
        result = self.run_installer(REIFY_TEST_NO_USER_BUS="1")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("WARN", result.stderr)
        self.assertFalse(self.unit_dir.exists() and any(self.unit_dir.iterdir()))

    def test_a_missing_unit_source_fails_even_without_a_bus(self):
        empty_repo = self.tmp / "empty-repo"
        empty_repo.mkdir()

        result = self.run_installer(
            REIFY_TEST_REPO_ROOT=str(empty_repo), REIFY_TEST_NO_USER_BUS="1"
        )

        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(str(empty_repo / "deploy" / "systemd"), result.stderr)
        self.assertFalse(self.unit_dir.exists())

    def test_cli_help_and_misuse(self):
        self.assertEqual(self.run_installer("--help").returncode, 0)
        self.assertEqual(self.run_installer("--unexpected").returncode, 2)
        self.assertEqual(self.systemctl_calls(), [])

    def test_lingering_off_warns_but_still_installs(self):
        result = self.run_installer(REIFY_TEST_LINGER="no")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("loginctl enable-linger", result.stderr)
        self.assert_units_installed()


if __name__ == "__main__":
    unittest.main()
