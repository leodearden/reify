#!/usr/bin/env python3
"""
test_bundle_clean_install.py — the clean-machine proof of the Reify installer
(task 9732, docs/prds/alpha-spine.md D6 / §5.2 / boundary row B7).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered
member is the thin wrapper tests/infra/test_bundle_clean_install.sh, which
invokes this file.

ARMED ONLY: every test skips unless REIFY_RUN_BUNDLE_CLEAN_INSTALL=1, because
the proof is a cold bundle build plus two docker containers. Once armed, a
missing docker is a FAIL, not a skip.

What it proves, each in a fresh stock ubuntu:24.04 container whose premises are
asserted first (no /opt/reify-deps, only the stock apt sources):
  * the .deb installs with apt alone, no OCCT/gmsh/OpenVDB/oneTBB package gets
    pulled in (the bundle, not a Depends, must carry them), `reify check` on
    examples/m5_geometry.ri exits 0, and `reify-gui` under xvfb answers a
    debug-MCP store_state with at least one mesh;
  * the AppImage does the same on a desktop-session stand-in (xvfb, xauth,
    curl, GTK 3 and the GL front-ends, but no WebKit, which the AppImage must
    carry itself).

Knobs: REIFY_BUNDLE_DEB + REIFY_BUNDLE_APPIMAGE reuse a prior build instead of
running scripts/build-bundle.sh; REIFY_BUNDLE_PROOF_READY_SECS (default 600) is
the hang guard on the store_state readiness poll — no elapsed time is asserted.
"""

import json
import os
import re
import secrets
import shlex
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BUILD_BUNDLE = REPO_ROOT / "scripts" / "build-bundle.sh"

ARM_VAR = "REIFY_RUN_BUNDLE_CLEAN_INSTALL"
BASE_IMAGE = "ubuntu:24.04"
EXAMPLE = "/work/examples/m5_geometry.ri"
DEBUG_PORT = 3939
GUI_ENV = {
    "REIFY_DEBUG": "1",
    "REIFY_DEBUG_PORT": str(DEBUG_PORT),
    # scripts/lib_gui_launch.sh pins this: under Xvfb WebKitGTK aborts on GBM EGL.
    "WEBKIT_DISABLE_DMABUF_RENDERER": "1",
}
GUI_LOG = "/tmp/reify-gui.log"
GUI_EXIT = "/tmp/reify-gui.exit"
STORE_STATE_BODY = json.dumps({
    "jsonrpc": "2.0", "id": 1, "method": "tools/call",
    "params": {"name": "store_state", "arguments": {}},
})
BUNDLED_NOT_DEPENDED = re.compile(r"^(libocct|gmsh|libgmsh|libopenvdb|libtbb)")
APPIMAGE_CARRIES_WEBKIT = re.compile(r"^libwebkit2gtk")
# The desktop-session stand-in for the AppImage: the libraries the AppImage
# excludelist leaves to the host. GTK 3 brings X11, fontconfig and freetype;
# the glvnd GL front-ends (linked, or dlopened by WebKit and libepoxy) come
# from the desktop install itself: ubuntu-24.04.5.1-desktop-amd64.manifest
# lists all four.
DESKTOP_STAND_IN = ("libgtk-3-0t64", "libopengl0", "libegl1", "libgles2", "libgl1")


def _log(message):
    print(f"bundle proof: {message}", file=sys.stderr, flush=True)


def _ready_secs():
    return float(os.environ.get("REIFY_BUNDLE_PROOF_READY_SECS", "600"))


def _docker(*args):
    return subprocess.run(
        ["docker", *args],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, check=False,
    )


def _store_state_mesh_count(reply_text):
    """engine.meshCount from a store_state JSON-RPC reply, or None while the
    reply is not (yet) a store_state result."""
    try:
        reply = json.loads(reply_text)
        state = json.loads(reply["result"]["content"][0]["text"])
        return int(state["engine"]["meshCount"])
    except (ValueError, KeyError, IndexError, TypeError):
        return None


