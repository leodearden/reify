#!/usr/bin/env python3
"""
test_cargo_inputs_lib.py — stdlib unittest for tests/infra/cargo_inputs_lib.sh
(task 9993).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_cargo_inputs_lib.sh, which invokes this
file.

Two halves:
- the lib's classification of cargo's output (stub cargo for the three shapes,
  real cargo for the two measured cold cases), and
- ADOPTERS: every member that sources the lib, run with an empty CARGO_HOME and
  CARGO_NET_OFFLINE=true, must stop at its top fetch with the infrastructure
  verdict (exit 75, the marker, no FAIL line). A member whose top fetch is
  missing reaches its cargo reads and FAILs instead.
"""

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INFRA_DIR = REPO_ROOT / "tests" / "infra"
LIB = INFRA_DIR / "cargo_inputs_lib.sh"

MARKER = "@@REIFY_INFRA_UNAVAILABLE@@"
UNAVAILABLE_RC = 75

# Members that source cargo_inputs_lib.sh; DriftTest keeps this exact.
ADOPTERS = (
    "test_affected_crates_lib.sh",
    "test_engine_hash_closure.sh",
    "test_run_gui_scripts.sh",
)

SOURCES_LIB = re.compile(r"^\s*(source|\.)\s+\S*cargo_inputs_lib\.sh", re.M)
FAIL_LINE = re.compile(r"^\s*FAIL:", re.M)

# A broken-infrastructure backstop, not a timing assertion.
SUBPROCESS_BACKSTOP_SECS = 1200

# One cargo line per network/offline signature cargo_inputs_lib.sh recognises,
# each carrying that signature and no other, so a typo in any one alternative
# reds its own subtest. The first two are cargo 1.96's no-network output (dead
# proxy), the third its CARGO_NET_OFFLINE note on an empty home; the last two
# are its crate-download wrappers.
NETWORK_SIGNATURE_LINES = {
    "unable to update registry": "  unable to update registry `crates-io`",
    "download of .* failed": "  download of config.json failed",
    "offline mode (via": (
        "note: offline mode (via `--offline`) can sometimes cause surprising "
        "resolution failures"
    ),
    "failed to download": (
        "error: failed to download from "
        "`https://static.crates.io/crates/serde/1.0.228/download`"
    ),
    "spurious network error": (
        "warning: spurious network error (2 tries remaining): [6] Couldn't "
        "resolve host name"
    ),
}

STALE_LOCK_TEXT = (
    "error: cannot update the lock file /x/Cargo.lock because --locked was "
    "passed to prevent this\n"
)

FETCH_SCRIPT = (
    'set -euo pipefail; source "$1"; shift; '
    'rc=0; cargo_inputs_fetch "$@" || rc=$?; exit "$rc"'
)


def run_fetch(args, env=None, cwd=None):
    return subprocess.run(
        ["bash", "-c", FETCH_SCRIPT, "_", str(LIB), *args],
        capture_output=True, text=True, env=env, cwd=cwd,
        timeout=SUBPROCESS_BACKSTOP_SECS,
    )


def cold_offline_env(cargo_home):
    env = dict(os.environ)
    env["CARGO_HOME"] = str(cargo_home)
    env["CARGO_NET_OFFLINE"] = "true"
    return env