class Container:
    """A detached ubuntu:24.04 container driven by `docker exec`; removed by
    the owning test's cleanup."""

    def __init__(self, test, mounts):
        self.test = test
        self.name = f"reify-bundle-proof-{os.getpid()}-{secrets.token_hex(4)}"
        volumes = [arg for src, dest in mounts for arg in ("-v", f"{src}:{dest}:ro")]
        # Cleanup first: `docker run -d` can create the container and still
        # fail to start it, and `rm -f` of a name that never existed is harmless.
        test.addCleanup(_docker, "rm", "-f", self.name)
        started = _docker("run", "-d", "--name", self.name, *volumes, BASE_IMAGE,
                          "sleep", "infinity")
        test.assertEqual(started.returncode, 0, started.stdout)

    def run(self, script, env=None):
        env_args = [arg for k, v in (env or {}).items() for arg in ("-e", f"{k}={v}")]
        return _docker("exec", *env_args, self.name, "bash", "-c", script)

    def check(self, script, env=None):
        proc = self.run(script, env)
        self.test.assertEqual(
            proc.returncode, 0, f"in {self.name}: {script}\n{proc.stdout[-6000:]}")
        return proc.stdout

    def start_detached(self, script, env=None):
        env_args = [arg for k, v in (env or {}).items() for arg in ("-e", f"{k}={v}")]
        started = _docker("exec", "-d", *env_args, self.name, "bash", "-c", script)
        self.test.assertEqual(started.returncode, 0, started.stdout)

    def assert_stock_premises(self):
        self.check("test ! -e /opt/reify-deps")
        self.test.assertEqual(self.check("ls -A /etc/apt/sources.list.d").split(),
                              ["ubuntu.sources"])
        self.test.assertEqual(
            self.check("grep -Ev '^[[:space:]]*(#|$)' /etc/apt/sources.list || true"), "")

    def apt_install(self, *packages):
        words = " ".join(shlex.quote(p) for p in packages)
        self.check("apt-get update -q && DEBIAN_FRONTEND=noninteractive "
                   f"apt-get install -y -q --no-install-recommends {words}")

    def installed_packages(self):
        return self.check("dpkg-query -W -f '${Package}\\n'").split()

    def assert_none_installed(self, pattern):
        offenders = [p for p in self.installed_packages() if pattern.match(p)]
        self.test.assertEqual(offenders, [], f"unexpected packages in {self.name}")

    def assert_gui_renders(self, gui_command):
        self.start_detached(
            f"{gui_command} > {GUI_LOG} 2>&1; echo $? > {GUI_EXIT}", env=GUI_ENV)
        deadline = time.monotonic() + _ready_secs()
        last_reply = ""
        while time.monotonic() < deadline:
            if self.run(f"test -e {GUI_EXIT}").returncode == 0:
                self.test.fail(f"the GUI exited before rendering: {self.gui_postmortem()}")
            poll = self.run(
                f"curl -fsS --max-time 5 -X POST http://127.0.0.1:{DEBUG_PORT}/mcp "
                f"-H 'Content-Type: application/json' -d {shlex.quote(STORE_STATE_BODY)}")
            if poll.returncode == 0:
                last_reply = poll.stdout
                count = _store_state_mesh_count(poll.stdout)
                if count is not None and count >= 1:
                    _log(f"{self.name}: store_state engine.meshCount = {count}")
                    return
            time.sleep(2)
        self.test.fail(
            f"no store_state with engine.meshCount >= 1 within the "
            f"{_ready_secs():.0f}s hang guard; last reply: {last_reply[-2000:]!r}; "
            f"{self.gui_postmortem()}")

    def gui_postmortem(self):
        proc = self.run(f"cat {GUI_EXIT} 2>/dev/null; tail -n 80 {GUI_LOG}")
        return f"GUI exit/log:\n{proc.stdout}"


class BundleCleanInstall(unittest.TestCase):

    @classmethod
    def setUpClass(cls):
        if os.environ.get(ARM_VAR) != "1":
            raise unittest.SkipTest(f"armed only: set {ARM_VAR}=1 (docker + a cold bundle build)")
        docker = _docker("info")
        if docker.returncode != 0:
            raise AssertionError(f"armed, but docker is unusable:\n{docker.stdout[-2000:]}")
        reused = (os.environ.get("REIFY_BUNDLE_DEB"), os.environ.get("REIFY_BUNDLE_APPIMAGE"))
        if all(reused):
            cls.build = None
            cls.deb, cls.appimage = (Path(p) for p in reused)
        else:
            cls.build = cls._run_build_bundle()
            cls.deb, cls.appimage = cls._assets_from(cls.build.stdout)

    @staticmethod
    def _run_build_bundle():
        _log(f"running {BUILD_BUNDLE} (stderr to a temp log)")
        with tempfile.TemporaryFile(mode="w+", encoding="utf-8") as log:
            proc = subprocess.run(
                [str(BUILD_BUNDLE)],
                cwd=REPO_ROOT, stdout=subprocess.PIPE, stderr=log, text=True, check=False,
            )
            if proc.returncode != 0:
                log.seek(0)
                tail = log.read()[-12000:]
                raise AssertionError(f"build-bundle.sh exited {proc.returncode}:\n{tail}")
        return proc

    @staticmethod
    def _assets_from(stdout):
        lines = stdout.splitlines()
        debs = [Path(line) for line in lines if line.endswith(".deb")]
        appimages = [Path(line) for line in lines if line.endswith(".AppImage")]
        if len(debs) != 1 or len(appimages) != 1:
            raise AssertionError(f"build-bundle.sh stdout names no single .deb/.AppImage: {stdout!r}")
        return debs[0], appimages[0]

    def container(self, *mounts):
        return Container(self, [(REPO_ROOT / "examples", "/work/examples"), *mounts])

    def test_build_bundle_stdout_contract(self):
        if self.build is None:
            self.skipTest("assets reused from REIFY_BUNDLE_DEB / REIFY_BUNDLE_APPIMAGE")
        lines = self.build.stdout.splitlines()
        self.assertEqual(len(lines), 2, self.build.stdout)
        self.assertEqual(sorted(Path(line).suffix for line in lines), [".AppImage", ".deb"])
        for line in lines:
            path = Path(line)
            self.assertTrue(path.is_absolute(), line)
            self.assertTrue(path.is_file() and path.stat().st_size > 0, line)

    def test_deb(self):
        deb = f"/assets/{self.deb.name}"
        box = self.container((self.deb, deb))
        box.assert_stock_premises()
        box.apt_install(deb, "xvfb", "xauth", "curl")
        self.assertEqual(box.check("command -v reify").strip(), "/usr/bin/reify")
        box.assert_none_installed(BUNDLED_NOT_DEPENDED)
        box.check(f"reify check {EXAMPLE}")
        box.assert_gui_renders(f"xvfb-run -a reify-gui {EXAMPLE}")

    def test_appimage(self):
        appimage = f"/assets/{self.appimage.name}"
        box = self.container((self.appimage, appimage))
        box.assert_stock_premises()
        box.apt_install("xvfb", "xauth", "curl", *DESKTOP_STAND_IN)
        box.assert_none_installed(BUNDLED_NOT_DEPENDED)
        box.assert_none_installed(APPIMAGE_CARRIES_WEBKIT)
        box.check(f"mkdir -p /tmp/extract && cd /tmp/extract && {appimage} --appimage-extract >/dev/null"
                  f" && squashfs-root/usr/bin/reify check {EXAMPLE}")
        box.assert_gui_renders(f"APPIMAGE_EXTRACT_AND_RUN=1 xvfb-run -a {appimage} {EXAMPLE}")


if __name__ == "__main__":
    unittest.main()