class StubCargoTest(unittest.TestCase):
    """The lib's classification of cargo's three outcome shapes."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="reify-cil-stub.")
        self.tmp = Path(self._tmp.name)
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        self.argv_file = self.tmp / "argv"

    def tearDown(self):
        self._tmp.cleanup()

    def stub(self, stderr_text, rc):
        text_file = self.tmp / "stderr.txt"
        text_file.write_text(stderr_text)
        cargo = self.bin / "cargo"
        cargo.write_text(
            "#!/usr/bin/env bash\n"
            f"printf '%s\\n' \"$@\" > '{self.argv_file}'\n"
            f"cat '{text_file}' >&2\n"
            f"exit {rc}\n"
        )
        cargo.chmod(0o755)
        env = dict(os.environ)
        env["PATH"] = f"{self.bin}{os.pathsep}{env.get('PATH', '')}"
        return env

    def test_success_is_silent_and_passes_the_scope_through(self):
        env = self.stub("", 0)
        r = run_fetch(["--target", "T", "--manifest-path", "M"], env=env)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout, "")
        self.assertEqual(r.stderr, "")
        self.assertEqual(
            self.argv_file.read_text().splitlines(),
            ["fetch", "--locked", "--target", "T", "--manifest-path", "M"],
        )

    def test_each_network_signature_alone_is_infrastructure(self):
        for signature, line in NETWORK_SIGNATURE_LINES.items():
            with self.subTest(signature=signature):
                env = self.stub(line + "\n", 101)
                r = run_fetch(["--manifest-path", "M"], env=env)
                self.assertEqual(r.returncode, UNAVAILABLE_RC, r.stderr)
                self.assertRegex(
                    r.stderr.splitlines()[0],
                    rf"^{re.escape(MARKER)} kind=cargo_fetch rc=101 ",
                )
                self.assertIn(line.strip(), r.stderr)
                self.assertEqual(r.stdout, "")

    def test_refusal_is_a_tree_defect_not_infrastructure(self):
        env = self.stub(STALE_LOCK_TEXT, 101)
        r = run_fetch(["--manifest-path", "M"], env=env)
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertNotIn(MARKER, r.stdout + r.stderr)
        self.assertIn("cannot update the lock file", r.stderr)


class RealCargoTest(unittest.TestCase):
    """Real cargo, hermetic: an empty CARGO_HOME and offline mode."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="reify-cil-real.")
        self.tmp = Path(self._tmp.name)
        self.home = self.tmp / "cargo-home"
        self.home.mkdir()

    def tearDown(self):
        self._tmp.cleanup()

    def test_cold_offline_registry_is_infrastructure(self):
        r = run_fetch(
            ["--manifest-path", str(REPO_ROOT / "Cargo.toml")],
            env=cold_offline_env(self.home), cwd=REPO_ROOT,
        )
        self.assertEqual(r.returncode, UNAVAILABLE_RC, r.stderr)
        self.assertRegex(r.stderr, rf"(?m)^{re.escape(MARKER)} kind=cargo_fetch ")

    def test_stale_lock_is_a_tree_defect_even_offline(self):
        ws = self.tmp / "ws"
        for crate in ("a", "b"):
            (ws / crate / "src").mkdir(parents=True)
            (ws / crate / "src" / "lib.rs").write_text("")
            (ws / crate / "Cargo.toml").write_text(
                f'[package]\nname = "{crate}"\nversion = "0.1.0"\n'
                'edition = "2021"\n'
            )
        (ws / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["a"]\nresolver = "2"\n'
        )
        env = cold_offline_env(self.home)
        manifest = str(ws / "Cargo.toml")
        lock = subprocess.run(
            ["cargo", "generate-lockfile", "--offline", "--manifest-path", manifest],
            capture_output=True, text=True, env=env, cwd=self.tmp,
            timeout=SUBPROCESS_BACKSTOP_SECS,
        )
        self.assertEqual(lock.returncode, 0, lock.stderr)
        with open(ws / "a" / "Cargo.toml", "a") as f:
            f.write('\n[dependencies]\nb = { path = "../b" }\n')

        r = run_fetch(["--manifest-path", manifest], env=env, cwd=self.tmp)
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("cannot update the lock file", r.stderr)
        self.assertNotIn(MARKER, r.stdout + r.stderr)


class AdoptersTest(unittest.TestCase):
    """Every member that sources the lib fetches before its first read."""

    def test_adopters_is_exactly_the_members_that_source_the_lib(self):
        sourcing = {
            p.name for p in INFRA_DIR.glob("test_*.sh")
            if SOURCES_LIB.search(p.read_text(errors="replace"))
        }
        self.assertEqual(sourcing, set(ADOPTERS))

    def test_each_adopter_stops_at_its_fetch_with_the_infra_verdict(self):
        self.assertTrue(ADOPTERS, "no adopter: this check would pass vacuously")
        for name in ADOPTERS:
            with self.subTest(member=name), \
                    tempfile.TemporaryDirectory(prefix="reify-cil-adopter.") as home:
                r = subprocess.run(
                    ["bash", str(INFRA_DIR / name)],
                    capture_output=True, text=True,
                    env=cold_offline_env(home), cwd=REPO_ROOT,
                    timeout=SUBPROCESS_BACKSTOP_SECS,
                )
                log = r.stdout + r.stderr
                self.assertEqual(r.returncode, UNAVAILABLE_RC, log[-4000:])
                self.assertRegex(log, rf"(?m)^{re.escape(MARKER)} kind=cargo_fetch")
                self.assertRegex(log, r"(?m)^INFRASTRUCTURE:")
                self.assertNotRegex(log, FAIL_LINE)


if __name__ == "__main__":
    unittest.main()
